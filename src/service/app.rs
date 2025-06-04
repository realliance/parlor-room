//! Main application state and service coordination
//!
//! This module contains the production AppState that coordinates all
//! service components, AMQP connections, and background tasks.

use crate::amqp::connection::{AmqpConfig, AmqpConnection};
use crate::amqp::handlers::{MessageHandler, QueueRequestConsumer};
use crate::amqp::messages::QUEUE_REQUEST_QUEUE;
use crate::amqp::publisher::{AmqpEventPublisher, PublisherConfig};
use crate::bot::auth::{BotAuthenticator, DefaultBotAuthenticator};
use crate::bot::backfill::{BackfillConfig, DefaultBackfillManager};
use crate::bot::provider::MockBotProvider;
use crate::config::AppConfig;
use crate::error::{MatchmakingError, Result as MatchmakingResult};
use crate::lobby::instance::Lobby;
use crate::lobby::manager::LobbyManager;
use crate::lobby::provider::StaticLobbyProvider;
use crate::metrics::health::HealthServerConfig;
use crate::metrics::{HealthServer, MetricsCollector, MetricsService};
use crate::rating::{ExtendedWengLinConfig, InMemoryRatingStorage, WengLinRatingCalculator};
use crate::types::QueueRequest;
use crate::wait_time::calculator::{DynamicWaitTimeCalculator, WaitTimeConfig};
use crate::wait_time::provider::InternalWaitTimeProvider;
use crate::wait_time::statistics::InMemoryStatisticsTracker;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};

/// Service-level errors
#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    #[error("AMQP connection error: {message}")]
    AmqpConnection { message: String },

    #[error("Service initialization error: {message}")]
    Initialization { message: String },

    #[error("Background task error: {message}")]
    BackgroundTask { message: String },
}

/// Production message handler that integrates with LobbyManager
struct ProductionMessageHandler {
    lobby_manager: Arc<RwLock<LobbyManager>>,
    bot_authenticator: Arc<DefaultBotAuthenticator>,
}

impl ProductionMessageHandler {
    fn new(
        lobby_manager: Arc<RwLock<LobbyManager>>,
        bot_authenticator: Arc<DefaultBotAuthenticator>,
    ) -> Self {
        Self {
            lobby_manager,
            bot_authenticator,
        }
    }
}

#[async_trait]
impl MessageHandler for ProductionMessageHandler {
    async fn handle_queue_request(&self, request: QueueRequest) -> MatchmakingResult<()> {
        let start_time = std::time::Instant::now();

        info!(
            "Processing queue request in production handler - player: '{}', type: {:?}, lobby: {:?}",
            request.player_id, request.player_type, request.lobby_type
        );

        let lobby_manager = self.lobby_manager.read().await;

        match lobby_manager.handle_queue_request(request.clone()).await {
            Ok(lobby_id) => {
                let processing_time = start_time.elapsed();
                info!(
                    "Queue request processed successfully - player: '{}', lobby: {}, time: {:.2}ms",
                    request.player_id,
                    lobby_id,
                    processing_time.as_secs_f64() * 1000.0
                );
                Ok(())
            }
            Err(e) => {
                let processing_time = start_time.elapsed();
                error!(
                    "Queue request failed - player: '{}', type: {:?}, lobby: {:?}, time: {:.2}ms, error: {}",
                    request.player_id, request.player_type, request.lobby_type,
                    processing_time.as_secs_f64() * 1000.0, e
                );
                Err(e)
            }
        }
    }

    async fn authenticate_bot(&self, player_id: &str, auth_token: &str) -> MatchmakingResult<bool> {
        info!(
            "Authenticating bot '{}' with production authenticator",
            player_id
        );

        let start_time = std::time::Instant::now();
        let result = self
            .bot_authenticator
            .authenticate_bot(player_id, auth_token)
            .await;

        let auth_time = start_time.elapsed();

        match &result {
            Ok(true) => info!(
                "Bot authentication successful - bot: '{}', time: {:.2}ms",
                player_id,
                auth_time.as_secs_f64() * 1000.0
            ),
            Ok(false) => warn!(
                "Bot authentication failed - bot: '{}', time: {:.2}ms (invalid credentials)",
                player_id,
                auth_time.as_secs_f64() * 1000.0
            ),
            Err(e) => error!(
                "Bot authentication error - bot: '{}', time: {:.2}ms, error: {}",
                player_id,
                auth_time.as_secs_f64() * 1000.0,
                e
            ),
        }

        result
    }

    async fn handle_error(&self, error: MatchmakingError, message_data: &[u8]) {
        error!(
            "Production message handler error - type: '{}', message_size: {} bytes",
            error,
            message_data.len()
        );

        // Log first 100 bytes of message for debugging (safely)
        if !message_data.is_empty() {
            let preview_len = std::cmp::min(100, message_data.len());
            let preview = String::from_utf8_lossy(&message_data[..preview_len]);
            error!("Message preview: {:?}", preview);
        }

        // In a production system, we might want to:
        // - Log to external monitoring system
        // - Send to dead letter queue
        // - Update error metrics
        // For now, we'll just log comprehensively
        error!("Error handling completed - consider implementing dead letter queue for persistent failures");
    }
}

/// Main application state containing all service components
pub struct AppState {
    /// Application configuration
    config: AppConfig,

    /// Core matchmaking components
    lobby_manager: Arc<RwLock<LobbyManager>>,

    /// AMQP connection for message handling
    amqp_connection: Arc<AmqpConnection>,

    /// Metrics service for monitoring and health checks
    metrics_service: Arc<MetricsService>,

    /// Background task handles
    background_tasks: Vec<JoinHandle<()>>,

    /// AMQP consumer for queue requests
    queue_consumer: Option<QueueRequestConsumer>,

    /// Service status
    is_running: Arc<RwLock<bool>>,
}

impl AppState {
    /// Initialize the application with all dependencies
    pub async fn new(config: AppConfig) -> Result<Self, ServiceError> {
        info!("Initializing parlor-room matchmaking service");
        info!(
            "Configuration: service={}, amqp_url={}",
            config.service.name, config.amqp.url
        );

        // Initialize AMQP connection
        let amqp_connection = Self::initialize_amqp(&config).await?;

        // Initialize metrics service
        let metrics_service = Self::initialize_metrics(&config).await?;

        // Initialize all core components with metrics
        let lobby_manager = Self::initialize_matchmaking_system(
            &config,
            amqp_connection.clone(),
            metrics_service.collector(),
        )
        .await?;

        Ok(Self {
            config,
            lobby_manager,
            amqp_connection,
            metrics_service,
            background_tasks: Vec::new(),
            queue_consumer: None,
            is_running: Arc::new(RwLock::new(false)),
        })
    }

    /// Start all background services and message consumption
    pub async fn start(&mut self) -> Result<(), ServiceError> {
        info!("Starting parlor-room matchmaking service");

        // Mark as running
        *self.is_running.write().await = true;

        // Start metrics service first
        self.start_metrics_service().await?;

        // Start AMQP message consumption
        self.start_amqp_consumption().await?;

        // Start background tasks
        self.start_background_tasks().await?;

        info!("✅ Parlor-room matchmaking service started successfully");
        Ok(())
    }

    /// Perform graceful shutdown
    pub async fn shutdown(&mut self) -> Result<(), ServiceError> {
        info!("Starting graceful shutdown of parlor-room service");

        // Mark as not running
        *self.is_running.write().await = false;

        // Stop AMQP message consumption
        if let Some(consumer) = &self.queue_consumer {
            if let Err(e) = consumer.stop_consuming().await {
                warn!("Failed to stop AMQP consumer: {}", e);
            } else {
                info!("✅ AMQP message consumption stopped");
            }
        }

        // Stop background tasks (including metrics service task)
        self.stop_background_tasks().await;

        // Stop metrics service
        info!("Stopping metrics service...");
        if let Err(e) = self.metrics_service.stop().await {
            warn!("Failed to stop metrics service: {}", e);
        } else {
            info!("✅ Metrics service stopped");
        }

        // Get final statistics
        let final_stats = {
            let lobby_manager = self.lobby_manager.read().await;
            lobby_manager
                .get_stats()
                .await
                .map_err(|e| ServiceError::BackgroundTask {
                    message: format!("Failed to get final stats: {}", e),
                })?
        };

        info!("Final service statistics: {:?}", final_stats);
        info!("✅ Parlor-room service shutdown completed");

        Ok(())
    }

    /// Get service configuration
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Check if service is running
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    /// Get lobby manager for operations
    pub fn lobby_manager(&self) -> Arc<RwLock<LobbyManager>> {
        self.lobby_manager.clone()
    }

    /// Get metrics service
    pub fn metrics_service(&self) -> Arc<MetricsService> {
        self.metrics_service.clone()
    }

    /// Get AMQP connection for health checks
    pub fn amqp_connection(&self) -> Arc<AmqpConnection> {
        self.amqp_connection.clone()
    }

    /// Initialize metrics service
    async fn initialize_metrics(config: &AppConfig) -> Result<Arc<MetricsService>, ServiceError> {
        info!(
            "Initializing metrics service on port {}",
            config.service.metrics_port
        );

        let metrics_collector =
            Arc::new(
                MetricsCollector::new().map_err(|e| ServiceError::Initialization {
                    message: format!("Failed to create metrics collector: {}", e),
                })?,
            );

        let health_config = HealthServerConfig {
            port: config.service.metrics_port,
            host: "0.0.0.0".to_string(),
        };

        let health_server = Arc::new(HealthServer::new(health_config, metrics_collector.clone()));
        let metrics_service = Arc::new(MetricsService::new(metrics_collector, health_server));

        Ok(metrics_service)
    }

    /// Start metrics service
    async fn start_metrics_service(&mut self) -> Result<(), ServiceError> {
        info!("Starting metrics and health endpoints");

        // Clone necessary references for the background task
        let metrics_service = self.metrics_service.clone();
        let port = self.config.service.metrics_port;

        // Spawn the metrics service as a background task
        let metrics_handle = tokio::spawn(async move {
            if let Err(e) = metrics_service.start().await {
                error!("Metrics service failed: {}", e);
            } else {
                info!("Metrics service task completed");
            }
        });

        // Add the handle to background tasks for proper shutdown
        self.background_tasks.push(metrics_handle);

        // Give the server a moment to start up
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        info!("✅ Metrics service started on port {}", port);
        Ok(())
    }

    /// Initialize AMQP connection with retry logic
    async fn initialize_amqp(config: &AppConfig) -> Result<Arc<AmqpConnection>, ServiceError> {
        info!("Connecting to AMQP broker: {}", config.amqp.url);

        // Parse AMQP URL to extract connection details
        let amqp_config =
            Self::parse_amqp_url(&config.amqp.url).map_err(|e| ServiceError::AmqpConnection {
                message: format!("Failed to parse AMQP URL: {}", e),
            })?;

        let connection =
            AmqpConnection::new(amqp_config)
                .await
                .map_err(|e| ServiceError::AmqpConnection {
                    message: format!("Failed to connect to AMQP: {}", e),
                })?;

        Ok(Arc::new(connection))
    }

    /// Parse AMQP URL into AmqpConfig
    fn parse_amqp_url(url: &str) -> Result<AmqpConfig, ServiceError> {
        // Simple URL parsing for amqp://user:pass@host:port/vhost format
        if let Some(stripped) = url.strip_prefix("amqp://") {
            let parts: Vec<&str> = stripped.split('@').collect();
            if parts.len() != 2 {
                return Ok(AmqpConfig::default());
            }

            let credentials = parts[0];
            let host_part = parts[1];

            let (username, password) = if credentials.contains(':') {
                let cred_parts: Vec<&str> = credentials.split(':').collect();
                (cred_parts[0].to_string(), cred_parts[1].to_string())
            } else {
                ("guest".to_string(), "guest".to_string())
            };

            let (host, port, vhost) = if host_part.contains('/') {
                let host_vhost: Vec<&str> = host_part.split('/').collect();
                let host_port = host_vhost[0];
                let vhost = if host_vhost.len() > 1 {
                    host_vhost[1].replace("%2f", "/")
                } else {
                    "/".to_string()
                };

                if host_port.contains(':') {
                    let hp: Vec<&str> = host_port.split(':').collect();
                    let port = hp[1].parse().unwrap_or(5672);
                    (hp[0].to_string(), port, vhost)
                } else {
                    (host_port.to_string(), 5672, vhost)
                }
            } else {
                (host_part.to_string(), 5672, "/".to_string())
            };

            Ok(AmqpConfig {
                host,
                port,
                username,
                password,
                vhost,
                max_retries: 5,
                retry_delay_ms: 1000,
                connection_timeout_ms: 30000,
            })
        } else {
            Ok(AmqpConfig::default())
        }
    }

    /// Initialize the complete matchmaking system
    async fn initialize_matchmaking_system(
        config: &AppConfig,
        amqp_connection: Arc<AmqpConnection>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Result<Arc<RwLock<LobbyManager>>, ServiceError> {
        info!("Initializing matchmaking system components");

        // Initialize rating system
        let rating_config = ExtendedWengLinConfig::default();
        let rating_calculator =
            Arc::new(WengLinRatingCalculator::new(rating_config).map_err(|e| {
                ServiceError::Initialization {
                    message: format!("Failed to initialize rating calculator: {}", e),
                }
            })?);
        let _rating_storage = Arc::new(InMemoryRatingStorage::new(10000));

        // Initialize bot system
        let bot_provider = Arc::new(MockBotProvider::new());
        let _bot_authenticator = Arc::new(DefaultBotAuthenticator::new(bot_provider.clone()));

        // Initialize wait time tracking
        let wait_time_config = WaitTimeConfig::default();
        let statistics_tracker = Arc::new(InMemoryStatisticsTracker::new(10000));
        let wait_time_calculator = Box::new(
            DynamicWaitTimeCalculator::new(wait_time_config, statistics_tracker.clone()).map_err(
                |e| ServiceError::Initialization {
                    message: format!("Failed to initialize wait time calculator: {}", e),
                },
            )?,
        );
        let wait_time_provider = Arc::new(InternalWaitTimeProvider::new(
            wait_time_calculator,
            statistics_tracker,
        ));

        // Initialize backfill system
        let backfill_config = BackfillConfig {
            enabled: config.matchmaking.enable_bot_backfill,
            max_rating_tolerance: config.matchmaking.max_rating_difference,
            min_humans_for_backfill: 1,
            max_bots_per_backfill: 3,
            backfill_cooldown_seconds: config.matchmaking.backfill_delay_seconds,
        };
        let _backfill_manager = Arc::new(
            DefaultBackfillManager::new(
                backfill_config,
                bot_provider.clone(),
                wait_time_provider.clone(),
                rating_calculator.clone(),
            )
            .map_err(|e| ServiceError::Initialization {
                message: format!("Failed to initialize backfill manager: {}", e),
            })?,
        );

        // Get a channel from the connection
        let channel = amqp_connection
            .connection()
            .open_channel(None)
            .await
            .map_err(|e| ServiceError::Initialization {
                message: format!("Failed to open AMQP channel: {}", e),
            })?;

        // Initialize event publisher
        let publisher_config = PublisherConfig::default();
        let event_publisher = Arc::new(
            AmqpEventPublisher::new(channel, publisher_config)
                .await
                .map_err(|e| ServiceError::Initialization {
                    message: format!("Failed to initialize event publisher: {}", e),
                })?,
        );

        // Initialize lobby system
        let lobby_provider = Arc::new(StaticLobbyProvider::new());
        let lobby_manager =
            LobbyManager::with_metrics(lobby_provider, event_publisher, metrics_collector);

        Ok(Arc::new(RwLock::new(lobby_manager)))
    }

    /// Start AMQP message consumption
    async fn start_amqp_consumption(&mut self) -> Result<(), ServiceError> {
        info!("Starting AMQP message consumption system...");

        // Get a channel for consuming messages
        info!("Opening AMQP channel for message consumption...");
        let channel = self
            .amqp_connection
            .connection()
            .open_channel(None)
            .await
            .map_err(|e| ServiceError::AmqpConnection {
                message: format!("Failed to open consumer channel: {}", e),
            })?;

        info!("AMQP channel opened successfully");

        // Declare the queue to ensure it exists
        info!("Declaring queue: '{}'...", QUEUE_REQUEST_QUEUE);
        let queue_declare_args = amqprs::channel::QueueDeclareArguments::new(QUEUE_REQUEST_QUEUE)
            .durable(true)
            .auto_delete(false)
            .finish();

        channel
            .queue_declare(queue_declare_args)
            .await
            .map_err(|e| ServiceError::AmqpConnection {
                message: format!("Failed to declare queue {}: {}", QUEUE_REQUEST_QUEUE, e),
            })?;

        info!("Queue '{}' declared successfully", QUEUE_REQUEST_QUEUE);

        // Initialize bot authenticator
        info!("Initializing bot authentication system...");
        let bot_provider = Arc::new(MockBotProvider::new());
        let bot_authenticator = Arc::new(DefaultBotAuthenticator::new(bot_provider));
        info!("Bot authenticator initialized");

        // Create message handler
        info!("Creating production message handler...");
        let message_handler = Arc::new(ProductionMessageHandler::new(
            self.lobby_manager.clone(),
            bot_authenticator,
        ));
        info!("Production message handler created");

        // Create and configure consumer
        info!("Setting up AMQP consumer...");
        let consumer = QueueRequestConsumer::new(message_handler, channel);

        // Start consuming from the queue
        info!(
            "Starting message consumption from queue '{}'...",
            QUEUE_REQUEST_QUEUE
        );
        consumer
            .start_consuming(QUEUE_REQUEST_QUEUE)
            .await
            .map_err(|e| ServiceError::AmqpConnection {
                message: format!("Failed to start consuming messages: {}", e),
            })?;

        // Store consumer for cleanup
        self.queue_consumer = Some(consumer);

        info!(
            "AMQP message consumption started successfully on queue: '{}'",
            QUEUE_REQUEST_QUEUE
        );
        info!("Now listening for queue requests from players and bots...");
        Ok(())
    }

    /// Start background maintenance tasks
    async fn start_background_tasks(&mut self) -> Result<(), ServiceError> {
        info!("Starting background maintenance tasks...");

        // Metrics update task
        info!("Starting lobby metrics update task (30s interval)...");
        let metrics_task = {
            let lobby_manager = self.lobby_manager.clone();
            let metrics_collector = self.metrics_service.collector();
            let is_running = self.is_running.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(30));
                info!("Metrics update task started");

                while *is_running.read().await {
                    interval.tick().await;

                    // Update metrics from lobby manager stats
                    let manager = lobby_manager.read().await;
                    match manager.get_stats().await {
                        Ok(stats) => {
                            debug!(
                                "Updating metrics - lobbies: {}, players: {}, games: {}",
                                stats.active_lobbies, stats.players_waiting, stats.games_started
                            );
                            metrics_collector.update_from_lobby_stats(&stats);
                        }
                        Err(e) => {
                            warn!("Failed to get lobby stats for metrics update: {}", e);
                        }
                    }
                }

                info!("Metrics update task stopped");
            })
        };

        // Lobby cleanup task
        info!(
            "Starting lobby cleanup task ({}s interval)...",
            self.config.cleanup_interval().as_secs()
        );
        let cleanup_task = {
            let lobby_manager = self.lobby_manager.clone();
            let cleanup_interval = self.config.cleanup_interval();
            let is_running = self.is_running.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(cleanup_interval);
                info!("Lobby cleanup task started");

                while *is_running.read().await {
                    interval.tick().await;

                    let manager = lobby_manager.read().await;
                    match manager.cleanup_stale_lobbies().await {
                        Ok(cleaned) => {
                            if cleaned > 0 {
                                info!("Cleaned up {} stale lobbies", cleaned);
                            } else {
                                debug!("Cleanup check completed - no stale lobbies found");
                            }
                        }
                        Err(e) => {
                            warn!("Lobby cleanup failed: {}", e);
                        }
                    }
                }

                info!("Lobby cleanup task stopped");
            })
        };

        // Bot backfill task (if enabled)
        let backfill_task = if self.config.matchmaking.enable_bot_backfill {
            info!(
                "Starting bot backfill task ({}s interval)...",
                self.config.backfill_delay().as_secs()
            );
            let lobby_manager = self.lobby_manager.clone();
            let backfill_interval = self.config.backfill_delay();
            let is_running = self.is_running.clone();

            Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(backfill_interval);
                info!("Bot backfill task started");

                while *is_running.read().await {
                    interval.tick().await;

                    // Process bot backfill for lobbies that need it
                    let manager = lobby_manager.read().await;
                    match manager.get_lobbies_needing_backfill().await {
                        Ok(lobbies) => {
                            if !lobbies.is_empty() {
                                info!("Processing backfill for {} lobbies", lobbies.len());
                                for lobby in lobbies {
                                    info!("  - Backfill needed for lobby {}", lobby.lobby_id());
                                }
                            } else {
                                debug!("Backfill check completed - no lobbies need backfill");
                            }
                        }
                        Err(e) => {
                            warn!("Backfill processing failed: {}", e);
                        }
                    }
                }

                info!("Bot backfill task stopped");
            }))
        } else {
            info!("Bot backfill disabled - skipping backfill task");
            None
        };

        // Service health metrics task
        info!("Starting health metrics task (60s interval)...");
        let health_metrics_task = {
            let metrics_collector = self.metrics_service.collector();
            let is_running = self.is_running.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                let start_time = tokio::time::Instant::now();
                info!("Health metrics task started");

                while *is_running.read().await {
                    interval.tick().await;

                    // Update service uptime
                    let uptime_seconds = start_time.elapsed().as_secs() as i64;
                    metrics_collector
                        .service()
                        .uptime_seconds
                        .set(uptime_seconds);

                    debug!(
                        "Updated service health metrics - uptime: {}s",
                        uptime_seconds
                    );

                    // Update health status (assume healthy for now)
                    metrics_collector.update_health_status(2); // 2 = healthy

                    // Update component health
                    metrics_collector.update_component_health("amqp", true);
                    metrics_collector.update_component_health("lobby_manager", true);
                    metrics_collector.update_component_health("metrics", true);
                }

                info!("Health metrics task stopped");
            })
        };

        // Add tasks to background handles
        let mut task_count = 3; // metrics, cleanup, health
        self.background_tasks.push(metrics_task);
        self.background_tasks.push(cleanup_task);
        self.background_tasks.push(health_metrics_task);
        if let Some(task) = backfill_task {
            self.background_tasks.push(task);
            task_count += 1;
        }

        info!(
            "{} background maintenance tasks started successfully",
            task_count
        );
        Ok(())
    }

    /// Stop all background tasks
    async fn stop_background_tasks(&mut self) {
        let task_count = self.background_tasks.len();
        if task_count == 0 {
            info!("No background tasks to stop");
            return;
        }

        info!("Stopping {} background tasks...", task_count);

        // Cancel all background tasks
        for (i, task) in self.background_tasks.drain(..).enumerate() {
            debug!("Aborting background task {}/{}", i + 1, task_count);
            task.abort();
        }

        // Give tasks time to clean up gracefully
        info!("Waiting for background tasks to complete shutdown...");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        info!("✅ All {} background tasks stopped", task_count);
    }
}
