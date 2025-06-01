//! Dynamic wait time calculation
//!
//! This module calculates dynamic wait times based on historical statistics
//! and configurable bounds for bot backfilling decisions.

use crate::types::{LobbyType, PlayerType};
use crate::wait_time::statistics::{StatisticsTracker, StatsKey};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Configuration for wait time calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitTimeConfig {
    /// Minimum wait time (safety bound)
    pub min_wait_seconds: u64,
    /// Maximum wait time (safety bound)  
    pub max_wait_seconds: u64,
    /// Multiplier for standard deviation in calculation
    pub std_dev_multiplier: f64,
    /// Minimum number of samples required for dynamic calculation
    pub min_samples_for_dynamic: u64,
    /// Default wait time when no historical data is available
    pub default_wait_seconds: u64,
    /// Maximum age of statistics before they're considered stale
    pub max_stats_age_seconds: u64,
}

impl Default for WaitTimeConfig {
    fn default() -> Self {
        Self {
            min_wait_seconds: 30,        // 30 seconds minimum
            max_wait_seconds: 300,       // 5 minutes maximum
            std_dev_multiplier: 1.0,     // mean + 1 * std_dev
            min_samples_for_dynamic: 10, // Need at least 10 samples
            default_wait_seconds: 120,   // 2 minutes default
            max_stats_age_seconds: 3600, // 1 hour max age
        }
    }
}

impl WaitTimeConfig {
    /// Create configuration optimized for quick games
    pub fn quick_games() -> Self {
        Self {
            min_wait_seconds: 15,
            max_wait_seconds: 120,
            std_dev_multiplier: 0.5,
            min_samples_for_dynamic: 5,
            default_wait_seconds: 60,
            max_stats_age_seconds: 1800, // 30 minutes
        }
    }

    /// Create configuration optimized for careful matching
    pub fn careful_matching() -> Self {
        Self {
            min_wait_seconds: 60,
            max_wait_seconds: 600, // 10 minutes
            std_dev_multiplier: 2.0,
            min_samples_for_dynamic: 20,
            default_wait_seconds: 180,   // 3 minutes
            max_stats_age_seconds: 7200, // 2 hours
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> crate::error::Result<()> {
        if self.min_wait_seconds >= self.max_wait_seconds {
            return Err(crate::error::MatchmakingError::ConfigurationError {
                message: "min_wait_seconds must be less than max_wait_seconds".to_string(),
            }
            .into());
        }

        if self.std_dev_multiplier < 0.0 {
            return Err(crate::error::MatchmakingError::ConfigurationError {
                message: "std_dev_multiplier must be non-negative".to_string(),
            }
            .into());
        }

        if self.min_samples_for_dynamic == 0 {
            return Err(crate::error::MatchmakingError::ConfigurationError {
                message: "min_samples_for_dynamic must be greater than 0".to_string(),
            }
            .into());
        }

        if self.default_wait_seconds < self.min_wait_seconds
            || self.default_wait_seconds > self.max_wait_seconds
        {
            return Err(crate::error::MatchmakingError::ConfigurationError {
                message: "default_wait_seconds must be within min/max bounds".to_string(),
            }
            .into());
        }

        Ok(())
    }
}

/// Trait for calculating wait times
pub trait WaitTimeCalculator: Send + Sync {
    /// Calculate wait time for a specific lobby type and player type
    fn calculate_wait_time(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<Duration>;

    /// Get the current configuration
    fn config(&self) -> &WaitTimeConfig;

    /// Update the configuration
    fn update_config(&mut self, config: WaitTimeConfig) -> crate::error::Result<()>;
}

/// Dynamic wait time calculator using historical statistics
pub struct DynamicWaitTimeCalculator {
    config: WaitTimeConfig,
    stats_tracker: Arc<dyn StatisticsTracker>,
}

impl DynamicWaitTimeCalculator {
    /// Create a new dynamic wait time calculator
    pub fn new(
        config: WaitTimeConfig,
        stats_tracker: Arc<dyn StatisticsTracker>,
    ) -> crate::error::Result<Self> {
        config.validate()?;

        Ok(Self {
            config,
            stats_tracker,
        })
    }

    /// Calculate wait time using statistical formula: mean + (multiplier * std_dev)
    fn calculate_dynamic_wait_time(
        &self,
        stats: &crate::wait_time::statistics::WaitTimeStats,
    ) -> Duration {
        let mean = stats.mean();
        let std_dev = stats.standard_deviation();

        // Calculate: mean + (multiplier * std_dev)
        let target_seconds =
            mean.as_secs_f64() + (self.config.std_dev_multiplier * std_dev.as_secs_f64());

        // Apply bounds
        let bounded_seconds = target_seconds
            .max(self.config.min_wait_seconds as f64)
            .min(self.config.max_wait_seconds as f64);

        Duration::from_secs_f64(bounded_seconds)
    }

    /// Check if statistics are fresh enough to use
    fn are_stats_fresh(&self, stats: &crate::wait_time::statistics::WaitTimeStats) -> bool {
        let max_age = Duration::from_secs(self.config.max_stats_age_seconds);
        stats.age() <= max_age
    }

    /// Get fallback wait time for different scenarios
    fn get_fallback_wait_time(&self, reason: &str) -> Duration {
        debug!("Using fallback wait time: {}", reason);
        Duration::from_secs(self.config.default_wait_seconds)
    }
}

impl WaitTimeCalculator for DynamicWaitTimeCalculator {
    fn calculate_wait_time(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<Duration> {
        let stats_key = StatsKey::new(lobby_type, player_type);

        // Try to get historical statistics
        match self.stats_tracker.get_stats(&stats_key)? {
            Some(stats) => {
                // Check if we have sufficient samples
                if !stats.has_sufficient_samples(self.config.min_samples_for_dynamic) {
                    return Ok(self.get_fallback_wait_time("insufficient samples"));
                }

                // Check if statistics are fresh
                if !self.are_stats_fresh(&stats) {
                    warn!(
                        "Statistics for {:?}/{:?} are stale (age: {:?})",
                        lobby_type,
                        player_type,
                        stats.age()
                    );
                    return Ok(self.get_fallback_wait_time("stale statistics"));
                }

                // Calculate dynamic wait time
                let calculated_time = self.calculate_dynamic_wait_time(&stats);

                debug!(
                    "Calculated dynamic wait time for {:?}/{:?}: {:?} (samples: {}, mean: {:?}, std_dev: {:?})",
                    lobby_type, player_type, calculated_time, stats.sample_count, stats.mean(), stats.standard_deviation()
                );

                Ok(calculated_time)
            }
            None => {
                // No historical data available
                Ok(self.get_fallback_wait_time("no historical data"))
            }
        }
    }

    fn config(&self) -> &WaitTimeConfig {
        &self.config
    }

    fn update_config(&mut self, config: WaitTimeConfig) -> crate::error::Result<()> {
        config.validate()?;
        self.config = config;
        Ok(())
    }
}

/// Simple static wait time calculator (for testing or fallback)
pub struct StaticWaitTimeCalculator {
    config: WaitTimeConfig,
}

impl StaticWaitTimeCalculator {
    /// Create a new static wait time calculator
    pub fn new(config: WaitTimeConfig) -> crate::error::Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }
}

impl WaitTimeCalculator for StaticWaitTimeCalculator {
    fn calculate_wait_time(
        &self,
        lobby_type: LobbyType,
        _player_type: PlayerType,
    ) -> crate::error::Result<Duration> {
        // Different wait times based on lobby type
        let wait_seconds = match lobby_type {
            LobbyType::AllBot => self.config.min_wait_seconds, // Quick for all-bot
            LobbyType::General => self.config.default_wait_seconds, // Normal for general
        };

        Ok(Duration::from_secs(wait_seconds))
    }

    fn config(&self) -> &WaitTimeConfig {
        &self.config
    }

    fn update_config(&mut self, config: WaitTimeConfig) -> crate::error::Result<()> {
        config.validate()?;
        self.config = config;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wait_time::statistics::{InMemoryStatisticsTracker, WaitTimeStats};

    #[test]
    fn test_wait_time_config_default() {
        let config = WaitTimeConfig::default();
        assert_eq!(config.min_wait_seconds, 30);
        assert_eq!(config.max_wait_seconds, 300);
        assert_eq!(config.std_dev_multiplier, 1.0);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_wait_time_config_validation() {
        let mut config = WaitTimeConfig::default();

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Invalid: min >= max
        config.min_wait_seconds = 300;
        config.max_wait_seconds = 300;
        assert!(config.validate().is_err());

        // Invalid: negative multiplier
        config = WaitTimeConfig::default();
        config.std_dev_multiplier = -1.0;
        assert!(config.validate().is_err());

        // Invalid: zero samples
        config = WaitTimeConfig::default();
        config.min_samples_for_dynamic = 0;
        assert!(config.validate().is_err());

        // Invalid: default outside bounds
        config = WaitTimeConfig::default();
        config.default_wait_seconds = 10; // Below min_wait_seconds
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_static_wait_time_calculator() {
        let config = WaitTimeConfig::default();
        let calculator = StaticWaitTimeCalculator::new(config).unwrap();

        let allbot_time = calculator
            .calculate_wait_time(LobbyType::AllBot, PlayerType::Bot)
            .unwrap();
        let general_time = calculator
            .calculate_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();

        assert_eq!(allbot_time, Duration::from_secs(30)); // min_wait_seconds
        assert_eq!(general_time, Duration::from_secs(120)); // default_wait_seconds
    }

    #[test]
    fn test_dynamic_calculator_no_stats() {
        let config = WaitTimeConfig::default();
        let tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let calculator = DynamicWaitTimeCalculator::new(config, tracker).unwrap();

        // Should use fallback when no stats available
        let wait_time = calculator
            .calculate_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();

        assert_eq!(wait_time, Duration::from_secs(120)); // default_wait_seconds
    }

    #[test]
    fn test_dynamic_calculator_with_stats() {
        let config = WaitTimeConfig::default();
        let tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let calculator = DynamicWaitTimeCalculator::new(config, tracker.clone()).unwrap();

        // Add sufficient statistics
        let key = StatsKey::new(LobbyType::General, PlayerType::Human);
        for i in 0..15 {
            let wait_time = Duration::from_secs(60 + i * 5); // 60, 65, 70, ..., 130
            tracker.record_wait_time(key.clone(), wait_time).unwrap();
        }

        let calculated_time = calculator
            .calculate_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();

        // Should be different from default (using actual statistics)
        assert_ne!(calculated_time, Duration::from_secs(120));
        // Should be within reasonable bounds
        assert!(calculated_time.as_secs() >= 30);
        assert!(calculated_time.as_secs() <= 300);
    }

    #[test]
    fn test_dynamic_calculator_insufficient_samples() {
        let config = WaitTimeConfig::default();
        let tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let calculator = DynamicWaitTimeCalculator::new(config, tracker.clone()).unwrap();

        // Add insufficient statistics (less than min_samples_for_dynamic)
        let key = StatsKey::new(LobbyType::General, PlayerType::Human);
        for i in 0..5 {
            let wait_time = Duration::from_secs(60 + i * 10);
            tracker.record_wait_time(key.clone(), wait_time).unwrap();
        }

        let calculated_time = calculator
            .calculate_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();

        // Should use fallback (default_wait_seconds)
        assert_eq!(calculated_time, Duration::from_secs(120));
    }

    #[test]
    fn test_bounds_enforcement() {
        let config = WaitTimeConfig {
            min_wait_seconds: 60,
            max_wait_seconds: 180,
            std_dev_multiplier: 10.0, // Very high to test upper bound
            ..Default::default()
        };
        let tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let calculator = DynamicWaitTimeCalculator::new(config, tracker.clone()).unwrap();

        // Add stats with high variability to create large standard deviation
        let key = StatsKey::new(LobbyType::General, PlayerType::Human);
        let wait_times = vec![
            50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190,
        ]; // Mean ~120, high std dev
        for time in wait_times {
            tracker
                .record_wait_time(key.clone(), Duration::from_secs(time))
                .unwrap();
        }

        let calculated_time = calculator
            .calculate_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();

        // Should be capped at max_wait_seconds due to high multiplier and std dev
        assert_eq!(calculated_time, Duration::from_secs(180));
    }

    #[test]
    fn test_presets() {
        let quick = WaitTimeConfig::quick_games();
        assert_eq!(quick.min_wait_seconds, 15);
        assert_eq!(quick.default_wait_seconds, 60);
        assert!(quick.validate().is_ok());

        let careful = WaitTimeConfig::careful_matching();
        assert_eq!(careful.max_wait_seconds, 600);
        assert_eq!(careful.std_dev_multiplier, 2.0);
        assert!(careful.validate().is_ok());
    }
}
