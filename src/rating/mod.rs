//! Rating system integration using Weng-Lin (OpenSkill) algorithm
//!
//! This module provides rating calculations, storage interfaces, and
//! integration with the skillratings crate for multi-player ranking.

pub mod calculator;
pub mod storage;
pub mod weng_lin;

// Re-export commonly used types
pub use calculator::{RatingCalculationResult, RatingCalculator};
pub use storage::{InMemoryRatingStorage, RatingStorage};
pub use weng_lin::{ExtendedWengLinConfig, WengLinRatingCalculator};
