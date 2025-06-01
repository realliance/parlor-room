//! Lobby manager implementation for handling multiple lobby instances
//!
//! This module provides the core LobbyManager that orchestrates lobby creation,
//! player matching, lobby lifecycle management, and cleanup.

use crate::amqp::publisher::EventPublisher;
use crate::error::{MatchmakingError, Result};
use crate::lobby::instance::{Lobby, LobbyInstance, LobbyState};
use crate::lobby::matching::{BasicLobbyMatcher, LobbyMatcher, MatchingConfig, MatchingResult};
use crate::lobby::provider::LobbyProvider;
use crate::metrics::MetricsCollector;
use crate::types::{GameStarting, LobbyId, LobbyType, Player, PlayerJoinedLobby, QueueRequest};
use crate::utils::{current_timestamp, generate_lobby_id};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, error, info, warn};

/// Statistics about lobby manager operations
#[derive(Debug, Clone, Default)]
pub struct LobbyManagerStats {
    /// Total number of lobbies created
    pub lobbies_created: u64,
    /// Total number of lobbies cleaned up
    pub lobbies_cleaned: u64,
    /// Total number of players queued
    pub players_queued: u64,
    /// Total number of games started
    pub games_started: u64,
    /// Current number of active lobbies
    pub active_lobbies: usize,
    /// Current number of players waiting
    pub players_waiting: usize,
}

/// Main lobby manager responsible for coordinating lobby operations
pub struct LobbyManager {
    /// Map of active lobbies by ID
    lobbies: Arc<RwLock<HashMap<LobbyId, LobbyInstance>>>,
    /// Lobby provider for configurations
    lobby_provider: Arc<dyn LobbyProvider>,
    /// Lobby matcher for finding suitable lobbies
    lobby_matcher: Arc<dyn LobbyMatcher>,
    /// Matching configuration
    matching_config: MatchingConfig,
    /// Event publisher for lobby events
    event_publisher: Arc<dyn EventPublisher>,
    /// Manager statistics
    stats: Arc<RwLock<LobbyManagerStats>>,
    /// Metrics collector for recording performance data
    metrics_collector: Arc<MetricsCollector>,
}

impl LobbyManager {
    /// Create a new lobby manager
    pub fn new(
        lobby_provider: Arc<dyn LobbyProvider>,
        event_publisher: Arc<dyn EventPublisher>,
    ) -> Self {
        // Create a default metrics collector if none provided
        let metrics_collector = Arc::new(MetricsCollector::new().unwrap_or_else(|_| {
            warn!("Failed to create metrics collector, using default");
            MetricsCollector::default()
        }));

        Self::with_metrics(lobby_provider, event_publisher, metrics_collector)
    }

    /// Create a new lobby manager with metrics collector
    pub fn with_metrics(
        lobby_provider: Arc<dyn LobbyProvider>,
        event_publisher: Arc<dyn EventPublisher>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Self {
        Self {
            lobbies: Arc::new(RwLock::new(HashMap::new())),
            lobby_provider,
            lobby_matcher: Arc::new(BasicLobbyMatcher::new()),
            matching_config: MatchingConfig::default(),
            event_publisher,
            stats: Arc::new(RwLock::new(LobbyManagerStats::default())),
            metrics_collector,
        }
    }

    /// Create with custom matcher and configuration
    pub fn with_matcher(
        lobby_provider: Arc<dyn LobbyProvider>,
        event_publisher: Arc<dyn EventPublisher>,
        lobby_matcher: Arc<dyn LobbyMatcher>,
        matching_config: MatchingConfig,
    ) -> Self {
        // Create a default metrics collector if none provided
        let metrics_collector = Arc::new(MetricsCollector::new().unwrap_or_else(|_| {
            warn!("Failed to create metrics collector, using default");
            MetricsCollector::default()
        }));

        Self::with_matcher_and_metrics(
            lobby_provider,
            event_publisher,
            lobby_matcher,
            matching_config,
            metrics_collector,
        )
    }

    /// Create with custom matcher, configuration, and metrics
    pub fn with_matcher_and_metrics(
        lobby_provider: Arc<dyn LobbyProvider>,
        event_publisher: Arc<dyn EventPublisher>,
        lobby_matcher: Arc<dyn LobbyMatcher>,
        matching_config: MatchingConfig,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Self {
        Self {
            lobbies: Arc::new(RwLock::new(HashMap::new())),
            lobby_provider,
            lobby_matcher,
            matching_config,
            event_publisher,
            stats: Arc::new(RwLock::new(LobbyManagerStats::default())),
            metrics_collector,
        }
    }

    /// Handle a queue request from a player or bot
    pub async fn handle_queue_request(&self, request: QueueRequest) -> Result<LobbyId> {
        let start_time = Instant::now();

        debug!(
            "Handling queue request for player {} (type: {:?}, lobby: {:?})",
            request.player_id, request.player_type, request.lobby_type
        );

        // Record queue request metric using the high-level API
        self.metrics_collector.record_queue_request(
            request.player_type,
            request.lobby_type,
            Duration::default(), // Will be set at the end
        );

        // Create player from request
        let player = Player {
            id: request.player_id.clone(),
            player_type: request.player_type,
            rating: request.current_rating,
            joined_at: current_timestamp(),
        };

        // Update stats
        {
            let mut stats = self
                .stats
                .write()
                .map_err(|_| MatchmakingError::InternalError {
                    message: "Failed to acquire stats lock".to_string(),
                })?;
            stats.players_queued += 1;
        }

        let result = async {
            // Find or create suitable lobby
            let lobby_id = self
                .find_or_create_lobby_for_player(player.clone(), request.lobby_type)
                .await?;

            // Add player to the lobby
            self.add_player_to_lobby(lobby_id, player).await?;

            // Check if lobby should start a game
            self.check_and_start_game(lobby_id).await?;

            Ok(lobby_id)
        }
        .await;

        // Record processing time using the high-level API
        let duration = start_time.elapsed();
        self.metrics_collector.record_queue_request(
            request.player_type,
            request.lobby_type,
            duration,
        );

        result
    }

    /// Find a suitable lobby for a player, or create a new one
    async fn find_or_create_lobby_for_player(
        &self,
        player: Player,
        lobby_type: LobbyType,
    ) -> Result<LobbyId> {
        // Get all available lobbies of the requested type
        let available_lobbies = self.get_available_lobbies_for_type(lobby_type).await?;

        // Convert to trait objects for matching
        let lobby_refs: Vec<&dyn Lobby> = available_lobbies
            .iter()
            .map(|lobby| lobby as &dyn Lobby)
            .collect();

        // Try to find a suitable lobby
        let matching_result = self.lobby_matcher.find_lobby_for_player(
            &player,
            &lobby_refs,
            &self.matching_config,
        )?;

        match matching_result {
            MatchingResult::MatchedToLobby(lobby_id) => {
                debug!("Found existing lobby {} for player {}", lobby_id, player.id);
                Ok(lobby_id)
            }
            MatchingResult::CreateNewLobby => {
                debug!("Creating new lobby for player {}", player.id);
                self.create_new_lobby(lobby_type).await
            }
            MatchingResult::ShouldWait { reason } => {
                // For Phase 2, we'll create a new lobby instead of waiting
                warn!(
                    "Player {} should wait ({}), but creating new lobby instead",
                    player.id, reason
                );
                self.create_new_lobby(lobby_type).await
            }
        }
    }

    /// Get available lobbies for a specific type
    async fn get_available_lobbies_for_type(
        &self,
        lobby_type: LobbyType,
    ) -> Result<Vec<LobbyInstance>> {
        let lobbies = self
            .lobbies
            .read()
            .map_err(|_| MatchmakingError::InternalError {
                message: "Failed to acquire lobbies lock".to_string(),
            })?;

        let available: Vec<LobbyInstance> = lobbies
            .values()
            .filter(|lobby| {
                lobby.config().lobby_type == lobby_type
                    && matches!(
                        lobby.state(),
                        LobbyState::WaitingForPlayers | LobbyState::ReadyToStart
                    )
                    && !lobby.is_full()
            })
            .cloned()
            .collect();

        Ok(available)
    }

    /// Create a new lobby of the specified type
    async fn create_new_lobby(&self, lobby_type: LobbyType) -> Result<LobbyId> {
        let config = self.lobby_provider.get_lobby_config(lobby_type)?;
        let lobby = LobbyInstance::new(config);
        let lobby_id = lobby.lobby_id();

        // Add to manager
        {
            let mut lobbies =
                self.lobbies
                    .write()
                    .map_err(|_| MatchmakingError::InternalError {
                        message: "Failed to acquire lobbies lock".to_string(),
                    })?;
            lobbies.insert(lobby_id, lobby);
        }

        // Update stats and metrics
        {
            let mut stats = self
                .stats
                .write()
                .map_err(|_| MatchmakingError::InternalError {
                    message: "Failed to acquire stats lock".to_string(),
                })?;
            stats.lobbies_created += 1;
            stats.active_lobbies += 1;
        }

        // Record lobby creation using the high-level API
        self.metrics_collector.record_lobby_created(lobby_type);

        info!("Created new {} lobby with ID {}", lobby_type, lobby_id);
        Ok(lobby_id)
    }

    /// Add a player to a specific lobby
    async fn add_player_to_lobby(&self, lobby_id: LobbyId, player: Player) -> Result<()> {
        let player_joined_event = {
            let mut lobbies =
                self.lobbies
                    .write()
                    .map_err(|_| MatchmakingError::InternalError {
                        message: "Failed to acquire lobbies lock".to_string(),
                    })?;

            let lobby =
                lobbies
                    .get_mut(&lobby_id)
                    .ok_or_else(|| MatchmakingError::LobbyNotFound {
                        lobby_id: lobby_id.to_string(),
                    })?;

            // Add player to lobby
            lobby.add_player(player.clone())?;

            // Create event
            PlayerJoinedLobby {
                lobby_id,
                player_id: player.id.clone(),
                player_type: player.player_type,
                current_players: lobby.get_players(),
                timestamp: current_timestamp(),
            }
        };

        // Update player-related metrics using direct access
        match player.player_type {
            crate::types::PlayerType::Human => {
                self.metrics_collector
                    .player()
                    .players_queued_total
                    .with_label_values(&["human", "joined"])
                    .inc();
            }
            crate::types::PlayerType::Bot => {
                self.metrics_collector.bot().active_bot_requests.inc();
            }
        }

        // Publish event
        self.event_publisher
            .publish_player_joined_lobby(player_joined_event)
            .await?;

        debug!("Player {} joined lobby {}", player.id, lobby_id);
        Ok(())
    }

    /// Check if a lobby should start a game and initiate if ready
    async fn check_and_start_game(&self, lobby_id: LobbyId) -> Result<()> {
        let game_starting_event = {
            let mut lobbies =
                self.lobbies
                    .write()
                    .map_err(|_| MatchmakingError::InternalError {
                        message: "Failed to acquire lobbies lock".to_string(),
                    })?;

            let lobby =
                lobbies
                    .get_mut(&lobby_id)
                    .ok_or_else(|| MatchmakingError::LobbyNotFound {
                        lobby_id: lobby_id.to_string(),
                    })?;

            if lobby.should_start() {
                lobby.mark_starting()?;

                // Update stats
                {
                    let mut stats =
                        self.stats
                            .write()
                            .map_err(|_| MatchmakingError::InternalError {
                                message: "Failed to acquire stats lock".to_string(),
                            })?;
                    stats.games_started += 1;
                }

                let players = lobby.get_players();
                let human_count = players
                    .iter()
                    .filter(|p| matches!(p.player_type, crate::types::PlayerType::Human))
                    .count();
                let bot_count = players.len() - human_count;

                // Record game start using the high-level API
                self.metrics_collector.record_game_started(
                    lobby.config().lobby_type,
                    human_count,
                    bot_count,
                );

                Some(GameStarting {
                    lobby_id,
                    players,
                    game_id: generate_lobby_id(), // Generate unique game ID
                    rating_changes_preview: vec![], // Empty for Phase 2 (no rating calculations yet)
                    timestamp: current_timestamp(),
                })
            } else {
                None
            }
        };

        if let Some(event) = game_starting_event {
            self.event_publisher.publish_game_starting(event).await?;
            info!("Game starting for lobby {}", lobby_id);
        }

        Ok(())
    }

    /// Remove a player from their current lobby
    pub async fn remove_player(&self, player_id: &str) -> Result<Option<LobbyId>> {
        let mut removed_from_lobby = None;

        {
            let mut lobbies =
                self.lobbies
                    .write()
                    .map_err(|_| MatchmakingError::InternalError {
                        message: "Failed to acquire lobbies lock".to_string(),
                    })?;

            // Find and remove player from any lobby
            for (lobby_id, lobby) in lobbies.iter_mut() {
                if let Ok(Some(_)) = lobby.remove_player(player_id) {
                    removed_from_lobby = Some(*lobby_id);
                    break;
                }
            }
        }

        if let Some(lobby_id) = removed_from_lobby {
            debug!("Removed player {} from lobby {}", player_id, lobby_id);
        }

        Ok(removed_from_lobby)
    }

    /// Get information about a specific lobby
    pub async fn get_lobby_info(&self, lobby_id: LobbyId) -> Result<Option<LobbyInstance>> {
        let lobbies = self
            .lobbies
            .read()
            .map_err(|_| MatchmakingError::InternalError {
                message: "Failed to acquire lobbies lock".to_string(),
            })?;

        Ok(lobbies.get(&lobby_id).cloned())
    }

    /// Get all active lobbies
    pub async fn get_all_lobbies(&self) -> Result<Vec<LobbyInstance>> {
        let lobbies = self
            .lobbies
            .read()
            .map_err(|_| MatchmakingError::InternalError {
                message: "Failed to acquire lobbies lock".to_string(),
            })?;

        Ok(lobbies.values().cloned().collect())
    }

    /// Get lobbies that need bot backfill
    pub async fn get_lobbies_needing_backfill(&self) -> Result<Vec<LobbyInstance>> {
        let lobbies = self
            .lobbies
            .read()
            .map_err(|_| MatchmakingError::InternalError {
                message: "Failed to acquire lobbies lock".to_string(),
            })?;

        let needing_backfill: Vec<LobbyInstance> = lobbies
            .values()
            .filter(|lobby| lobby.needs_backfill())
            .cloned()
            .collect();

        Ok(needing_backfill)
    }

    /// Periodic cleanup of stale lobbies
    pub async fn cleanup_stale_lobbies(&self) -> Result<usize> {
        let mut cleaned_count = 0;
        let mut lobbies_to_remove = Vec::new();

        {
            let lobbies = self
                .lobbies
                .read()
                .map_err(|_| MatchmakingError::InternalError {
                    message: "Failed to acquire lobbies lock".to_string(),
                })?;

            // Find lobbies that should be cleaned up
            for (lobby_id, lobby) in lobbies.iter() {
                if lobby.should_cleanup() {
                    lobbies_to_remove.push(*lobby_id);
                }
            }
        }

        // Remove stale lobbies
        if !lobbies_to_remove.is_empty() {
            let mut lobbies =
                self.lobbies
                    .write()
                    .map_err(|_| MatchmakingError::InternalError {
                        message: "Failed to acquire lobbies lock".to_string(),
                    })?;

            for lobby_id in lobbies_to_remove {
                if lobbies.remove(&lobby_id).is_some() {
                    cleaned_count += 1;
                    debug!("Cleaned up stale lobby {}", lobby_id);
                }
            }

            // Update stats and metrics
            {
                let mut stats =
                    self.stats
                        .write()
                        .map_err(|_| MatchmakingError::InternalError {
                            message: "Failed to acquire stats lock".to_string(),
                        })?;
                stats.lobbies_cleaned += cleaned_count;
                stats.active_lobbies = lobbies.len();
                stats.players_waiting = lobbies
                    .values()
                    .map(|lobby| lobby.get_players().len())
                    .sum();
            }

            // Record cleanup metrics using direct access
            self.metrics_collector
                .lobby()
                .lobbies_cleaned_total
                .inc_by(cleaned_count);
        }

        if cleaned_count > 0 {
            info!("Cleaned up {} stale lobbies", cleaned_count);
        }

        Ok(cleaned_count as usize)
    }

    /// Start the cleanup task that runs periodically
    pub fn start_cleanup_task(self: Arc<Self>) -> Result<()> {
        let manager = Arc::clone(&self);

        tokio::spawn(async move {
            let mut cleanup_interval = interval(Duration::from_secs(60)); // Every minute

            loop {
                cleanup_interval.tick().await;

                if let Err(e) = manager.cleanup_stale_lobbies().await {
                    error!("Error during lobby cleanup: {}", e);
                }
            }
        });

        info!("Started lobby cleanup task");
        Ok(())
    }

    /// Get current manager statistics
    pub async fn get_stats(&self) -> Result<LobbyManagerStats> {
        let stats = self
            .stats
            .read()
            .map_err(|_| MatchmakingError::InternalError {
                message: "Failed to acquire stats lock".to_string(),
            })?;

        Ok(stats.clone())
    }

    /// Update the matching configuration
    pub fn update_matching_config(&mut self, config: MatchingConfig) {
        self.matching_config = config;
    }
}

// Implement Clone manually since we need to clone the Arc references
impl Clone for LobbyManager {
    fn clone(&self) -> Self {
        Self {
            lobbies: Arc::clone(&self.lobbies),
            lobby_provider: Arc::clone(&self.lobby_provider),
            lobby_matcher: Arc::clone(&self.lobby_matcher),
            matching_config: self.matching_config.clone(),
            event_publisher: Arc::clone(&self.event_publisher),
            stats: Arc::clone(&self.stats),
            metrics_collector: Arc::clone(&self.metrics_collector),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amqp::publisher::MockEventPublisher;
    use crate::lobby::provider::StaticLobbyProvider;
    use crate::types::{PlayerRating, PlayerType, QueueRequest};
    use tokio;

    async fn create_test_manager() -> LobbyManager {
        let lobby_provider = Arc::new(StaticLobbyProvider::new());
        let event_publisher = Arc::new(MockEventPublisher::new());
        LobbyManager::new(lobby_provider, event_publisher)
    }

    fn create_test_queue_request(
        player_id: &str,
        player_type: PlayerType,
        lobby_type: LobbyType,
    ) -> QueueRequest {
        QueueRequest {
            player_id: player_id.to_string(),
            player_type,
            lobby_type,
            current_rating: PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            timestamp: current_timestamp(),
            auth_token: None, // No auth token for tests
        }
    }

    #[tokio::test]
    async fn test_handle_queue_request() {
        let manager = create_test_manager().await;

        let request = create_test_queue_request("bot1", PlayerType::Bot, LobbyType::AllBot);
        let lobby_id = manager.handle_queue_request(request).await.unwrap();

        // Verify lobby was created and player was added
        let lobby_info = manager.get_lobby_info(lobby_id).await.unwrap().unwrap();
        assert_eq!(lobby_info.get_players().len(), 1);
        assert_eq!(lobby_info.get_players()[0].id, "bot1");
    }

    #[tokio::test]
    async fn test_create_allbot_lobby() {
        let manager = create_test_manager().await;

        let lobby_id = manager.create_new_lobby(LobbyType::AllBot).await.unwrap();
        let lobby_info = manager.get_lobby_info(lobby_id).await.unwrap().unwrap();

        assert_eq!(lobby_info.config().lobby_type, LobbyType::AllBot);
        assert_eq!(lobby_info.config().capacity, 4);
    }

    #[tokio::test]
    async fn test_create_general_lobby() {
        let manager = create_test_manager().await;

        let lobby_id = manager.create_new_lobby(LobbyType::General).await.unwrap();
        let lobby_info = manager.get_lobby_info(lobby_id).await.unwrap().unwrap();

        assert_eq!(lobby_info.config().lobby_type, LobbyType::General);
        assert_eq!(lobby_info.config().capacity, 4);
        assert!(lobby_info.config().allow_bot_backfill);
    }

    #[tokio::test]
    async fn test_full_lobby_game_start() {
        let manager = create_test_manager().await;

        // Add 4 bots to AllBot lobby
        let mut lobby_id = None;
        for i in 1..=4 {
            let request =
                create_test_queue_request(&format!("bot{}", i), PlayerType::Bot, LobbyType::AllBot);
            lobby_id = Some(manager.handle_queue_request(request).await.unwrap());
        }

        let lobby_id = lobby_id.unwrap();
        let lobby_info = manager.get_lobby_info(lobby_id).await.unwrap().unwrap();

        // Lobby should be in Starting state
        assert_eq!(lobby_info.state(), LobbyState::Starting);
        assert!(lobby_info.is_full());
    }

    #[tokio::test]
    async fn test_player_removal() {
        let manager = create_test_manager().await;

        let request = create_test_queue_request("human1", PlayerType::Human, LobbyType::General);
        let lobby_id = manager.handle_queue_request(request).await.unwrap();

        // Verify player was added
        let lobby_info = manager.get_lobby_info(lobby_id).await.unwrap().unwrap();
        assert_eq!(lobby_info.get_players().len(), 1);

        // Remove player
        let removed_from = manager.remove_player("human1").await.unwrap();
        assert_eq!(removed_from, Some(lobby_id));

        // Verify player was removed
        let lobby_info = manager.get_lobby_info(lobby_id).await.unwrap().unwrap();
        assert_eq!(lobby_info.get_players().len(), 0);
    }

    #[tokio::test]
    async fn test_lobby_matching() {
        let manager = create_test_manager().await;

        // Create first bot in AllBot lobby
        let request1 = create_test_queue_request("bot1", PlayerType::Bot, LobbyType::AllBot);
        let lobby_id1 = manager.handle_queue_request(request1).await.unwrap();

        // Create second bot - should join same lobby
        let request2 = create_test_queue_request("bot2", PlayerType::Bot, LobbyType::AllBot);
        let lobby_id2 = manager.handle_queue_request(request2).await.unwrap();

        assert_eq!(lobby_id1, lobby_id2);

        let lobby_info = manager.get_lobby_info(lobby_id1).await.unwrap().unwrap();
        assert_eq!(lobby_info.get_players().len(), 2);
    }

    #[tokio::test]
    async fn test_human_bot_priority_in_general_lobby() {
        let manager = create_test_manager().await;

        // Add bot first
        let bot_request = create_test_queue_request("bot1", PlayerType::Bot, LobbyType::General);
        let lobby_id = manager.handle_queue_request(bot_request).await.unwrap();

        // Add human second
        let human_request =
            create_test_queue_request("human1", PlayerType::Human, LobbyType::General);
        let same_lobby_id = manager.handle_queue_request(human_request).await.unwrap();

        assert_eq!(lobby_id, same_lobby_id);

        let lobby_info = manager.get_lobby_info(lobby_id).await.unwrap().unwrap();
        let players = lobby_info.get_players();

        // Human should be first due to priority
        assert_eq!(players.len(), 2);
        assert_eq!(players[0].player_type, PlayerType::Human);
        assert_eq!(players[1].player_type, PlayerType::Bot);
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let manager = create_test_manager().await;

        // Create and handle multiple requests
        let request1 = create_test_queue_request("player1", PlayerType::Human, LobbyType::General);
        let request2 = create_test_queue_request("bot1", PlayerType::Bot, LobbyType::AllBot);

        let _lobby1 = manager.handle_queue_request(request1).await.unwrap();
        let _lobby2 = manager.handle_queue_request(request2).await.unwrap();

        // Check stats
        let stats = manager.get_stats().await.unwrap();
        assert_eq!(stats.players_queued, 2);
        assert_eq!(stats.lobbies_created, 2);
        assert!(stats.active_lobbies >= 2);
    }

    #[tokio::test]
    async fn test_metrics_integration() {
        use crate::metrics::MetricsCollector;

        // Create a lobby manager with explicit metrics collector
        let lobby_provider = Arc::new(StaticLobbyProvider::new());
        let event_publisher = Arc::new(MockEventPublisher::new());
        let metrics_collector = Arc::new(MetricsCollector::new().unwrap());

        let manager =
            LobbyManager::with_metrics(lobby_provider, event_publisher, metrics_collector.clone());

        // Handle a queue request
        let request = create_test_queue_request("player1", PlayerType::Human, LobbyType::General);
        let _lobby_id = manager.handle_queue_request(request).await.unwrap();

        // Verify metrics were recorded by checking that the registry has some metrics
        let registry = metrics_collector.registry();
        let metric_families = registry.gather();

        // Should have several metric families now
        assert!(!metric_families.is_empty(), "Metrics should be recorded");

        // Look for specific metrics
        let metric_names: Vec<String> = metric_families
            .iter()
            .map(|mf| mf.get_name().to_string())
            .collect();

        // Should have lobby-related metrics
        assert!(
            metric_names
                .iter()
                .any(|name| name.contains("lobbies") || name.contains("active_lobbies")),
            "Should have lobby metrics, found: {:?}",
            metric_names
        );

        // Should have player-related metrics
        assert!(
            metric_names.iter().any(|name| name.contains("players")),
            "Should have player metrics, found: {:?}",
            metric_names
        );
    }
}
