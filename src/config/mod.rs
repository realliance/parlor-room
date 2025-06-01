//! Configuration management for the matchmaking service

pub mod amqp;
pub mod lobby;
pub mod rating;

// Re-export commonly used types
pub use amqp::AmqpConfig;
pub use lobby::LobbyConfig;
pub use rating::RatingConfig;
