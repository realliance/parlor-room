//! Parlor Room - Matchmaking microservice for mahjong games
//!
//! This crate provides AMQP-based matchmaking with lobby management,
//! rating systems, and bot integration for mahjong game rooms.

pub mod amqp;
pub mod bot;
pub mod config;
pub mod error;
pub mod lobby;
pub mod metrics;
pub mod rating;
pub mod types;
pub mod utils;
pub mod wait_time;

// Re-export commonly used types and traits
pub use error::{MatchmakingError, Result};
pub use types::*;

// Re-export key components
pub use amqp::publisher::EventPublisher;
pub use lobby::{LobbyManager, LobbyProvider, StaticLobbyProvider};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
