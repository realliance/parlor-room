//! Lobby configuration

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Comprehensive lobby configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyConfig {
    /// Maximum number of players in a lobby
    pub capacity: usize,
    /// Base wait time before considering backfill (seconds)
    pub wait_time_seconds: u64,
    /// Whether to enable strict rating matching
    pub enable_rating_matching: bool,
    /// Maximum rating difference allowed for matching
    pub max_rating_difference: f64,
    /// Whether to prioritize human players over bots
    pub prioritize_humans: bool,
    /// Minimum number of human players required
    pub min_human_players: usize,
    /// Whether to allow bot backfilling
    pub allow_bot_backfill: bool,
    /// Whether lobbies should start immediately when full
    pub immediate_start_when_full: bool,
    /// Timeout for lobby cleanup (seconds)
    pub lobby_cleanup_timeout_seconds: u64,
    /// Quality threshold for lobby matching (0.0 to 1.0)
    pub match_quality_threshold: f64,
}

impl Default for LobbyConfig {
    fn default() -> Self {
        Self {
            capacity: 4,
            wait_time_seconds: 120,
            enable_rating_matching: false,
            max_rating_difference: 300.0,
            prioritize_humans: true,
            min_human_players: 0,
            allow_bot_backfill: true,
            immediate_start_when_full: true,
            lobby_cleanup_timeout_seconds: 600, // 10 minutes
            match_quality_threshold: 0.3,       // Accept matches with 30% quality or higher
        }
    }
}

impl LobbyConfig {
    /// Create configuration optimized for competitive play
    pub fn competitive() -> Self {
        Self {
            enable_rating_matching: true,
            max_rating_difference: 200.0, // Tighter rating bands
            match_quality_threshold: 0.7, // Higher quality requirement
            wait_time_seconds: 180,       // Longer wait for better matches
            min_human_players: 2,         // Require at least 2 humans
            ..Default::default()
        }
    }

    /// Create configuration optimized for casual play
    pub fn casual() -> Self {
        Self {
            enable_rating_matching: false,
            max_rating_difference: 500.0, // Wider rating bands
            match_quality_threshold: 0.1, // Accept almost any match
            wait_time_seconds: 60,        // Quick matching
            min_human_players: 0,         // No human requirement
            ..Default::default()
        }
    }

    /// Create configuration for bot-only lobbies
    pub fn bot_only() -> Self {
        Self {
            prioritize_humans: false,
            min_human_players: 0,
            allow_bot_backfill: false, // No backfill needed for bot-only
            immediate_start_when_full: true,
            wait_time_seconds: 0, // Immediate start
            ..Default::default()
        }
    }

    /// Get wait time as Duration
    pub fn wait_time(&self) -> Duration {
        Duration::from_secs(self.wait_time_seconds)
    }

    /// Get cleanup timeout as Duration
    pub fn cleanup_timeout(&self) -> Duration {
        Duration::from_secs(self.lobby_cleanup_timeout_seconds)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.capacity == 0 {
            return Err("Lobby capacity must be greater than 0".to_string());
        }

        if self.capacity > 8 {
            return Err("Lobby capacity cannot exceed 8 players".to_string());
        }

        if self.min_human_players > self.capacity {
            return Err("Minimum human players cannot exceed lobby capacity".to_string());
        }

        if self.max_rating_difference < 0.0 {
            return Err("Maximum rating difference must be non-negative".to_string());
        }

        if self.match_quality_threshold < 0.0 || self.match_quality_threshold > 1.0 {
            return Err("Match quality threshold must be between 0.0 and 1.0".to_string());
        }

        Ok(())
    }
}
