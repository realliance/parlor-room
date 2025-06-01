//! Bot provider interface and implementations
//!
//! This module defines the interface for bot selection, validation, and management
//! for both active queuing and automatic backfilling scenarios.

use crate::types::{Player, PlayerRating, PlayerType};
use crate::utils::current_timestamp;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// Criteria for selecting bots for backfilling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSelectionCriteria {
    /// Target rating range (min, max)
    pub rating_range: (f64, f64),
    /// Target uncertainty range (min, max)
    pub uncertainty_range: (f64, f64),
    /// Number of bots needed
    pub count: usize,
    /// Exclude these bot IDs (e.g., already in lobby)
    pub exclude_bots: Vec<String>,
    /// Preferred rating (for closest match)
    pub preferred_rating: Option<f64>,
}

impl BotSelectionCriteria {
    /// Create criteria for a specific target rating
    pub fn for_rating(target_rating: f64, uncertainty_tolerance: f64, count: usize) -> Self {
        Self {
            rating_range: (
                target_rating - uncertainty_tolerance,
                target_rating + uncertainty_tolerance,
            ),
            uncertainty_range: (50.0, 400.0), // Reasonable uncertainty bounds
            count,
            exclude_bots: Vec::new(),
            preferred_rating: Some(target_rating),
        }
    }

    /// Create criteria for rating range
    pub fn for_range(min_rating: f64, max_rating: f64, count: usize) -> Self {
        Self {
            rating_range: (min_rating, max_rating),
            uncertainty_range: (50.0, 400.0),
            count,
            exclude_bots: Vec::new(),
            preferred_rating: Some((min_rating + max_rating) / 2.0),
        }
    }

    /// Add bots to exclude from selection
    pub fn excluding_bots(mut self, bot_ids: Vec<String>) -> Self {
        self.exclude_bots = bot_ids;
        self
    }
}

/// Trait for providing and managing bots
#[async_trait]
pub trait BotProvider: Send + Sync {
    /// Select bots for automatic backfilling based on criteria
    async fn select_backfill_bots(
        &self,
        criteria: BotSelectionCriteria,
    ) -> crate::error::Result<Vec<Player>>;

    /// Validate a bot queue request (for active queuing)
    async fn validate_bot_request(
        &self,
        bot_id: &str,
        auth_token: &str,
    ) -> crate::error::Result<bool>;

    /// Get a specific bot by ID (for active queuing)
    async fn get_bot(&self, bot_id: &str) -> crate::error::Result<Option<Player>>;

    /// Mark bots as in use (reserve them)
    async fn reserve_bots(&self, bot_ids: Vec<String>) -> crate::error::Result<()>;

    /// Release reserved bots (make them available again)
    async fn release_bots(&self, bot_ids: Vec<String>) -> crate::error::Result<()>;

    /// Get available bot count
    async fn available_bot_count(&self) -> crate::error::Result<usize>;

    /// Check if a bot is available for selection
    async fn is_bot_available(&self, bot_id: &str) -> crate::error::Result<bool>;
}

/// Mock bot provider for testing and development
#[derive(Debug)]
pub struct MockBotProvider {
    /// Available bots by ID
    available_bots: RwLock<HashMap<String, Player>>,
    /// Reserved bot IDs
    reserved_bots: RwLock<Vec<String>>,
    /// Valid auth tokens for bot authentication
    valid_tokens: RwLock<HashMap<String, String>>, // bot_id -> auth_token
}

impl MockBotProvider {
    /// Create a new mock bot provider
    pub fn new() -> Self {
        Self {
            available_bots: RwLock::new(HashMap::new()),
            reserved_bots: RwLock::new(Vec::new()),
            valid_tokens: RwLock::new(HashMap::new()),
        }
    }

    /// Create a mock provider with pre-configured bots
    pub fn with_test_bots() -> Self {
        let provider = Self::new();

        // Add some test bots with varying ratings
        let test_bots = vec![
            ("testbot1", 1200.0, 180.0, "token1"),
            ("testbot2", 1400.0, 160.0, "token2"),
            ("testbot3", 1500.0, 200.0, "token3"),
            ("testbot4", 1600.0, 150.0, "token4"),
            ("testbot5", 1800.0, 170.0, "token5"),
            ("weakbot1", 1000.0, 250.0, "token6"),
            ("strongbot1", 2000.0, 120.0, "token7"),
            ("newbot1", 1500.0, 350.0, "token8"), // High uncertainty
        ];

        {
            let mut bots = provider.available_bots.write().unwrap();
            let mut tokens = provider.valid_tokens.write().unwrap();

            for (bot_id, rating, uncertainty, token) in test_bots {
                let bot = Player {
                    id: bot_id.to_string(),
                    player_type: PlayerType::Bot,
                    rating: PlayerRating {
                        rating,
                        uncertainty,
                    },
                    joined_at: current_timestamp(),
                };
                bots.insert(bot_id.to_string(), bot);
                tokens.insert(bot_id.to_string(), token.to_string());
            }
        }

        provider
    }

    /// Add a bot to the provider
    pub fn add_bot(&self, bot: Player, auth_token: String) -> crate::error::Result<()> {
        if bot.player_type != PlayerType::Bot {
            return Err(crate::error::MatchmakingError::InvalidQueueRequest {
                reason: "Player must be of type Bot".to_string(),
            }
            .into());
        }

        let mut bots = self.available_bots.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire bots write lock".to_string(),
            }
        })?;

        let mut tokens = self.valid_tokens.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire tokens write lock".to_string(),
            }
        })?;

        bots.insert(bot.id.clone(), bot.clone());
        tokens.insert(bot.id.clone(), auth_token);

        Ok(())
    }

    /// Remove a bot from the provider
    pub fn remove_bot(&self, bot_id: &str) -> crate::error::Result<bool> {
        let mut bots = self.available_bots.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire bots write lock".to_string(),
            }
        })?;

        let mut tokens = self.valid_tokens.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire tokens write lock".to_string(),
            }
        })?;

        let removed = bots.remove(bot_id).is_some();
        tokens.remove(bot_id);

        Ok(removed)
    }

    /// Get all available bots (for testing)
    pub fn get_all_bots(&self) -> Vec<Player> {
        self.available_bots
            .read()
            .map(|bots| bots.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Clear all reserved bots (for testing)
    pub fn clear_reservations(&self) {
        if let Ok(mut reserved) = self.reserved_bots.write() {
            reserved.clear();
        }
    }
}

impl Default for MockBotProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BotProvider for MockBotProvider {
    async fn select_backfill_bots(
        &self,
        criteria: BotSelectionCriteria,
    ) -> crate::error::Result<Vec<Player>> {
        let bots = self.available_bots.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire bots read lock".to_string(),
            }
        })?;

        let reserved = self.reserved_bots.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved read lock".to_string(),
            }
        })?;

        // Filter bots based on criteria
        let mut suitable_bots: Vec<Player> = bots
            .values()
            .filter(|bot| {
                // Skip reserved bots
                if reserved.contains(&bot.id) {
                    return false;
                }

                // Skip excluded bots
                if criteria.exclude_bots.contains(&bot.id) {
                    return false;
                }

                // Check rating range
                let rating = bot.rating.rating;
                if rating < criteria.rating_range.0 || rating > criteria.rating_range.1 {
                    return false;
                }

                // Check uncertainty range
                let uncertainty = bot.rating.uncertainty;
                if uncertainty < criteria.uncertainty_range.0
                    || uncertainty > criteria.uncertainty_range.1
                {
                    return false;
                }

                true
            })
            .cloned()
            .collect();

        // Sort by distance from preferred rating if specified
        if let Some(preferred_rating) = criteria.preferred_rating {
            suitable_bots.sort_by(|a, b| {
                let diff_a = (a.rating.rating - preferred_rating).abs();
                let diff_b = (b.rating.rating - preferred_rating).abs();
                diff_a
                    .partial_cmp(&diff_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Take only the requested count
        suitable_bots.truncate(criteria.count);

        Ok(suitable_bots)
    }

    async fn validate_bot_request(
        &self,
        bot_id: &str,
        auth_token: &str,
    ) -> crate::error::Result<bool> {
        let tokens = self.valid_tokens.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire tokens read lock".to_string(),
            }
        })?;

        Ok(tokens
            .get(bot_id)
            .is_some_and(|token| token == auth_token))
    }

    async fn get_bot(&self, bot_id: &str) -> crate::error::Result<Option<Player>> {
        let bots = self.available_bots.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire bots read lock".to_string(),
            }
        })?;

        Ok(bots.get(bot_id).cloned())
    }

    async fn reserve_bots(&self, bot_ids: Vec<String>) -> crate::error::Result<()> {
        let mut reserved = self.reserved_bots.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved write lock".to_string(),
            }
        })?;

        for bot_id in bot_ids {
            if !reserved.contains(&bot_id) {
                reserved.push(bot_id);
            }
        }

        Ok(())
    }

    async fn release_bots(&self, bot_ids: Vec<String>) -> crate::error::Result<()> {
        let mut reserved = self.reserved_bots.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved write lock".to_string(),
            }
        })?;

        reserved.retain(|id| !bot_ids.contains(id));

        Ok(())
    }

    async fn available_bot_count(&self) -> crate::error::Result<usize> {
        let bots = self.available_bots.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire bots read lock".to_string(),
            }
        })?;

        let reserved = self.reserved_bots.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved read lock".to_string(),
            }
        })?;

        Ok(bots.len() - reserved.len())
    }

    async fn is_bot_available(&self, bot_id: &str) -> crate::error::Result<bool> {
        let bots = self.available_bots.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire bots read lock".to_string(),
            }
        })?;

        let reserved = self.reserved_bots.read().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved read lock".to_string(),
            }
        })?;

        Ok(bots.contains_key(bot_id) && !reserved.contains(&bot_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_bot_provider_creation() {
        let provider = MockBotProvider::with_test_bots();
        let bot_count = provider.available_bot_count().await.unwrap();
        assert!(bot_count > 0);
    }

    #[tokio::test]
    async fn test_bot_selection_by_rating() {
        let provider = MockBotProvider::with_test_bots();

        // Select bots around 1500 rating
        let criteria = BotSelectionCriteria::for_rating(1500.0, 200.0, 3);
        let selected_bots = provider.select_backfill_bots(criteria).await.unwrap();

        assert!(!selected_bots.is_empty());
        assert!(selected_bots.len() <= 3);

        // All selected bots should be within rating range
        for bot in &selected_bots {
            assert!(bot.rating.rating >= 1300.0 && bot.rating.rating <= 1700.0);
        }
    }

    #[tokio::test]
    async fn test_bot_authentication() {
        let provider = MockBotProvider::with_test_bots();

        // Test valid authentication
        let is_valid = provider
            .validate_bot_request("testbot1", "token1")
            .await
            .unwrap();
        assert!(is_valid);

        // Test invalid token
        let is_invalid = provider
            .validate_bot_request("testbot1", "wrong_token")
            .await
            .unwrap();
        assert!(!is_invalid);

        // Test non-existent bot
        let is_nonexistent = provider
            .validate_bot_request("nonexistent", "token")
            .await
            .unwrap();
        assert!(!is_nonexistent);
    }

    #[tokio::test]
    async fn test_bot_reservation() {
        let provider = MockBotProvider::with_test_bots();

        let initial_count = provider.available_bot_count().await.unwrap();

        // Reserve some bots
        provider
            .reserve_bots(vec!["testbot1".to_string(), "testbot2".to_string()])
            .await
            .unwrap();

        let after_reservation = provider.available_bot_count().await.unwrap();
        assert_eq!(after_reservation, initial_count - 2);

        // Check specific bot availability
        let is_available = provider.is_bot_available("testbot1").await.unwrap();
        assert!(!is_available);

        // Release bots
        provider
            .release_bots(vec!["testbot1".to_string()])
            .await
            .unwrap();

        let after_release = provider.available_bot_count().await.unwrap();
        assert_eq!(after_release, initial_count - 1);

        let is_available_again = provider.is_bot_available("testbot1").await.unwrap();
        assert!(is_available_again);
    }

    #[tokio::test]
    async fn test_bot_exclusion() {
        let provider = MockBotProvider::with_test_bots();

        // Select bots excluding specific ones
        let criteria = BotSelectionCriteria::for_rating(1500.0, 500.0, 5)
            .excluding_bots(vec!["testbot1".to_string(), "testbot2".to_string()]);

        let selected_bots = provider.select_backfill_bots(criteria).await.unwrap();

        // Should not contain excluded bots
        for bot in &selected_bots {
            assert!(bot.id != "testbot1" && bot.id != "testbot2");
        }
    }

    #[tokio::test]
    async fn test_preferred_rating_sorting() {
        let provider = MockBotProvider::with_test_bots();

        // Select bots with preferred rating
        let criteria = BotSelectionCriteria::for_rating(1400.0, 500.0, 3);
        let selected_bots = provider.select_backfill_bots(criteria).await.unwrap();

        // First bot should be closest to 1400
        if !selected_bots.is_empty() {
            let first_bot_diff = (selected_bots[0].rating.rating - 1400.0).abs();
            for bot in &selected_bots[1..] {
                let bot_diff = (bot.rating.rating - 1400.0).abs();
                assert!(first_bot_diff <= bot_diff);
            }
        }
    }

    #[tokio::test]
    async fn test_add_remove_bot() {
        let provider = MockBotProvider::new();

        let bot = Player {
            id: "custom_bot".to_string(),
            player_type: PlayerType::Bot,
            rating: PlayerRating {
                rating: 1300.0,
                uncertainty: 200.0,
            },
            joined_at: current_timestamp(),
        };

        // Add bot
        provider
            .add_bot(bot.clone(), "custom_token".to_string())
            .unwrap();

        let retrieved_bot = provider.get_bot("custom_bot").await.unwrap();
        assert!(retrieved_bot.is_some());
        assert_eq!(retrieved_bot.unwrap().id, "custom_bot");

        // Test authentication
        let is_valid = provider
            .validate_bot_request("custom_bot", "custom_token")
            .await
            .unwrap();
        assert!(is_valid);

        // Remove bot
        let removed = provider.remove_bot("custom_bot").unwrap();
        assert!(removed);

        let retrieved_after_removal = provider.get_bot("custom_bot").await.unwrap();
        assert!(retrieved_after_removal.is_none());
    }
}
