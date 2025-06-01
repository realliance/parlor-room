//! Service layer for the parlor-room matchmaking service
//!
//! This module contains the main application state, service coordination,
//! and background task management for the production service.

pub mod app;
pub mod health;

pub use app::{AppState, ServiceError};
pub use health::{HealthCheck, HealthStatus};
