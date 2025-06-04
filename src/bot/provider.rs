//! Bot provider interface and implementations
//!
//! This module defines the BotProvider trait for managing bot availability,
//! selection, and lifecycle. It provides both mock implementations for testing
//! and interfaces for production bot management systems.

use crate::types::{Player, PlayerRating, PlayerType};
use crate::utils::current_timestamp;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;

/// Criteria for selecting bots for backfilling
#[derive(Debug, Clone)]
pub struct BotSelectionCriteria {
    /// Number of bots needed
    pub count: usize,
    /// Rating range (min, max)
    pub rating_range: (f64, f64),
    /// Uncertainty range (min, max)
    pub uncertainty_range: (f64, f64),
    /// Preferred rating for sorting (bots closest to this rating preferred)
    pub preferred_rating: Option<f64>,
    /// Bot IDs to exclude from selection
    pub exclude_bots: Vec<String>,
}

impl BotSelectionCriteria {
    /// Create criteria optimized for a specific rating and uncertainty
    pub fn for_rating(target_rating: f64, rating_tolerance: f64, count: usize) -> Self {
        Self {
            count,
            rating_range: (
                target_rating - rating_tolerance,
                target_rating + rating_tolerance,
            ),
            uncertainty_range: (0.0, f64::MAX),
            preferred_rating: Some(target_rating),
            exclude_bots: Vec::new(),
        }
    }

    /// Create criteria for any available bots
    pub fn any_available(count: usize) -> Self {
        Self {
            count,
            rating_range: (0.0, f64::MAX),
            uncertainty_range: (0.0, f64::MAX),
            preferred_rating: None,
            exclude_bots: Vec::new(),
        }
    }

    /// Add bots to exclude from selection
    pub fn exclude_bots(mut self, bot_ids: Vec<String>) -> Self {
        self.exclude_bots = bot_ids;
        self
    }
}

/// Bot provider interface for managing bot availability and selection
#[async_trait::async_trait]
pub trait BotProvider: Send + Sync {
    /// Select bots for automatic backfilling based on criteria
    async fn select_backfill_bots(
        &self,
        criteria: BotSelectionCriteria,
    ) -> crate::error::Result<Vec<Player>>;

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
}

impl MockBotProvider {
    /// Create a new mock bot provider
    pub fn new() -> Self {
        Self {
            available_bots: RwLock::new(HashMap::new()),
            reserved_bots: RwLock::new(Vec::new()),
        }
    }

    /// Create a mock provider with pre-configured bots
    pub fn with_test_bots() -> Self {
        let provider = Self::new();

        // Add some test bots with varying ratings
        let test_bots = vec![
            ("testbot1", 1200.0, 180.0),
            ("testbot2", 1400.0, 160.0),
            ("testbot3", 1500.0, 200.0),
            ("testbot4", 1600.0, 150.0),
            ("testbot5", 1800.0, 170.0),
            ("weakbot1", 1000.0, 250.0),
            ("strongbot1", 2000.0, 120.0),
            ("newbot1", 1500.0, 350.0), // High uncertainty
        ];

        {
            let mut bots = provider.available_bots.write().unwrap();

            for (bot_id, rating, uncertainty) in test_bots {
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
            }
        }

        provider
    }

    /// Add a bot to the provider
    pub fn add_bot(&self, bot: Player) -> crate::error::Result<()> {
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

        bots.insert(bot.id.clone(), bot);

        Ok(())
    }

    /// Remove a bot from the provider
    pub fn remove_bot(&self, bot_id: &str) -> crate::error::Result<bool> {
        let mut bots = self.available_bots.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire bots write lock".to_string(),
            }
        })?;

        let removed = bots.remove(bot_id).is_some();

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
    pub fn clear_reservations(&self) -> crate::error::Result<()> {
        let mut reserved = self.reserved_bots.write().map_err(|_| {
            crate::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved write lock".to_string(),
            }
        })?;

        reserved.clear();
        Ok(())
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
            .exclude_bots(vec!["testbot1".to_string(), "testbot2".to_string()]);

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
        provider.add_bot(bot.clone()).unwrap();

        let retrieved_bot = provider.get_bot("custom_bot").await.unwrap();
        assert!(retrieved_bot.is_some());
        assert_eq!(retrieved_bot.unwrap().id, "custom_bot");

        // Remove bot
        let removed = provider.remove_bot("custom_bot").unwrap();
        assert!(removed);

        let retrieved_after_removal = provider.get_bot("custom_bot").await.unwrap();
        assert!(retrieved_after_removal.is_none());
    }
}
