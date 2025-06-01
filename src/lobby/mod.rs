//! Lobby management system for the matchmaking service
//!
//! This module handles lobby creation, management, and player matching.
//! It supports both AllBot and General lobby types with different behaviors.

pub mod instance;
pub mod manager;
pub mod matching;
pub mod provider;

// Re-export commonly used types
pub use instance::{Lobby, LobbyInstance, LobbyState};
pub use manager::LobbyManager;
pub use matching::{LobbyMatcher, MatchingResult};
pub use provider::{LobbyConfiguration, LobbyProvider, StaticLobbyProvider};
