//! Configuration management for the parlor-room service
//!
//! This module handles all configuration loading from environment variables,
//! validation, and default values for the matchmaking service.

pub mod amqp;
pub mod app;
pub mod lobby;
pub mod rating;

// Re-export commonly used types
pub use amqp::AmqpConfig;
pub use app::{validate_config, AmqpSettings, AppConfig, ServiceSettings};
pub use lobby::LobbyConfig;
pub use rating::RatingConfig;
