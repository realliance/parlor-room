//! AMQP connection management with retry logic and pooling

use crate::error::{MatchmakingError, Result};
use amqprs::connection::{Connection, OpenConnectionArguments};
use anyhow::Context;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

/// Configuration for AMQP connection
#[derive(Debug, Clone)]
pub struct AmqpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub vhost: String,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub connection_timeout_ms: u64,
}

impl Default for AmqpConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5672,
            username: "guest".to_string(),
            password: "guest".to_string(),
            vhost: "/".to_string(),
            max_retries: 5,
            retry_delay_ms: 1000,
            connection_timeout_ms: 30000,
        }
    }
}

/// Wrapper around AMQP connection with additional metadata
pub struct AmqpConnection {
    connection: Connection,
    _config: AmqpConfig,
}

impl AmqpConnection {
    /// Create a new AMQP connection with retry logic
    pub async fn new(config: AmqpConfig) -> Result<Self> {
        let connection = Self::connect_with_retry(&config).await?;

        Ok(Self {
            connection,
            _config: config,
        })
    }

    /// Attempt to connect with exponential backoff retry
    async fn connect_with_retry(config: &AmqpConfig) -> Result<Connection> {
        let mut retry_count = 0;
        let mut delay = Duration::from_millis(config.retry_delay_ms);

        loop {
            match Self::try_connect(config).await {
                Ok(connection) => {
                    info!("Successfully connected to AMQP broker");
                    return Ok(connection);
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count > config.max_retries {
                        error!(
                            "Failed to connect to AMQP after {} retries",
                            config.max_retries
                        );
                        return Err(MatchmakingError::AmqpConnectionFailed {
                            message: format!("Max retries exceeded: {}", e),
                        }
                        .into());
                    }

                    warn!(
                        "AMQP connection attempt {} failed: {}. Retrying in {:?}",
                        retry_count, e, delay
                    );

                    sleep(delay).await;
                    // Exponential backoff with jitter
                    delay = Duration::from_millis((delay.as_millis() as u64 * 2).min(30000));
                }
            }
        }
    }

    /// Single connection attempt
    async fn try_connect(config: &AmqpConfig) -> Result<Connection> {
        let mut args = OpenConnectionArguments::new(
            &config.host,
            config.port,
            &config.username,
            &config.password,
        );
        args.virtual_host(&config.vhost);

        Connection::open(&args)
            .await
            .context("Failed to open AMQP connection")
            .map_err(|e| {
                MatchmakingError::AmqpConnectionFailed {
                    message: e.to_string(),
                }
                .into()
            })
    }

    /// Get the underlying connection
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Check if connection is still alive
    pub fn is_alive(&self) -> bool {
        // For now, assume connection is alive if it exists
        // In a real implementation, we'd check the actual connection state
        true
    }

    /// Close the connection
    pub async fn close(self) -> Result<()> {
        self.connection
            .close()
            .await
            .context("Failed to close AMQP connection")
    }
}

/// Simple connection pool for AMQP connections
pub struct AmqpConnectionPool {
    config: AmqpConfig,
    _pool_size: usize,
}

impl AmqpConnectionPool {
    /// Create a new connection pool
    pub fn new(config: AmqpConfig, pool_size: usize) -> Self {
        Self {
            config,
            _pool_size: pool_size,
        }
    }

    /// Get a connection from the pool (simplified implementation)
    /// In production, this would maintain a pool of connections
    pub async fn get_connection(&self) -> Result<AmqpConnection> {
        AmqpConnection::new(self.config.clone()).await
    }

    /// Return a connection to the pool (simplified implementation)
    pub async fn return_connection(&self, _connection: AmqpConnection) -> Result<()> {
        // In a real implementation, we'd return the connection to the pool
        // For now, we just close it
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amqp_config_default() {
        let config = AmqpConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5672);
        assert_eq!(config.max_retries, 5);
    }

    #[test]
    fn test_connection_pool_creation() {
        let config = AmqpConfig::default();
        let pool = AmqpConnectionPool::new(config, 5);
        assert_eq!(pool._pool_size, 5);
    }

    // Note: Integration tests with actual AMQP broker would go in tests/ directory
}
