//! AMQP message definitions and serialization

use crate::error::{MatchmakingError, Result};
use crate::types::*;
use serde_json;

/// AMQP queue names
pub const QUEUE_REQUEST_QUEUE: &str = "matchmaking.queue_requests";
pub const PLAYER_EVENTS_EXCHANGE: &str = "matchmaking.player_events";
pub const GAME_EVENTS_EXCHANGE: &str = "matchmaking.game_events";

/// Routing keys for events
pub const PLAYER_JOINED_ROUTING_KEY: &str = "player.joined";
pub const PLAYER_LEFT_ROUTING_KEY: &str = "player.left";
pub const GAME_STARTING_ROUTING_KEY: &str = "game.starting";

/// Message envelope with metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessageEnvelope<T> {
    pub payload: T,
    pub correlation_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub routing_key: String,
}

impl<T> MessageEnvelope<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    /// Create a new message envelope
    pub fn new(payload: T, routing_key: String) -> Self {
        Self {
            payload,
            correlation_id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            routing_key,
        }
    }

    /// Serialize the envelope to JSON bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| {
            MatchmakingError::InternalError {
                message: format!("Failed to serialize message: {}", e),
            }
            .into()
        })
    }

    /// Deserialize envelope from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| {
            MatchmakingError::InvalidQueueRequest {
                reason: format!("Failed to deserialize message: {}", e),
            }
            .into()
        })
    }
}

/// Message serialization and validation utilities
pub struct MessageUtils;

impl MessageUtils {
    /// Serialize a queue request to bytes
    pub fn serialize_queue_request(request: &QueueRequest) -> Result<Vec<u8>> {
        Self::validate_queue_request(request)?;
        serde_json::to_vec(request).map_err(|e| {
            MatchmakingError::InternalError {
                message: format!("Failed to serialize queue request: {}", e),
            }
            .into()
        })
    }

    /// Deserialize queue request from bytes
    pub fn deserialize_queue_request(bytes: &[u8]) -> Result<QueueRequest> {
        let request: QueueRequest =
            serde_json::from_slice(bytes).map_err(|e| MatchmakingError::InvalidQueueRequest {
                reason: format!("Failed to deserialize queue request: {}", e),
            })?;

        Self::validate_queue_request(&request)?;
        Ok(request)
    }

    /// Validate a queue request
    pub fn validate_queue_request(request: &QueueRequest) -> Result<()> {
        if request.player_id.is_empty() {
            return Err(MatchmakingError::InvalidQueueRequest {
                reason: "Player ID cannot be empty".to_string(),
            }
            .into());
        }

        if request.current_rating.rating < 0.0 {
            return Err(MatchmakingError::InvalidQueueRequest {
                reason: "Rating cannot be negative".to_string(),
            }
            .into());
        }

        if request.current_rating.uncertainty < 0.0 {
            return Err(MatchmakingError::InvalidQueueRequest {
                reason: "Uncertainty cannot be negative".to_string(),
            }
            .into());
        }

        // Bot requests must have authentication token
        if request.player_type == PlayerType::Bot && request.auth_token.is_none() {
            return Err(MatchmakingError::InvalidQueueRequest {
                reason: "Bot requests must include authentication token".to_string(),
            }
            .into());
        }

        Ok(())
    }

    /// Serialize any AMQP message to bytes
    pub fn serialize_message<T: serde::Serialize>(message: &T) -> Result<Vec<u8>> {
        serde_json::to_vec(message).map_err(|e| {
            MatchmakingError::InternalError {
                message: format!("Failed to serialize message: {}", e),
            }
            .into()
        })
    }

    /// Get routing key for a message type
    pub fn get_routing_key(message: &AmqpMessage) -> &'static str {
        match message {
            AmqpMessage::QueueRequest(_) => "queue.request",
            AmqpMessage::PlayerJoinedLobby(_) => PLAYER_JOINED_ROUTING_KEY,
            AmqpMessage::PlayerLeftLobby(_) => PLAYER_LEFT_ROUTING_KEY,
            AmqpMessage::GameStarting(_) => GAME_STARTING_ROUTING_KEY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn create_test_queue_request() -> QueueRequest {
        QueueRequest {
            player_id: "test_player".to_string(),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            timestamp: chrono::Utc::now(),
            auth_token: None,
        }
    }

    #[test]
    fn test_message_envelope_creation() {
        let request = create_test_queue_request();
        let envelope = MessageEnvelope::new(request, "test.routing.key".to_string());

        assert_eq!(envelope.routing_key, "test.routing.key");
        assert!(!envelope.correlation_id.is_empty());
    }

    #[test]
    fn test_queue_request_validation() {
        let valid_request = create_test_queue_request();
        assert!(MessageUtils::validate_queue_request(&valid_request).is_ok());

        // Test empty player ID
        let mut invalid_request = create_test_queue_request();
        invalid_request.player_id = "".to_string();
        assert!(MessageUtils::validate_queue_request(&invalid_request).is_err());

        // Test negative rating
        let mut invalid_request = create_test_queue_request();
        invalid_request.current_rating.rating = -100.0;
        assert!(MessageUtils::validate_queue_request(&invalid_request).is_err());

        // Test bot without auth token
        let mut invalid_request = create_test_queue_request();
        invalid_request.player_type = PlayerType::Bot;
        invalid_request.auth_token = None;
        assert!(MessageUtils::validate_queue_request(&invalid_request).is_err());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let request = create_test_queue_request();
        let bytes = MessageUtils::serialize_queue_request(&request).unwrap();
        let deserialized = MessageUtils::deserialize_queue_request(&bytes).unwrap();

        assert_eq!(request.player_id, deserialized.player_id);
        assert_eq!(request.player_type, deserialized.player_type);
        assert_eq!(request.lobby_type, deserialized.lobby_type);
    }

    #[test]
    fn test_routing_key_generation() {
        let queue_request = AmqpMessage::QueueRequest(create_test_queue_request());
        assert_eq!(
            MessageUtils::get_routing_key(&queue_request),
            "queue.request"
        );

        let player_joined = AmqpMessage::PlayerJoinedLobby(PlayerJoinedLobby {
            lobby_id: uuid::Uuid::new_v4(),
            player_id: "test".to_string(),
            player_type: PlayerType::Human,
            current_players: vec![],
            timestamp: chrono::Utc::now(),
        });
        assert_eq!(
            MessageUtils::get_routing_key(&player_joined),
            PLAYER_JOINED_ROUTING_KEY
        );
    }
}
