//! Bot integration system for dual bot entry methods
//!
//! This module provides two ways for bots to enter lobbies:
//! 1. Active Queuing: Bots send QueueRequest messages like humans
//! 2. Automatic Backfilling: System adds bots when wait times expire

pub mod backfill;
pub mod provider;

// Re-export commonly used types
pub use backfill::{BackfillManager, BackfillTrigger};
pub use provider::{BotProvider, BotSelectionCriteria, MockBotProvider};
