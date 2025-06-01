//! Parlor Room - A sophisticated matchmaking microservice
//!
//! This crate provides a complete matchmaking solution with:
//! - AMQP-based message queuing
//! - Dynamic lobby management
//! - Weng-Lin rating system integration
//! - Intelligent bot backfilling
//! - Real-time event publishing

// Core modules
pub mod amqp;
pub mod bot;
pub mod config;
pub mod error;
pub mod lobby;
pub mod metrics;
pub mod rating;
pub mod service;
pub mod types;
pub mod utils;
pub mod wait_time;

// Re-export commonly used types
pub use config::AppConfig;
pub use error::{MatchmakingError, Result};
pub use service::{AppState, HealthCheck, ServiceError};
pub use types::*;

// Re-export key components
pub use amqp::publisher::EventPublisher;
pub use lobby::{LobbyManager, LobbyProvider, StaticLobbyProvider};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
