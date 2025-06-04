//! Health check endpoints and Prometheus metrics server
//!
//! This module provides HTTP endpoints for health checks and Prometheus metrics
//! for the parlor-room matchmaking service using Axum.

use crate::metrics::collector::MetricsCollector;
use crate::service::app::AppState;
use crate::service::health::{HealthCheck, HealthStatus};
use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

/// Health server configuration
#[derive(Debug, Clone)]
pub struct HealthServerConfig {
    /// Port to bind the health server to
    pub port: u16,
    /// Host to bind to (typically "0.0.0.0" for all interfaces)
    pub host: String,
}

impl Default for HealthServerConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            host: "0.0.0.0".to_string(),
        }
    }
}

/// Shared state for the health server
#[derive(Clone)]
pub struct HealthServerState {
    pub metrics_collector: Arc<MetricsCollector>,
    pub app_state: Option<Arc<AppState>>,
}

/// Health server that provides HTTP endpoints for monitoring
pub struct HealthServer {
    config: HealthServerConfig,
    state: HealthServerState,
    shutdown_tx: broadcast::Sender<()>,
    shutdown_rx: broadcast::Receiver<()>,
}

impl HealthServer {
    /// Create a new health server
    pub fn new(config: HealthServerConfig, metrics_collector: Arc<MetricsCollector>) -> Self {
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        Self {
            config,
            state: HealthServerState {
                metrics_collector,
                app_state: None,
            },
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Set the application state for health checks
    pub fn with_app_state(mut self, app_state: Arc<AppState>) -> Self {
        self.state.app_state = Some(app_state);
        self
    }

    /// Start the health server
    pub async fn start(&self) -> Result<()> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .context("Invalid health server address")?;

        let app = self.create_router();
        let listener = TcpListener::bind(addr).await?;

        info!("Health server listening on http://{}", addr);

        // Create a shutdown receiver for this task
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Serve with graceful shutdown
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.recv().await;
                info!("Health server shutdown signal received");
            })
            .await?;

        info!("Health server stopped");
        Ok(())
    }

    /// Create the Axum router with all health endpoints
    fn create_router(&self) -> Router {
        Router::new()
            .route("/", get(root_handler))
            .route("/health", get(health_handler))
            .route("/ready", get(ready_handler))
            .route("/alive", get(alive_handler))
            .route("/metrics", get(metrics_handler))
            .route("/stats", get(stats_handler))
            .with_state(self.state.clone())
    }

    /// Stop the health server
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping health server...");

        if let Err(e) = self.shutdown_tx.send(()) {
            warn!("Failed to send shutdown signal to health server: {}", e);
        }

        info!("Health server stop signal sent");
        Ok(())
    }
}

/// Root endpoint handler - shows service information
async fn root_handler() -> impl IntoResponse {
    let info = json!({
        "service": "parlor-room",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": [
            "/health",
            "/ready",
            "/alive",
            "/metrics",
            "/stats"
        ]
    });

    Json(info)
}

/// Lightweight health check endpoint handler
async fn health_handler(State(state): State<HealthServerState>) -> impl IntoResponse {
    debug!("Health check requested");

    match &state.app_state {
        Some(app_state) => match HealthCheck::liveness_check(app_state.clone()).await {
            Ok(HealthStatus::Healthy) => (
                StatusCode::OK,
                Json(json!({
                    "status": "healthy",
                    "service": "parlor-room",
                    "version": env!("CARGO_PKG_VERSION")
                })),
            ),
            Ok(HealthStatus::Degraded) => (
                StatusCode::OK,
                Json(json!({
                    "status": "degraded",
                    "service": "parlor-room",
                    "version": env!("CARGO_PKG_VERSION")
                })),
            ),
            Ok(HealthStatus::Unhealthy) | Err(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "unhealthy",
                    "service": "parlor-room",
                    "version": env!("CARGO_PKG_VERSION")
                })),
            ),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "unhealthy",
                "service": "parlor-room",
                "version": env!("CARGO_PKG_VERSION"),
                "error": "Service not initialized"
            })),
        ),
    }
}

/// Readiness check endpoint handler
async fn ready_handler(State(state): State<HealthServerState>) -> impl IntoResponse {
    debug!("Readiness check requested");

    match &state.app_state {
        Some(app_state) => match HealthCheck::readiness_check(app_state.clone()).await {
            Ok(HealthStatus::Healthy) => (StatusCode::OK, "Ready"),
            Ok(HealthStatus::Degraded) => (StatusCode::OK, "Degraded but ready"),
            Ok(HealthStatus::Unhealthy) => (StatusCode::SERVICE_UNAVAILABLE, "Not ready"),
            Err(e) => {
                error!("Readiness check failed: {}", e);
                (StatusCode::SERVICE_UNAVAILABLE, "Not ready")
            }
        },
        None => (StatusCode::SERVICE_UNAVAILABLE, "Service not initialized"),
    }
}

/// Liveness check endpoint handler
async fn alive_handler(State(state): State<HealthServerState>) -> impl IntoResponse {
    debug!("Liveness check requested");

    match &state.app_state {
        Some(app_state) => match HealthCheck::liveness_check(app_state.clone()).await {
            Ok(HealthStatus::Healthy) => (StatusCode::OK, "Alive"),
            _ => (StatusCode::SERVICE_UNAVAILABLE, "Not alive"),
        },
        None => (StatusCode::SERVICE_UNAVAILABLE, "Service not initialized"),
    }
}

/// Prometheus metrics endpoint handler
async fn metrics_handler(State(state): State<HealthServerState>) -> impl IntoResponse {
    debug!("Metrics endpoint requested");

    let registry = state.metrics_collector.registry();
    let metric_families = registry.gather();
    let encoder = TextEncoder::new();

    match encoder.encode_to_string(&metric_families) {
        Ok(metrics_output) => {
            debug!("Serving {} metric families", metric_families.len());

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", encoder.format_type())
                .body(metrics_output)
                .unwrap()
        }
        Err(e) => {
            error!("Failed to encode metrics: {}", e);

            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body("Failed to encode metrics".to_string())
                .unwrap()
        }
    }
}

/// Detailed service statistics endpoint handler (for debugging/human consumption)
async fn stats_handler(State(state): State<HealthServerState>) -> impl IntoResponse {
    debug!("Stats endpoint requested");

    match &state.app_state {
        Some(app_state) => {
            match HealthCheck::check(app_state.clone()).await {
                Ok(health) => {
                    // Combine service stats with some basic metrics info
                    let stats = json!({
                        "service": {
                            "name": "parlor-room",
                            "version": env!("CARGO_PKG_VERSION"),
                            "status": health.status,
                            "uptime": health.stats.uptime_info
                        },
                        "lobbies": {
                            "active": health.stats.active_lobbies,
                            "games_started": health.stats.games_started
                        },
                        "players": {
                            "waiting": health.stats.players_waiting,
                            "matched": health.stats.players_matched
                        },
                        "components": health.checks,
                        "timestamp": chrono::Utc::now()
                    });

                    (StatusCode::OK, Json(stats))
                }
                Err(e) => {
                    error!("Failed to get stats: {}", e);

                    let error_response = json!({
                        "service": {
                            "name": "parlor-room",
                            "version": env!("CARGO_PKG_VERSION"),
                            "status": "error"
                        },
                        "error": "Failed to get service stats",
                        "timestamp": chrono::Utc::now()
                    });

                    (StatusCode::SERVICE_UNAVAILABLE, Json(error_response))
                }
            }
        }
        None => {
            let error_response = json!({
                "service": {
                    "name": "parlor-room",
                    "version": env!("CARGO_PKG_VERSION"),
                    "status": "error"
                },
                "error": "Service not initialized",
                "timestamp": chrono::Utc::now()
            });

            (StatusCode::SERVICE_UNAVAILABLE, Json(error_response))
        }
    }
}

/// Health endpoints implementation (for compatibility/testing)
pub struct HealthEndpoints;

impl HealthEndpoints {
    /// Get health status as JSON (for programmatic access)
    pub async fn get_health_status(app_state: Option<Arc<AppState>>) -> Result<serde_json::Value> {
        match app_state {
            Some(state) => match HealthCheck::liveness_check(state).await {
                Ok(HealthStatus::Healthy) => Ok(json!({
                    "status": "healthy",
                    "service": "parlor-room"
                })),
                Ok(HealthStatus::Degraded) => Ok(json!({
                    "status": "degraded",
                    "service": "parlor-room"
                })),
                Ok(HealthStatus::Unhealthy) | Err(_) => Ok(json!({
                    "status": "unhealthy",
                    "service": "parlor-room"
                })),
            },
            None => Ok(json!({
                "status": "unhealthy",
                "service": "parlor-room",
                "error": "Service not initialized"
            })),
        }
    }

    /// Get metrics as Prometheus text format
    pub async fn get_metrics_text(metrics_collector: Arc<MetricsCollector>) -> Result<String> {
        let registry = metrics_collector.registry();
        let metric_families = registry.gather();
        let encoder = TextEncoder::new();

        encoder
            .encode_to_string(&metric_families)
            .map_err(|e| anyhow::anyhow!("Failed to encode metrics: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::collector::MetricsCollector;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt; // for oneshot

    #[tokio::test]
    async fn test_root_endpoint() {
        let collector = Arc::new(MetricsCollector::new().expect("Failed to create collector"));
        let server = HealthServer::new(HealthServerConfig::default(), collector);
        let app = server.create_router();

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let collector = Arc::new(MetricsCollector::new().expect("Failed to create collector"));

        // Record some test metrics
        collector.record_lobby_created(crate::types::LobbyType::General);
        collector.update_health_status(2);

        let server = HealthServer::new(HealthServerConfig::default(), collector);
        let app = server.create_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Check content type
        let content_type = response.headers().get("content-type").unwrap();
        assert!(content_type.to_str().unwrap().contains("text/plain"));
    }

    #[tokio::test]
    async fn test_health_endpoints_without_app_state() {
        let collector = Arc::new(MetricsCollector::new().expect("Failed to create collector"));
        let server = HealthServer::new(HealthServerConfig::default(), collector);
        let app = server.create_router();

        // Test health endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Test ready endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Test alive endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/alive")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Test stats endpoint
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn test_health_server_config() {
        let config = HealthServerConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "0.0.0.0");

        let custom_config = HealthServerConfig {
            port: 9090,
            host: "127.0.0.1".to_string(),
        };
        assert_eq!(custom_config.port, 9090);
        assert_eq!(custom_config.host, "127.0.0.1");
    }

    #[tokio::test]
    async fn test_404_handling() {
        let collector = Arc::new(MetricsCollector::new().expect("Failed to create collector"));
        let server = HealthServer::new(HealthServerConfig::default(), collector);
        let app = server.create_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_health_endpoints_compatibility() {
        let collector = Arc::new(MetricsCollector::new().expect("Failed to create collector"));

        // Test programmatic access
        let health_status = HealthEndpoints::get_health_status(None).await.unwrap();
        assert_eq!(health_status["status"], "unhealthy");

        let metrics_text = HealthEndpoints::get_metrics_text(collector).await.unwrap();
        assert!(metrics_text.contains("parlor_room"));
    }

    #[tokio::test]
    async fn test_lightweight_health_check() {
        let collector = Arc::new(MetricsCollector::new().expect("Failed to create collector"));
        let server = HealthServer::new(HealthServerConfig::default(), collector);
        let app = server.create_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Without app state, should return unhealthy but still valid JSON
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
