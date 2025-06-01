//! Main application configuration
//!
//! This module defines the primary configuration structures for the parlor-room
//! matchmaking service, including environment variable loading and validation.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub service: ServiceSettings,
    pub amqp: AmqpSettings,
    pub matchmaking: MatchmakingSettings,
}

/// Service-level settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSettings {
    /// Service name for logging and metrics
    pub name: String,
    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,
    /// Port for health check endpoint
    pub health_port: u16,
    /// Graceful shutdown timeout in seconds
    pub shutdown_timeout_seconds: u64,
    /// Maximum concurrent lobby operations
    pub max_concurrent_operations: usize,
}

/// AMQP connection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmqpSettings {
    /// AMQP broker URL
    pub url: String,
    /// Queue name for incoming queue requests
    pub queue_name: String,
    /// Exchange name for outbound events
    pub exchange_name: String,
    /// Connection timeout in seconds
    pub connection_timeout_seconds: u64,
    /// Maximum retry attempts for failed operations
    pub max_retry_attempts: u32,
    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,
}

/// Matchmaking-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchmakingSettings {
    /// Maximum wait time for players in seconds
    pub max_wait_time_seconds: u64,
    /// Lobby cleanup interval in seconds
    pub cleanup_interval_seconds: u64,
    /// Bot backfill trigger delay in seconds
    pub backfill_delay_seconds: u64,
    /// Maximum rating difference for matching
    pub max_rating_difference: f64,
    /// Enable bot backfilling
    pub enable_bot_backfill: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            service: ServiceSettings::default(),
            amqp: AmqpSettings::default(),
            matchmaking: MatchmakingSettings::default(),
        }
    }
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            name: "parlor-room".to_string(),
            log_level: "info".to_string(),
            health_port: 8080,
            shutdown_timeout_seconds: 30,
            max_concurrent_operations: 1000,
        }
    }
}

impl Default for AmqpSettings {
    fn default() -> Self {
        Self {
            url: "amqp://guest:guest@localhost:5672/%2f".to_string(),
            queue_name: "matchmaking.queue_requests".to_string(),
            exchange_name: "matchmaking.events".to_string(),
            connection_timeout_seconds: 30,
            max_retry_attempts: 5,
            retry_delay_ms: 1000,
        }
    }
}

impl Default for MatchmakingSettings {
    fn default() -> Self {
        Self {
            max_wait_time_seconds: 300,   // 5 minutes
            cleanup_interval_seconds: 60, // 1 minute
            backfill_delay_seconds: 30,   // 30 seconds
            max_rating_difference: 500.0,
            enable_bot_backfill: true,
        }
    }
}

impl AppConfig {
    /// Load configuration from environment variables with fallback to defaults
    pub fn from_env() -> Result<Self> {
        let mut config = Self::default();

        // Service settings
        if let Ok(name) = env::var("SERVICE_NAME") {
            config.service.name = name;
        }
        if let Ok(log_level) = env::var("LOG_LEVEL") {
            config.service.log_level = log_level;
        }
        if let Ok(port) = env::var("HEALTH_PORT") {
            config.service.health_port = port
                .parse()
                .map_err(|_| anyhow!("Invalid HEALTH_PORT value: {}", port))?;
        }
        if let Ok(timeout) = env::var("SHUTDOWN_TIMEOUT_SECONDS") {
            config.service.shutdown_timeout_seconds = timeout
                .parse()
                .map_err(|_| anyhow!("Invalid SHUTDOWN_TIMEOUT_SECONDS value: {}", timeout))?;
        }
        if let Ok(max_ops) = env::var("MAX_CONCURRENT_OPERATIONS") {
            config.service.max_concurrent_operations = max_ops
                .parse()
                .map_err(|_| anyhow!("Invalid MAX_CONCURRENT_OPERATIONS value: {}", max_ops))?;
        }

        // AMQP settings
        if let Ok(url) = env::var("AMQP_URL") {
            config.amqp.url = url;
        }
        if let Ok(queue) = env::var("AMQP_QUEUE_NAME") {
            config.amqp.queue_name = queue;
        }
        if let Ok(exchange) = env::var("AMQP_EXCHANGE_NAME") {
            config.amqp.exchange_name = exchange;
        }
        if let Ok(timeout) = env::var("AMQP_CONNECTION_TIMEOUT_SECONDS") {
            config.amqp.connection_timeout_seconds = timeout.parse().map_err(|_| {
                anyhow!("Invalid AMQP_CONNECTION_TIMEOUT_SECONDS value: {}", timeout)
            })?;
        }
        if let Ok(retries) = env::var("AMQP_MAX_RETRY_ATTEMPTS") {
            config.amqp.max_retry_attempts = retries
                .parse()
                .map_err(|_| anyhow!("Invalid AMQP_MAX_RETRY_ATTEMPTS value: {}", retries))?;
        }
        if let Ok(delay) = env::var("AMQP_RETRY_DELAY_MS") {
            config.amqp.retry_delay_ms = delay
                .parse()
                .map_err(|_| anyhow!("Invalid AMQP_RETRY_DELAY_MS value: {}", delay))?;
        }

        // Matchmaking settings
        if let Ok(wait_time) = env::var("MAX_WAIT_TIME_SECONDS") {
            config.matchmaking.max_wait_time_seconds = wait_time
                .parse()
                .map_err(|_| anyhow!("Invalid MAX_WAIT_TIME_SECONDS value: {}", wait_time))?;
        }
        if let Ok(cleanup) = env::var("CLEANUP_INTERVAL_SECONDS") {
            config.matchmaking.cleanup_interval_seconds = cleanup
                .parse()
                .map_err(|_| anyhow!("Invalid CLEANUP_INTERVAL_SECONDS value: {}", cleanup))?;
        }
        if let Ok(backfill) = env::var("BACKFILL_DELAY_SECONDS") {
            config.matchmaking.backfill_delay_seconds = backfill
                .parse()
                .map_err(|_| anyhow!("Invalid BACKFILL_DELAY_SECONDS value: {}", backfill))?;
        }
        if let Ok(rating_diff) = env::var("MAX_RATING_DIFFERENCE") {
            config.matchmaking.max_rating_difference = rating_diff
                .parse()
                .map_err(|_| anyhow!("Invalid MAX_RATING_DIFFERENCE value: {}", rating_diff))?;
        }
        if let Ok(enable_backfill) = env::var("ENABLE_BOT_BACKFILL") {
            config.matchmaking.enable_bot_backfill = enable_backfill
                .parse()
                .map_err(|_| anyhow!("Invalid ENABLE_BOT_BACKFILL value: {}", enable_backfill))?;
        }

        validate_config(&config)?;
        Ok(config)
    }

    /// Get shutdown timeout as Duration
    pub fn shutdown_timeout(&self) -> Duration {
        Duration::from_secs(self.service.shutdown_timeout_seconds)
    }

    /// Get AMQP connection timeout as Duration
    pub fn amqp_connection_timeout(&self) -> Duration {
        Duration::from_secs(self.amqp.connection_timeout_seconds)
    }

    /// Get retry delay as Duration
    pub fn amqp_retry_delay(&self) -> Duration {
        Duration::from_millis(self.amqp.retry_delay_ms)
    }

    /// Get cleanup interval as Duration
    pub fn cleanup_interval(&self) -> Duration {
        Duration::from_secs(self.matchmaking.cleanup_interval_seconds)
    }

    /// Get backfill delay as Duration
    pub fn backfill_delay(&self) -> Duration {
        Duration::from_secs(self.matchmaking.backfill_delay_seconds)
    }
}

/// Validate configuration values
pub fn validate_config(config: &AppConfig) -> Result<()> {
    // Validate log level
    match config.service.log_level.to_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => {}
        _ => return Err(anyhow!("Invalid log level: {}", config.service.log_level)),
    }

    // Validate ports
    if config.service.health_port == 0 {
        return Err(anyhow!("Health port cannot be 0"));
    }

    // Validate timeouts
    if config.service.shutdown_timeout_seconds == 0 {
        return Err(anyhow!("Shutdown timeout must be greater than 0"));
    }
    if config.amqp.connection_timeout_seconds == 0 {
        return Err(anyhow!("AMQP connection timeout must be greater than 0"));
    }

    // Validate AMQP settings
    if config.amqp.url.is_empty() {
        return Err(anyhow!("AMQP URL cannot be empty"));
    }
    if config.amqp.queue_name.is_empty() {
        return Err(anyhow!("AMQP queue name cannot be empty"));
    }
    if config.amqp.exchange_name.is_empty() {
        return Err(anyhow!("AMQP exchange name cannot be empty"));
    }

    // Validate matchmaking settings
    if config.matchmaking.max_wait_time_seconds == 0 {
        return Err(anyhow!("Max wait time must be greater than 0"));
    }
    if config.matchmaking.cleanup_interval_seconds == 0 {
        return Err(anyhow!("Cleanup interval must be greater than 0"));
    }
    if config.matchmaking.max_rating_difference <= 0.0 {
        return Err(anyhow!("Max rating difference must be positive"));
    }

    Ok(())
}
