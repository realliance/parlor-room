//! Lobby instance implementation and lifecycle management
//!
//! This module contains the core lobby logic for managing players,
//! state transitions, and lobby behavior.

use crate::error::{MatchmakingError, Result};
use crate::lobby::provider::LobbyConfiguration;
use crate::types::{LobbyId, LobbyType, Player, PlayerType};
use crate::utils::{current_timestamp, generate_lobby_id};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Possible states of a lobby
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LobbyState {
    /// Lobby is waiting for players to join
    WaitingForPlayers,
    /// Lobby has enough players but waiting for start conditions
    ReadyToStart,
    /// Lobby is full and game is starting
    Starting,
    /// Lobby has been abandoned or timed out
    Abandoned,
    /// Lobby game has started (terminal state)
    GameStarted,
}

/// Queue entry that preserves join order and priority
#[derive(Debug, Clone)]
struct QueueEntry {
    player: Player,
    joined_at: DateTime<Utc>,
    priority: u32, // Lower numbers = higher priority
}

impl QueueEntry {
    fn new(player: Player, prioritize_humans: bool) -> Self {
        let priority = if prioritize_humans && player.player_type == PlayerType::Human {
            1 // High priority for humans
        } else {
            2 // Normal priority for bots
        };

        Self {
            player,
            joined_at: current_timestamp(),
            priority,
        }
    }
}

/// Core trait for lobby behavior
pub trait Lobby: Send + Sync {
    /// Add a player to the lobby
    fn add_player(&mut self, player: Player) -> Result<bool>;

    /// Remove a player from the lobby
    fn remove_player(&mut self, player_id: &str) -> Result<Option<Player>>;

    /// Check if the lobby is full
    fn is_full(&self) -> bool;

    /// Check if the lobby should start the game
    fn should_start(&self) -> bool;

    /// Get current players in the lobby
    fn get_players(&self) -> Vec<Player>;

    /// Get lobby state
    fn state(&self) -> LobbyState;

    /// Get lobby ID
    fn lobby_id(&self) -> LobbyId;

    /// Get lobby configuration
    fn config(&self) -> &LobbyConfiguration;

    /// Check if lobby accepts this player type
    fn accepts_player_type(&self, player_type: PlayerType) -> bool;

    /// Get number of players by type
    fn player_count_by_type(&self, player_type: PlayerType) -> usize;

    /// Check if lobby has been waiting too long and needs backfill
    fn needs_backfill(&self) -> bool;

    /// Mark lobby as starting
    fn mark_starting(&mut self) -> Result<()>;

    /// Mark lobby as game started
    fn mark_game_started(&mut self) -> Result<()>;

    /// Check if lobby should be cleaned up
    fn should_cleanup(&self) -> bool;

    /// Get lobby ID (convenience method)
    fn id(&self) -> LobbyId {
        self.lobby_id()
    }

    /// Get creation timestamp
    fn created_at(&self) -> Option<DateTime<Utc>>;
}

/// Concrete implementation of a lobby instance
#[derive(Debug, Clone)]
pub struct LobbyInstance {
    id: LobbyId,
    config: LobbyConfiguration,
    state: LobbyState,
    player_queue: VecDeque<QueueEntry>,
    created_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    wait_timeout: Option<DateTime<Utc>>,
}

impl LobbyInstance {
    /// Create a new lobby instance with default ID
    pub fn new(config: LobbyConfiguration) -> Self {
        let now = current_timestamp();
        Self {
            id: generate_lobby_id(),
            config,
            state: LobbyState::WaitingForPlayers,
            player_queue: VecDeque::new(),
            created_at: now,
            last_activity: now,
            wait_timeout: None,
        }
    }

    /// Create a lobby instance with specific ID
    pub fn with_id(id: LobbyId, config: LobbyConfiguration) -> Self {
        let now = current_timestamp();
        Self {
            id,
            config,
            state: LobbyState::WaitingForPlayers,
            player_queue: VecDeque::new(),
            created_at: now,
            last_activity: now,
            wait_timeout: None,
        }
    }

    /// Get lobby ID
    pub fn id(&self) -> LobbyId {
        self.id
    }

    /// Get created timestamp
    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        Some(self.created_at)
    }

    /// Set wait timeout (for testing)
    #[cfg(test)]
    pub fn set_wait_timeout(&mut self, timeout: Option<DateTime<Utc>>) {
        self.wait_timeout = timeout;
    }

    /// Update last activity timestamp
    fn update_activity(&mut self) {
        self.last_activity = current_timestamp();
    }

    /// Set wait timeout if not already set
    fn set_wait_timeout_if_needed(&mut self) {
        if self.wait_timeout.is_none() && !self.player_queue.is_empty() {
            let timeout =
                self.last_activity + Duration::seconds(self.config.base_wait_time_seconds as i64);
            self.wait_timeout = Some(timeout);
        }
    }

    /// Sort queue by priority and join time
    fn sort_queue(&mut self) {
        let mut queue_vec: Vec<_> = self.player_queue.drain(..).collect();
        queue_vec.sort_by(|a, b| {
            // First by priority (lower = higher priority)
            a.priority
                .cmp(&b.priority)
                // Then by join time (earlier = higher priority)
                .then(a.joined_at.cmp(&b.joined_at))
        });
        self.player_queue = queue_vec.into();
    }

    /// Get players ready to play (up to capacity)
    fn get_active_players(&self) -> Vec<Player> {
        self.player_queue
            .iter()
            .take(self.config.capacity)
            .map(|entry| entry.player.clone())
            .collect()
    }

    /// Count humans in active players
    fn active_human_count(&self) -> usize {
        self.get_active_players()
            .iter()
            .filter(|p| p.player_type == PlayerType::Human)
            .count()
    }

    /// Check if minimum human requirement is met
    fn meets_human_requirement(&self) -> bool {
        self.active_human_count() >= self.config.min_human_players
    }

    /// Update lobby state based on current conditions
    fn update_state(&mut self) {
        match self.state {
            LobbyState::WaitingForPlayers => {
                if self.is_full() && self.meets_human_requirement() {
                    if self.config.immediate_start_when_full {
                        self.state = LobbyState::ReadyToStart;
                    }
                }
            }
            LobbyState::ReadyToStart => {
                // Can transition back to waiting if players leave
                if !self.is_full() || !self.meets_human_requirement() {
                    self.state = LobbyState::WaitingForPlayers;
                }
            }
            LobbyState::Starting | LobbyState::GameStarted | LobbyState::Abandoned => {
                // Terminal states - no transitions
            }
        }
    }
}

impl Lobby for LobbyInstance {
    fn add_player(&mut self, player: Player) -> Result<bool> {
        // Check state
        if matches!(
            self.state,
            LobbyState::Starting | LobbyState::GameStarted | LobbyState::Abandoned
        ) {
            return Err(MatchmakingError::LobbyFull {
                lobby_id: self.id.to_string(),
            }
            .into());
        }

        // Check if player type is allowed
        if !self.accepts_player_type(player.player_type) {
            return Err(MatchmakingError::InvalidQueueRequest {
                reason: format!(
                    "Player type {:?} not allowed in {:?} lobby",
                    player.player_type, self.config.lobby_type
                ),
            }
            .into());
        }

        // Check if player already exists
        if self
            .player_queue
            .iter()
            .any(|entry| entry.player.id == player.id)
        {
            return Err(MatchmakingError::InvalidQueueRequest {
                reason: format!("Player {} already in lobby", player.id),
            }
            .into());
        }

        // Check capacity - allow queueing beyond capacity for priority ordering
        if self.player_queue.len() >= self.config.capacity * 2 {
            return Err(MatchmakingError::LobbyFull {
                lobby_id: self.id.to_string(),
            }
            .into());
        }

        // Add player to queue
        let entry = QueueEntry::new(player, self.config.prioritize_humans);
        self.player_queue.push_back(entry);

        // Sort queue to maintain priority order
        self.sort_queue();

        // Set wait timeout if this is the first player
        self.set_wait_timeout_if_needed();

        self.update_activity();
        self.update_state();

        Ok(true)
    }

    fn remove_player(&mut self, player_id: &str) -> Result<Option<Player>> {
        let initial_len = self.player_queue.len();

        let mut removed_player = None;
        self.player_queue.retain(|entry| {
            if entry.player.id == player_id {
                removed_player = Some(entry.player.clone());
                false
            } else {
                true
            }
        });

        if removed_player.is_some() {
            self.update_activity();

            // Clear wait timeout if lobby is now empty
            if self.player_queue.is_empty() {
                self.wait_timeout = None;
            }

            self.update_state();
        }

        Ok(removed_player)
    }

    fn is_full(&self) -> bool {
        self.player_queue.len() >= self.config.capacity
    }

    fn should_start(&self) -> bool {
        match self.state {
            LobbyState::ReadyToStart => self.is_full() && self.meets_human_requirement(),
            _ => false,
        }
    }

    fn get_players(&self) -> Vec<Player> {
        self.get_active_players()
    }

    fn state(&self) -> LobbyState {
        self.state.clone()
    }

    fn lobby_id(&self) -> LobbyId {
        self.id
    }

    fn config(&self) -> &LobbyConfiguration {
        &self.config
    }

    fn accepts_player_type(&self, player_type: PlayerType) -> bool {
        self.config.allows_player_type(player_type)
    }

    fn player_count_by_type(&self, player_type: PlayerType) -> usize {
        self.get_active_players()
            .iter()
            .filter(|p| p.player_type == player_type)
            .count()
    }

    fn needs_backfill(&self) -> bool {
        // Only General lobbies support backfill
        if self.config.lobby_type != LobbyType::General || !self.config.allow_bot_backfill {
            return false;
        }

        // Check if we have a timeout and it has passed
        if let Some(timeout) = self.wait_timeout {
            if current_timestamp() > timeout {
                // Need backfill if we're not full and have humans waiting
                return !self.is_full() && self.active_human_count() > 0;
            }
        }

        false
    }

    fn mark_starting(&mut self) -> Result<()> {
        if !self.should_start() {
            return Err(MatchmakingError::InternalError {
                message: "Lobby is not ready to start".to_string(),
            }
            .into());
        }

        self.state = LobbyState::Starting;
        self.update_activity();
        Ok(())
    }

    fn mark_game_started(&mut self) -> Result<()> {
        if self.state != LobbyState::Starting {
            return Err(MatchmakingError::InternalError {
                message: "Lobby must be in Starting state to mark as game started".to_string(),
            }
            .into());
        }

        self.state = LobbyState::GameStarted;
        self.update_activity();
        Ok(())
    }

    fn should_cleanup(&self) -> bool {
        // Cleanup if abandoned or inactive for too long
        match self.state {
            LobbyState::Abandoned => true,
            LobbyState::GameStarted => true, // Can cleanup after game started
            _ => {
                // Clean up if no activity for 30 minutes
                let inactive_duration = current_timestamp() - self.last_activity;
                inactive_duration > Duration::minutes(30)
            }
        }
    }

    fn created_at(&self) -> Option<DateTime<Utc>> {
        Some(self.created_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PlayerRating;

    fn create_test_player(id: &str, player_type: PlayerType) -> Player {
        Player {
            id: id.to_string(),
            player_type,
            rating: PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            joined_at: current_timestamp(),
        }
    }

    #[test]
    fn test_lobby_instance_creation() {
        let config = LobbyConfiguration::all_bot();
        let lobby = LobbyInstance::new(config.clone());

        assert_eq!(lobby.state(), LobbyState::WaitingForPlayers);
        assert_eq!(lobby.config().lobby_type, LobbyType::AllBot);
        assert!(!lobby.is_full());
        assert!(!lobby.should_start());
        assert!(lobby.get_players().is_empty());
    }

    #[test]
    fn test_add_player_to_allbot_lobby() {
        let config = LobbyConfiguration::all_bot();
        let mut lobby = LobbyInstance::new(config);

        let bot = create_test_player("bot1", PlayerType::Bot);
        assert!(lobby.add_player(bot.clone()).is_ok());

        assert_eq!(lobby.get_players().len(), 1);
        assert_eq!(lobby.get_players()[0].id, "bot1");

        // Should reject humans
        let human = create_test_player("human1", PlayerType::Human);
        assert!(lobby.add_player(human).is_err());
    }

    #[test]
    fn test_add_player_to_general_lobby() {
        let config = LobbyConfiguration::general();
        let mut lobby = LobbyInstance::new(config);

        let human = create_test_player("human1", PlayerType::Human);
        assert!(lobby.add_player(human.clone()).is_ok());

        let bot = create_test_player("bot1", PlayerType::Bot);
        assert!(lobby.add_player(bot.clone()).is_ok());

        assert_eq!(lobby.get_players().len(), 2);

        // Human should be first due to priority
        assert_eq!(lobby.get_players()[0].player_type, PlayerType::Human);
        assert_eq!(lobby.get_players()[1].player_type, PlayerType::Bot);
    }

    #[test]
    fn test_lobby_full_and_ready() {
        let config = LobbyConfiguration::all_bot();
        let mut lobby = LobbyInstance::new(config);

        // Add 4 bots
        for i in 1..=4 {
            let bot = create_test_player(&format!("bot{}", i), PlayerType::Bot);
            assert!(lobby.add_player(bot).is_ok());
        }

        assert!(lobby.is_full());
        assert_eq!(lobby.state(), LobbyState::ReadyToStart);
        assert!(lobby.should_start());
    }

    #[test]
    fn test_remove_player() {
        let config = LobbyConfiguration::general();
        let mut lobby = LobbyInstance::new(config);

        let human = create_test_player("human1", PlayerType::Human);
        lobby.add_player(human.clone()).unwrap();

        let removed = lobby.remove_player("human1").unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, "human1");
        assert!(lobby.get_players().is_empty());

        // Removing non-existent player
        let not_removed = lobby.remove_player("nonexistent").unwrap();
        assert!(not_removed.is_none());
    }

    #[test]
    fn test_human_priority_in_general_lobby() {
        let config = LobbyConfiguration::general();
        let mut lobby = LobbyInstance::new(config);

        // Add bot first
        let bot = create_test_player("bot1", PlayerType::Bot);
        lobby.add_player(bot).unwrap();

        // Add human second
        let human = create_test_player("human1", PlayerType::Human);
        lobby.add_player(human).unwrap();

        let players = lobby.get_players();
        // Human should be first despite joining second
        assert_eq!(players[0].player_type, PlayerType::Human);
        assert_eq!(players[1].player_type, PlayerType::Bot);
    }

    #[test]
    fn test_needs_backfill() {
        let config = LobbyConfiguration::general();
        let mut lobby = LobbyInstance::new(config);

        // Initially no backfill needed
        assert!(!lobby.needs_backfill());

        // Add a human
        let human = create_test_player("human1", PlayerType::Human);
        lobby.add_player(human).unwrap();

        // Still no backfill needed (within wait time)
        assert!(!lobby.needs_backfill());

        // Manually set timeout to past time to simulate timeout
        lobby.wait_timeout = Some(current_timestamp() - chrono::Duration::seconds(1));

        // Now should need backfill
        assert!(lobby.needs_backfill());

        // Fill lobby completely
        for i in 1..=3 {
            let bot = create_test_player(&format!("bot{}", i), PlayerType::Bot);
            lobby.add_player(bot).unwrap();
        }

        // No backfill needed when full
        assert!(!lobby.needs_backfill());
    }

    #[test]
    fn test_duplicate_player_rejection() {
        let config = LobbyConfiguration::general();
        let mut lobby = LobbyInstance::new(config);

        let human = create_test_player("human1", PlayerType::Human);
        assert!(lobby.add_player(human.clone()).is_ok());

        // Adding same player again should fail
        assert!(lobby.add_player(human).is_err());
    }

    #[test]
    fn test_lobby_state_transitions() {
        let config = LobbyConfiguration::all_bot();
        let mut lobby = LobbyInstance::new(config);

        assert_eq!(lobby.state(), LobbyState::WaitingForPlayers);

        // Add players until full
        for i in 1..=4 {
            let bot = create_test_player(&format!("bot{}", i), PlayerType::Bot);
            lobby.add_player(bot).unwrap();
        }

        assert_eq!(lobby.state(), LobbyState::ReadyToStart);

        // Mark as starting
        lobby.mark_starting().unwrap();
        assert_eq!(lobby.state(), LobbyState::Starting);

        // Mark as game started
        lobby.mark_game_started().unwrap();
        assert_eq!(lobby.state(), LobbyState::GameStarted);
    }
}
