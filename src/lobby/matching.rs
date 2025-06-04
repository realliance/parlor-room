//! Lobby matching algorithms for finding suitable lobbies
//!
//! This module handles the logic for matching players to existing lobbies
//! or determining when new lobbies should be created.

use crate::error::Result;
use crate::lobby::instance::{Lobby, LobbyState};
use crate::rating::weng_lin::{ExtendedWengLinConfig, WengLinRatingCalculator};
use crate::types::{LobbyId, LobbyType, Player, PlayerType};

/// Result of a lobby matching operation
#[derive(Debug, Clone)]
pub enum MatchingResult {
    /// Player was matched to an existing lobby
    MatchedToLobby(LobbyId),
    /// No suitable lobby found, should create new one
    CreateNewLobby,
    /// Player should wait for better match
    ShouldWait { reason: String },
}

/// Configuration for lobby matching behavior
#[derive(Debug, Clone)]
pub struct MatchingConfig {
    /// Maximum rating difference allowed for matching
    pub max_rating_difference: f64,
    /// Whether to prefer lobbies with same player types
    pub prefer_same_player_types: bool,
    /// Minimum score threshold to wait for better matches
    pub wait_threshold: f64,
}

impl Default for MatchingConfig {
    fn default() -> Self {
        Self {
            max_rating_difference: 300.0, // ±300 rating points
            prefer_same_player_types: false,
            wait_threshold: 30.0, // Minimum score to join a lobby
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

/// Rating-based lobby matcher implementation
///
/// This matcher focuses on:
/// - Player type compatibility
/// - Lobby availability (not full, not started)
/// - Rating-based compatibility using Weng-Lin algorithm
/// - Smart waiting logic for better matches
#[derive(Debug)]
pub struct RatingBasedLobbyMatcher {
    rating_calculator: WengLinRatingCalculator,
}

impl RatingBasedLobbyMatcher {
    pub fn new() -> Self {
        Self {
            rating_calculator: WengLinRatingCalculator::new(ExtendedWengLinConfig::default())
                .expect("Failed to create rating calculator"),
        }
    }

    pub fn with_rating_config(config: ExtendedWengLinConfig) -> Result<Self> {
        Ok(Self {
            rating_calculator: WengLinRatingCalculator::new(config)?,
        })
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
        // Simple wait time scoring based on lobby state
        // Just prefer lobbies that are ready to start
        match lobby.state() {
            LobbyState::ReadyToStart => 20.0,
            LobbyState::WaitingForPlayers => 10.0,
            _ => 0.0,
        }
    }

    /// Calculate rating compatibility score using Weng-Lin algorithm
    fn rating_compatibility_score(
        &self,
        player: &Player,
        lobby: &dyn Lobby,
        config: &MatchingConfig,
    ) -> f64 {
        let lobby_players = lobby.get_players();
        if lobby_players.is_empty() {
            return 50.0; // High neutral score for empty lobby
        }

        // Extract all player ratings including the lobby players
        let all_ratings: Vec<_> = lobby_players
            .iter()
            .map(|p| p.rating.clone())
            .chain(std::iter::once(player.rating.clone()))
            .collect();

        // Calculate match quality for the entire potential lobby
        let match_quality = self.rating_calculator.calculate_match_quality(&all_ratings);

        // Convert match quality (0.0-1.0) to compatibility score (0.0-100.0)
        let base_score = match_quality * 100.0;

        // Check individual compatibility with each player in lobby
        let opponent_ratings: Vec<_> = lobby_players.iter().map(|p| p.rating.clone()).collect();
        let expected_score = self
            .rating_calculator
            .calculate_expected_score(&player.rating, &opponent_ratings);

        // Bonus for balanced expected scores (closer to 0.5)
        let balance_bonus = (1.0 - (expected_score - 0.5).abs() * 2.0) * 20.0;

        // Check rating compatibility using tolerance
        let max_uncertainty_units = config.max_rating_difference / 200.0; // Convert to uncertainty units
        let all_compatible = opponent_ratings.iter().all(|opp_rating| {
            self.rating_calculator.are_players_compatible(
                &player.rating,
                opp_rating,
                max_uncertainty_units,
            )
        });

        let final_score = base_score + balance_bonus;

        if all_compatible {
            final_score
        } else {
            // Heavily penalize incompatible ratings
            final_score - 50.0
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
        let mut best_lobby: Option<(LobbyId, f64)> = None;

        for lobby in available_lobbies {
            if !self.can_player_join_lobby(player, *lobby) {
                continue;
            }

            let score = self.calculate_lobby_score(player, *lobby, config);

            // Consider waiting if no good matches found
            if score < config.wait_threshold && !lobby.get_players().is_empty() {
                continue;
            }

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
                // Check if we should wait for better match or create new lobby
                if !available_lobbies.is_empty()
                    && available_lobbies
                        .iter()
                        .any(|l| !l.get_players().is_empty())
                {
                    Ok(MatchingResult::ShouldWait {
                        reason: format!("No suitable lobby found with rating compatibility. Player rating: {:.1}", player.rating.rating)
                    })
                } else {
                    Ok(MatchingResult::CreateNewLobby)
                }
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

        // Rating compatibility using Weng-Lin algorithm (high weight since this is our primary focus)
        total_score += self.rating_compatibility_score(player, lobby, config) * 1.2;

        total_score
    }

    fn can_player_join_lobby(&self, player: &Player, lobby: &dyn Lobby) -> bool {
        self.is_lobby_eligible(player, lobby) && !lobby.is_full()
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
    fn test_matcher_allbot_lobby() {
        let matcher = RatingBasedLobbyMatcher::new();
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
    fn test_matcher_general_lobby() {
        let matcher = RatingBasedLobbyMatcher::new();
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
        let matcher = RatingBasedLobbyMatcher::new();
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
        let matcher = RatingBasedLobbyMatcher::new();

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
        let matcher = RatingBasedLobbyMatcher::new();
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
        let matcher = RatingBasedLobbyMatcher::new();

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
    fn test_rating_compatibility() {
        let matcher = RatingBasedLobbyMatcher::new();
        let config = MatchingConfig::default();

        let player = create_test_player("player1", PlayerType::Human, 1500.0);
        let similar_player = create_test_player("player2", PlayerType::Human, 1520.0);
        let different_player = create_test_player("player3", PlayerType::Human, 2000.0);

        let mut similar_lobby = LobbyInstance::new(LobbyConfiguration::general());
        let mut different_lobby = LobbyInstance::new(LobbyConfiguration::general());

        similar_lobby.add_player(similar_player).unwrap();
        different_lobby.add_player(different_player).unwrap();

        let similar_score = matcher.calculate_lobby_score(&player, &similar_lobby, &config);
        let different_score = matcher.calculate_lobby_score(&player, &different_lobby, &config);

        // Should prefer lobby with similar rating
        assert!(similar_score > different_score);
    }
}
