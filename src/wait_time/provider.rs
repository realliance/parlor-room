//! Wait time provider interface and implementations
//!
//! This module defines the provider interface for wait time calculations
//! and implementations that integrate statistics tracking with calculators.

use crate::types::{LobbyType, PlayerType};
use crate::wait_time::calculator::{WaitTimeCalculator, WaitTimeConfig};
use crate::wait_time::statistics::{StatisticsTracker, StatsKey};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Trait for providing wait time calculations and managing statistics
pub trait WaitTimeProvider: Send + Sync {
    /// Get the wait time for a specific lobby type and player type
    fn get_wait_time(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<Duration>;

    /// Record a completed wait time for future calculations
    fn record_wait_time(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
        actual_wait_time: Duration,
    ) -> crate::error::Result<()>;

    /// Get the current configuration
    fn get_config(&self) -> crate::error::Result<WaitTimeConfig>;

    /// Update the configuration
    fn update_config(&self, config: WaitTimeConfig) -> crate::error::Result<()>;

    /// Get statistics for a specific category (for monitoring)
    fn get_statistics(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<Option<crate::wait_time::statistics::WaitTimeStats>>;

    /// Clear statistics for a specific category
    fn clear_statistics(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<()>;
}

/// Internal wait time provider that combines statistics tracking with calculation
pub struct InternalWaitTimeProvider {
    calculator: std::sync::RwLock<Box<dyn WaitTimeCalculator>>,
    stats_tracker: Arc<dyn StatisticsTracker>,
}

impl InternalWaitTimeProvider {
    /// Create a new internal wait time provider
    pub fn new(
        calculator: Box<dyn WaitTimeCalculator>,
        stats_tracker: Arc<dyn StatisticsTracker>,
    ) -> Self {
        Self {
            calculator: std::sync::RwLock::new(calculator),
            stats_tracker,
        }
    }

    /// Get a reference to the statistics tracker
    pub fn stats_tracker(&self) -> Arc<dyn StatisticsTracker> {
        Arc::clone(&self.stats_tracker)
    }
}

impl WaitTimeProvider for InternalWaitTimeProvider {
    fn get_wait_time(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<Duration> {
        let calculator =
            self.calculator
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire calculator read lock".to_string(),
                })?;

        let wait_time = calculator.calculate_wait_time(lobby_type, player_type)?;

        debug!(
            "Calculated wait time for {:?}/{:?}: {:?}",
            lobby_type, player_type, wait_time
        );

        Ok(wait_time)
    }

    fn record_wait_time(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
        actual_wait_time: Duration,
    ) -> crate::error::Result<()> {
        let stats_key = StatsKey::new(lobby_type, player_type);
        self.stats_tracker
            .record_wait_time(stats_key, actual_wait_time)?;

        debug!(
            "Recorded wait time for {:?}/{:?}: {:?}",
            lobby_type, player_type, actual_wait_time
        );

        Ok(())
    }

    fn get_config(&self) -> crate::error::Result<WaitTimeConfig> {
        let calculator =
            self.calculator
                .read()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire calculator read lock".to_string(),
                })?;

        Ok(calculator.config().clone())
    }

    fn update_config(&self, config: WaitTimeConfig) -> crate::error::Result<()> {
        let mut calculator =
            self.calculator
                .write()
                .map_err(|_| crate::error::MatchmakingError::InternalError {
                    message: "Failed to acquire calculator write lock".to_string(),
                })?;

        calculator.update_config(config)?;

        info!("Updated wait time configuration");
        Ok(())
    }

    fn get_statistics(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<Option<crate::wait_time::statistics::WaitTimeStats>> {
        let stats_key = StatsKey::new(lobby_type, player_type);
        self.stats_tracker.get_stats(&stats_key)
    }

    fn clear_statistics(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
    ) -> crate::error::Result<()> {
        let stats_key = StatsKey::new(lobby_type, player_type);
        self.stats_tracker.clear_stats(&stats_key)?;

        info!("Cleared statistics for {:?}/{:?}", lobby_type, player_type);

        Ok(())
    }
}

/// Mock wait time provider for testing
#[derive(Debug)]
pub struct MockWaitTimeProvider {
    fixed_wait_time: Duration,
    recorded_times: std::sync::Mutex<Vec<(LobbyType, PlayerType, Duration)>>,
}

impl MockWaitTimeProvider {
    /// Create a new mock provider with a fixed wait time
    pub fn new(fixed_wait_time: Duration) -> Self {
        Self {
            fixed_wait_time,
            recorded_times: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Get all recorded wait times (for testing)
    pub fn get_recorded_times(&self) -> Vec<(LobbyType, PlayerType, Duration)> {
        self.recorded_times
            .lock()
            .map(|times| times.clone())
            .unwrap_or_default()
    }

    /// Clear recorded times (for testing)
    pub fn clear_recorded_times(&self) {
        if let Ok(mut times) = self.recorded_times.lock() {
            times.clear();
        }
    }
}

impl WaitTimeProvider for MockWaitTimeProvider {
    fn get_wait_time(
        &self,
        _lobby_type: LobbyType,
        _player_type: PlayerType,
    ) -> crate::error::Result<Duration> {
        Ok(self.fixed_wait_time)
    }

    fn record_wait_time(
        &self,
        lobby_type: LobbyType,
        player_type: PlayerType,
        actual_wait_time: Duration,
    ) -> crate::error::Result<()> {
        if let Ok(mut times) = self.recorded_times.lock() {
            times.push((lobby_type, player_type, actual_wait_time));
        }
        Ok(())
    }

    fn get_config(&self) -> crate::error::Result<WaitTimeConfig> {
        Ok(WaitTimeConfig::default())
    }

    fn update_config(&self, _config: WaitTimeConfig) -> crate::error::Result<()> {
        Ok(())
    }

    fn get_statistics(
        &self,
        _lobby_type: LobbyType,
        _player_type: PlayerType,
    ) -> crate::error::Result<Option<crate::wait_time::statistics::WaitTimeStats>> {
        Ok(None)
    }

    fn clear_statistics(
        &self,
        _lobby_type: LobbyType,
        _player_type: PlayerType,
    ) -> crate::error::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wait_time::calculator::{DynamicWaitTimeCalculator, StaticWaitTimeCalculator};
    use crate::wait_time::statistics::InMemoryStatisticsTracker;

    #[test]
    fn test_mock_wait_time_provider() {
        let provider = MockWaitTimeProvider::new(Duration::from_secs(60));

        // Should return fixed wait time
        let wait_time = provider
            .get_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();
        assert_eq!(wait_time, Duration::from_secs(60));

        // Should record wait times
        provider
            .record_wait_time(
                LobbyType::General,
                PlayerType::Human,
                Duration::from_secs(90),
            )
            .unwrap();

        let recorded = provider.get_recorded_times();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, LobbyType::General);
        assert_eq!(recorded[0].1, PlayerType::Human);
        assert_eq!(recorded[0].2, Duration::from_secs(90));
    }

    #[test]
    fn test_internal_provider_with_static_calculator() {
        let config = WaitTimeConfig::default();
        let calculator = Box::new(StaticWaitTimeCalculator::new(config).unwrap());
        let stats_tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let provider = InternalWaitTimeProvider::new(calculator, stats_tracker);

        // Should get wait time from calculator
        let wait_time = provider
            .get_wait_time(LobbyType::AllBot, PlayerType::Bot)
            .unwrap();
        assert_eq!(wait_time, Duration::from_secs(30)); // min_wait_seconds for AllBot

        // Should record statistics
        provider
            .record_wait_time(LobbyType::AllBot, PlayerType::Bot, Duration::from_secs(25))
            .unwrap();

        let stats = provider
            .get_statistics(LobbyType::AllBot, PlayerType::Bot)
            .unwrap()
            .unwrap();
        assert_eq!(stats.sample_count, 1);
        assert_eq!(stats.mean(), Duration::from_secs(25));
    }

    #[test]
    fn test_internal_provider_with_dynamic_calculator() {
        let config = WaitTimeConfig::default();
        let stats_tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let stats_tracker_dyn: Arc<dyn StatisticsTracker> =
            Arc::clone(&stats_tracker) as Arc<dyn StatisticsTracker>;
        let calculator =
            Box::new(DynamicWaitTimeCalculator::new(config, stats_tracker_dyn).unwrap());
        let provider = InternalWaitTimeProvider::new(calculator, stats_tracker);

        // Initially should use fallback (no statistics)
        let initial_wait_time = provider
            .get_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();
        assert_eq!(initial_wait_time, Duration::from_secs(120)); // default_wait_seconds

        // Record some wait times
        for i in 0..15 {
            provider
                .record_wait_time(
                    LobbyType::General,
                    PlayerType::Human,
                    Duration::from_secs(60 + i * 5),
                )
                .unwrap();
        }

        // Now should use dynamic calculation
        let dynamic_wait_time = provider
            .get_wait_time(LobbyType::General, PlayerType::Human)
            .unwrap();

        // Should be different from default (using statistics now)
        assert_ne!(dynamic_wait_time, Duration::from_secs(120));
    }

    #[test]
    fn test_config_update() {
        let config = WaitTimeConfig::default();
        let calculator = Box::new(StaticWaitTimeCalculator::new(config).unwrap());
        let stats_tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let provider = InternalWaitTimeProvider::new(calculator, stats_tracker);

        // Get initial config
        let initial_config = provider.get_config().unwrap();
        assert_eq!(initial_config.default_wait_seconds, 120);

        // Update config
        let new_config = WaitTimeConfig {
            default_wait_seconds: 180,
            ..initial_config
        };
        provider.update_config(new_config).unwrap();

        // Verify config was updated
        let updated_config = provider.get_config().unwrap();
        assert_eq!(updated_config.default_wait_seconds, 180);
    }

    #[test]
    fn test_statistics_operations() {
        let config = WaitTimeConfig::default();
        let calculator = Box::new(StaticWaitTimeCalculator::new(config).unwrap());
        let stats_tracker = Arc::new(InMemoryStatisticsTracker::new(100));
        let provider = InternalWaitTimeProvider::new(calculator, stats_tracker);

        // Initially no statistics
        let stats = provider
            .get_statistics(LobbyType::General, PlayerType::Human)
            .unwrap();
        assert!(stats.is_none());

        // Record some wait times
        provider
            .record_wait_time(
                LobbyType::General,
                PlayerType::Human,
                Duration::from_secs(60),
            )
            .unwrap();
        provider
            .record_wait_time(
                LobbyType::General,
                PlayerType::Human,
                Duration::from_secs(90),
            )
            .unwrap();

        // Should have statistics now
        let stats = provider
            .get_statistics(LobbyType::General, PlayerType::Human)
            .unwrap()
            .unwrap();
        assert_eq!(stats.sample_count, 2);
        assert_eq!(stats.mean(), Duration::from_secs(75));

        // Clear statistics
        provider
            .clear_statistics(LobbyType::General, PlayerType::Human)
            .unwrap();

        // Should be empty again
        let stats = provider
            .get_statistics(LobbyType::General, PlayerType::Human)
            .unwrap();
        assert!(stats.is_none());
    }
}
