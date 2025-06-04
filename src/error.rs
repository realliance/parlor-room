//! Error types for the matchmaking service
//!
//! This module defines all error types using anyhow for consistent error handling
//! throughout the application.

/// Result type alias for convenience
pub type Result<T> = anyhow::Result<T>;

/// Custom error types for specific matchmaking scenarios
#[derive(Debug, thiserror::Error)]
pub enum MatchmakingError {
    #[error("AMQP connection failed: {message}")]
    AmqpConnectionFailed { message: String },

    #[error("Invalid queue request: {reason}")]
    InvalidQueueRequest { reason: String },

    #[error("Lobby not found: {lobby_id}")]
    LobbyNotFound { lobby_id: String },

    #[error("Lobby is full: {lobby_id}")]
    LobbyFull { lobby_id: String },

    #[error("Player not found: {player_id}")]
    PlayerNotFound { player_id: String },

    #[error("Rating calculation failed: {reason}")]
    RatingCalculationFailed { reason: String },

    #[error("Wait time calculation failed: {reason}")]
    WaitTimeCalculationFailed { reason: String },

    #[error("Configuration error: {message}")]
    ConfigurationError { message: String },

    #[error("Internal service error: {message}")]
    InternalError { message: String },
}
