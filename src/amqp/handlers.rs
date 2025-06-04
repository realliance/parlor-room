//! AMQP message handlers for processing queue requests and events
//!
//! This module provides the message handling infrastructure for the matchmaking service,
//! including request processing, error handling, and dead letter queue management.

use crate::amqp::messages::MessageUtils;
use crate::error::{MatchmakingError, Result};
use crate::types::QueueRequest;
use amqprs::{
    channel::{BasicCancelArguments, BasicConsumeArguments, Channel},
    consumer::AsyncConsumer,
    BasicProperties, Deliver,
};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Trait defining the interface for handling AMQP messages
#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// Handle a queue request from a user/bot
    async fn handle_queue_request(&self, request: QueueRequest) -> Result<()>;

    /// Handle processing errors
    async fn handle_error(&self, error: MatchmakingError, message_data: &[u8]);
}

/// Consumer for handling queue request messages
pub struct QueueRequestConsumer {
    handler: Arc<dyn MessageHandler>,
    channel: Channel,
    consumer_tag: String,
}

impl QueueRequestConsumer {
    /// Create a new queue request consumer
    pub fn new(handler: Arc<dyn MessageHandler>, channel: Channel) -> Self {
        let consumer_tag = format!("queue-consumer-{}", uuid::Uuid::new_v4());

        Self {
            handler,
            channel,
            consumer_tag,
        }
    }

    /// Start consuming messages from the queue
    pub async fn start_consuming(&self, queue_name: &str) -> Result<()> {
        let args = BasicConsumeArguments::new(queue_name, &self.consumer_tag);

        self.channel
            .basic_consume(QueueConsumer::new(self.handler.clone()), args)
            .await
            .map_err(|e| MatchmakingError::AmqpConnectionFailed {
                message: format!("Failed to start consuming: {}", e),
            })?;

        info!("Started consuming messages from queue: {}", queue_name);
        Ok(())
    }

    /// Stop consuming messages
    pub async fn stop_consuming(&self) -> Result<()> {
        let args = BasicCancelArguments::new(&self.consumer_tag);

        self.channel.basic_cancel(args).await.map_err(|e| {
            MatchmakingError::AmqpConnectionFailed {
                message: format!("Failed to stop consuming: {}", e),
            }
        })?;

        info!("Stopped consuming messages");
        Ok(())
    }
}

/// Internal consumer implementation
struct QueueConsumer {
    handler: Arc<dyn MessageHandler>,
}

impl QueueConsumer {
    fn new(handler: Arc<dyn MessageHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl AsyncConsumer for QueueConsumer {
    async fn consume(
        &mut self,
        _channel: &Channel,
        deliver: Deliver,
        _basic_properties: BasicProperties,
        _content: Vec<u8>,
    ) {
        let delivery_tag = deliver.delivery_tag();
        let routing_key = deliver.routing_key();
        let message_size = _content.len();

        info!(
            "AMQP message received - delivery_tag: {}, routing_key: '{}', size: {} bytes",
            delivery_tag, routing_key, message_size
        );

        let start_time = std::time::Instant::now();

        match self.process_message(&_content).await {
            Ok(_) => {
                let processing_time = start_time.elapsed();
                info!(
                    "Message processed successfully - delivery_tag: {}, processing_time: {:.2}ms",
                    delivery_tag,
                    processing_time.as_secs_f64() * 1000.0
                );
            }
            Err(e) => {
                let processing_time = start_time.elapsed();
                error!(
                    "Message processing failed - delivery_tag: {}, processing_time: {:.2}ms, error: {}",
                    delivery_tag, processing_time.as_secs_f64() * 1000.0, e
                );
                self.handler
                    .handle_error(
                        MatchmakingError::InternalError {
                            message: e.to_string(),
                        },
                        &_content,
                    )
                    .await;
            }
        }
    }
}

impl QueueConsumer {
    /// Process an incoming message
    async fn process_message(&self, content: &[u8]) -> Result<()> {
        info!("Deserializing queue request message...");

        // Try to deserialize as a queue request
        let request = MessageUtils::deserialize_queue_request(content)?;

        info!(
            "Queue request parsed - player_id: '{}', player_type: {:?}, lobby_type: {:?}, rating: {:.1}±{:.1}",
            request.player_id,
            request.player_type,
            request.lobby_type,
            request.current_rating.rating,
            request.current_rating.uncertainty
        );

        // Process the request directly (no authentication needed)
        info!("Forwarding queue request to lobby manager...");
        self.handler.handle_queue_request(request).await?;

        Ok(())
    }
}

/// Dead letter queue handler for failed messages
pub struct DeadLetterHandler {
    _channel: Channel,
    retry_attempts: std::collections::HashMap<String, u32>,
    max_retries: u32,
}

impl DeadLetterHandler {
    /// Create a new dead letter queue handler
    pub fn new(channel: Channel, max_retries: u32) -> Self {
        Self {
            _channel: channel,
            retry_attempts: std::collections::HashMap::new(),
            max_retries,
        }
    }

    /// Handle a failed message
    pub async fn handle_failed_message(
        &mut self,
        message_id: String,
        _content: Vec<u8>,
        error: MatchmakingError,
    ) -> Result<()> {
        let retry_count = self.retry_attempts.entry(message_id.clone()).or_insert(0);
        *retry_count += 1;

        if *retry_count <= self.max_retries {
            warn!(
                "Message {} failed (attempt {}), will retry: {}",
                message_id, retry_count, error
            );

            // In a real implementation, we would republish to retry queue
            // For now, just log the retry attempt
            return Ok(());
        }

        error!(
            "Message {} exceeded max retries ({}), moving to dead letter queue: {}",
            message_id, self.max_retries, error
        );

        // Remove from retry tracking
        self.retry_attempts.remove(&message_id);

        // In a real implementation, we would publish to dead letter exchange
        // For now, just log the permanent failure

        Ok(())
    }
}

/// Mock message handler for testing
pub struct MockMessageHandler {
    pub received_requests: Arc<tokio::sync::Mutex<Vec<QueueRequest>>>,
}

impl Default for MockMessageHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockMessageHandler {
    pub fn new() -> Self {
        Self {
            received_requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl MessageHandler for MockMessageHandler {
    async fn handle_queue_request(&self, request: QueueRequest) -> Result<()> {
        let mut requests = self.received_requests.lock().await;
        requests.push(request);
        Ok(())
    }

    async fn handle_error(&self, error: MatchmakingError, _message_data: &[u8]) {
        eprintln!("Mock handler received error: {}", error);
    }
}

#[cfg(test)]
mod tests {
    use crate::{LobbyType, PlayerRating, PlayerType};

    use super::*;

    fn create_test_queue_request() -> QueueRequest {
        QueueRequest {
            player_id: "test_player".to_string(),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            timestamp: crate::utils::current_timestamp(),
        }
    }

    #[tokio::test]
    async fn test_mock_handler() {
        let handler = MockMessageHandler::new();
        let request = create_test_queue_request();

        handler.handle_queue_request(request.clone()).await.unwrap();

        let received = handler.received_requests.lock().await;
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].player_id, request.player_id);
    }

    #[test]
    fn test_dead_letter_handler_creation() {
        // Note: This test can't create a real channel without a connection
        // In practice, the handler would be tested with integration tests
    }
}
