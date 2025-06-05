//! Queue Testing Tool and Test Suite
//!
//! This module provides utilities to test queue functionality including:
//! - Adding arbitrary humans/bots to queues
//! - Monitoring queue outputs and match formation
//! - Automated test suites for various scenarios
//!
//! Run with: `cargo test queue_tester`
//! Or use the CLI tool: `cargo run --bin queue-tester`

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use amqprs::{
    channel::{
        BasicConsumeArguments, BasicPublishArguments, ExchangeDeclareArguments,
        QueueDeclareArguments,
    },
    consumer::AsyncConsumer,
    BasicProperties, Deliver,
};
use anyhow::Context;
use async_trait::async_trait;
use parlor_room::amqp::connection::{AmqpConfig, AmqpConnection};
use parlor_room::amqp::messages::{
    MessageEnvelope, MessageUtils, GAME_EVENTS_EXCHANGE, PLAYER_EVENTS_EXCHANGE,
    QUEUE_REQUEST_QUEUE,
};
use parlor_room::types::{GameStarting, LobbyType, PlayerRating, PlayerType, QueueRequest};
use parlor_room::utils::current_timestamp;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use tokio::sync::Mutex as TokioMutex;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Queue tester that can simulate queue operations and monitor results against real RabbitMQ
#[allow(dead_code)]
pub struct QueueTester {
    amqp_connection: Arc<AmqpConnection>,
    publish_channel: amqprs::channel::Channel,
    consume_channel: amqprs::channel::Channel,
    queue_stats: Arc<Mutex<QueueStats>>,
    game_events: Arc<Mutex<Vec<GameStarting>>>,
    consumer_tag: String,
    events_queue_name: String,
}

/// Statistics about queue operations
#[derive(Debug, Default, Clone)]
pub struct QueueStats {
    pub total_requests: u32,
    pub successful_matches: u32,
    pub failed_requests: u32,
    pub average_wait_time_ms: u64,
    pub human_count: u32,
    pub bot_count: u32,
}

/// Configuration for queue testing scenarios
#[derive(Debug, Clone)]
pub struct QueueTestConfig {
    pub scenario_name: String,
    pub human_players: Vec<PlayerConfig>,
    pub bot_players: Vec<PlayerConfig>,
    pub expected_matches: u32,
    pub timeout_seconds: u64,
}

/// Configuration for a test player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    pub id: String,
    pub lobby_type: LobbyType,
    pub rating: f64,
    pub uncertainty: f64,
}

impl PlayerConfig {
    /// Create a new player config
    pub fn new(id: String, lobby_type: LobbyType, rating: f64, uncertainty: f64) -> Self {
        Self {
            id,
            lobby_type,
            rating,
            uncertainty,
        }
    }

    /// Create a human player config
    pub fn human(id: String, lobby_type: LobbyType, rating: f64, uncertainty: f64) -> Self {
        Self::new(id, lobby_type, rating, uncertainty)
    }

    /// Create a bot player config
    pub fn bot(id: String, lobby_type: LobbyType, rating: f64, uncertainty: f64) -> Self {
        Self::new(id, lobby_type, rating, uncertainty)
    }
}

impl QueueTester {
    /// Create a new queue tester that connects to actual RabbitMQ
    pub async fn new() -> anyhow::Result<Self> {
        Self::new_with_config(create_amqp_config_from_env()?).await
    }

    /// Create a new queue tester with custom AMQP config
    pub async fn new_with_config(amqp_config: AmqpConfig) -> anyhow::Result<Self> {
        info!(
            "🔌 Connecting to RabbitMQ at {}:{}",
            amqp_config.host, amqp_config.port
        );

        // Create AMQP connection
        let amqp_connection = Arc::new(
            AmqpConnection::new(amqp_config)
                .await
                .context("Failed to connect to RabbitMQ")?,
        );

        // Create channels for publishing and consuming
        let publish_channel = amqp_connection
            .connection()
            .open_channel(None)
            .await
            .context("Failed to open publish channel")?;

        let consume_channel = amqp_connection
            .connection()
            .open_channel(None)
            .await
            .context("Failed to open consume channel")?;

        let consumer_tag = format!("queue-tester-{}", uuid::Uuid::new_v4());
        let events_queue_name = format!("queue-tester-events-{}", uuid::Uuid::new_v4());

        let tester = Self {
            amqp_connection,
            publish_channel,
            consume_channel,
            queue_stats: Arc::new(Mutex::new(QueueStats::default())),
            game_events: Arc::new(Mutex::new(Vec::new())),
            consumer_tag,
            events_queue_name,
        };

        // Set up queues and exchanges
        tester.setup_amqp().await?;

        // Start consuming game events
        tester.start_consuming_events().await?;

        info!("✅ Queue tester initialized and ready");
        Ok(tester)
    }

    /// Set up AMQP exchanges and queues
    async fn setup_amqp(&self) -> anyhow::Result<()> {
        info!("🔧 Setting up AMQP exchanges and queues...");

        // Declare game events exchange
        let args = ExchangeDeclareArguments::new(GAME_EVENTS_EXCHANGE, "topic");
        self.consume_channel
            .exchange_declare(args)
            .await
            .context("Failed to declare game events exchange")?;

        // Declare player events exchange
        let args = ExchangeDeclareArguments::new(PLAYER_EVENTS_EXCHANGE, "topic");
        self.consume_channel
            .exchange_declare(args)
            .await
            .context("Failed to declare player events exchange")?;

        // Declare queue for consuming game events
        let args = QueueDeclareArguments::new(&self.events_queue_name)
            .exclusive(true)
            .auto_delete(true)
            .finish();
        self.consume_channel
            .queue_declare(args)
            .await
            .context("Failed to declare events queue")?;

        // Bind queue to game events exchange
        let args = amqprs::channel::QueueBindArguments::new(
            &self.events_queue_name,
            GAME_EVENTS_EXCHANGE,
            "game.starting",
        );
        self.consume_channel
            .queue_bind(args)
            .await
            .context("Failed to bind queue to game events")?;

        info!("✅ AMQP setup complete - queue: {}", self.events_queue_name);
        Ok(())
    }

    /// Start consuming events from the parlor room service
    async fn start_consuming_events(&self) -> anyhow::Result<()> {
        info!(
            "👂 Starting to consume events from queue: {}",
            self.events_queue_name
        );

        let consumer = GameEventConsumer::new(self.game_events.clone());
        let args = BasicConsumeArguments::new(&self.events_queue_name, &self.consumer_tag);

        self.consume_channel
            .basic_consume(consumer, args)
            .await
            .context("Failed to start consuming events")?;

        info!("✅ Event consumer started");
        Ok(())
    }

    /// Queue a human player to the specified lobby type
    pub async fn queue_human(
        &self,
        player_id: &str,
        lobby_type: LobbyType,
        rating: f64,
        uncertainty: f64,
    ) -> anyhow::Result<()> {
        let request = QueueRequest {
            player_id: player_id.to_string(),
            player_type: PlayerType::Human,
            lobby_type,
            current_rating: PlayerRating {
                rating,
                uncertainty,
            },
            timestamp: current_timestamp(),
        };

        let start_time = Instant::now();
        let result = self.publish_queue_request(request).await;

        self.update_stats(PlayerType::Human, start_time, result.is_ok());

        match result {
            Ok(_) => {
                println!("✅ Human '{}' queued to {:?} lobby", player_id, lobby_type);
                Ok(())
            }
            Err(e) => {
                println!("❌ Failed to queue human '{}': {}", player_id, e);
                Err(e)
            }
        }
    }

    /// Queue a bot player to the specified lobby type
    pub async fn queue_bot(
        &self,
        bot_id: &str,
        lobby_type: LobbyType,
        rating: f64,
        uncertainty: f64,
    ) -> anyhow::Result<()> {
        let request = QueueRequest {
            player_id: bot_id.to_string(),
            player_type: PlayerType::Bot,
            lobby_type,
            current_rating: PlayerRating {
                rating,
                uncertainty,
            },
            timestamp: current_timestamp(),
        };

        let start_time = Instant::now();
        let result = self.publish_queue_request(request).await;

        self.update_stats(PlayerType::Bot, start_time, result.is_ok());

        match result {
            Ok(_) => {
                println!("✅ Bot '{}' queued to {:?} lobby", bot_id, lobby_type);
                Ok(())
            }
            Err(e) => {
                println!("❌ Failed to queue bot '{}': {}", bot_id, e);
                Err(e)
            }
        }
    }

    /// Publish a queue request directly to RabbitMQ
    async fn publish_queue_request(&self, request: QueueRequest) -> anyhow::Result<()> {
        info!(
            "📤 Publishing queue request for '{}' ({:?}) to {:?} lobby",
            request.player_id, request.player_type, request.lobby_type
        );

        // Validate the request
        MessageUtils::validate_queue_request(&request).context("Invalid queue request")?;

        // Serialize the request
        let payload = MessageUtils::serialize_queue_request(&request)
            .context("Failed to serialize queue request")?;

        // Create basic properties
        let mut properties = BasicProperties::default();
        properties
            .with_message_id(&uuid::Uuid::new_v4().to_string())
            .with_timestamp(request.timestamp.timestamp() as u64)
            .with_content_type("application/json");

        // Publish to the queue
        let args = BasicPublishArguments::new("", QUEUE_REQUEST_QUEUE);
        self.publish_channel
            .basic_publish(properties, payload, args)
            .await
            .context("Failed to publish message to RabbitMQ")?;

        debug!("✅ Queue request published successfully");
        Ok(())
    }

    /// Check for matches that have started and return only those containing current test players
    #[cfg(test)]
    pub fn check_for_matches_filtered(&self, expected_player_ids: &[String]) -> Vec<GameStarting> {
        let all_matches = self.check_for_matches();

        // Filter matches to only include games with our expected players
        all_matches
            .into_iter()
            .filter(|game| {
                // Check if this game contains any of our expected players
                game.players
                    .iter()
                    .any(|player| expected_player_ids.contains(&player.id))
            })
            .collect()
    }

    /// Check for matches that have started and return them
    pub fn check_for_matches(&self) -> Vec<GameStarting> {
        self.game_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    /// Get current queue statistics
    pub fn get_stats(&self) -> QueueStats {
        self.queue_stats
            .lock()
            .map(|stats| stats.clone())
            .unwrap_or_default()
    }

    /// Monitor queues for a specified duration and report activity
    pub async fn monitor_queues(&self, duration: Duration) -> anyhow::Result<()> {
        println!("🔍 Monitoring queues for {} seconds...", duration.as_secs());

        let start_time = Instant::now();
        let mut last_match_count = 0;

        while start_time.elapsed() < duration {
            tokio::time::sleep(Duration::from_millis(500)).await;

            let matches = self.check_for_matches();
            if matches.len() > last_match_count {
                let new_matches = &matches[last_match_count..];
                for game in new_matches {
                    println!(
                        "🎮 Match started! Game ID: {}, Players: {:?}",
                        game.game_id,
                        game.players
                            .iter()
                            .map(|p| format!("{}({:?})", p.id, p.player_type))
                            .collect::<Vec<_>>()
                    );
                }
                last_match_count = matches.len();
            }
        }

        println!(
            "📊 Monitoring complete. Total matches found: {}",
            last_match_count
        );
        Ok(())
    }

    /// Run an automated test scenario
    pub async fn run_test_scenario(&self, config: QueueTestConfig) -> anyhow::Result<bool> {
        println!("🧪 Running test scenario: {}", config.scenario_name);

        let start_time = Instant::now();

        // Clear previous events
        if let Ok(mut events) = self.game_events.lock() {
            events.clear();
        }

        // Queue all human players
        for human in &config.human_players {
            self.queue_human(&human.id, human.lobby_type, human.rating, human.uncertainty)
                .await?;
        }

        // Queue all bot players
        for bot in &config.bot_players {
            self.queue_bot(&bot.id, bot.lobby_type, bot.rating, bot.uncertainty)
                .await?;
        }

        // Wait for expected matches with timeout
        let timeout_duration = Duration::from_secs(config.timeout_seconds);
        let result = timeout(timeout_duration, async {
            loop {
                let matches = self.check_for_matches();
                if matches.len() >= config.expected_matches as usize {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        let success = result.unwrap_or(false);
        let duration = start_time.elapsed();

        if success {
            println!(
                "✅ Scenario '{}' completed successfully in {:.2}s",
                config.scenario_name,
                duration.as_secs_f64()
            );
        } else {
            println!(
                "❌ Scenario '{}' failed or timed out after {:.2}s",
                config.scenario_name,
                duration.as_secs_f64()
            );
        }

        Ok(success)
    }

    /// Update internal statistics
    fn update_stats(&self, player_type: PlayerType, start_time: Instant, success: bool) {
        if let Ok(mut stats) = self.queue_stats.lock() {
            stats.total_requests += 1;

            if success {
                let wait_time = start_time.elapsed().as_millis() as u64;
                stats.average_wait_time_ms =
                    (stats.average_wait_time_ms + wait_time) / stats.total_requests as u64;
            } else {
                stats.failed_requests += 1;
            }

            match player_type {
                PlayerType::Human => stats.human_count += 1,
                PlayerType::Bot => stats.bot_count += 1,
            }
        }
    }

    /// Restart the parlor room Docker container to ensure completely fresh state
    #[cfg(test)]
    pub async fn restart_parlor_room_service() -> anyhow::Result<()> {
        info!("🔄 Restarting parlor room Docker container for fresh state...");

        // Stop the parlor room container
        let stop_result = tokio::process::Command::new("docker")
            .args(["compose", "stop", "parlor-room"])
            .output()
            .await
            .context("Failed to execute docker stop command")?;

        if !stop_result.status.success() {
            warn!(
                "Docker stop command failed (container may not be running): {}",
                String::from_utf8_lossy(&stop_result.stderr)
            );
        }

        // Start the parlor room container
        let start_result = tokio::process::Command::new("docker")
            .args(["compose", "start", "parlor-room"])
            .output()
            .await
            .context("Failed to execute docker start command")?;

        if !start_result.status.success() {
            return Err(anyhow::anyhow!(
                "Docker start command failed: {}",
                String::from_utf8_lossy(&start_result.stderr)
            ));
        }

        // Wait for the service to be ready
        tokio::time::sleep(Duration::from_millis(2000)).await;

        info!("✅ Parlor room service restarted and ready");
        Ok(())
    }

    /// Reset local state only (no longer need complex purging)
    pub fn reset(&self) {
        if let Ok(mut stats) = self.queue_stats.lock() {
            *stats = QueueStats::default();
        }

        if let Ok(mut events) = self.game_events.lock() {
            events.clear();
        }
    }
}

/// Consumer for game events from RabbitMQ
struct GameEventConsumer {
    game_events: Arc<Mutex<Vec<GameStarting>>>,
}

impl GameEventConsumer {
    fn new(game_events: Arc<Mutex<Vec<GameStarting>>>) -> Self {
        Self { game_events }
    }
}

#[async_trait]
impl AsyncConsumer for GameEventConsumer {
    async fn consume(
        &mut self,
        _channel: &amqprs::channel::Channel,
        deliver: Deliver,
        _basic_properties: BasicProperties,
        content: Vec<u8>,
    ) {
        let routing_key = deliver.routing_key();
        debug!("📨 Received event with routing key: {}", routing_key);

        // Try to parse as game starting event
        if routing_key == "game.starting" {
            match serde_json::from_slice::<MessageEnvelope<GameStarting>>(&content) {
                Ok(envelope) => {
                    info!(
                        "🎮 Game starting event received: {} with {} players",
                        envelope.payload.game_id,
                        envelope.payload.players.len()
                    );

                    // Add debugging to see what games we're collecting
                    println!(
                        "🔍 DEBUG: Received game event - ID: {}, Players: {:?}",
                        envelope.payload.game_id,
                        envelope
                            .payload
                            .players
                            .iter()
                            .map(|p| format!("{}({:?})", p.id, p.player_type))
                            .collect::<Vec<_>>()
                    );

                    if let Ok(mut events) = self.game_events.lock() {
                        events.push(envelope.payload);
                        println!("🔍 DEBUG: Total events now: {}", events.len());
                    }
                }
                Err(e) => {
                    error!("❌ Failed to parse game starting event: {}", e);
                }
            }
        }
    }
}

/// Helper function to create AmqpConfig from environment variables
fn create_amqp_config_from_env() -> anyhow::Result<AmqpConfig> {
    let url = std::env::var("AMQP_URL")
        .unwrap_or_else(|_| "amqp://parlor_user:parlor_pass@localhost:5672/parlor".to_string());

    info!("🔗 Parsing AMQP URL: {}", url);

    // Parse the AMQP URL to extract components
    if url.starts_with("amqp://") {
        let without_scheme = url.strip_prefix("amqp://").unwrap_or(&url);
        let parts: Vec<&str> = without_scheme.split('@').collect();

        if parts.len() == 2 {
            let auth_parts: Vec<&str> = parts[0].split(':').collect();
            let host_parts: Vec<&str> = parts[1].split(':').collect();
            let host_port_vhost: Vec<&str> = host_parts
                .get(1)
                .unwrap_or(&"5672/parlor")
                .split('/')
                .collect();

            let username = auth_parts.first().unwrap_or(&"guest").to_string();
            let password = auth_parts.get(1).unwrap_or(&"guest").to_string();
            let host = host_parts.first().unwrap_or(&"localhost").to_string();
            let port = host_port_vhost
                .first()
                .unwrap_or(&"5672")
                .parse()
                .unwrap_or(5672);

            // The vhost should NOT include the leading slash, as the connection code adds it
            let vhost = host_port_vhost.get(1).unwrap_or(&"").to_string();

            let config = AmqpConfig {
                host,
                port,
                username,
                password,
                vhost,
                max_retries: 5,
                retry_delay_ms: 1000,
                connection_timeout_ms: 30000,
            };

            info!(
                "🔧 Parsed AMQP config: host={}, port={}, user={}, vhost='{}'",
                config.host, config.port, config.username, config.vhost
            );

            return Ok(config);
        }
    }

    // Fallback to defaults
    warn!("⚠️ Failed to parse AMQP URL, using defaults");
    Ok(AmqpConfig::default())
}

/// Pre-defined test scenarios for common use cases
pub struct TestScenarios;

impl TestScenarios {
    /// Test scenario: 4 humans queue for general lobby -> should form 1 match
    pub fn four_humans_general() -> QueueTestConfig {
        QueueTestConfig {
            scenario_name: "Four Humans General Lobby".to_string(),
            human_players: vec![
                PlayerConfig::human("human_1".to_string(), LobbyType::General, 1500.0, 200.0),
                PlayerConfig::human("human_2".to_string(), LobbyType::General, 1550.0, 180.0),
                PlayerConfig::human("human_3".to_string(), LobbyType::General, 1450.0, 220.0),
                PlayerConfig::human("human_4".to_string(), LobbyType::General, 1600.0, 190.0),
            ],
            bot_players: vec![],
            expected_matches: 1,
            timeout_seconds: 10,
        }
    }

    /// Test scenario: 4 bots queue for AllBot lobby -> should form 1 match immediately
    pub fn four_bots_allbot() -> QueueTestConfig {
        QueueTestConfig {
            scenario_name: "Four Bots AllBot Lobby".to_string(),
            human_players: vec![],
            bot_players: vec![
                PlayerConfig::bot("bot_1".to_string(), LobbyType::AllBot, 1500.0, 200.0),
                PlayerConfig::bot("bot_2".to_string(), LobbyType::AllBot, 1550.0, 180.0),
                PlayerConfig::bot("bot_3".to_string(), LobbyType::AllBot, 1450.0, 220.0),
                PlayerConfig::bot("bot_4".to_string(), LobbyType::AllBot, 1600.0, 190.0),
            ],
            expected_matches: 1,
            timeout_seconds: 5,
        }
    }

    /// Test scenario: Mixed lobby with 2 humans and 2 bots
    pub fn mixed_lobby() -> QueueTestConfig {
        QueueTestConfig {
            scenario_name: "Mixed Lobby (2 Humans + 2 Bots)".to_string(),
            human_players: vec![
                PlayerConfig::human(
                    "human_mixed_1".to_string(),
                    LobbyType::General,
                    1500.0,
                    200.0,
                ),
                PlayerConfig::human(
                    "human_mixed_2".to_string(),
                    LobbyType::General,
                    1550.0,
                    180.0,
                ),
            ],
            bot_players: vec![
                PlayerConfig::bot("bot_mixed_1".to_string(), LobbyType::General, 1450.0, 220.0),
                PlayerConfig::bot("bot_mixed_2".to_string(), LobbyType::General, 1600.0, 190.0),
            ],
            expected_matches: 1,
            timeout_seconds: 10,
        }
    }

    /// Test scenario: Multiple simultaneous lobbies
    pub fn multiple_lobbies() -> QueueTestConfig {
        QueueTestConfig {
            scenario_name: "Multiple Simultaneous Lobbies".to_string(),
            human_players: vec![
                // First lobby - all same rating
                PlayerConfig::human(
                    "human_lobby1_1".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
                PlayerConfig::human(
                    "human_lobby1_2".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
                PlayerConfig::human(
                    "human_lobby1_3".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
                PlayerConfig::human(
                    "human_lobby1_4".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
            ],
            bot_players: vec![
                // Second lobby - all same rating
                PlayerConfig::bot(
                    "bot_lobby2_1".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
                PlayerConfig::bot(
                    "bot_lobby2_2".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
                PlayerConfig::bot(
                    "bot_lobby2_3".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
                PlayerConfig::bot(
                    "bot_lobby2_4".to_string(),
                    LobbyType::General,
                    1500.0, // Same rating
                    200.0,  // Same uncertainty
                ),
            ],
            expected_matches: 2,
            timeout_seconds: 15,
        }
    }
}

// ============================================================================
// AUTOMATED TEST SUITE
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Static mutex to ensure tests run one at a time to prevent AMQP event interference
    static TEST_MUTEX: TokioMutex<()> = TokioMutex::const_new(());

    #[tokio::test]
    async fn test_queue_tester_setup() {
        let _guard = TEST_MUTEX.lock().await;

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");
        let stats = tester.get_stats();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.successful_matches, 0);
    }

    #[tokio::test]
    async fn test_queue_single_human() {
        let _guard = TEST_MUTEX.lock().await;

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");

        let result = tester
            .queue_human("test_human", LobbyType::General, 1500.0, 200.0)
            .await;

        assert!(result.is_ok(), "Failed to queue human: {:?}", result);

        let stats = tester.get_stats();
        assert_eq!(stats.human_count, 1);
        assert_eq!(stats.total_requests, 1);
    }

    #[tokio::test]
    async fn test_queue_single_bot() {
        let _guard = TEST_MUTEX.lock().await;

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");

        let result = tester
            .queue_bot("test_bot", LobbyType::AllBot, 1500.0, 200.0)
            .await;

        assert!(result.is_ok(), "Failed to queue bot: {:?}", result);

        let stats = tester.get_stats();
        assert_eq!(stats.bot_count, 1);
        assert_eq!(stats.total_requests, 1);
    }

    #[tokio::test]
    async fn test_scenario_four_humans_general() {
        let _guard = TEST_MUTEX.lock().await;

        // Restart parlor room service for completely fresh state
        QueueTester::restart_parlor_room_service()
            .await
            .expect("Failed to restart service");

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");

        // Reset local state
        tester.reset();

        // Use unique test-specific player IDs
        let test_prefix = "test_four_humans_";
        let player_ids = vec![
            format!("{}human_1", test_prefix),
            format!("{}human_2", test_prefix),
            format!("{}human_3", test_prefix),
            format!("{}human_4", test_prefix),
        ];

        // Queue players with unique IDs
        for (i, player_id) in player_ids.iter().enumerate() {
            let rating = 1500.0 + (i as f64 * 25.0);
            let uncertainty = 200.0 - (i as f64 * 10.0);
            tester
                .queue_human(player_id, LobbyType::General, rating, uncertainty)
                .await
                .expect("Failed to queue human");
        }

        // Wait for match with timeout
        let timeout_duration = Duration::from_secs(10);
        let result = tokio::time::timeout(timeout_duration, async {
            loop {
                let matches = tester.check_for_matches_filtered(&player_ids);
                if !matches.is_empty() {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        assert!(result.unwrap_or(false), "Scenario should have succeeded");

        let matches = tester.check_for_matches_filtered(&player_ids);
        println!(
            "🔍 DEBUG: Found {} matches: {:?}",
            matches.len(),
            matches
                .iter()
                .map(|g| format!(
                    "Game {} with players: {:?}",
                    g.game_id,
                    g.players
                        .iter()
                        .map(|p| format!("{}({:?})", p.id, p.player_type))
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(matches.len(), 1, "Should have exactly 1 match");
    }

    #[tokio::test]
    async fn test_scenario_four_bots_allbot() {
        let _guard = TEST_MUTEX.lock().await;

        // Restart parlor room service for completely fresh state
        QueueTester::restart_parlor_room_service()
            .await
            .expect("Failed to restart service");

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");

        // Reset local state
        tester.reset();

        // Use unique test-specific player IDs
        let test_prefix = "test_four_bots_";
        let player_ids = vec![
            format!("{}bot_1", test_prefix),
            format!("{}bot_2", test_prefix),
            format!("{}bot_3", test_prefix),
            format!("{}bot_4", test_prefix),
        ];

        // Queue bots with unique IDs
        for (i, player_id) in player_ids.iter().enumerate() {
            let rating = 1500.0 + (i as f64 * 25.0);
            let uncertainty = 200.0 - (i as f64 * 10.0);
            tester
                .queue_bot(player_id, LobbyType::AllBot, rating, uncertainty)
                .await
                .expect("Failed to queue bot");
        }

        // Wait for match with timeout
        let timeout_duration = Duration::from_secs(10);
        let result = tokio::time::timeout(timeout_duration, async {
            loop {
                let matches = tester.check_for_matches_filtered(&player_ids);
                if !matches.is_empty() {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        assert!(
            result.unwrap_or(false),
            "AllBot scenario should succeed quickly"
        );

        let matches = tester.check_for_matches_filtered(&player_ids);
        println!(
            "🔍 DEBUG: Found {} matches: {:?}",
            matches.len(),
            matches
                .iter()
                .map(|g| format!(
                    "Game {} with players: {:?}",
                    g.game_id,
                    g.players
                        .iter()
                        .map(|p| format!("{}({:?})", p.id, p.player_type))
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(matches.len(), 1, "Should have exactly 1 match");

        // Verify all players are bots
        let game = &matches[0];
        assert_eq!(game.players.len(), 4, "Should have 4 players");
        assert!(
            game.players
                .iter()
                .all(|p| p.player_type == PlayerType::Bot),
            "All players should be bots"
        );
    }

    #[tokio::test]
    async fn test_scenario_mixed_lobby() {
        let _guard = TEST_MUTEX.lock().await;

        // Restart parlor room service for completely fresh state
        QueueTester::restart_parlor_room_service()
            .await
            .expect("Failed to restart service");

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");

        // Reset local state
        tester.reset();

        // Use unique test-specific player IDs
        let test_prefix = "test_mixed_";
        let human_ids = vec![
            format!("{}human_1", test_prefix),
            format!("{}human_2", test_prefix),
        ];
        let bot_ids = vec![
            format!("{}bot_1", test_prefix),
            format!("{}bot_2", test_prefix),
        ];

        let mut all_player_ids = human_ids.clone();
        all_player_ids.extend(bot_ids.clone());

        // Use the same rating and uncertainty for all players to ensure they are queued together
        let rating = 1500.0;
        let uncertainty = 200.0;

        // Queue humans
        for player_id in human_ids.iter() {
            tester
                .queue_human(player_id, LobbyType::General, rating, uncertainty)
                .await
                .expect("Failed to queue human");
        }

        // Queue bots
        for player_id in bot_ids.iter() {
            tester
                .queue_bot(player_id, LobbyType::General, rating, uncertainty)
                .await
                .expect("Failed to queue bot");
        }

        // Wait for match with timeout
        let timeout_duration = Duration::from_secs(10);
        let result = tokio::time::timeout(timeout_duration, async {
            loop {
                let matches = tester.check_for_matches_filtered(&all_player_ids);
                if !matches.is_empty() {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        assert!(result.unwrap_or(false), "Mixed scenario should succeed");

        let matches = tester.check_for_matches_filtered(&all_player_ids);
        println!(
            "🔍 DEBUG: Found {} matches: {:?}",
            matches.len(),
            matches
                .iter()
                .map(|g| format!(
                    "Game {} with players: {:?}",
                    g.game_id,
                    g.players
                        .iter()
                        .map(|p| format!("{}({:?})", p.id, p.player_type))
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(matches.len(), 1, "Should have exactly 1 match");

        // Verify mix of humans and bots
        let game = &matches[0];
        let human_count = game
            .players
            .iter()
            .filter(|p| p.player_type == PlayerType::Human)
            .count();
        let bot_count = game
            .players
            .iter()
            .filter(|p| p.player_type == PlayerType::Bot)
            .count();

        assert_eq!(human_count, 2, "Should have 2 humans");
        assert_eq!(bot_count, 2, "Should have 2 bots");
    }

    #[tokio::test]
    async fn test_scenario_multiple_lobbies() {
        let _guard = TEST_MUTEX.lock().await;

        // Restart parlor room service for completely fresh state
        QueueTester::restart_parlor_room_service()
            .await
            .expect("Failed to restart service");

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");

        // Reset local state
        tester.reset();

        let scenario = TestScenarios::multiple_lobbies();

        let result = tester.run_test_scenario(scenario).await;
        assert!(
            result.is_ok(),
            "Multiple lobbies scenario failed: {:?}",
            result
        );

        let success = result.unwrap();
        assert!(success, "Multiple lobbies scenario should succeed");

        let matches = tester.check_for_matches();
        println!(
            "🔍 DEBUG: Found {} matches: {:?}",
            matches.len(),
            matches
                .iter()
                .map(|g| format!(
                    "Game {} with players: {:?}",
                    g.game_id,
                    g.players
                        .iter()
                        .map(|p| format!("{}({:?})", p.id, p.player_type))
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(matches.len(), 2, "Should have exactly 2 matches");
    }

    #[tokio::test]
    async fn test_queue_monitoring() {
        let _guard = TEST_MUTEX.lock().await;

        let tester = QueueTester::new()
            .await
            .expect("Failed to create queue tester");

        // Queue some players in the background
        tokio::spawn({
            let tester = QueueTester::new()
                .await
                .expect("Failed to create background tester");
            async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let _ = tester
                    .queue_human("bg_human_1", LobbyType::General, 1500.0, 200.0)
                    .await;
                let _ = tester
                    .queue_human("bg_human_2", LobbyType::General, 1550.0, 180.0)
                    .await;
                let _ = tester
                    .queue_bot("bg_bot_1", LobbyType::General, 1450.0, 220.0)
                    .await;
                let _ = tester
                    .queue_bot("bg_bot_2", LobbyType::General, 1600.0, 190.0)
                    .await;
            }
        });

        // Monitor for a short duration
        let result = tester.monitor_queues(Duration::from_millis(500)).await;
        assert!(result.is_ok(), "Monitoring should not fail");
    }
}
