# Parlor Room - Production Matchmaking Microservice

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](#tests)
[![Test Coverage](https://img.shields.io/badge/tests-114%2F114-brightgreen)](#tests)
[![Performance](https://img.shields.io/badge/benchmarks-optimized-blue)](#benchmarks)

A sophisticated, production-ready matchmaking microservice for mahjong games, built with Rust for high performance and reliability.

## 🚀 Features

### Core Matchmaking

- **Multi-lobby Support**: AllBot (immediate 4-bot games) and General (mixed human/bot) lobbies
- **Smart Player Matching**: Rating-based matching with configurable tolerance
- **Dynamic Wait Time Calculation**: Statistical analysis of historical wait times
- **Intelligent Bot Backfilling**: Automatic bot addition with rating compatibility

### Production Architecture

- **AMQP Message Queue Integration**: Scalable event-driven architecture
- **Comprehensive Health Monitoring**: Health checks, readiness probes, and statistics
- **Advanced Rating System**: Weng-Lin rating algorithm via skillratings crate
- **Real-time Event Publishing**: Game events to external systems
- **Graceful Shutdown**: Proper cleanup and state preservation

### Performance & Reliability

- **High Throughput**: ~1µs queue request processing, ~900ns rating calculations
- **Concurrent Safety**: Thread-safe operations with proper locking
- **Error Recovery**: Comprehensive error handling with anyhow
- **Memory Efficiency**: Bounded caches and cleanup routines

## 📋 Quick Start

### Prerequisites

- Rust 1.70+ (or NixOS with provided shell.nix)
- AMQP broker (RabbitMQ recommended)

### Installation

```bash
# Clone the repository
git clone https://github.com/your-org/parlor-room.git
cd parlor-room

# Build the project
cargo build --release

# Run tests
cargo test

# Run benchmarks
cargo bench
```

### Configuration

Configure via environment variables:

```bash
# Service Configuration
export SERVICE_NAME="parlor-room"
export LOG_LEVEL="info"
export HEALTH_PORT="8080"

# AMQP Configuration
export AMQP_URL="amqp://guest:guest@localhost:5672/%2f"
export AMQP_QUEUE_NAME="matchmaking.queue_requests"
export AMQP_EXCHANGE_NAME="matchmaking.events"

# Matchmaking Settings
export MAX_WAIT_TIME_SECONDS="300"
export ENABLE_BOT_BACKFILL="true"
export MAX_RATING_DIFFERENCE="500.0"
```

### Running the Service

```bash
# Development
cargo run

# Production
./target/release/parlor-room
```

## 🏗️ Architecture

### Service Components

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   AMQP Queue    │────│  Lobby Manager   │────│  Event Publisher│
│   (Requests)    │    │                  │    │   (Events)      │
└─────────────────┘    └──────────────────┘    └─────────────────┘
                              │
                        ┌─────────────┐
                        │   Rating    │
                        │   System    │
                        └─────────────┘
                              │
                        ┌─────────────┐
                        │ Bot Provider│
                        │ & Backfill  │
                        └─────────────┘
```

### Message Flow

1. **Queue Request** → AMQP → `MessageHandler`
2. **Lobby Matching** → `LobbyManager` → Rating Analysis
3. **Bot Backfill** → Wait Time Analysis → Bot Selection
4. **Game Start** → Event Publishing → External Systems

## 🧪 Tests

```bash
# Run all tests
cargo test

# Run specific test suites
cargo test --test integration_tests
cargo test --lib lobby::

# Run benchmarks
cargo bench
```

## 🔧 API Reference

### Queue Request Format

```json
{
  "player_id": "player_123",
  "lobby_type": "General",
  "player_type": "Human",
  "rating": {
    "rating": 1500.0,
    "uncertainty": 350.0
  }
}
```

### Event Publishing

The service publishes events to configured AMQP exchanges:

- `PlayerJoinedLobby`: When a player joins a lobby
- `PlayerLeftLobby`: When a player leaves
- `GameStarting`: When a lobby is full and game begins

## 🧩 Development

### Architecture Modules

```
src/
├── amqp/          # Message queue integration
├── bot/           # Bot management and backfilling
├── config/        # Configuration management
├── lobby/         # Core matchmaking logic
├── rating/        # Weng-Lin rating system
├── service/       # Production service coordination
├── wait_time/     # Dynamic wait time calculation
└── main.rs        # Service entry point
```

### Adding New Features

1. **New Lobby Types**: Extend `LobbyType` enum and update `LobbyConfiguration`
2. **Rating Algorithms**: Implement `RatingCalculator` trait
3. **Bot Providers**: Implement `BotProvider` trait for different bot sources
4. **Event Types**: Add new events to `amqp::messages` module

## Metrics and Monitoring

The parlor-room service includes comprehensive metrics collection and monitoring capabilities:

### Metrics Endpoints

- **Health Check**: `GET /health` - Service health status
- **Readiness**: `GET /ready` - Service readiness status
- **Liveness**: `GET /alive` - Service liveness status
- **Prometheus Metrics**: `GET /metrics` - Prometheus-formatted metrics
- **Statistics**: `GET /stats` - Human-readable service statistics

### Available Metrics

#### Service Metrics

- `parlor_room_uptime_seconds` - Service uptime in seconds
- `parlor_room_health_status` - Overall health status (0=unhealthy, 1=degraded, 2=healthy)
- `parlor_room_amqp_messages_total` - Total AMQP messages processed
- `parlor_room_amqp_errors_total` - AMQP processing errors

#### Lobby Metrics

- `parlor_room_active_lobbies` - Number of active lobbies by type
- `parlor_room_lobbies_created_total` - Total lobbies created by type
- `parlor_room_lobbies_cleaned_total` - Total lobbies cleaned up
- `parlor_room_games_started_total` - Total games started by lobby type
- `parlor_room_lobby_utilization` - Lobby capacity utilization (0.0 to 1.0)

#### Player Metrics

- `parlor_room_players_queued_total` - Total players queued by type and lobby
- `parlor_room_players_waiting` - Players currently waiting in queue
- `parlor_room_players_matched_total` - Total players matched and started games
- `parlor_room_queue_wait_time_seconds` - Player queue wait time distribution

#### Bot Metrics

- `parlor_room_active_bot_requests` - Active bot queue requests
- `parlor_room_backfill_operations_total` - Bot backfill operations by status
- `parlor_room_bot_utilization` - Bot utilization in lobbies by type
- `parlor_room_bot_auth_failures_total` - Bot authentication failures

#### Performance Metrics

- `parlor_room_queue_processing_duration_seconds` - Queue request processing time
- `parlor_room_rating_calculation_duration_seconds` - Rating calculation time
- `parlor_room_lobby_operation_duration_seconds` - Lobby operation durations
- `parlor_room_memory_usage_bytes` - Memory usage
- `parlor_room_thread_pool_active` - Active threads

### Configuration

Metrics are enabled by default and can be configured via:

```toml
[service]
metrics_port = 9090  # Port for metrics endpoints
```

Or via environment variables:

```bash
METRICS_PORT=9090
```

Or via command line:

```bash
parlor-room --metrics-port 9090
```

### Integration with Monitoring Systems

The metrics endpoint (`/metrics`) provides Prometheus-compatible metrics that can be scraped by:

- Prometheus
- Grafana
- DataDog
- New Relic
- Any monitoring system that supports Prometheus format

Example Prometheus scrape configuration:

```yaml
scrape_configs:
  - job_name: "parlor-room"
    static_configs:
      - targets: ["localhost:9090"]
    scrape_interval: 15s
    metrics_path: /metrics
```
