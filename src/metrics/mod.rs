//! Metrics and monitoring for the parlor-room matchmaking service
//!
//! This module provides comprehensive metrics collection, health monitoring,
//! and performance tracking for the matchmaking service.

pub mod collector;
pub mod health;

pub use collector::{
    BotMetrics, LobbyMetrics, MetricsCollector, PerformanceMetrics, PlayerMetrics, ServiceMetrics,
};
pub use health::{HealthEndpoints, HealthServer};

use std::sync::Arc;

/// Unified metrics service that combines all monitoring capabilities
#[derive(Clone)]
pub struct MetricsService {
    collector: Arc<MetricsCollector>,
    health_server: Arc<HealthServer>,
}

impl MetricsService {
    /// Create a new metrics service
    pub fn new(collector: Arc<MetricsCollector>, health_server: Arc<HealthServer>) -> Self {
        Self {
            collector,
            health_server,
        }
    }

    /// Get the metrics collector
    pub fn collector(&self) -> Arc<MetricsCollector> {
        self.collector.clone()
    }

    /// Get the health server
    pub fn health_server(&self) -> Arc<HealthServer> {
        self.health_server.clone()
    }

    /// Start the metrics service (health endpoints)
    pub async fn start(&self) -> anyhow::Result<()> {
        self.health_server.start().await
    }

    /// Stop the metrics service
    pub async fn stop(&self) -> anyhow::Result<()> {
        self.health_server.stop().await
    }
}
