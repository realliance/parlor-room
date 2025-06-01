//! Lobby matching algorithms for finding suitable lobbies
//!
//! This module handles the logic for matching players to existing lobbies
//! or determining when new lobbies should be created.

use crate::error::{MatchmakingError, Result};
use crate::lobby::instance::{Lobby, LobbyState};
use crate::types::{LobbyId, LobbyType, Player, PlayerType};
use crate::utils::rating_difference;

/// Result of a lobby matching operation
#[derive(Debug, Clone)]
pub enum MatchingResult {
    /// Player was matched to an existing lobby
    MatchedToLobby(LobbyId),
    /// No suitable lobby found, should create new one
    CreateNewLobby,
    /// Player should wait for better match (used for future rating-based matching)
    ShouldWait { reason: String },
}

/// Configuration for lobby matching behavior
#[derive(Debug, Clone)]
pub struct MatchingConfig {
    /// Maximum rating difference allowed for matching (for future Phase 3)
    pub max_rating_difference: f64,
    /// Whether to use strict rating matching
    pub strict_rating_matching: bool,
    /// Whether to prefer lobbies with same player types
    pub prefer_same_player_types: bool,
}

impl Default for MatchingConfig {
    fn default() -> Self {
        Self {
            max_rating_difference: 300.0,  // ±300 rating points
            strict_rating_matching: false, // Disabled for Phase 2
            prefer_same_player_types: false,
        }
    }
}

/// Trait for lobby matching algorithms
pub trait LobbyMatcher: Send + Sync {
    /// Find the best lobby for a player to join
    fn find_lobby_for_player(
        &self,
        player: &Player,
        available_lobbies: &[&dyn Lobby],
        config: &MatchingConfig,
    ) -> Result<MatchingResult>;

    /// Score how well a player fits in a lobby (higher = better fit)
    fn calculate_lobby_score(
        &self,
        player: &Player,
        lobby: &dyn Lobby,
        config: &MatchingConfig,
    ) -> f64;

    /// Check if a player can join a specific lobby
    fn can_player_join_lobby(&self, player: &Player, lobby: &dyn Lobby) -> bool;
}

/// Basic lobby matcher implementation for Phase 2
///
/// This matcher focuses on:
/// - Player type compatibility
/// - Lobby availability (not full, not started)
/// - Basic priority ordering (humans first in general lobbies)
#[derive(Debug, Default)]
pub struct BasicLobbyMatcher;

impl BasicLobbyMatcher {
    pub fn new() -> Self {
        Self
    }

    /// Check basic eligibility for joining a lobby
    fn is_lobby_eligible(&self, player: &Player, lobby: &dyn Lobby) -> bool {
        // Check if lobby accepts this player type
        if !lobby.accepts_player_type(player.player_type) {
            return false;
        }

        // Check lobby state - only allow joining if waiting for players or ready to start
        match lobby.state() {
            LobbyState::WaitingForPlayers | LobbyState::ReadyToStart => true,
            LobbyState::Starting | LobbyState::GameStarted | LobbyState::Abandoned => false,
        }
    }

    /// Calculate preference score for lobby type matching
    fn lobby_type_score(&self, player: &Player, lobby: &dyn Lobby) -> f64 {
        match (player.player_type, lobby.config().lobby_type) {
            // Perfect matches
            (PlayerType::Bot, LobbyType::AllBot) => 100.0,
            (PlayerType::Human, LobbyType::General) => 90.0,
            (PlayerType::Bot, LobbyType::General) => 80.0,
            // Humans can't join AllBot lobbies
            (PlayerType::Human, LobbyType::AllBot) => 0.0,
        }
    }

    /// Calculate fullness score (prefer lobbies closer to being full)
    fn fullness_score(&self, lobby: &dyn Lobby) -> f64 {
        let players = lobby.get_players().len();
        let capacity = lobby.config().capacity;

        if players == 0 {
            return 10.0; // New lobby
        }

        // Prefer lobbies that are closer to full but not completely full
        let fullness_ratio = players as f64 / capacity as f64;

        if fullness_ratio >= 1.0 {
            0.0 // Full lobby
        } else {
            // Score increases as lobby gets fuller
            50.0 + (fullness_ratio * 40.0)
        }
    }

    /// Calculate wait time score (prefer lobbies that haven't been waiting too long)
    fn wait_time_score(&self, lobby: &dyn Lobby) -> f64 {
        // For Phase 2, we don't have detailed wait time tracking
        // Just prefer lobbies that are ready to start
        match lobby.state() {
            LobbyState::ReadyToStart => 20.0,
            LobbyState::WaitingForPlayers => 10.0,
            _ => 0.0,
        }
    }

    /// Calculate rating compatibility score (placeholder for Phase 3)
    fn rating_score(&self, _player: &Player, _lobby: &dyn Lobby, _config: &MatchingConfig) -> f64 {
        // For Phase 2, rating matching is not implemented
        // Return neutral score
        0.0
    }
}

impl LobbyMatcher for BasicLobbyMatcher {
    fn find_lobby_for_player(
        &self,
        player: &Player,
        available_lobbies: &[&dyn Lobby],
        config: &MatchingConfig,
    ) -> Result<MatchingResult> {
        let mut best_lobby: Option<(LobbyId, f64)> = None;

        for lobby in available_lobbies {
            if !self.can_player_join_lobby(player, *lobby) {
                continue;
            }

            let score = self.calculate_lobby_score(player, *lobby, config);

            if let Some((_, best_score)) = best_lobby {
                if score > best_score {
                    best_lobby = Some((lobby.lobby_id(), score));
                }
            } else {
                best_lobby = Some((lobby.lobby_id(), score));
            }
        }

        match best_lobby {
            Some((lobby_id, score)) if score > 0.0 => Ok(MatchingResult::MatchedToLobby(lobby_id)),
            _ => {
                // No suitable lobby found, should create new one
                Ok(MatchingResult::CreateNewLobby)
            }
        }
    }

    fn calculate_lobby_score(
        &self,
        player: &Player,
        lobby: &dyn Lobby,
        config: &MatchingConfig,
    ) -> f64 {
        if !self.is_lobby_eligible(player, lobby) {
            return 0.0;
        }

        let mut total_score = 0.0;

        // Player type compatibility (most important)
        total_score += self.lobby_type_score(player, lobby) * 1.0;

        // Lobby fullness (prefer fuller lobbies)
        total_score += self.fullness_score(lobby) * 0.8;

        // Wait time considerations
        total_score += self.wait_time_score(lobby) * 0.6;

        // Rating compatibility (placeholder for Phase 3)
        total_score += self.rating_score(player, lobby, config) * 0.4;

        total_score
    }

    fn can_player_join_lobby(&self, player: &Player, lobby: &dyn Lobby) -> bool {
        self.is_lobby_eligible(player, lobby) && !lobby.is_full()
    }
}

/// Advanced lobby matcher with rating-based matching (for future phases)
#[derive(Debug)]
pub struct RatingBasedLobbyMatcher {
    basic_matcher: BasicLobbyMatcher,
}

impl RatingBasedLobbyMatcher {
    pub fn new() -> Self {
        Self {
            basic_matcher: BasicLobbyMatcher::new(),
        }
    }

    /// Calculate rating compatibility score
    fn rating_compatibility_score(
        &self,
        player: &Player,
        lobby: &dyn Lobby,
        config: &MatchingConfig,
    ) -> f64 {
        if !config.strict_rating_matching {
            return 0.0; // Disabled
        }

        let lobby_players = lobby.get_players();
        if lobby_players.is_empty() {
            return 50.0; // Neutral score for empty lobby
        }

        // Calculate average rating of players in lobby
        let total_rating: f64 = lobby_players.iter().map(|p| p.rating.rating).sum();
        let avg_rating = total_rating / lobby_players.len() as f64;

        let rating_diff = rating_difference(player.rating.rating, avg_rating);

        if rating_diff <= config.max_rating_difference {
            // Good match - closer ratings get higher scores
            let normalized_diff = rating_diff / config.max_rating_difference;
            50.0 * (1.0 - normalized_diff)
        } else {
            // Rating difference too large
            0.0
        }
    }
}

impl Default for RatingBasedLobbyMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl LobbyMatcher for RatingBasedLobbyMatcher {
    fn find_lobby_for_player(
        &self,
        player: &Player,
        available_lobbies: &[&dyn Lobby],
        config: &MatchingConfig,
    ) -> Result<MatchingResult> {
        // Use basic matcher as fallback for Phase 2
        self.basic_matcher
            .find_lobby_for_player(player, available_lobbies, config)
    }

    fn calculate_lobby_score(
        &self,
        player: &Player,
        lobby: &dyn Lobby,
        config: &MatchingConfig,
    ) -> f64 {
        if !self.can_player_join_lobby(player, lobby) {
            return 0.0;
        }

        // Start with basic score
        let mut score = self
            .basic_matcher
            .calculate_lobby_score(player, lobby, config);

        // Add rating-based scoring if enabled
        if config.strict_rating_matching {
            score += self.rating_compatibility_score(player, lobby, config) * 1.2;
        }

        score
    }

    fn can_player_join_lobby(&self, player: &Player, lobby: &dyn Lobby) -> bool {
        self.basic_matcher.can_player_join_lobby(player, lobby)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lobby::instance::LobbyInstance;
    use crate::lobby::provider::LobbyConfiguration;
    use crate::types::PlayerRating;
    use crate::utils::current_timestamp;

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

    #[test]
    fn test_basic_matcher_allbot_lobby() {
        let matcher = BasicLobbyMatcher::new();
        let config = MatchingConfig::default();

        let bot = create_test_player("bot1", PlayerType::Bot, 1500.0);
        let human = create_test_player("human1", PlayerType::Human, 1500.0);

        let lobby = LobbyInstance::new(LobbyConfiguration::all_bot());
        let lobbies: Vec<&dyn Lobby> = vec![&lobby];

        // Bot should be able to join AllBot lobby
        let bot_result = matcher
            .find_lobby_for_player(&bot, &lobbies, &config)
            .unwrap();
        assert!(matches!(bot_result, MatchingResult::MatchedToLobby(_)));

        // Human should not be able to join AllBot lobby
        let human_result = matcher
            .find_lobby_for_player(&human, &lobbies, &config)
            .unwrap();
        assert!(matches!(human_result, MatchingResult::CreateNewLobby));
    }

    #[test]
    fn test_basic_matcher_general_lobby() {
        let matcher = BasicLobbyMatcher::new();
        let config = MatchingConfig::default();

        let bot = create_test_player("bot1", PlayerType::Bot, 1500.0);
        let human = create_test_player("human1", PlayerType::Human, 1500.0);

        let lobby = LobbyInstance::new(LobbyConfiguration::general());
        let lobbies: Vec<&dyn Lobby> = vec![&lobby];

        // Both should be able to join General lobby
        let bot_result = matcher
            .find_lobby_for_player(&bot, &lobbies, &config)
            .unwrap();
        assert!(matches!(bot_result, MatchingResult::MatchedToLobby(_)));

        let human_result = matcher
            .find_lobby_for_player(&human, &lobbies, &config)
            .unwrap();
        assert!(matches!(human_result, MatchingResult::MatchedToLobby(_)));
    }

    #[test]
    fn test_lobby_scoring() {
        let matcher = BasicLobbyMatcher::new();
        let config = MatchingConfig::default();

        let bot = create_test_player("bot1", PlayerType::Bot, 1500.0);

        let empty_allbot_lobby = LobbyInstance::new(LobbyConfiguration::all_bot());
        let empty_general_lobby = LobbyInstance::new(LobbyConfiguration::general());

        let allbot_score = matcher.calculate_lobby_score(&bot, &empty_allbot_lobby, &config);
        let general_score = matcher.calculate_lobby_score(&bot, &empty_general_lobby, &config);

        // Bot should prefer AllBot lobby over General lobby
        assert!(allbot_score > general_score);
    }

    #[test]
    fn test_can_player_join_lobby() {
        let matcher = BasicLobbyMatcher::new();

        let bot = create_test_player("bot1", PlayerType::Bot, 1500.0);
        let human = create_test_player("human1", PlayerType::Human, 1500.0);

        let allbot_lobby = LobbyInstance::new(LobbyConfiguration::all_bot());
        let general_lobby = LobbyInstance::new(LobbyConfiguration::general());

        // Bot can join both lobby types
        assert!(matcher.can_player_join_lobby(&bot, &allbot_lobby));
        assert!(matcher.can_player_join_lobby(&bot, &general_lobby));

        // Human can only join General lobby
        assert!(!matcher.can_player_join_lobby(&human, &allbot_lobby));
        assert!(matcher.can_player_join_lobby(&human, &general_lobby));
    }

    #[test]
    fn test_no_available_lobbies() {
        let matcher = BasicLobbyMatcher::new();
        let config = MatchingConfig::default();

        let bot = create_test_player("bot1", PlayerType::Bot, 1500.0);
        let lobbies: Vec<&dyn Lobby> = vec![];

        let result = matcher
            .find_lobby_for_player(&bot, &lobbies, &config)
            .unwrap();
        assert!(matches!(result, MatchingResult::CreateNewLobby));
    }

    #[test]
    fn test_fullness_scoring() {
        let matcher = BasicLobbyMatcher::new();

        let empty_lobby = LobbyInstance::new(LobbyConfiguration::general());
        let mut partial_lobby = LobbyInstance::new(LobbyConfiguration::general());

        // Add some players to partial lobby
        let player1 = create_test_player("player1", PlayerType::Human, 1500.0);
        partial_lobby.add_player(player1).unwrap();

        let empty_score = matcher.fullness_score(&empty_lobby);
        let partial_score = matcher.fullness_score(&partial_lobby);

        // Partially filled lobby should score higher than empty lobby
        assert!(partial_score > empty_score);
    }

    #[test]
    fn test_rating_based_matcher_fallback() {
        let matcher = RatingBasedLobbyMatcher::new();
        let config = MatchingConfig::default();

        let bot = create_test_player("bot1", PlayerType::Bot, 1500.0);
        let lobby = LobbyInstance::new(LobbyConfiguration::all_bot());
        let lobbies: Vec<&dyn Lobby> = vec![&lobby];

        // Should work the same as basic matcher when rating matching is disabled
        let result = matcher
            .find_lobby_for_player(&bot, &lobbies, &config)
            .unwrap();
        assert!(matches!(result, MatchingResult::MatchedToLobby(_)));
    }
}
