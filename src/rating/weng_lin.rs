//! Weng-Lin (OpenSkill) rating system implementation
//!
//! This module provides a concrete implementation of the rating calculator
//! using the Weng-Lin algorithm from the skillratings crate.

use crate::rating::calculator::{RatingCalculationResult, RatingCalculator};
use crate::types::{PlayerId, PlayerRating, RatingChange};
use serde::{Deserialize, Serialize};
use skillratings::weng_lin::{weng_lin_multi_team, WengLinConfig, WengLinRating};
use skillratings::MultiTeamOutcome;
use std::collections::HashMap;
use tracing::warn;

/// Extended configuration for the Weng-Lin rating system
/// This wraps the skillratings WengLinConfig with additional parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedWengLinConfig {
    /// Core Weng-Lin parameters
    pub weng_lin_config: WengLinConfig,
    /// Initial rating for new players
    pub initial_rating: f64,
    /// Initial uncertainty for new players
    pub initial_uncertainty: f64,
}

impl Default for ExtendedWengLinConfig {
    fn default() -> Self {
        Self {
            weng_lin_config: WengLinConfig {
                beta: 200.0,
                uncertainty_tolerance: 0.0001,
            },
            initial_rating: 1500.0,
            initial_uncertainty: 200.0,
        }
    }
}

impl ExtendedWengLinConfig {
    /// Create conservative configuration (slower rating changes)
    pub fn conservative() -> Self {
        Self {
            weng_lin_config: WengLinConfig {
                beta: 150.0,
                uncertainty_tolerance: 0.00001,
            },
            initial_rating: 1500.0,
            initial_uncertainty: 150.0,
        }
    }

    /// Create aggressive configuration (faster rating changes)
    pub fn aggressive() -> Self {
        Self {
            weng_lin_config: WengLinConfig {
                beta: 250.0,
                uncertainty_tolerance: 0.001,
            },
            initial_rating: 1500.0,
            initial_uncertainty: 250.0,
        }
    }

    /// Validate configuration parameters
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.weng_lin_config.beta <= 0.0 {
            return Err(crate::error::MatchmakingError::ConfigurationError {
                message: "Beta must be positive".to_string(),
            }
            .into());
        }

        if self.weng_lin_config.uncertainty_tolerance < 0.0 {
            return Err(crate::error::MatchmakingError::ConfigurationError {
                message: "Uncertainty tolerance must be non-negative".to_string(),
            }
            .into());
        }

        if self.initial_uncertainty <= 0.0 {
            return Err(crate::error::MatchmakingError::ConfigurationError {
                message: "Initial uncertainty must be positive".to_string(),
            }
            .into());
        }

        Ok(())
    }
}

/// Weng-Lin rating calculator implementation
#[derive(Debug)]
pub struct WengLinRatingCalculator {
    config: ExtendedWengLinConfig,
}

impl WengLinRatingCalculator {
    /// Create a new Weng-Lin rating calculator
    pub fn new(config: ExtendedWengLinConfig) -> crate::error::Result<Self> {
        config.validate()?;

        Ok(Self { config })
    }

    /// Get default rating for new players
    pub fn default_rating(&self) -> PlayerRating {
        PlayerRating {
            rating: self.config.initial_rating,
            uncertainty: self.config.initial_uncertainty,
        }
    }

    /// Calculate expected score for a player against other players
    pub fn calculate_expected_score(
        &self,
        player_rating: &PlayerRating,
        opponent_ratings: &[PlayerRating],
    ) -> f64 {
        if opponent_ratings.is_empty() {
            return 0.5; // Neutral expectation when no opponents
        }

        let player_weng_lin: WengLinRating = player_rating.clone().into();
        let opponent_weng_lins: Vec<WengLinRating> =
            opponent_ratings.iter().map(|r| r.clone().into()).collect();

        // Calculate win probability against each opponent and average
        let mut total_expected = 0.0;
        for opponent in &opponent_weng_lins {
            // Use the skillratings expected_score function with config
            let (expected_win, _expected_draw) = skillratings::weng_lin::expected_score(
                &player_weng_lin,
                opponent,
                &self.config.weng_lin_config,
            );
            total_expected += expected_win;
        }

        total_expected / opponent_weng_lins.len() as f64
    }

    /// Check if players are within rating range for matching
    pub fn are_players_compatible(
        &self,
        player1: &PlayerRating,
        player2: &PlayerRating,
        max_uncertainty_units: f64,
    ) -> bool {
        let rating_diff = (player1.rating - player2.rating).abs();
        let combined_uncertainty = player1.uncertainty + player2.uncertainty;
        let tolerance = max_uncertainty_units * combined_uncertainty / 2.0;

        rating_diff <= tolerance
    }

    /// Get quality score for a match (0.0 to 1.0, higher is better)
    pub fn calculate_match_quality(&self, player_ratings: &[PlayerRating]) -> f64 {
        if player_ratings.len() < 2 {
            return 0.0;
        }

        let weng_lin_ratings: Vec<WengLinRating> =
            player_ratings.iter().map(|r| r.clone().into()).collect();

        // Calculate quality based on rating distribution
        let ratings: Vec<f64> = weng_lin_ratings.iter().map(|r| r.rating).collect();
        let mean_rating = ratings.iter().sum::<f64>() / ratings.len() as f64;

        // Calculate variance
        let variance = ratings
            .iter()
            .map(|r| (r - mean_rating).powi(2))
            .sum::<f64>()
            / ratings.len() as f64;

        let std_dev = variance.sqrt();

        // Quality is inversely related to standard deviation
        // Normalize to 0-1 range where low std_dev = high quality
        let max_reasonable_std_dev = self.config.weng_lin_config.beta; // Use beta as reference
        let quality = 1.0 - (std_dev / max_reasonable_std_dev).min(1.0);

        quality.max(0.0)
    }
}

impl RatingCalculator for WengLinRatingCalculator {
    fn calculate_rating_changes(
        &self,
        players: &[(PlayerId, PlayerRating)],
        rankings: &[(PlayerId, u32)], // (player_id, rank) where 1 = first place
    ) -> crate::error::Result<RatingCalculationResult> {
        if players.is_empty() {
            return Err(crate::error::MatchmakingError::InvalidQueueRequest {
                reason: "No players provided for rating calculation".to_string(),
            }
            .into());
        }

        if rankings.is_empty() {
            return Err(crate::error::MatchmakingError::InvalidQueueRequest {
                reason: "No rankings provided for rating calculation".to_string(),
            }
            .into());
        }

        // Create ranking map
        let ranking_map: HashMap<PlayerId, u32> = rankings.iter().cloned().collect();

        // Ensure all players have rankings
        for (player_id, _) in players {
            if !ranking_map.contains_key(player_id) {
                return Err(crate::error::MatchmakingError::InvalidQueueRequest {
                    reason: format!("No ranking provided for player {}", player_id),
                }
                .into());
            }
        }

        // Prepare data for skillratings calculation
        let mut teams_with_outcomes = Vec::new();
        for (player_id, rating) in players {
            let rank = ranking_map[player_id];
            let team = vec![rating.clone().into()];
            let outcome = MultiTeamOutcome::new(rank as usize); // Convert u32 to usize
            teams_with_outcomes.push((team, outcome));
        }

        // Convert to the format expected by weng_lin_multi_team
        let teams_refs: Vec<(&[WengLinRating], MultiTeamOutcome)> = teams_with_outcomes
            .iter()
            .map(|(team, outcome)| (team.as_slice(), *outcome))
            .collect();

        // Calculate new ratings using Weng-Lin
        let new_ratings_result = weng_lin_multi_team(&teams_refs, &self.config.weng_lin_config);

        // Process results
        let mut rating_changes = Vec::new();
        for (i, (player_id, old_rating)) in players.iter().enumerate() {
            if i >= new_ratings_result.len() {
                warn!("Missing rating result for player {}", player_id);
                continue;
            }

            let new_weng_lin_rating = &new_ratings_result[i][0]; // First (and only) player in team
            let new_rating: PlayerRating = (*new_weng_lin_rating).into();
            let rank = ranking_map[player_id];

            let change = RatingChange {
                player_id: player_id.clone(),
                old_rating: old_rating.clone(),
                new_rating: new_rating.clone(),
                rank,
            };

            rating_changes.push(change);
        }

        Ok(RatingCalculationResult {
            rating_changes,
            match_quality: self.calculate_match_quality(
                &players.iter().map(|(_, r)| r.clone()).collect::<Vec<_>>(),
            ),
        })
    }

    fn get_initial_rating(&self) -> PlayerRating {
        self.default_rating()
    }

    fn config(&self) -> serde_json::Value {
        serde_json::to_value(&self.config).unwrap_or(serde_json::Value::Null)
    }

    fn update_config(&mut self, config: serde_json::Value) -> crate::error::Result<()> {
        let new_config: ExtendedWengLinConfig = serde_json::from_value(config).map_err(|e| {
            crate::error::MatchmakingError::ConfigurationError {
                message: format!("Invalid Weng-Lin configuration: {}", e),
            }
        })?;

        new_config.validate()?;
        self.config = new_config;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extended_weng_lin_config_default() {
        let config = ExtendedWengLinConfig::default();
        assert_eq!(config.initial_rating, 1500.0);
        assert_eq!(config.initial_uncertainty, 200.0);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_extended_weng_lin_config_validation() {
        let mut config = ExtendedWengLinConfig::default();

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Invalid beta
        config.weng_lin_config.beta = -1.0;
        assert!(config.validate().is_err());

        // Invalid uncertainty tolerance
        config = ExtendedWengLinConfig::default();
        config.weng_lin_config.uncertainty_tolerance = -1.0;
        assert!(config.validate().is_err());

        // Invalid initial uncertainty
        config = ExtendedWengLinConfig::default();
        config.initial_uncertainty = 0.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_calculator_creation() {
        let config = ExtendedWengLinConfig::default();
        let calculator = WengLinRatingCalculator::new(config).unwrap();

        let default_rating = calculator.get_initial_rating();
        assert_eq!(default_rating.rating, 1500.0);
        assert_eq!(default_rating.uncertainty, 200.0);
    }

    #[test]
    fn test_rating_calculation_two_players() {
        let config = ExtendedWengLinConfig::default();
        let calculator = WengLinRatingCalculator::new(config).unwrap();

        let players = vec![
            (
                "player1".to_string(),
                PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
            ),
            (
                "player2".to_string(),
                PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
            ),
        ];

        let rankings = vec![
            ("player1".to_string(), 1), // Winner
            ("player2".to_string(), 2), // Loser
        ];

        let result = calculator
            .calculate_rating_changes(&players, &rankings)
            .unwrap();

        assert_eq!(result.rating_changes.len(), 2);

        // Winner should gain rating
        let winner_change = &result.rating_changes[0];
        assert_eq!(winner_change.player_id, "player1");
        assert!(winner_change.new_rating.rating > winner_change.old_rating.rating);
        assert_eq!(winner_change.rank, 1);

        // Loser should lose rating
        let loser_change = &result.rating_changes[1];
        assert_eq!(loser_change.player_id, "player2");
        assert!(loser_change.new_rating.rating < loser_change.old_rating.rating);
        assert_eq!(loser_change.rank, 2);

        // Match quality should be high for equal ratings
        assert!(result.match_quality > 0.5);
    }

    #[test]
    fn test_rating_calculation_four_players() {
        let config = ExtendedWengLinConfig::default();
        let calculator = WengLinRatingCalculator::new(config).unwrap();

        let players = vec![
            (
                "player1".to_string(),
                PlayerRating {
                    rating: 1600.0,
                    uncertainty: 150.0,
                },
            ),
            (
                "player2".to_string(),
                PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
            ),
            (
                "player3".to_string(),
                PlayerRating {
                    rating: 1400.0,
                    uncertainty: 180.0,
                },
            ),
            (
                "player4".to_string(),
                PlayerRating {
                    rating: 1450.0,
                    uncertainty: 220.0,
                },
            ),
        ];

        let rankings = vec![
            ("player1".to_string(), 1), // 1st place
            ("player2".to_string(), 2), // 2nd place
            ("player3".to_string(), 4), // 4th place
            ("player4".to_string(), 3), // 3rd place
        ];

        let result = calculator
            .calculate_rating_changes(&players, &rankings)
            .unwrap();

        assert_eq!(result.rating_changes.len(), 4);

        // Find changes by player
        let changes: HashMap<String, &RatingChange> = result
            .rating_changes
            .iter()
            .map(|change| (change.player_id.clone(), change))
            .collect();

        // First place should gain rating
        let first_place = changes["player1"];
        assert_eq!(first_place.rank, 1);
        assert!(first_place.new_rating.rating >= first_place.old_rating.rating);

        // Last place should lose rating
        let last_place = changes["player3"];
        assert_eq!(last_place.rank, 4);
        assert!(last_place.new_rating.rating <= last_place.old_rating.rating);
    }

    #[test]
    fn test_expected_score_calculation() {
        let config = ExtendedWengLinConfig::default();
        let calculator = WengLinRatingCalculator::new(config).unwrap();

        let strong_player = PlayerRating {
            rating: 1700.0,
            uncertainty: 150.0,
        };
        let weak_player = PlayerRating {
            rating: 1300.0,
            uncertainty: 150.0,
        };
        let equal_player = PlayerRating {
            rating: 1500.0,
            uncertainty: 150.0,
        };

        // Strong player vs weak player should have high expected score
        let score_vs_weak =
            calculator.calculate_expected_score(&strong_player, &[weak_player.clone()]);
        assert!(score_vs_weak > 0.7);

        // Weak player vs strong player should have low expected score
        let score_vs_strong =
            calculator.calculate_expected_score(&weak_player, &[strong_player.clone()]);
        assert!(score_vs_strong < 0.3);

        // Equal players should have ~0.5 expected score
        let score_vs_equal =
            calculator.calculate_expected_score(&equal_player, &[equal_player.clone()]);
        assert!((score_vs_equal - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_player_compatibility() {
        let config = ExtendedWengLinConfig::default();
        let calculator = WengLinRatingCalculator::new(config).unwrap();

        let player1 = PlayerRating {
            rating: 1500.0,
            uncertainty: 150.0,
        };
        let player2 = PlayerRating {
            rating: 1520.0,
            uncertainty: 150.0,
        }; // Close rating
        let player3 = PlayerRating {
            rating: 1900.0, // Much farther rating to ensure incompatibility
            uncertainty: 150.0,
        }; // Far rating

        // Close ratings should be compatible
        assert!(calculator.are_players_compatible(&player1, &player2, 2.0));

        // Far ratings should not be compatible (400 point difference > 300 tolerance)
        assert!(!calculator.are_players_compatible(&player1, &player3, 2.0));

        // With higher tolerance, should be compatible
        assert!(calculator.are_players_compatible(&player1, &player3, 4.0));
    }

    #[test]
    fn test_match_quality() {
        let config = ExtendedWengLinConfig::default();
        let calculator = WengLinRatingCalculator::new(config).unwrap();

        // High quality match (similar ratings)
        let similar_ratings = vec![
            PlayerRating {
                rating: 1500.0,
                uncertainty: 150.0,
            },
            PlayerRating {
                rating: 1510.0,
                uncertainty: 150.0,
            },
            PlayerRating {
                rating: 1490.0,
                uncertainty: 150.0,
            },
            PlayerRating {
                rating: 1505.0,
                uncertainty: 150.0,
            },
        ];

        // Low quality match (varied ratings)
        let varied_ratings = vec![
            PlayerRating {
                rating: 1200.0,
                uncertainty: 150.0,
            },
            PlayerRating {
                rating: 1500.0,
                uncertainty: 150.0,
            },
            PlayerRating {
                rating: 1800.0,
                uncertainty: 150.0,
            },
            PlayerRating {
                rating: 1400.0,
                uncertainty: 150.0,
            },
        ];

        let high_quality = calculator.calculate_match_quality(&similar_ratings);
        let low_quality = calculator.calculate_match_quality(&varied_ratings);

        assert!(high_quality > low_quality);
        assert!(high_quality > 0.5);
        assert!(high_quality <= 1.0);
        assert!(low_quality >= 0.0);
        assert!(low_quality <= 1.0);
    }

    #[test]
    fn test_config_presets() {
        let conservative = ExtendedWengLinConfig::conservative();
        let aggressive = ExtendedWengLinConfig::aggressive();
        let default = ExtendedWengLinConfig::default();

        // Conservative should have smaller uncertainty tolerance (more stable)
        assert!(
            conservative.weng_lin_config.uncertainty_tolerance
                < default.weng_lin_config.uncertainty_tolerance
        );
        assert!(
            aggressive.weng_lin_config.uncertainty_tolerance
                > default.weng_lin_config.uncertainty_tolerance
        );

        // All should be valid
        assert!(conservative.validate().is_ok());
        assert!(aggressive.validate().is_ok());
        assert!(default.validate().is_ok());
    }

    #[test]
    fn test_invalid_inputs() {
        let config = ExtendedWengLinConfig::default();
        let calculator = WengLinRatingCalculator::new(config).unwrap();

        // No players
        let result = calculator.calculate_rating_changes(&[], &[]);
        assert!(result.is_err());

        // No rankings
        let players = vec![(
            "player1".to_string(),
            PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
        )];
        let result = calculator.calculate_rating_changes(&players, &[]);
        assert!(result.is_err());

        // Missing ranking for player
        let players = vec![
            (
                "player1".to_string(),
                PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
            ),
            (
                "player2".to_string(),
                PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
            ),
        ];
        let rankings = vec![("player1".to_string(), 1)]; // Missing player2
        let result = calculator.calculate_rating_changes(&players, &rankings);
        assert!(result.is_err());
    }
}
