//! Rating storage interface and implementations
//!
//! This module defines the interface for persisting and retrieving player ratings,
//! with both in-memory and database-ready implementations.

use crate::types::{PlayerId, PlayerRating};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// Storage entry for a player's rating with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingEntry {
    pub player_id: PlayerId,
    pub rating: PlayerRating,
    pub games_played: u64,
    pub last_updated: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl RatingEntry {
    /// Create a new rating entry for a new player
    pub fn new(player_id: PlayerId, initial_rating: PlayerRating) -> Self {
        let now = Utc::now();
        Self {
            player_id,
            rating: initial_rating,
            games_played: 0,
            last_updated: now,
            created_at: now,
        }
    }

    /// Update the rating and increment games played
    pub fn update_rating(&mut self, new_rating: PlayerRating) {
        self.rating = new_rating;
        self.games_played += 1;
        self.last_updated = Utc::now();
    }
}

/// Trait for rating storage operations
pub trait RatingStorage: Send + Sync {
    /// Get a player's rating entry
    fn get_rating(&self, player_id: &PlayerId) -> crate::error::Result<Option<RatingEntry>>;

    /// Store or update a player's rating
    fn store_rating(&self, entry: RatingEntry) -> crate::error::Result<()>;

    /// Get ratings for multiple players
    fn get_ratings(
        &self,
        player_ids: &[PlayerId],
    ) -> crate::error::Result<HashMap<PlayerId, RatingEntry>>;

    /// Store multiple rating updates atomically
    fn store_ratings(&self, entries: Vec<RatingEntry>) -> crate::error::Result<()>;

    /// Get all players with ratings (for admin/debugging)
    fn get_all_ratings(&self) -> crate::error::Result<HashMap<PlayerId, RatingEntry>>;

    /// Remove a player's rating (for GDPR compliance)
    fn remove_rating(&self, player_id: &PlayerId) -> crate::error::Result<bool>;

    /// Get players by rating range (for matchmaking)
    fn get_players_by_rating_range(
        &self,
        min_rating: f64,
        max_rating: f64,
        limit: Option<usize>,
    ) -> crate::error::Result<Vec<RatingEntry>>;

    /// Get total number of rated players
    fn get_player_count(&self) -> crate::error::Result<usize>;
}

/// In-memory rating storage implementation
#[derive(Debug)]
pub struct InMemoryRatingStorage {
    ratings: RwLock<HashMap<PlayerId, RatingEntry>>,
    max_entries: usize,
}

impl InMemoryRatingStorage {
    /// Create a new in-memory rating storage
    pub fn new(max_entries: usize) -> Self {
        Self {
            ratings: RwLock::new(HashMap::new()),
            max_entries,
        }
    }

    /// Cleanup old entries if we exceed max_entries
    fn cleanup_if_needed(&self) -> crate::error::Result<()> {
        let mut ratings =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        if ratings.len() > self.max_entries {
            // Remove oldest entries (by last_updated timestamp)
            let mut entries: Vec<_> = ratings
                .iter()
                .map(|(k, v)| (k.clone(), v.last_updated))
                .collect();
            entries.sort_by(|a, b| a.1.cmp(&b.1));

            let to_remove = ratings.len() - self.max_entries;
            for (player_id, _) in entries.into_iter().take(to_remove) {
                ratings.remove(&player_id);
            }
        }

        Ok(())
    }
}

impl Default for InMemoryRatingStorage {
    fn default() -> Self {
        Self::new(10000) // Default to 10,000 max entries
    }
}

impl RatingStorage for InMemoryRatingStorage {
    fn get_rating(&self, player_id: &PlayerId) -> crate::error::Result<Option<RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        Ok(ratings.get(player_id).cloned())
    }

    fn store_rating(&self, entry: RatingEntry) -> crate::error::Result<()> {
        let mut ratings =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        ratings.insert(entry.player_id.clone(), entry);

        drop(ratings); // Release lock before cleanup
        self.cleanup_if_needed()?;

        Ok(())
    }

    fn get_ratings(
        &self,
        player_ids: &[PlayerId],
    ) -> crate::error::Result<HashMap<PlayerId, RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        let mut result = HashMap::new();
        for player_id in player_ids {
            if let Some(entry) = ratings.get(player_id) {
                result.insert(player_id.clone(), entry.clone());
            }
        }

        Ok(result)
    }

    fn store_ratings(&self, entries: Vec<RatingEntry>) -> crate::error::Result<()> {
        let mut ratings =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        for entry in entries {
            ratings.insert(entry.player_id.clone(), entry);
        }

        drop(ratings); // Release lock before cleanup
        self.cleanup_if_needed()?;

        Ok(())
    }

    fn get_all_ratings(&self) -> crate::error::Result<HashMap<PlayerId, RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        Ok(ratings.clone())
    }

    fn remove_rating(&self, player_id: &PlayerId) -> crate::error::Result<bool> {
        let mut ratings =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        Ok(ratings.remove(player_id).is_some())
    }

    fn get_players_by_rating_range(
        &self,
        min_rating: f64,
        max_rating: f64,
        limit: Option<usize>,
    ) -> crate::error::Result<Vec<RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        let mut matching_entries: Vec<RatingEntry> = ratings
            .values()
            .filter(|entry| {
                let rating = entry.rating.rating;
                rating >= min_rating && rating <= max_rating
            })
            .cloned()
            .collect();

        // Sort by rating (descending)
        matching_entries.sort_by(|a, b| {
            b.rating
                .rating
                .partial_cmp(&a.rating.rating)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(limit) = limit {
            matching_entries.truncate(limit);
        }

        Ok(matching_entries)
    }

    fn get_player_count(&self) -> crate::error::Result<usize> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        Ok(ratings.len())
    }
}

/// Mock rating storage for testing
#[derive(Debug, Default)]
pub struct MockRatingStorage {
    ratings: RwLock<HashMap<PlayerId, RatingEntry>>,
    store_calls: RwLock<Vec<RatingEntry>>,
}

impl MockRatingStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all store calls made (for testing)
    pub fn get_store_calls(&self) -> Vec<RatingEntry> {
        self.store_calls
            .read()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    /// Clear store calls (for testing)
    pub fn clear_store_calls(&self) {
        if let Ok(mut calls) = self.store_calls.write() {
            calls.clear();
        }
    }

    /// Preset ratings for testing
    pub fn preset_ratings(
        &self,
        ratings: HashMap<PlayerId, RatingEntry>,
    ) -> crate::error::Result<()> {
        let mut storage =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        *storage = ratings;
        Ok(())
    }
}

impl RatingStorage for MockRatingStorage {
    fn get_rating(&self, player_id: &PlayerId) -> crate::error::Result<Option<RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        Ok(ratings.get(player_id).cloned())
    }

    fn store_rating(&self, entry: RatingEntry) -> crate::error::Result<()> {
        // Record the call for testing
        if let Ok(mut calls) = self.store_calls.write() {
            calls.push(entry.clone());
        }

        let mut ratings =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        ratings.insert(entry.player_id.clone(), entry);
        Ok(())
    }

    fn get_ratings(
        &self,
        player_ids: &[PlayerId],
    ) -> crate::error::Result<HashMap<PlayerId, RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        let mut result = HashMap::new();
        for player_id in player_ids {
            if let Some(entry) = ratings.get(player_id) {
                result.insert(player_id.clone(), entry.clone());
            }
        }

        Ok(result)
    }

    fn store_ratings(&self, entries: Vec<RatingEntry>) -> crate::error::Result<()> {
        // Record the calls for testing
        if let Ok(mut calls) = self.store_calls.write() {
            calls.extend(entries.clone());
        }

        let mut ratings =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        for entry in entries {
            ratings.insert(entry.player_id.clone(), entry);
        }

        Ok(())
    }

    fn get_all_ratings(&self) -> crate::error::Result<HashMap<PlayerId, RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        Ok(ratings.clone())
    }

    fn remove_rating(&self, player_id: &PlayerId) -> crate::error::Result<bool> {
        let mut ratings =
            self.ratings
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings write lock".to_string(),
                })?;

        Ok(ratings.remove(player_id).is_some())
    }

    fn get_players_by_rating_range(
        &self,
        min_rating: f64,
        max_rating: f64,
        limit: Option<usize>,
    ) -> crate::error::Result<Vec<RatingEntry>> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        let mut matching_entries: Vec<RatingEntry> = ratings
            .values()
            .filter(|entry| {
                let rating = entry.rating.rating;
                rating >= min_rating && rating <= max_rating
            })
            .cloned()
            .collect();

        matching_entries.sort_by(|a, b| {
            b.rating
                .rating
                .partial_cmp(&a.rating.rating)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(limit) = limit {
            matching_entries.truncate(limit);
        }

        Ok(matching_entries)
    }

    fn get_player_count(&self) -> crate::error::Result<usize> {
        let ratings =
            self.ratings
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire ratings read lock".to_string(),
                })?;

        Ok(ratings.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PlayerRating;

    fn create_test_rating_entry(player_id: &str, rating: f64) -> RatingEntry {
        RatingEntry::new(
            player_id.to_string(),
            PlayerRating {
                rating,
                uncertainty: 200.0,
            },
        )
    }

    #[test]
    fn test_rating_entry_creation() {
        let entry = create_test_rating_entry("player1", 1500.0);
        assert_eq!(entry.player_id, "player1");
        assert_eq!(entry.rating.rating, 1500.0);
        assert_eq!(entry.games_played, 0);
    }

    #[test]
    fn test_rating_entry_update() {
        let mut entry = create_test_rating_entry("player1", 1500.0);
        let original_updated_time = entry.last_updated;

        let new_rating = PlayerRating {
            rating: 1550.0,
            uncertainty: 180.0,
        };
        entry.update_rating(new_rating);

        assert_eq!(entry.rating.rating, 1550.0);
        assert_eq!(entry.rating.uncertainty, 180.0);
        assert_eq!(entry.games_played, 1);
        assert!(entry.last_updated > original_updated_time);
    }

    #[test]
    fn test_in_memory_storage_basic_operations() {
        let storage = InMemoryRatingStorage::new(100);
        let entry = create_test_rating_entry("player1", 1500.0);

        // Initially no rating
        assert!(storage
            .get_rating(&"player1".to_string())
            .unwrap()
            .is_none());

        // Store rating
        storage.store_rating(entry.clone()).unwrap();

        // Should be retrievable now
        let retrieved = storage.get_rating(&"player1".to_string()).unwrap().unwrap();
        assert_eq!(retrieved.player_id, "player1");
        assert_eq!(retrieved.rating.rating, 1500.0);
    }

    #[test]
    fn test_bulk_operations() {
        let storage = InMemoryRatingStorage::new(100);

        let entries = vec![
            create_test_rating_entry("player1", 1500.0),
            create_test_rating_entry("player2", 1600.0),
            create_test_rating_entry("player3", 1400.0),
        ];

        // Store multiple ratings
        storage.store_ratings(entries).unwrap();

        // Get multiple ratings
        let player_ids = vec![
            "player1".to_string(),
            "player2".to_string(),
            "player3".to_string(),
        ];
        let retrieved = storage.get_ratings(&player_ids).unwrap();

        assert_eq!(retrieved.len(), 3);
        assert!(retrieved.contains_key("player1"));
        assert!(retrieved.contains_key("player2"));
        assert!(retrieved.contains_key("player3"));
    }

    #[test]
    fn test_rating_range_query() {
        let storage = InMemoryRatingStorage::new(100);

        let entries = vec![
            create_test_rating_entry("player1", 1400.0),
            create_test_rating_entry("player2", 1500.0),
            create_test_rating_entry("player3", 1600.0),
            create_test_rating_entry("player4", 1700.0),
        ];

        storage.store_ratings(entries).unwrap();

        // Query for ratings between 1450 and 1650
        let in_range = storage
            .get_players_by_rating_range(1450.0, 1650.0, None)
            .unwrap();

        assert_eq!(in_range.len(), 2);
        // Should be sorted by rating (descending)
        assert_eq!(in_range[0].rating.rating, 1600.0);
        assert_eq!(in_range[1].rating.rating, 1500.0);
    }

    #[test]
    fn test_player_removal() {
        let storage = InMemoryRatingStorage::new(100);
        let entry = create_test_rating_entry("player1", 1500.0);

        storage.store_rating(entry).unwrap();
        assert!(storage
            .get_rating(&"player1".to_string())
            .unwrap()
            .is_some());

        // Remove player
        let removed = storage.remove_rating(&"player1".to_string()).unwrap();
        assert!(removed);

        // Should be gone now
        assert!(storage
            .get_rating(&"player1".to_string())
            .unwrap()
            .is_none());

        // Removing non-existent player should return false
        let not_removed = storage.remove_rating(&"nonexistent".to_string()).unwrap();
        assert!(!not_removed);
    }

    #[test]
    fn test_max_entries_cleanup() {
        let storage = InMemoryRatingStorage::new(2); // Very small limit

        let entries = vec![
            create_test_rating_entry("player1", 1500.0),
            create_test_rating_entry("player2", 1600.0),
            create_test_rating_entry("player3", 1700.0),
        ];

        storage.store_ratings(entries).unwrap();

        let count = storage.get_player_count().unwrap();
        assert!(count <= 2); // Should have cleaned up to max entries
    }

    #[test]
    fn test_mock_storage() {
        let storage = MockRatingStorage::new();
        let entry = create_test_rating_entry("player1", 1500.0);

        storage.store_rating(entry.clone()).unwrap();

        // Should be able to retrieve
        let retrieved = storage.get_rating(&"player1".to_string()).unwrap().unwrap();
        assert_eq!(retrieved.player_id, "player1");

        // Should have recorded the store call
        let calls = storage.get_store_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].player_id, "player1");
    }
}
