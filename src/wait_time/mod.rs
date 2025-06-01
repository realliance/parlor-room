//! Wait time calculation system for dynamic lobby timeouts
//!
//! This module handles wait time statistics tracking and dynamic calculation
//! for determining when lobbies should be backfilled with bots.

pub mod calculator;
pub mod provider;
pub mod statistics;

// Re-export commonly used types
pub use calculator::{DynamicWaitTimeCalculator, WaitTimeCalculator};
pub use provider::{InternalWaitTimeProvider, WaitTimeProvider};
pub use statistics::{StatisticsTracker, WaitTimeStats};
