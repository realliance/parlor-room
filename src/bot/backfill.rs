//! Automatic bot backfilling system
//!
//! This module handles automatic addition of bots to General lobbies when
//! human wait times expire, maintaining game balance through skill matching.

use crate::bot::provider::{BotProvider, BotSelectionCriteria};
use crate::error::{MatchmakingError, Result};
use crate::lobby::instance::Lobby;
use crate::rating::calculator::RatingCalculator;
use crate::types::{LobbyId, LobbyType, Player, PlayerType};
use crate::wait_time::provider::WaitTimeProvider;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Configuration for bot backfilling behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillConfig {
    /// Maximum rating tolerance for bot selection (uncertainty units)
    pub max_rating_tolerance: f64,
    /// Minimum number of humans required before backfilling
    pub min_humans_for_backfill: usize,
    /// Maximum number of bots to add in a single backfill operation
    pub max_bots_per_backfill: usize,
    /// Cooldown period between backfill attempts (seconds)
    pub backfill_cooldown_seconds: u64,
    /// Enable backfilling
    pub enabled: bool,
}

impl Default for BackfillConfig {
    fn default() -> Self {
        Self {
            max_rating_tolerance: 300.0,   // 300 points tolerance
            min_humans_for_backfill: 1,    // At least 1 human required
            max_bots_per_backfill: 3,      // Maximum 3 bots per operation
            backfill_cooldown_seconds: 30, // 30 seconds between attempts
            enabled: true,
        }
    }
}

impl BackfillConfig {
    /// Create configuration for aggressive backfilling (faster lobby fills)
    pub fn aggressive() -> Self {
        Self {
            max_rating_tolerance: 500.0,
            min_humans_for_backfill: 1,
            max_bots_per_backfill: 3,
            backfill_cooldown_seconds: 15,
            enabled: true,
        }
    }

    /// Create configuration for conservative backfilling (better balance)
    pub fn conservative() -> Self {
        Self {
            max_rating_tolerance: 200.0,
            min_humans_for_backfill: 2,
            max_bots_per_backfill: 2,
            backfill_cooldown_seconds: 60,
            enabled: true,
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.max_rating_tolerance <= 0.0 {
            return Err(MatchmakingError::ConfigurationError {
                message: "max_rating_tolerance must be positive".to_string(),
            }
            .into());
        }

        if self.min_humans_for_backfill == 0 {
            return Err(MatchmakingError::ConfigurationError {
                message: "min_humans_for_backfill must be at least 1".to_string(),
            }
            .into());
        }

        if self.max_bots_per_backfill == 0 {
            return Err(MatchmakingError::ConfigurationError {
                message: "max_bots_per_backfill must be at least 1".to_string(),
            }
            .into());
        }

        Ok(())
    }
}

/// Trigger condition for bot backfilling
#[derive(Debug, Clone, PartialEq)]
pub enum BackfillTrigger {
    /// Wait time has expired for the lobby
    WaitTimeExpired,
    /// Manual trigger (for testing or admin control)
    Manual,
    /// Lobby has been waiting too long with humans
    ExtendedWait,
}

/// Result of a backfill operation
#[derive(Debug, Clone)]
pub struct BackfillResult {
    /// Lobby that was backfilled
    pub lobby_id: LobbyId,
    /// Bots that were added
    pub added_bots: Vec<Player>,
    /// Trigger condition that caused the backfill
    pub trigger: BackfillTrigger,
    /// Timestamp of the operation
    pub timestamp: DateTime<Utc>,
    /// Whether the lobby is now full
    pub lobby_full: bool,
}

/// Trait for bot backfilling operations
#[async_trait]
pub trait BackfillManager: Send + Sync {
    /// Check if a lobby needs backfilling
    async fn needs_backfill(&self, lobby: &dyn Lobby) -> Result<Option<BackfillTrigger>>;

    /// Perform backfill operation on a lobby
    async fn backfill_lobby(
        &self,
        lobby_id: LobbyId,
        lobby: &mut dyn Lobby,
        trigger: BackfillTrigger,
    ) -> Result<BackfillResult>;

    /// Get current backfill configuration
    fn config(&self) -> &BackfillConfig;

    /// Update backfill configuration
    fn update_config(&mut self, config: BackfillConfig) -> Result<()>;

    /// Get statistics about backfill operations
    fn get_stats(&self) -> BackfillStats;
}

/// Statistics for backfill operations
#[derive(Debug, Clone, Default)]
pub struct BackfillStats {
    pub total_backfills: u64,
    pub successful_backfills: u64,
    pub failed_backfills: u64,
    pub bots_added: u64,
    pub lobbies_filled: u64,
    pub last_backfill: Option<DateTime<Utc>>,
}

/// Default implementation of backfill manager
pub struct DefaultBackfillManager {
    config: BackfillConfig,
    bot_provider: Arc<dyn BotProvider>,
    wait_time_provider: Arc<dyn WaitTimeProvider>,
    rating_calculator: Arc<dyn RatingCalculator>,
    stats: BackfillStats,
    last_backfill_times: std::collections::HashMap<LobbyId, DateTime<Utc>>,
}

impl DefaultBackfillManager {
    /// Create a new default backfill manager
    pub fn new(
        config: BackfillConfig,
        bot_provider: Arc<dyn BotProvider>,
        wait_time_provider: Arc<dyn WaitTimeProvider>,
        rating_calculator: Arc<dyn RatingCalculator>,
    ) -> Result<Self> {
        config.validate()?;

        Ok(Self {
            config,
            bot_provider,
            wait_time_provider,
            rating_calculator,
            stats: BackfillStats::default(),
            last_backfill_times: std::collections::HashMap::new(),
        })
    }

    /// Calculate average rating of humans in the lobby
    fn calculate_target_rating(&self, lobby: &dyn Lobby) -> Option<f64> {
        let players = lobby.get_players();
        let human_ratings: Vec<f64> = players
            .iter()
            .filter(|p| p.player_type == PlayerType::Human)
            .map(|p| p.rating.rating)
            .collect();

        if human_ratings.is_empty() {
            return None;
        }

        Some(human_ratings.iter().sum::<f64>() / human_ratings.len() as f64)
    }

    /// Calculate how many bots are needed
    fn calculate_bots_needed(&self, lobby: &dyn Lobby) -> usize {
        let current_count = lobby.get_players().len();
        let capacity = lobby.config().capacity;
        let needed = capacity.saturating_sub(current_count);

        // Respect the max bots per backfill limit
        needed.min(self.config.max_bots_per_backfill)
    }

    /// Check if cooldown period has passed
    fn is_cooldown_expired(&self, lobby_id: LobbyId) -> bool {
        match self.last_backfill_times.get(&lobby_id) {
            Some(last_time) => {
                let cooldown = Duration::seconds(self.config.backfill_cooldown_seconds as i64);
                crate::utils::current_timestamp() > *last_time + cooldown
            }
            None => true, // Never backfilled before
        }
    }

    /// Update backfill statistics
    fn update_stats(&mut self, result: &BackfillResult, success: bool) {
        self.stats.total_backfills += 1;
        if success {
            self.stats.successful_backfills += 1;
            self.stats.bots_added += result.added_bots.len() as u64;
            if result.lobby_full {
                self.stats.lobbies_filled += 1;
            }
        } else {
            self.stats.failed_backfills += 1;
        }
        self.stats.last_backfill = Some(result.timestamp);
        self.last_backfill_times
            .insert(result.lobby_id, result.timestamp);
    }
}

#[async_trait]
impl BackfillManager for DefaultBackfillManager {
    async fn needs_backfill(&self, lobby: &dyn Lobby) -> Result<Option<BackfillTrigger>> {
        // Check if backfilling is enabled
        if !self.config.enabled {
            return Ok(None);
        }

        // Only General lobbies support backfill
        if lobby.config().lobby_type != LobbyType::General {
            return Ok(None);
        }

        // Don't backfill if lobby is full
        if lobby.is_full() {
            return Ok(None);
        }

        // Check if we have enough humans
        let human_count = lobby
            .get_players()
            .iter()
            .filter(|p| p.player_type == PlayerType::Human)
            .count();

        if human_count < self.config.min_humans_for_backfill {
            return Ok(None);
        }

        // Check cooldown
        if !self.is_cooldown_expired(lobby.id()) {
            return Ok(None);
        }

        // Check if backfilling is needed based on wait time
        if lobby.needs_backfill() {
            debug!("Lobby {} needs backfill: wait time expired", lobby.id());
            return Ok(Some(BackfillTrigger::WaitTimeExpired));
        }

        // Check for extended wait (lobby has been waiting a long time)
        let wait_time = self
            .wait_time_provider
            .get_wait_time(LobbyType::General, PlayerType::Human)?;

        if let Some(created_at) = lobby.created_at() {
            let elapsed = crate::utils::current_timestamp() - created_at;
            if elapsed
                > chrono::Duration::from_std(wait_time * 2)
                    .unwrap_or(chrono::Duration::seconds(240))
            {
                debug!("Lobby {} needs backfill: extended wait time", lobby.id());
                return Ok(Some(BackfillTrigger::ExtendedWait));
            }
        }

        Ok(None)
    }

    async fn backfill_lobby(
        &self,
        lobby_id: LobbyId,
        lobby: &mut dyn Lobby,
        trigger: BackfillTrigger,
    ) -> Result<BackfillResult> {
        info!(
            "Starting backfill for lobby {} (trigger: {:?})",
            lobby_id, trigger
        );

        let mut result = BackfillResult {
            lobby_id,
            added_bots: Vec::new(),
            trigger: trigger.clone(),
            timestamp: crate::utils::current_timestamp(),
            lobby_full: false,
        };

        // Calculate how many bots we need
        let bots_needed = self.calculate_bots_needed(lobby);
        if bots_needed == 0 {
            info!("No bots needed for lobby {}", lobby_id);
            return Ok(result);
        }

        // Calculate target rating for bot selection
        let target_rating = match self.calculate_target_rating(lobby) {
            Some(rating) => rating,
            None => {
                warn!(
                    "No human players found in lobby {} for rating calculation",
                    lobby_id
                );
                return Err(MatchmakingError::InternalError {
                    message: "Cannot calculate target rating without human players".to_string(),
                }
                .into());
            }
        };

        // Get existing bot IDs to exclude from selection
        let existing_bot_ids: Vec<String> = lobby
            .get_players()
            .iter()
            .filter(|p| p.player_type == PlayerType::Bot)
            .map(|p| p.id.clone())
            .collect();

        // Create bot selection criteria
        let criteria = BotSelectionCriteria::for_rating(
            target_rating,
            self.config.max_rating_tolerance,
            bots_needed,
        )
        .excluding_bots(existing_bot_ids);

        debug!(
            "Selecting {} bots around rating {:.1} for lobby {}",
            bots_needed, target_rating, lobby_id
        );

        // Select bots
        let selected_bots = self.bot_provider.select_backfill_bots(criteria).await?;

        if selected_bots.is_empty() {
            warn!("No suitable bots available for lobby {}", lobby_id);
            return Err(MatchmakingError::InternalError {
                message: "No suitable bots available for backfilling".to_string(),
            }
            .into());
        }

        // Reserve the selected bots
        let bot_ids: Vec<String> = selected_bots.iter().map(|b| b.id.clone()).collect();
        self.bot_provider.reserve_bots(bot_ids.clone()).await?;

        // Add bots to lobby
        let mut added_bots = Vec::new();
        for bot in selected_bots {
            match lobby.add_player(bot.clone()) {
                Ok(_) => {
                    info!(
                        "Added bot {} (rating: {:.1}) to lobby {}",
                        bot.id, bot.rating.rating, lobby_id
                    );
                    added_bots.push(bot);
                }
                Err(e) => {
                    warn!("Failed to add bot {} to lobby {}: {}", bot.id, lobby_id, e);
                    // Release the bot if we couldn't add it
                    if let Err(release_err) =
                        self.bot_provider.release_bots(vec![bot.id.clone()]).await
                    {
                        warn!(
                            "Failed to release bot {} after add failure: {}",
                            bot.id, release_err
                        );
                    }
                }
            }
        }

        result.added_bots = added_bots;
        result.lobby_full = lobby.is_full();

        if result.added_bots.is_empty() {
            // Release all reserved bots if none were added
            if let Err(e) = self.bot_provider.release_bots(bot_ids).await {
                warn!("Failed to release reserved bots: {}", e);
            }
            return Err(MatchmakingError::InternalError {
                message: "Failed to add any bots to lobby".to_string(),
            }
            .into());
        }

        info!(
            "Backfill completed for lobby {}: added {} bots, lobby full: {}",
            lobby_id,
            result.added_bots.len(),
            result.lobby_full
        );

        Ok(result)
    }

    fn config(&self) -> &BackfillConfig {
        &self.config
    }

    fn update_config(&mut self, config: BackfillConfig) -> Result<()> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    fn get_stats(&self) -> BackfillStats {
        self.stats.clone()
    }
}

/// Mock backfill manager for testing
pub struct MockBackfillManager {
    config: BackfillConfig,
    should_need_backfill: bool,
    should_succeed: bool,
    stats: BackfillStats,
}

impl MockBackfillManager {
    /// Create a mock backfill manager
    pub fn new(config: BackfillConfig) -> Self {
        Self {
            config,
            should_need_backfill: true,
            should_succeed: true,
            stats: BackfillStats::default(),
        }
    }

    /// Set whether the manager should report that backfill is needed
    pub fn set_needs_backfill(&mut self, needs: bool) {
        self.should_need_backfill = needs;
    }

    /// Set whether backfill operations should succeed
    pub fn set_should_succeed(&mut self, succeed: bool) {
        self.should_succeed = succeed;
    }
}

#[async_trait]
impl BackfillManager for MockBackfillManager {
    async fn needs_backfill(&self, lobby: &dyn Lobby) -> Result<Option<BackfillTrigger>> {
        if self.should_need_backfill
            && lobby.config().lobby_type == LobbyType::General
            && !lobby.is_full()
        {
            Ok(Some(BackfillTrigger::Manual))
        } else {
            Ok(None)
        }
    }

    async fn backfill_lobby(
        &self,
        lobby_id: LobbyId,
        _lobby: &mut dyn Lobby,
        trigger: BackfillTrigger,
    ) -> Result<BackfillResult> {
        if !self.should_succeed {
            return Err(MatchmakingError::InternalError {
                message: "Mock backfill failure".to_string(),
            }
            .into());
        }

        // Create a mock result
        Ok(BackfillResult {
            lobby_id,
            added_bots: vec![Player {
                id: "mock_bot".to_string(),
                player_type: PlayerType::Bot,
                rating: crate::types::PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
                joined_at: crate::utils::current_timestamp(),
            }],
            trigger,
            timestamp: crate::utils::current_timestamp(),
            lobby_full: false,
        })
    }

    fn config(&self) -> &BackfillConfig {
        &self.config
    }

    fn update_config(&mut self, config: BackfillConfig) -> Result<()> {
        config.validate()?;
        self.config = config;
        Ok(())
    }

    fn get_stats(&self) -> BackfillStats {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::provider::MockBotProvider;
    use crate::lobby::instance::LobbyInstance;
    use crate::lobby::provider::LobbyConfiguration;
    use crate::rating::calculator::MockRatingCalculator;
    use crate::types::PlayerRating;
    use crate::utils::current_timestamp;
    use crate::wait_time::provider::MockWaitTimeProvider;

    fn create_test_player(id: &str, player_type: PlayerType, rating: f64) -> Player {
        Player {
            id: id.to_string(),
            player_type,
            rating: PlayerRating {
                rating,
                uncertainty: 200.0,
            },
            joined_at: current_timestamp(),
        }
    }

    #[tokio::test]
    async fn test_backfill_config_validation() {
        let valid_config = BackfillConfig::default();
        assert!(valid_config.validate().is_ok());

        let mut invalid_config = BackfillConfig::default();
        invalid_config.max_rating_tolerance = -1.0;
        assert!(invalid_config.validate().is_err());

        invalid_config = BackfillConfig::default();
        invalid_config.min_humans_for_backfill = 0;
        assert!(invalid_config.validate().is_err());
    }

    #[tokio::test]
    async fn test_backfill_manager_creation() {
        let config = BackfillConfig::default();
        let bot_provider = Arc::new(MockBotProvider::with_test_bots());
        let wait_time_provider = Arc::new(MockWaitTimeProvider::new(
            std::time::Duration::from_secs(120),
        ));
        let rating_calculator = Arc::new(MockRatingCalculator::new());

        let manager = DefaultBackfillManager::new(
            config,
            bot_provider,
            wait_time_provider,
            rating_calculator,
        );

        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_needs_backfill_conditions() {
        let config = BackfillConfig::default();
        let bot_provider = Arc::new(MockBotProvider::with_test_bots());
        let wait_time_provider = Arc::new(MockWaitTimeProvider::new(
            std::time::Duration::from_secs(120),
        ));
        let rating_calculator = Arc::new(MockRatingCalculator::new());

        let manager = DefaultBackfillManager::new(
            config,
            bot_provider,
            wait_time_provider,
            rating_calculator,
        )
        .unwrap();

        // Test AllBot lobby (should not need backfill)
        let allbot_lobby = LobbyInstance::new(LobbyConfiguration::all_bot());
        let needs_backfill = manager.needs_backfill(&allbot_lobby).await.unwrap();
        assert!(needs_backfill.is_none());

        // Test General lobby with no humans (should not need backfill)
        let general_lobby = LobbyInstance::new(LobbyConfiguration::general());
        let needs_backfill = manager.needs_backfill(&general_lobby).await.unwrap();
        assert!(needs_backfill.is_none());

        // Test General lobby with humans but not enough
        let mut lobby_with_humans = LobbyInstance::new(LobbyConfiguration::general());
        let human = create_test_player("human1", PlayerType::Human, 1500.0);
        lobby_with_humans.add_player(human).unwrap();

        // Make lobby appear to need backfill by setting wait timeout in the past
        lobby_with_humans
            .set_wait_timeout(Some(current_timestamp() - chrono::Duration::seconds(10)));

        let needs_backfill = manager.needs_backfill(&lobby_with_humans).await.unwrap();
        assert!(needs_backfill.is_some());
    }

    #[tokio::test]
    async fn test_mock_backfill_manager() {
        let config = BackfillConfig::default();
        let mut manager = MockBackfillManager::new(config);

        let lobby = LobbyInstance::new(LobbyConfiguration::general());

        // Test needs backfill
        let needs = manager.needs_backfill(&lobby).await.unwrap();
        assert!(needs.is_some());

        // Test successful backfill
        let mut lobby_mut = lobby;
        let result = manager
            .backfill_lobby(lobby_mut.id(), &mut lobby_mut, BackfillTrigger::Manual)
            .await
            .unwrap();

        assert_eq!(result.lobby_id, lobby_mut.id());
        assert!(!result.added_bots.is_empty());

        // Test failed backfill
        manager.set_should_succeed(false);
        let result = manager
            .backfill_lobby(lobby_mut.id(), &mut lobby_mut, BackfillTrigger::Manual)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_backfill_presets() {
        let aggressive = BackfillConfig::aggressive();
        assert_eq!(aggressive.max_rating_tolerance, 500.0);
        assert_eq!(aggressive.backfill_cooldown_seconds, 15);
        assert!(aggressive.validate().is_ok());

        let conservative = BackfillConfig::conservative();
        assert_eq!(conservative.max_rating_tolerance, 200.0);
        assert_eq!(conservative.min_humans_for_backfill, 2);
        assert!(conservative.validate().is_ok());
    }
}
