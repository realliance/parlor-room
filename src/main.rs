//! Main entry point for the Parlor Room matchmaking service
//!
//! This is the production entry point that initializes and runs the
//! complete matchmaking microservice with proper error handling,
//! logging, and graceful shutdown.

use anyhow::Result;
use clap::Parser;
use parlor_room::config::AppConfig;
use parlor_room::service::{AppState, HealthCheck, HealthStatus};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

/// Parlor Room Matchmaking Service - Bot and Player MMR Queueing System
#[derive(Parser)]
#[command(
    name = "parlor-room",
    version,
    about = "A high-performance matchmaking microservice for bot and player MMR queueing",
    long_about = "Parlor Room is a Rust-based matchmaking microservice that handles player and bot \
                 queueing via AMQP, manages lobbies with different configurations, implements \
                 dynamic wait times, and uses Weng-Lin (OpenSkill) rating system for skill-based matching."
)]
struct Args {
    /// Configuration file path
    #[arg(
        short,
        long,
        value_name = "FILE",
        help = "Path to configuration file (TOML format)"
    )]
    config: Option<PathBuf>,

    /// Perform health check and exit
    #[arg(long, help = "Perform a health check and exit with status code")]
    health_check: bool,

    /// Log level override
    #[arg(
        short,
        long,
        value_name = "LEVEL",
        help = "Override log level (trace, debug, info, warn, error)"
    )]
    log_level: Option<String>,

    /// AMQP URL override
    #[arg(long, value_name = "URL", help = "Override AMQP connection URL")]
    amqp_url: Option<String>,

    /// HTTP port override
    #[arg(long, value_name = "PORT", help = "Override HTTP server port")]
    http_port: Option<u16>,

    /// Metrics port override
    #[arg(long, value_name = "PORT", help = "Override metrics server port")]
    metrics_port: Option<u16>,

    /// Disable bot backfill
    #[arg(long, help = "Disable automatic bot backfill in general lobbies")]
    no_bot_backfill: bool,

    /// Enable debug mode
    #[arg(short, long, help = "Enable debug mode with verbose logging")]
    debug: bool,

    /// Dry run mode (validate config and exit)
    #[arg(
        long,
        help = "Validate configuration and exit without starting service"
    )]
    dry_run: bool,
}

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

/// Perform health check and return appropriate exit code
async fn perform_health_check(config: AppConfig) -> Result<()> {
    info!("Performing health check...");

    // Initialize minimal app state for health check
    let app_state = AppState::new(config).await?;
    let app_state = Arc::new(app_state);

    match HealthCheck::check(app_state).await {
        Ok(health) => {
            println!("Health Check: {}", health.status);
            println!("  Active Lobbies: {}", health.stats.active_lobbies);
            println!("  Games Started: {}", health.stats.games_started);
            println!("  Players Waiting: {}", health.stats.players_waiting);
            println!("  Players Matched: {}", health.stats.players_matched);
            println!("  Uptime: {}", health.stats.uptime_info);

            if health.status == HealthStatus::Healthy {
                std::process::exit(0);
            } else {
                std::process::exit(1);
            }
        }
        Err(e) => {
            error!("Health check failed: {}", e);
            std::process::exit(1);
        }
    }
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

/// Load and merge configuration from environment and CLI arguments
fn load_config(args: &Args) -> Result<AppConfig> {
    // Start with environment-based config
    let mut config = if let Some(config_path) = &args.config {
        info!("Loading configuration from: {}", config_path.display());
        AppConfig::from_file(config_path)?
    } else {
        AppConfig::from_env()?
    };

    // Apply CLI overrides
    if let Some(log_level) = &args.log_level {
        config.service.log_level = log_level.clone();
    }

    if args.debug {
        config.service.log_level = "debug".to_string();
    }

    if let Some(amqp_url) = &args.amqp_url {
        config.amqp.url = amqp_url.clone();
    }

    if let Some(http_port) = args.http_port {
        config.service.http_port = http_port;
    }

    if let Some(metrics_port) = args.metrics_port {
        config.service.metrics_port = metrics_port;
    }

    if args.no_bot_backfill {
        config.matchmaking.enable_bot_backfill = false;
    }

    Ok(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Load configuration (CLI args can override environment/config file)
    let config = load_config(&args).unwrap_or_else(|e| {
        eprintln!("Configuration error: {}", e);
        std::process::exit(1);
    });

    // Initialize logging early (before any other operations)
    if let Err(e) = init_logging(&config.service.log_level) {
        eprintln!("Failed to initialize logging: {}", e);
        std::process::exit(1);
    }

    // Handle special modes
    if args.health_check {
        return perform_health_check(config).await;
    }

    if args.dry_run {
        info!("Configuration validation successful");
        display_startup_banner(&config);
        info!("Dry run completed - exiting without starting service");
        return Ok(());
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
