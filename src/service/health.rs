//! Health check endpoints and monitoring
//!
//! This module provides health check functionality for the parlor-room
//! matchmaking service, including readiness and liveness probes.

use crate::service::app::AppState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, error};

/// Health check status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "✅ healthy"),
            HealthStatus::Degraded => write!(f, "⚠️  degraded"),
            HealthStatus::Unhealthy => write!(f, "❌ unhealthy"),
        }
    }
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Overall service status
    pub status: HealthStatus,
    /// Service name
    pub service: String,
    /// Service version (could be from environment)
    pub version: String,
    /// Current timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Detailed component checks
    pub checks: Vec<ComponentCheck>,
    /// Service statistics
    pub stats: ServiceStats,
}

/// Individual component health check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentCheck {
    /// Component name
    pub name: String,
    /// Component status
    pub status: HealthStatus,
    /// Optional error message if unhealthy
    pub message: Option<String>,
    /// Check duration in milliseconds
    pub duration_ms: u64,
}

/// Service statistics for health reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStats {
    /// Number of active lobbies
    pub active_lobbies: usize,
    /// Total players currently waiting
    pub players_waiting: usize,
    /// Total games started since service start
    pub games_started: u64,
    /// Total players matched since service start
    pub players_matched: u64,
    /// Service uptime information
    pub uptime_info: String,
}

impl HealthCheck {
    /// Perform a comprehensive health check of the service
    pub async fn check(app_state: Arc<AppState>) -> Result<Self> {
        let start_time = std::time::Instant::now();
        let mut checks = Vec::new();
        let mut overall_status = HealthStatus::Healthy;

        // Check if service is running
        let service_check = Self::check_service_running(&app_state).await;
        if service_check.status != HealthStatus::Healthy {
            overall_status = HealthStatus::Unhealthy;
        }
        checks.push(service_check);

        // Check lobby manager
        let lobby_check = Self::check_lobby_manager(&app_state).await;
        if lobby_check.status == HealthStatus::Unhealthy {
            overall_status = HealthStatus::Unhealthy;
        } else if lobby_check.status == HealthStatus::Degraded
            && overall_status == HealthStatus::Healthy
        {
            overall_status = HealthStatus::Degraded;
        }
        checks.push(lobby_check);

        // Check AMQP connectivity (simplified)
        let amqp_check = Self::check_amqp_health(&app_state).await;
        if amqp_check.status == HealthStatus::Unhealthy {
            overall_status = HealthStatus::Unhealthy;
        } else if amqp_check.status == HealthStatus::Degraded
            && overall_status == HealthStatus::Healthy
        {
            overall_status = HealthStatus::Degraded;
        }
        checks.push(amqp_check);

        // Gather service statistics
        let stats = Self::gather_service_stats(&app_state).await;

        Ok(HealthCheck {
            status: overall_status,
            service: app_state.config().service.name.clone(),
            version: std::env::var("SERVICE_VERSION").unwrap_or_else(|_| "unknown".to_string()),
            timestamp: chrono::Utc::now(),
            checks,
            stats,
        })
    }

    /// Simple liveness check - just verify service is running
    pub async fn liveness_check(app_state: Arc<AppState>) -> Result<HealthStatus> {
        if app_state.is_running().await {
            Ok(HealthStatus::Healthy)
        } else {
            Ok(HealthStatus::Unhealthy)
        }
    }

    /// Readiness check - verify service can handle requests
    pub async fn readiness_check(app_state: Arc<AppState>) -> Result<HealthStatus> {
        // Service must be running
        if !app_state.is_running().await {
            return Ok(HealthStatus::Unhealthy);
        }

        // Check if lobby manager is accessible
        match Self::check_lobby_manager(&app_state).await.status {
            HealthStatus::Healthy => Ok(HealthStatus::Healthy),
            HealthStatus::Degraded => Ok(HealthStatus::Degraded),
            HealthStatus::Unhealthy => Ok(HealthStatus::Unhealthy),
        }
    }

    /// Check if service is running
    async fn check_service_running(app_state: &AppState) -> ComponentCheck {
        let start = std::time::Instant::now();

        let (status, message) = if app_state.is_running().await {
            (HealthStatus::Healthy, None)
        } else {
            (
                HealthStatus::Unhealthy,
                Some("Service is not running".to_string()),
            )
        };

        ComponentCheck {
            name: "service_running".to_string(),
            status,
            message,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Check lobby manager health
    async fn check_lobby_manager(app_state: &AppState) -> ComponentCheck {
        let start = std::time::Instant::now();

        let (status, message) = match app_state.lobby_manager().try_read() {
            Ok(manager) => {
                // Try to get stats to verify functionality
                match manager.get_stats().await {
                    Ok(_stats) => (HealthStatus::Healthy, None),
                    Err(e) => {
                        error!("Lobby manager stats check failed: {}", e);
                        (
                            HealthStatus::Degraded,
                            Some(format!("Stats check failed: {}", e)),
                        )
                    }
                }
            }
            Err(_) => (
                HealthStatus::Unhealthy,
                Some("Cannot access lobby manager".to_string()),
            ),
        };

        ComponentCheck {
            name: "lobby_manager".to_string(),
            status,
            message,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Check AMQP health (simplified)
    async fn check_amqp_health(_app_state: &AppState) -> ComponentCheck {
        let start = std::time::Instant::now();

        // In a real implementation, we'd ping the AMQP connection
        // For now, assume healthy if we have connection
        let status = HealthStatus::Healthy;
        let message = None;

        ComponentCheck {
            name: "amqp_connection".to_string(),
            status,
            message,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Gather current service statistics
    async fn gather_service_stats(app_state: &AppState) -> ServiceStats {
        let default_stats = ServiceStats {
            active_lobbies: 0,
            players_waiting: 0,
            games_started: 0,
            players_matched: 0,
            uptime_info: "Service running".to_string(),
        };

        match app_state.lobby_manager().try_read() {
            Ok(manager) => match manager.get_stats().await {
                Ok(lobby_stats) => ServiceStats {
                    active_lobbies: lobby_stats.active_lobbies,
                    players_waiting: lobby_stats.players_waiting,
                    games_started: lobby_stats.games_started,
                    players_matched: lobby_stats.players_queued,
                    uptime_info: format!(
                        "Lobbies created: {}, cleaned: {}",
                        lobby_stats.lobbies_created, lobby_stats.lobbies_cleaned
                    ),
                },
                Err(e) => {
                    debug!("Failed to get lobby stats for health check: {}", e);
                    default_stats
                }
            },
            Err(_) => default_stats,
        }
    }
}

/// Convert health check to JSON string
impl HealthCheck {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize health check: {}", e))
    }
}
