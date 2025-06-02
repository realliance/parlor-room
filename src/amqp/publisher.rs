//! AMQP event publisher for outbound events

use crate::amqp::messages::{MessageEnvelope, GAME_EVENTS_EXCHANGE, PLAYER_EVENTS_EXCHANGE};
use crate::error::{MatchmakingError, Result};
use crate::types::*;
use amqprs::{
    channel::{BasicPublishArguments, Channel, ExchangeDeclareArguments},
    BasicProperties,
};
use async_trait::async_trait;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Trait for publishing matchmaking events
#[async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish a PlayerJoinedLobby event
    async fn publish_player_joined_lobby(&self, event: PlayerJoinedLobby) -> Result<()>;

    /// Publish a PlayerLeftLobby event  
    async fn publish_player_left_lobby(&self, event: PlayerLeftLobby) -> Result<()>;

    /// Publish a GameStarting event
    async fn publish_game_starting(&self, event: GameStarting) -> Result<()>;
}

/// Configuration for event publishing
#[derive(Debug, Clone)]
pub struct PublisherConfig {
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub enable_deduplication: bool,
    pub publish_timeout_ms: u64,
}

impl Default for PublisherConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_delay_ms: 500,
            enable_deduplication: true,
            publish_timeout_ms: 5000,
        }
    }
}

/// AMQP-based event publisher implementation
pub struct AmqpEventPublisher {
    channel: Channel,
    config: PublisherConfig,
    published_messages: std::sync::Mutex<std::collections::HashSet<String>>, // For deduplication
}

impl AmqpEventPublisher {
    /// Create a new event publisher
    pub async fn new(channel: Channel, config: PublisherConfig) -> Result<Self> {
        let publisher = Self {
            channel,
            config,
            published_messages: std::sync::Mutex::new(std::collections::HashSet::new()),
        };

        // Set up exchanges and queues
        publisher.setup_exchanges().await?;

        Ok(publisher)
    }

    /// Set up AMQP exchanges for events
    async fn setup_exchanges(&self) -> Result<()> {
        // Declare player events exchange
        let args = ExchangeDeclareArguments::new(PLAYER_EVENTS_EXCHANGE, "topic");
        self.channel.exchange_declare(args).await.map_err(|e| {
            MatchmakingError::AmqpConnectionFailed {
                message: format!("Failed to declare player events exchange: {}", e),
            }
        })?;

        // Declare game events exchange
        let args = ExchangeDeclareArguments::new(GAME_EVENTS_EXCHANGE, "topic");
        self.channel.exchange_declare(args).await.map_err(|e| {
            MatchmakingError::AmqpConnectionFailed {
                message: format!("Failed to declare game events exchange: {}", e),
            }
        })?;

        info!("Successfully set up AMQP exchanges");
        Ok(())
    }

    /// Generic method to publish to an exchange with retry logic
    async fn publish_to_exchange<T>(
        &self,
        exchange: &str,
        envelope: &MessageEnvelope<T>,
    ) -> Result<()>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Clone,
    {
        // Check for deduplication
        if self.config.enable_deduplication {
            let published_messages =
                self.published_messages
                    .lock()
                    .map_err(|_| MatchmakingError::InternalError {
                        message: "Failed to acquire published messages lock".to_string(),
                    })?;
            if published_messages.contains(&envelope.correlation_id) {
                debug!(
                    "Message {} already published, skipping",
                    envelope.correlation_id
                );
                return Ok(());
            }
        }

        let mut retry_count = 0;
        let mut delay = Duration::from_millis(self.config.retry_delay_ms);

        loop {
            match self.try_publish(exchange, envelope).await {
                Ok(_) => {
                    if self.config.enable_deduplication {
                        let mut published_messages =
                            self.published_messages.lock().map_err(|_| {
                                MatchmakingError::InternalError {
                                    message: "Failed to acquire published messages lock"
                                        .to_string(),
                                }
                            })?;
                        published_messages.insert(envelope.correlation_id.clone());
                    }

                    debug!(
                        "Successfully published message {} to exchange {}",
                        envelope.correlation_id, exchange
                    );
                    return Ok(());
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count > self.config.max_retries {
                        error!(
                            "Failed to publish message {} after {} retries: {}",
                            envelope.correlation_id, self.config.max_retries, e
                        );
                        return Err(e);
                    }

                    warn!(
                        "Publish attempt {} failed for message {}: {}. Retrying in {:?}",
                        retry_count, envelope.correlation_id, e, delay
                    );

                    sleep(delay).await;
                    delay = Duration::from_millis((delay.as_millis() as u64 * 2).min(5000));
                }
            }
        }
    }

    /// Single publish attempt
    async fn try_publish<T>(&self, exchange: &str, envelope: &MessageEnvelope<T>) -> Result<()>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let payload = envelope.to_bytes()?;

        let args = BasicPublishArguments::new(exchange, &envelope.routing_key);
        let mut properties = BasicProperties::default();
        properties
            .with_message_id(&envelope.correlation_id)
            .with_timestamp(envelope.timestamp.timestamp() as u64)
            .with_content_type("application/json");

        self.channel
            .basic_publish(properties, payload, args)
            .await
            .map_err(|e| MatchmakingError::AmqpConnectionFailed {
                message: format!("Failed to publish message: {}", e),
            })?;

        Ok(())
    }

    /// Clear deduplication cache (useful for testing or memory management)
    pub fn clear_deduplication_cache(&self) {
        if let Ok(mut published_messages) = self.published_messages.lock() {
            published_messages.clear();
        }
    }

    /// Get number of cached message IDs (for monitoring)
    pub fn cached_message_count(&self) -> usize {
        self.published_messages
            .lock()
            .map(|cache| cache.len())
            .unwrap_or(0)
    }
}

#[async_trait]
impl EventPublisher for AmqpEventPublisher {
    async fn publish_player_joined_lobby(&self, event: PlayerJoinedLobby) -> Result<()> {
        let envelope = MessageEnvelope::new(event, "player.joined".to_string());
        self.publish_to_exchange(PLAYER_EVENTS_EXCHANGE, &envelope)
            .await
    }

    async fn publish_player_left_lobby(&self, event: PlayerLeftLobby) -> Result<()> {
        let envelope = MessageEnvelope::new(event, "player.left".to_string());
        self.publish_to_exchange(PLAYER_EVENTS_EXCHANGE, &envelope)
            .await
    }

    async fn publish_game_starting(&self, event: GameStarting) -> Result<()> {
        let envelope = MessageEnvelope::new(event, "game.starting".to_string());
        self.publish_to_exchange(GAME_EVENTS_EXCHANGE, &envelope)
            .await
    }
}

/// Mock event publisher for testing
#[derive(Debug, Default)]
pub struct MockEventPublisher {
    published_events: std::sync::Mutex<Vec<String>>,
}

impl MockEventPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all published event types (for testing)
    pub fn get_published_events(&self) -> Vec<String> {
        self.published_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    /// Clear published events (for testing)
    pub fn clear_events(&self) {
        if let Ok(mut events) = self.published_events.lock() {
            events.clear();
        }
    }
}

#[async_trait]
impl EventPublisher for MockEventPublisher {
    async fn publish_player_joined_lobby(&self, _event: PlayerJoinedLobby) -> Result<()> {
        if let Ok(mut events) = self.published_events.lock() {
            events.push("PlayerJoinedLobby".to_string());
        }
        Ok(())
    }

    async fn publish_player_left_lobby(&self, _event: PlayerLeftLobby) -> Result<()> {
        if let Ok(mut events) = self.published_events.lock() {
            events.push("PlayerLeftLobby".to_string());
        }
        Ok(())
    }

    async fn publish_game_starting(&self, _event: GameStarting) -> Result<()> {
        if let Ok(mut events) = self.published_events.lock() {
            events.push("GameStarting".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils;

    fn create_test_player_joined_event() -> PlayerJoinedLobby {
        PlayerJoinedLobby {
            lobby_id: utils::generate_lobby_id(),
            player_id: "test_player".to_string(),
            player_type: PlayerType::Human,
            current_players: vec![],
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_publisher_config_default() {
        let config = PublisherConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_delay_ms, 500);
        assert!(config.enable_deduplication);
    }

    #[test]
    fn test_message_envelope_creation() {
        let event = create_test_player_joined_event();
        let envelope = MessageEnvelope::new(event, "player.joined".to_string());

        assert_eq!(envelope.routing_key, "player.joined");
        assert!(!envelope.correlation_id.is_empty());
    }

    // Note: Integration tests with actual AMQP broker would go in tests/ directory
    // These would test the actual publishing functionality
}
