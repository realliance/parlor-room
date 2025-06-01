//! Metrics collection using Prometheus
//!
//! This module provides comprehensive metrics collection for the parlor-room
//! matchmaking service using Prometheus metrics.

use crate::lobby::manager::LobbyManagerStats;
use crate::types::{LobbyType, PlayerType};
use anyhow::Result;
use prometheus::{
    Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntCounter,
    IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Main metrics collector for the matchmaking service
#[derive(Clone)]
pub struct MetricsCollector {
    /// Prometheus registry
    registry: Arc<Registry>,

    /// Service-level metrics
    service_metrics: ServiceMetrics,

    /// Lobby-related metrics
    lobby_metrics: LobbyMetrics,

    /// Player-related metrics
    player_metrics: PlayerMetrics,

    /// Bot-related metrics
    bot_metrics: BotMetrics,

    /// Performance metrics
    performance_metrics: PerformanceMetrics,
}

/// Service-level metrics
#[derive(Clone)]
pub struct ServiceMetrics {
    /// Service uptime in seconds
    pub uptime_seconds: IntGauge,

    /// Total AMQP messages processed
    pub amqp_messages_total: IntCounterVec,

    /// AMQP message processing errors
    pub amqp_errors_total: IntCounterVec,

    /// Health check status (0=unhealthy, 1=degraded, 2=healthy)
    pub health_status: IntGauge,

    /// Component health status
    pub component_health: IntGaugeVec,
}

/// Lobby-related metrics
#[derive(Clone)]
pub struct LobbyMetrics {
    /// Number of active lobbies by type
    pub active_lobbies: IntGaugeVec,

    /// Total lobbies created
    pub lobbies_created_total: IntCounterVec,

    /// Total lobbies cleaned up
    pub lobbies_cleaned_total: IntCounter,

    /// Total games started
    pub games_started_total: IntCounterVec,

    /// Lobby capacity utilization (0.0 to 1.0)
    pub lobby_utilization: GaugeVec,

    /// Wait time before lobby fills up
    pub lobby_wait_time_seconds: HistogramVec,
}

/// Player-related metrics
#[derive(Clone)]
pub struct PlayerMetrics {
    /// Total players queued
    pub players_queued_total: IntCounterVec,

    /// Players currently waiting in queue
    pub players_waiting: IntGaugeVec,

    /// Total players matched and started games
    pub players_matched_total: IntCounterVec,

    /// Player queue wait time
    pub queue_wait_time_seconds: HistogramVec,

    /// Rating distribution by player type
    pub rating_distribution: HistogramVec,
}

/// Bot-related metrics
#[derive(Clone)]
pub struct BotMetrics {
    /// Active bot queue requests
    pub active_bot_requests: IntGauge,

    /// Bot backfill operations
    pub backfill_operations_total: IntCounterVec,

    /// Bot backfill success rate
    pub backfill_success_rate: Gauge,

    /// Bot utilization in lobbies
    pub bot_utilization: GaugeVec,

    /// Bot authentication failures
    pub bot_auth_failures_total: IntCounter,
}

/// Performance metrics
#[derive(Clone)]
pub struct PerformanceMetrics {
    /// Queue request processing time
    pub queue_processing_duration: Histogram,

    /// Rating calculation time
    pub rating_calculation_duration: Histogram,

    /// Lobby operation durations
    pub lobby_operation_duration: HistogramVec,

    /// AMQP operation durations
    pub amqp_operation_duration: HistogramVec,

    /// Memory usage metrics
    pub memory_usage_bytes: IntGauge,

    /// Thread pool metrics
    pub thread_pool_active: IntGauge,
}

impl MetricsCollector {
    /// Create a new metrics collector with default registry
    pub fn new() -> Result<Self> {
        let registry = Arc::new(Registry::new());
        Self::with_registry(registry)
    }

    /// Create a new metrics collector with custom registry
    pub fn with_registry(registry: Arc<Registry>) -> Result<Self> {
        let service_metrics = ServiceMetrics::new(&registry)?;
        let lobby_metrics = LobbyMetrics::new(&registry)?;
        let player_metrics = PlayerMetrics::new(&registry)?;
        let bot_metrics = BotMetrics::new(&registry)?;
        let performance_metrics = PerformanceMetrics::new(&registry)?;

        Ok(Self {
            registry,
            service_metrics,
            lobby_metrics,
            player_metrics,
            bot_metrics,
            performance_metrics,
        })
    }

    /// Get the Prometheus registry
    pub fn registry(&self) -> Arc<Registry> {
        self.registry.clone()
    }

    /// Get service metrics
    pub fn service(&self) -> &ServiceMetrics {
        &self.service_metrics
    }

    /// Get lobby metrics
    pub fn lobby(&self) -> &LobbyMetrics {
        &self.lobby_metrics
    }

    /// Get player metrics
    pub fn player(&self) -> &PlayerMetrics {
        &self.player_metrics
    }

    /// Get bot metrics
    pub fn bot(&self) -> &BotMetrics {
        &self.bot_metrics
    }

    /// Get performance metrics
    pub fn performance(&self) -> &PerformanceMetrics {
        &self.performance_metrics
    }

    /// Update metrics from lobby manager stats
    pub fn update_from_lobby_stats(&self, stats: &LobbyManagerStats) {
        self.lobby_metrics
            .lobbies_created_total
            .with_label_values(&["total"])
            .inc_by(stats.lobbies_created);

        self.lobby_metrics
            .lobbies_cleaned_total
            .inc_by(stats.lobbies_cleaned);

        self.lobby_metrics
            .games_started_total
            .with_label_values(&["total"])
            .inc_by(stats.games_started);

        self.player_metrics
            .players_queued_total
            .with_label_values(&["total"])
            .inc_by(stats.players_queued);

        // Update current state
        self.lobby_metrics
            .active_lobbies
            .with_label_values(&["total"])
            .set(stats.active_lobbies as i64);

        self.player_metrics
            .players_waiting
            .with_label_values(&["total"])
            .set(stats.players_waiting as i64);
    }

    /// Record a queue request being processed
    pub fn record_queue_request(
        &self,
        player_type: PlayerType,
        lobby_type: LobbyType,
        duration: Duration,
    ) {
        let player_type_str = match player_type {
            PlayerType::Human => "human",
            PlayerType::Bot => "bot",
        };

        let lobby_type_str = match lobby_type {
            LobbyType::General => "general",
            LobbyType::AllBot => "allbot",
        };

        self.player_metrics
            .players_queued_total
            .with_label_values(&[player_type_str, lobby_type_str])
            .inc();

        self.performance_metrics
            .queue_processing_duration
            .observe(duration.as_secs_f64());
    }

    /// Record a lobby being created
    pub fn record_lobby_created(&self, lobby_type: LobbyType) {
        let lobby_type_str = match lobby_type {
            LobbyType::General => "general",
            LobbyType::AllBot => "allbot",
        };

        self.lobby_metrics
            .lobbies_created_total
            .with_label_values(&[lobby_type_str])
            .inc();

        self.lobby_metrics
            .active_lobbies
            .with_label_values(&[lobby_type_str])
            .inc();
    }

    /// Record a game starting
    pub fn record_game_started(&self, lobby_type: LobbyType, human_count: usize, bot_count: usize) {
        let lobby_type_str = match lobby_type {
            LobbyType::General => "general",
            LobbyType::AllBot => "allbot",
        };

        self.lobby_metrics
            .games_started_total
            .with_label_values(&[lobby_type_str])
            .inc();

        // Calculate bot utilization
        let total_players = human_count + bot_count;
        if total_players > 0 {
            let bot_ratio = bot_count as f64 / total_players as f64;
            self.bot_metrics
                .bot_utilization
                .with_label_values(&[lobby_type_str])
                .set(bot_ratio);
        }
    }

    /// Record bot backfill operation
    pub fn record_bot_backfill(&self, success: bool) {
        let status = if success { "success" } else { "failed" };

        self.bot_metrics
            .backfill_operations_total
            .with_label_values(&[status])
            .inc();
    }

    /// Record rating calculation duration
    pub fn record_rating_calculation(&self, duration: Duration) {
        self.performance_metrics
            .rating_calculation_duration
            .observe(duration.as_secs_f64());
    }

    /// Record lobby operation duration
    pub fn record_lobby_operation(&self, operation: &str, duration: Duration) {
        self.performance_metrics
            .lobby_operation_duration
            .with_label_values(&[operation])
            .observe(duration.as_secs_f64());
    }

    /// Record AMQP operation
    pub fn record_amqp_operation(&self, operation: &str, success: bool, duration: Duration) {
        let status = if success { "success" } else { "error" };

        self.service_metrics
            .amqp_messages_total
            .with_label_values(&[operation, status])
            .inc();

        if !success {
            self.service_metrics
                .amqp_errors_total
                .with_label_values(&[operation])
                .inc();
        }

        self.performance_metrics
            .amqp_operation_duration
            .with_label_values(&[operation, status])
            .observe(duration.as_secs_f64());
    }

    /// Update health status
    pub fn update_health_status(&self, status: u8) {
        self.service_metrics.health_status.set(status as i64);
    }

    /// Update component health
    pub fn update_component_health(&self, component: &str, healthy: bool) {
        let status = if healthy { 1 } else { 0 };
        self.service_metrics
            .component_health
            .with_label_values(&[component])
            .set(status);
    }

    /// Create a timer for measuring operation duration
    pub fn start_timer(&self) -> MetricsTimer {
        MetricsTimer::new()
    }
}

/// Timer for measuring operation durations
pub struct MetricsTimer {
    start: Instant,
}

impl MetricsTimer {
    fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get the elapsed duration
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Stop the timer and return the duration
    pub fn stop(self) -> Duration {
        self.elapsed()
    }
}

impl ServiceMetrics {
    fn new(registry: &Registry) -> Result<Self> {
        let uptime_seconds =
            IntGauge::new("parlor_room_uptime_seconds", "Service uptime in seconds")?;
        registry.register(Box::new(uptime_seconds.clone()))?;

        let amqp_messages_total = IntCounterVec::new(
            Opts::new(
                "parlor_room_amqp_messages_total",
                "Total AMQP messages processed",
            ),
            &["operation", "status"],
        )?;
        registry.register(Box::new(amqp_messages_total.clone()))?;

        let amqp_errors_total = IntCounterVec::new(
            Opts::new("parlor_room_amqp_errors_total", "Total AMQP errors"),
            &["operation"],
        )?;
        registry.register(Box::new(amqp_errors_total.clone()))?;

        let health_status = IntGauge::new(
            "parlor_room_health_status",
            "Health status (0=unhealthy, 1=degraded, 2=healthy)",
        )?;
        registry.register(Box::new(health_status.clone()))?;

        let component_health = IntGaugeVec::new(
            Opts::new("parlor_room_component_health", "Component health status"),
            &["component"],
        )?;
        registry.register(Box::new(component_health.clone()))?;

        Ok(Self {
            uptime_seconds,
            amqp_messages_total,
            amqp_errors_total,
            health_status,
            component_health,
        })
    }
}

impl LobbyMetrics {
    fn new(registry: &Registry) -> Result<Self> {
        let active_lobbies = IntGaugeVec::new(
            Opts::new("parlor_room_active_lobbies", "Number of active lobbies"),
            &["lobby_type"],
        )?;
        registry.register(Box::new(active_lobbies.clone()))?;

        let lobbies_created_total = IntCounterVec::new(
            Opts::new("parlor_room_lobbies_created_total", "Total lobbies created"),
            &["lobby_type"],
        )?;
        registry.register(Box::new(lobbies_created_total.clone()))?;

        let lobbies_cleaned_total = IntCounter::new(
            "parlor_room_lobbies_cleaned_total",
            "Total lobbies cleaned up",
        )?;
        registry.register(Box::new(lobbies_cleaned_total.clone()))?;

        let games_started_total = IntCounterVec::new(
            Opts::new("parlor_room_games_started_total", "Total games started"),
            &["lobby_type"],
        )?;
        registry.register(Box::new(games_started_total.clone()))?;

        let lobby_utilization = GaugeVec::new(
            Opts::new(
                "parlor_room_lobby_utilization",
                "Lobby capacity utilization",
            ),
            &["lobby_type"],
        )?;
        registry.register(Box::new(lobby_utilization.clone()))?;

        let lobby_wait_time_seconds = HistogramVec::new(
            HistogramOpts::new(
                "parlor_room_lobby_wait_time_seconds",
                "Lobby wait time in seconds",
            ),
            &["lobby_type"],
        )?;
        registry.register(Box::new(lobby_wait_time_seconds.clone()))?;

        Ok(Self {
            active_lobbies,
            lobbies_created_total,
            lobbies_cleaned_total,
            games_started_total,
            lobby_utilization,
            lobby_wait_time_seconds,
        })
    }
}

impl PlayerMetrics {
    fn new(registry: &Registry) -> Result<Self> {
        let players_queued_total = IntCounterVec::new(
            Opts::new("parlor_room_players_queued_total", "Total players queued"),
            &["player_type", "lobby_type"],
        )?;
        registry.register(Box::new(players_queued_total.clone()))?;

        let players_waiting = IntGaugeVec::new(
            Opts::new(
                "parlor_room_players_waiting",
                "Players currently waiting in queue",
            ),
            &["player_type"],
        )?;
        registry.register(Box::new(players_waiting.clone()))?;

        let players_matched_total = IntCounterVec::new(
            Opts::new("parlor_room_players_matched_total", "Total players matched"),
            &["player_type", "lobby_type"],
        )?;
        registry.register(Box::new(players_matched_total.clone()))?;

        let queue_wait_time_seconds = HistogramVec::new(
            HistogramOpts::new(
                "parlor_room_queue_wait_time_seconds",
                "Player queue wait time",
            ),
            &["player_type", "lobby_type"],
        )?;
        registry.register(Box::new(queue_wait_time_seconds.clone()))?;

        let rating_distribution = HistogramVec::new(
            HistogramOpts::new(
                "parlor_room_rating_distribution",
                "Player rating distribution",
            )
            .buckets(vec![
                500.0, 1000.0, 1200.0, 1400.0, 1600.0, 1800.0, 2000.0, 2500.0,
            ]),
            &["player_type"],
        )?;
        registry.register(Box::new(rating_distribution.clone()))?;

        Ok(Self {
            players_queued_total,
            players_waiting,
            players_matched_total,
            queue_wait_time_seconds,
            rating_distribution,
        })
    }
}

impl BotMetrics {
    fn new(registry: &Registry) -> Result<Self> {
        let active_bot_requests = IntGauge::new(
            "parlor_room_active_bot_requests",
            "Active bot queue requests",
        )?;
        registry.register(Box::new(active_bot_requests.clone()))?;

        let backfill_operations_total = IntCounterVec::new(
            Opts::new(
                "parlor_room_backfill_operations_total",
                "Bot backfill operations",
            ),
            &["status"],
        )?;
        registry.register(Box::new(backfill_operations_total.clone()))?;

        let backfill_success_rate = Gauge::new(
            "parlor_room_backfill_success_rate",
            "Bot backfill success rate",
        )?;
        registry.register(Box::new(backfill_success_rate.clone()))?;

        let bot_utilization = GaugeVec::new(
            Opts::new("parlor_room_bot_utilization", "Bot utilization in lobbies"),
            &["lobby_type"],
        )?;
        registry.register(Box::new(bot_utilization.clone()))?;

        let bot_auth_failures_total = IntCounter::new(
            "parlor_room_bot_auth_failures_total",
            "Bot authentication failures",
        )?;
        registry.register(Box::new(bot_auth_failures_total.clone()))?;

        Ok(Self {
            active_bot_requests,
            backfill_operations_total,
            backfill_success_rate,
            bot_utilization,
            bot_auth_failures_total,
        })
    }
}

impl PerformanceMetrics {
    fn new(registry: &Registry) -> Result<Self> {
        let queue_processing_duration = Histogram::with_opts(
            HistogramOpts::new(
                "parlor_room_queue_processing_duration_seconds",
                "Queue processing time",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
        )?;
        registry.register(Box::new(queue_processing_duration.clone()))?;

        let rating_calculation_duration = Histogram::with_opts(
            HistogramOpts::new(
                "parlor_room_rating_calculation_duration_seconds",
                "Rating calculation time",
            )
            .buckets(vec![0.0001, 0.001, 0.005, 0.01, 0.05, 0.1]),
        )?;
        registry.register(Box::new(rating_calculation_duration.clone()))?;

        let lobby_operation_duration = HistogramVec::new(
            HistogramOpts::new(
                "parlor_room_lobby_operation_duration_seconds",
                "Lobby operation duration",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0]),
            &["operation"],
        )?;
        registry.register(Box::new(lobby_operation_duration.clone()))?;

        let amqp_operation_duration = HistogramVec::new(
            HistogramOpts::new(
                "parlor_room_amqp_operation_duration_seconds",
                "AMQP operation duration",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
            &["operation", "status"],
        )?;
        registry.register(Box::new(amqp_operation_duration.clone()))?;

        let memory_usage_bytes =
            IntGauge::new("parlor_room_memory_usage_bytes", "Memory usage in bytes")?;
        registry.register(Box::new(memory_usage_bytes.clone()))?;

        let thread_pool_active =
            IntGauge::new("parlor_room_thread_pool_active", "Active threads in pool")?;
        registry.register(Box::new(thread_pool_active.clone()))?;

        Ok(Self {
            queue_processing_duration,
            rating_calculation_duration,
            lobby_operation_duration,
            amqp_operation_duration,
            memory_usage_bytes,
            thread_pool_active,
        })
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new().expect("Failed to create default metrics collector")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LobbyType, PlayerType};
    use std::time::Duration;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new().expect("Failed to create metrics collector");

        // Test that we can access all metric groups
        let _service = collector.service();
        let _lobby = collector.lobby();
        let _player = collector.player();
        let _bot = collector.bot();
        let _performance = collector.performance();
    }

    #[test]
    fn test_queue_request_recording() {
        let collector = MetricsCollector::new().expect("Failed to create metrics collector");

        collector.record_queue_request(
            PlayerType::Human,
            LobbyType::General,
            Duration::from_millis(100),
        );

        // Should not panic and metrics should be recorded
        let timer = collector.start_timer();
        std::thread::sleep(Duration::from_millis(10));
        let _duration = timer.stop();
    }

    #[test]
    fn test_lobby_operations() {
        let collector = MetricsCollector::new().expect("Failed to create metrics collector");

        collector.record_lobby_created(LobbyType::General);
        collector.record_game_started(LobbyType::General, 2, 2);
        collector.record_bot_backfill(true);
        collector.record_rating_calculation(Duration::from_nanos(1000));
    }

    #[test]
    fn test_health_status_updates() {
        let collector = MetricsCollector::new().expect("Failed to create metrics collector");

        collector.update_health_status(2); // Healthy
        collector.update_component_health("lobby_manager", true);
        collector.update_component_health("amqp", false);
    }

    #[test]
    fn test_metrics_timer() {
        let collector = MetricsCollector::new().expect("Failed to create metrics collector");
        let timer = collector.start_timer();

        std::thread::sleep(Duration::from_millis(10));
        let duration = timer.elapsed();

        assert!(duration >= Duration::from_millis(10));

        let final_duration = timer.stop();
        assert!(final_duration >= Duration::from_millis(10));
    }
}
