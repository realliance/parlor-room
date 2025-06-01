//! Statistical tracking for wait times
//!
//! This module provides statistical analysis of wait times to support
//! dynamic wait time calculations for bot backfilling.

use crate::types::{LobbyType, PlayerType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Statistics for a specific wait time category
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitTimeStats {
    /// Number of samples collected
    pub sample_count: u64,
    /// Sum of all wait times (for calculating mean)
    pub sum_seconds: f64,
    /// Sum of squared wait times (for calculating variance)
    pub sum_squared_seconds: f64,
    /// Minimum wait time observed
    pub min_seconds: f64,
    /// Maximum wait time observed
    pub max_seconds: f64,
    /// Last update timestamp
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl WaitTimeStats {
    /// Create new empty statistics
    pub fn new() -> Self {
        Self {
            sample_count: 0,
            sum_seconds: 0.0,
            sum_squared_seconds: 0.0,
            min_seconds: f64::INFINITY,
            max_seconds: 0.0,
            last_updated: chrono::Utc::now(),
        }
    }

    /// Add a new wait time sample
    pub fn add_sample(&mut self, wait_time: Duration) {
        let seconds = wait_time.as_secs_f64();

        self.sample_count += 1;
        self.sum_seconds += seconds;
        self.sum_squared_seconds += seconds * seconds;
        self.min_seconds = self.min_seconds.min(seconds);
        self.max_seconds = self.max_seconds.max(seconds);
        self.last_updated = chrono::Utc::now();
    }

    /// Calculate the mean wait time
    pub fn mean(&self) -> Duration {
        if self.sample_count == 0 {
            return Duration::from_secs(0);
        }

        let mean_seconds = self.sum_seconds / self.sample_count as f64;
        Duration::from_secs_f64(mean_seconds)
    }

    /// Calculate the standard deviation
    pub fn standard_deviation(&self) -> Duration {
        if self.sample_count <= 1 {
            return Duration::from_secs(0);
        }

        let mean_seconds = self.sum_seconds / self.sample_count as f64;
        let variance =
            (self.sum_squared_seconds / self.sample_count as f64) - (mean_seconds * mean_seconds);
        let std_dev_seconds = variance.max(0.0).sqrt();

        Duration::from_secs_f64(std_dev_seconds)
    }

    /// Calculate the confidence interval for the mean
    pub fn confidence_interval_95(&self) -> (Duration, Duration) {
        if self.sample_count < 2 {
            let mean = self.mean();
            return (mean, mean);
        }

        let mean = self.mean();
        let std_dev = self.standard_deviation();
        let std_error = std_dev.as_secs_f64() / (self.sample_count as f64).sqrt();

        // Using 1.96 for 95% confidence interval (normal approximation)
        let margin_of_error = 1.96 * std_error;

        let lower = Duration::from_secs_f64((mean.as_secs_f64() - margin_of_error).max(0.0));
        let upper = Duration::from_secs_f64(mean.as_secs_f64() + margin_of_error);

        (lower, upper)
    }

    /// Get minimum wait time
    pub fn min(&self) -> Duration {
        if self.min_seconds == f64::INFINITY {
            Duration::from_secs(0)
        } else {
            Duration::from_secs_f64(self.min_seconds)
        }
    }

    /// Get maximum wait time
    pub fn max(&self) -> Duration {
        Duration::from_secs_f64(self.max_seconds)
    }

    /// Check if we have enough samples for reliable statistics
    pub fn has_sufficient_samples(&self, min_samples: u64) -> bool {
        self.sample_count >= min_samples
    }

    /// Get age of statistics since last update
    pub fn age(&self) -> Duration {
        let now = chrono::Utc::now();
        let age = now.signed_duration_since(self.last_updated);
        Duration::from_secs(age.num_seconds().max(0) as u64)
    }
}

impl Default for WaitTimeStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Key for identifying different wait time categories
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsKey {
    pub lobby_type: LobbyType,
    pub player_type: PlayerType,
}

impl StatsKey {
    pub fn new(lobby_type: LobbyType, player_type: PlayerType) -> Self {
        Self {
            lobby_type,
            player_type,
        }
    }
}

/// Trait for tracking wait time statistics
pub trait StatisticsTracker: Send + Sync {
    /// Record a wait time sample
    fn record_wait_time(&self, key: StatsKey, wait_time: Duration) -> crate::error::Result<()>;

    /// Get statistics for a specific category
    fn get_stats(&self, key: &StatsKey) -> crate::error::Result<Option<WaitTimeStats>>;

    /// Get all tracked statistics
    fn get_all_stats(&self) -> crate::error::Result<HashMap<StatsKey, WaitTimeStats>>;

    /// Clear statistics for a specific category
    fn clear_stats(&self, key: &StatsKey) -> crate::error::Result<()>;

    /// Clear all statistics
    fn clear_all_stats(&self) -> crate::error::Result<()>;
}

/// In-memory statistics tracker
#[derive(Debug)]
pub struct InMemoryStatisticsTracker {
    stats: std::sync::RwLock<HashMap<StatsKey, WaitTimeStats>>,
    max_entries: usize,
}

impl InMemoryStatisticsTracker {
    /// Create a new in-memory statistics tracker
    pub fn new(max_entries: usize) -> Self {
        Self {
            stats: std::sync::RwLock::new(HashMap::new()),
            max_entries,
        }
    }

    /// Cleanup old entries if we exceed max_entries
    fn cleanup_if_needed(&self) -> crate::error::Result<()> {
        let mut stats =
            self.stats
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire statistics write lock".to_string(),
                })?;

        if stats.len() > self.max_entries {
            // Remove oldest entries (by last_updated timestamp)
            let mut entries: Vec<_> = stats
                .iter()
                .map(|(k, v)| (k.clone(), v.last_updated))
                .collect();
            entries.sort_by(|a, b| a.1.cmp(&b.1));

            let to_remove = stats.len() - self.max_entries;
            for (key, _) in entries.into_iter().take(to_remove) {
                stats.remove(&key);
            }
        }

        Ok(())
    }
}

impl Default for InMemoryStatisticsTracker {
    fn default() -> Self {
        Self::new(1000) // Default to 1000 max entries
    }
}

impl StatisticsTracker for InMemoryStatisticsTracker {
    fn record_wait_time(&self, key: StatsKey, wait_time: Duration) -> crate::error::Result<()> {
        let mut stats =
            self.stats
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire statistics write lock".to_string(),
                })?;

        let entry = stats.entry(key).or_insert_with(WaitTimeStats::new);
        entry.add_sample(wait_time);

        drop(stats); // Release lock before cleanup
        self.cleanup_if_needed()?;

        Ok(())
    }

    fn get_stats(&self, key: &StatsKey) -> crate::error::Result<Option<WaitTimeStats>> {
        let stats =
            self.stats
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire statistics read lock".to_string(),
                })?;

        Ok(stats.get(key).cloned())
    }

    fn get_all_stats(&self) -> crate::error::Result<HashMap<StatsKey, WaitTimeStats>> {
        let stats =
            self.stats
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire statistics read lock".to_string(),
                })?;

        Ok(stats.clone())
    }

    fn clear_stats(&self, key: &StatsKey) -> crate::error::Result<()> {
        let mut stats =
            self.stats
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire statistics write lock".to_string(),
                })?;

        stats.remove(key);
        Ok(())
    }

    fn clear_all_stats(&self) -> crate::error::Result<()> {
        let mut stats =
            self.stats
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire statistics write lock".to_string(),
                })?;

        stats.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_time_stats_empty() {
        let stats = WaitTimeStats::new();
        assert_eq!(stats.sample_count, 0);
        assert_eq!(stats.mean(), Duration::from_secs(0));
        assert_eq!(stats.standard_deviation(), Duration::from_secs(0));
        assert!(!stats.has_sufficient_samples(5));
    }

    #[test]
    fn test_wait_time_stats_single_sample() {
        let mut stats = WaitTimeStats::new();
        stats.add_sample(Duration::from_secs(60));

        assert_eq!(stats.sample_count, 1);
        assert_eq!(stats.mean(), Duration::from_secs(60));
        assert_eq!(stats.standard_deviation(), Duration::from_secs(0));
        assert_eq!(stats.min(), Duration::from_secs(60));
        assert_eq!(stats.max(), Duration::from_secs(60));
    }

    #[test]
    fn test_wait_time_stats_multiple_samples() {
        let mut stats = WaitTimeStats::new();
        stats.add_sample(Duration::from_secs(30));
        stats.add_sample(Duration::from_secs(60));
        stats.add_sample(Duration::from_secs(90));

        assert_eq!(stats.sample_count, 3);
        assert_eq!(stats.mean(), Duration::from_secs(60));
        assert_eq!(stats.min(), Duration::from_secs(30));
        assert_eq!(stats.max(), Duration::from_secs(90));

        // Standard deviation should be around 24.49 seconds
        let std_dev = stats.standard_deviation();
        assert!(std_dev.as_secs() >= 24 && std_dev.as_secs() <= 25);
    }

    #[test]
    fn test_confidence_interval() {
        let mut stats = WaitTimeStats::new();
        for i in 1..=10 {
            stats.add_sample(Duration::from_secs(i * 10)); // 10, 20, 30, ..., 100 seconds
        }

        let (lower, upper) = stats.confidence_interval_95();
        let mean = stats.mean();

        assert!(lower < mean);
        assert!(upper > mean);
        assert!(lower.as_secs() > 0);
        assert!(upper.as_secs() < 200); // Should be reasonable bounds
    }

    #[test]
    fn test_stats_key() {
        let key1 = StatsKey::new(LobbyType::AllBot, PlayerType::Bot);
        let key2 = StatsKey::new(LobbyType::AllBot, PlayerType::Bot);
        let key3 = StatsKey::new(LobbyType::General, PlayerType::Human);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_in_memory_statistics_tracker() {
        let tracker = InMemoryStatisticsTracker::new(100);
        let key = StatsKey::new(LobbyType::General, PlayerType::Human);

        // Initially no stats
        assert!(tracker.get_stats(&key).unwrap().is_none());

        // Record some wait times
        tracker
            .record_wait_time(key.clone(), Duration::from_secs(30))
            .unwrap();
        tracker
            .record_wait_time(key.clone(), Duration::from_secs(60))
            .unwrap();
        tracker
            .record_wait_time(key.clone(), Duration::from_secs(90))
            .unwrap();

        // Should have stats now
        let stats = tracker.get_stats(&key).unwrap().unwrap();
        assert_eq!(stats.sample_count, 3);
        assert_eq!(stats.mean(), Duration::from_secs(60));

        // Clear stats
        tracker.clear_stats(&key).unwrap();
        assert!(tracker.get_stats(&key).unwrap().is_none());
    }

    #[test]
    fn test_tracker_max_entries_cleanup() {
        let tracker = InMemoryStatisticsTracker::new(2); // Very small limit

        // Add entries for different keys
        let key1 = StatsKey::new(LobbyType::AllBot, PlayerType::Bot);
        let key2 = StatsKey::new(LobbyType::General, PlayerType::Human);
        let key3 = StatsKey::new(LobbyType::General, PlayerType::Bot);

        tracker
            .record_wait_time(key1.clone(), Duration::from_secs(30))
            .unwrap();
        tracker
            .record_wait_time(key2.clone(), Duration::from_secs(60))
            .unwrap();
        tracker
            .record_wait_time(key3.clone(), Duration::from_secs(90))
            .unwrap();

        let all_stats = tracker.get_all_stats().unwrap();
        assert!(all_stats.len() <= 2); // Should have cleaned up to max entries
    }

    #[test]
    fn test_sufficient_samples() {
        let mut stats = WaitTimeStats::new();
        assert!(!stats.has_sufficient_samples(5));

        for i in 0..3 {
            stats.add_sample(Duration::from_secs(i * 10));
        }
        assert!(!stats.has_sufficient_samples(5));

        for i in 3..6 {
            stats.add_sample(Duration::from_secs(i * 10));
        }
        assert!(stats.has_sufficient_samples(5));
    }
}
