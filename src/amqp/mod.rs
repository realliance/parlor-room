//! AMQP integration for the matchmaking service
//!
//! This module handles all AMQP connections, message handling, and event publishing
//! for the matchmaking microservice.

pub mod connection;
pub mod handlers;
pub mod messages;
pub mod publisher;

// Re-export commonly used types
pub use connection::{AmqpConnection, AmqpConnectionPool};
pub use handlers::MessageHandler;
pub use messages::*;
pub use publisher::EventPublisher;
