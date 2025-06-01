//! Main entry point for the Parlor Room matchmaking service

use anyhow::Result;
use tracing::{error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting Parlor Room matchmaking service");

    // TODO: Phase 1 - Set up AMQP connections and message handlers
    // TODO: Phase 2 - Initialize lobby management
    // TODO: Phase 3 - Set up rating system
    // TODO: Phase 4 - Initialize bot integration
    // TODO: Phase 5 - Set up wait time calculation
    // TODO: Phase 6 - Implement matchmaking logic
    // TODO: Phase 7 - Add configuration management
    // TODO: Phase 8 - Set up metrics and monitoring

    info!("Parlor Room matchmaking service started successfully");

    // Keep the service running
    tokio::signal::ctrl_c().await?;
    info!("Shutting down Parlor Room matchmaking service");

    Ok(())
}
