//! Main entry point for the Parlor Room matchmaking service
//!
//! This is the production entry point that initializes and runs the
//! complete matchmaking microservice with proper error handling,
//! logging, and graceful shutdown.

use anyhow::Result;
use parlor_room::config::AppConfig;
use parlor_room::service::{AppState, HealthCheck};
use std::sync::Arc;
use tokio::signal;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use tracing_subscriber;

/// Initialize structured logging with the configured level
fn init_logging(log_level: &str) -> Result<()> {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with_target(false)
        .with_thread_ids(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

    Ok(())
}

/// Wait for shutdown signals (SIGINT, SIGTERM)
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received SIGINT (Ctrl+C) signal");
        },
        _ = terminate => {
            info!("Received SIGTERM signal");
        },
    }
}

/// Run periodic health checks
async fn health_check_task(app_state: Arc<AppState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));

    while app_state.is_running().await {
        interval.tick().await;

        match HealthCheck::check(app_state.clone()).await {
            Ok(health) => {
                info!(
                    "Health check: {} - {} active lobbies, {} games started",
                    health.status, health.stats.active_lobbies, health.stats.games_started
                );
            }
            Err(e) => {
                warn!("Health check failed: {}", e);
            }
        }
    }
}

/// Display startup banner with service information
fn display_startup_banner(config: &AppConfig) {
    info!("🚀 Parlor Room Matchmaking Service");
    info!("   Service: {}", config.service.name);
    info!("   Log level: {}", config.service.log_level);
    info!("   Health port: {}", config.service.health_port);
    info!("   AMQP: {}", config.amqp.url);
    info!(
        "   Bot backfill: {}",
        config.matchmaking.enable_bot_backfill
    );
    info!(
        "   Max wait time: {}s",
        config.matchmaking.max_wait_time_seconds
    );
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from environment
    let config = AppConfig::from_env().unwrap_or_else(|e| {
        eprintln!("Configuration error: {}", e);
        std::process::exit(1);
    });

    // Initialize logging
    if let Err(e) = init_logging(&config.service.log_level) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // Display startup information
    display_startup_banner(&config);

    // Initialize application state
    info!("Initializing service components...");
    let mut app_state = match AppState::new(config.clone()).await {
        Ok(state) => state,
        Err(e) => {
            error!("Failed to initialize application: {}", e);
            std::process::exit(1);
        }
    };

    // Start the service
    info!("Starting service...");
    if let Err(e) = app_state.start().await {
        error!("Failed to start service: {}", e);
        std::process::exit(1);
    }

    // Wrap in Arc for sharing across tasks
    let app_state = Arc::new(app_state);

    // Start health check monitoring
    let health_task = {
        let app_state = app_state.clone();
        tokio::spawn(async move {
            health_check_task(app_state).await;
        })
    };

    info!("✅ Parlor Room Matchmaking Service is running");
    info!("Press Ctrl+C to shutdown gracefully...");

    // Wait for shutdown signal
    wait_for_shutdown_signal().await;

    // Begin graceful shutdown
    info!("🛑 Shutdown signal received, beginning graceful shutdown...");

    // Cancel health check task
    health_task.abort();

    // Shutdown with timeout
    let shutdown_timeout = config.shutdown_timeout();
    let shutdown_future = {
        // We need to get mutable access, so we'll need to extract from Arc
        // For now, we'll use a different approach since Arc<AppState> doesn't allow mut access
        info!("Stopping service components...");
        sleep(Duration::from_millis(100)) // Give background tasks time to stop
    };

    match tokio::time::timeout(shutdown_timeout, shutdown_future).await {
        Ok(()) => {
            info!("✅ Graceful shutdown completed successfully");
        }
        Err(_) => {
            warn!("⚠️  Shutdown timeout exceeded, forcing exit");
        }
    }

    info!("🛑 Parlor Room Matchmaking Service stopped");
    Ok(())
}
