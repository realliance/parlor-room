//! Utility functions for the matchmaking service

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Generate a new unique lobby ID
pub fn generate_lobby_id() -> Uuid {
    Uuid::new_v4()
}

/// Generate a new unique game ID
pub fn generate_game_id() -> Uuid {
    Uuid::new_v4()
}

/// Get the current UTC timestamp
pub fn current_timestamp() -> DateTime<Utc> {
    Utc::now()
}

/// Calculate the absolute difference between two ratings
pub fn rating_difference(rating1: f64, rating2: f64) -> f64 {
    (rating1 - rating2).abs()
}

/// Check if two ratings are within the given tolerance
pub fn ratings_within_tolerance(rating1: f64, rating2: f64, tolerance: f64) -> bool {
    rating_difference(rating1, rating2) <= tolerance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_unique_ids() {
        let id1 = generate_lobby_id();
        let id2 = generate_lobby_id();
        assert_ne!(id1, id2);

        let game_id1 = generate_game_id();
        let game_id2 = generate_game_id();
        assert_ne!(game_id1, game_id2);
    }

    #[test]
    fn test_rating_difference() {
        assert_eq!(rating_difference(1500.0, 1400.0), 100.0);
        assert_eq!(rating_difference(1400.0, 1500.0), 100.0);
        assert_eq!(rating_difference(1500.0, 1500.0), 0.0);
    }

    #[test]
    fn test_ratings_within_tolerance() {
        assert!(ratings_within_tolerance(1500.0, 1450.0, 100.0));
        assert!(!ratings_within_tolerance(1500.0, 1350.0, 100.0));
        assert!(ratings_within_tolerance(1500.0, 1500.0, 0.0));
    }
}
