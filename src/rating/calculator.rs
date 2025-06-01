//! Rating calculator trait and implementations
//!
//! This module defines the interface for rating calculations and provides
//! basic implementations for different rating systems.

use crate::types::{PlayerId, PlayerRating, RatingChange};
use serde::{Deserialize, Serialize};

/// Result of a rating calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingCalculationResult {
    /// Rating changes for all players
    pub rating_changes: Vec<RatingChange>,
    /// Quality score of the match (0.0 to 1.0, higher is better)
    pub match_quality: f64,
}

/// Trait for calculating rating changes after games
pub trait RatingCalculator: Send + Sync {
    /// Calculate rating changes for players based on game results
    ///
    /// # Arguments
    /// * `players` - List of (player_id, current_rating) pairs
    /// * `rankings` - List of (player_id, rank) pairs where 1 = first place
    ///
    /// # Returns
    /// Result containing rating changes and match quality
    fn calculate_rating_changes(
        &self,
        players: &[(PlayerId, PlayerRating)],
        rankings: &[(PlayerId, u32)],
    ) -> crate::error::Result<RatingCalculationResult>;

    /// Get the initial rating for new players
    fn get_initial_rating(&self) -> PlayerRating;

    /// Get current configuration as JSON
    fn config(&self) -> serde_json::Value;

    /// Update configuration from JSON
    fn update_config(&mut self, config: serde_json::Value) -> crate::error::Result<()>;
}

/// Simple rating calculator for testing or fallback
#[derive(Debug, Clone)]
pub struct NoOpRatingCalculator {
    initial_rating: PlayerRating,
}

impl NoOpRatingCalculator {
    /// Create a new no-op rating calculator
    pub fn new(initial_rating: PlayerRating) -> Self {
        Self { initial_rating }
    }
}

impl Default for NoOpRatingCalculator {
    fn default() -> Self {
        Self::new(PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        })
    }
}

impl RatingCalculator for NoOpRatingCalculator {
    fn calculate_rating_changes(
        &self,
        players: &[(PlayerId, PlayerRating)],
        rankings: &[(PlayerId, u32)],
    ) -> crate::error::Result<RatingCalculationResult> {
        if players.is_empty() {
            return Err(crate::error::MatchmakingError::InvalidQueueRequest {
                reason: "No players provided for rating calculation".to_string(),
            }
            .into());
        }

        // No-op: return unchanged ratings
        let rating_changes = players
            .iter()
            .map(|(player_id, rating)| {
                // Find rank for this player
                let rank = rankings
                    .iter()
                    .find(|(id, _)| id == player_id)
                    .map(|(_, rank)| *rank)
                    .unwrap_or(1); // Default to first place if not found

                RatingChange {
                    player_id: player_id.clone(),
                    old_rating: rating.clone(),
                    new_rating: rating.clone(), // No change
                    rank,
                }
            })
            .collect();

        Ok(RatingCalculationResult {
            rating_changes,
            match_quality: 1.0, // Perfect quality for no-op
        })
    }

    fn get_initial_rating(&self) -> PlayerRating {
        self.initial_rating.clone()
    }

    fn config(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "no_op",
            "initial_rating": self.initial_rating.rating,
            "initial_uncertainty": self.initial_rating.uncertainty
        })
    }

    fn update_config(&mut self, config: serde_json::Value) -> crate::error::Result<()> {
        if let Some(rating) = config.get("initial_rating").and_then(|v| v.as_f64()) {
            self.initial_rating.rating = rating;
        }
        if let Some(uncertainty) = config.get("initial_uncertainty").and_then(|v| v.as_f64()) {
            self.initial_rating.uncertainty = uncertainty;
        }
        Ok(())
    }
}

/// Mock rating calculator for testing
#[derive(Debug, Default)]
pub struct MockRatingCalculator {
    calculation_calls: std::sync::Mutex<Vec<(Vec<(PlayerId, PlayerRating)>, Vec<(PlayerId, u32)>)>>,
    fixed_result: std::sync::RwLock<Option<RatingCalculationResult>>,
    initial_rating: PlayerRating,
}

impl MockRatingCalculator {
    pub fn new() -> Self {
        Self {
            calculation_calls: std::sync::Mutex::new(Vec::new()),
            fixed_result: std::sync::RwLock::new(None),
            initial_rating: PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
        }
    }

    /// Set a fixed result to return for all calculations
    pub fn set_fixed_result(&self, result: RatingCalculationResult) {
        if let Ok(mut fixed) = self.fixed_result.write() {
            *fixed = Some(result);
        }
    }

    /// Get all calculation calls made (for testing)
    pub fn get_calculation_calls(
        &self,
    ) -> Vec<(Vec<(PlayerId, PlayerRating)>, Vec<(PlayerId, u32)>)> {
        self.calculation_calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    /// Clear recorded calls
    pub fn clear_calls(&self) {
        if let Ok(mut calls) = self.calculation_calls.lock() {
            calls.clear();
        }
    }
}

impl RatingCalculator for MockRatingCalculator {
    fn calculate_rating_changes(
        &self,
        players: &[(PlayerId, PlayerRating)],
        rankings: &[(PlayerId, u32)],
    ) -> crate::error::Result<RatingCalculationResult> {
        // Record the call
        if let Ok(mut calls) = self.calculation_calls.lock() {
            calls.push((players.to_vec(), rankings.to_vec()));
        }

        // Return fixed result if set, otherwise create default
        if let Ok(fixed) = self.fixed_result.read() {
            if let Some(result) = fixed.as_ref() {
                return Ok(result.clone());
            }
        }

        // Default behavior: no rating change
        let rating_changes = players
            .iter()
            .map(|(player_id, rating)| {
                let rank = rankings
                    .iter()
                    .find(|(id, _)| id == player_id)
                    .map(|(_, rank)| *rank)
                    .unwrap_or(1);

                RatingChange {
                    player_id: player_id.clone(),
                    old_rating: rating.clone(),
                    new_rating: rating.clone(),
                    rank,
                }
            })
            .collect();

        Ok(RatingCalculationResult {
            rating_changes,
            match_quality: 0.8, // Default quality
        })
    }

    fn get_initial_rating(&self) -> PlayerRating {
        self.initial_rating.clone()
    }

    fn config(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "mock",
            "initial_rating": self.initial_rating.rating,
            "initial_uncertainty": self.initial_rating.uncertainty
        })
    }

    fn update_config(&mut self, config: serde_json::Value) -> crate::error::Result<()> {
        if let Some(rating) = config.get("initial_rating").and_then(|v| v.as_f64()) {
            self.initial_rating.rating = rating;
        }
        if let Some(uncertainty) = config.get("initial_uncertainty").and_then(|v| v.as_f64()) {
            self.initial_rating.uncertainty = uncertainty;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rating_calculation_result() {
        let result = RatingCalculationResult {
            rating_changes: vec![],
            match_quality: 0.8,
        };

        assert_eq!(result.rating_changes.len(), 0);
        assert_eq!(result.match_quality, 0.8);
    }

    #[test]
    fn test_noop_calculator() {
        let calculator = NoOpRatingCalculator::default();

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
                    rating: 1600.0,
                    uncertainty: 150.0,
                },
            ),
        ];

        let rankings = vec![("player1".to_string(), 1), ("player2".to_string(), 2)];

        let result = calculator
            .calculate_rating_changes(&players, &rankings)
            .unwrap();

        assert_eq!(result.rating_changes.len(), 2);
        assert_eq!(result.match_quality, 1.0);

        // Ratings should be unchanged
        assert_eq!(
            result.rating_changes[0].old_rating.rating,
            result.rating_changes[0].new_rating.rating
        );
        assert_eq!(
            result.rating_changes[1].old_rating.rating,
            result.rating_changes[1].new_rating.rating
        );

        // Ranks should be preserved
        assert_eq!(result.rating_changes[0].rank, 1);
        assert_eq!(result.rating_changes[1].rank, 2);
    }

    #[test]
    fn test_noop_calculator_config() {
        let mut calculator = NoOpRatingCalculator::default();

        let initial = calculator.get_initial_rating();
        assert_eq!(initial.rating, 1500.0);

        // Update config
        let new_config = serde_json::json!({
            "initial_rating": 1400.0,
            "initial_uncertainty": 180.0
        });

        calculator.update_config(new_config).unwrap();

        let updated = calculator.get_initial_rating();
        assert_eq!(updated.rating, 1400.0);
        assert_eq!(updated.uncertainty, 180.0);
    }

    #[test]
    fn test_mock_calculator() {
        let calculator = MockRatingCalculator::new();

        let players = vec![(
            "player1".to_string(),
            PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
        )];

        let rankings = vec![("player1".to_string(), 1)];

        // Should work with default behavior
        let result = calculator
            .calculate_rating_changes(&players, &rankings)
            .unwrap();
        assert_eq!(result.rating_changes.len(), 1);
        assert_eq!(result.match_quality, 0.8);

        // Should record the call
        let calls = calculator.get_calculation_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.len(), 1);
        assert_eq!(calls[0].1.len(), 1);
    }

    #[test]
    fn test_mock_calculator_fixed_result() {
        let calculator = MockRatingCalculator::new();

        let fixed_result = RatingCalculationResult {
            rating_changes: vec![RatingChange {
                player_id: "test".to_string(),
                old_rating: PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
                new_rating: PlayerRating {
                    rating: 1550.0,
                    uncertainty: 190.0,
                },
                rank: 1,
            }],
            match_quality: 0.95,
        };

        calculator.set_fixed_result(fixed_result.clone());

        let players = vec![(
            "player1".to_string(),
            PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
        )];

        let rankings = vec![("player1".to_string(), 1)];

        let result = calculator
            .calculate_rating_changes(&players, &rankings)
            .unwrap();
        assert_eq!(result.match_quality, 0.95);
        assert_eq!(result.rating_changes.len(), 1);
        assert_eq!(result.rating_changes[0].player_id, "test");
    }

    #[test]
    fn test_empty_players_error() {
        let calculator = NoOpRatingCalculator::default();
        let result = calculator.calculate_rating_changes(&[], &[]);
        assert!(result.is_err());
    }
}
