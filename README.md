# Parlor Room - Majong Matchmaking

A rating-based matchmaking for bot and human-bot mahjong games

## Features

### Core Matchmaking

- **Multi-lobby Support**: All Bot (immediate 4-bot games) and General (mixed human/bot) lobbies
- **Smart Player Matching**: Rating-based matching with configurable tolerance
- **Dynamic Wait Time Calculation**: Statistical analysis of historical wait times
- **Bot Backfilling**: Automatic bot addition with rating compatibility

## 📋 Quick Start

### Prerequisites

- Rust 1.70+ (or NixOS with provided shell.nix)
- AMQP broker (RabbitMQ recommended, can be provided with docker compose)

### Installation

#### Using Docker (Recommended)

Pull and run the latest image from GitHub Container Registry:

```bash
# Pull the latest image
docker pull ghcr.io/realliance/parlor-room:main

# Run with basic configuration
docker run -d \
  --name parlor-room \
  -p 8080:8080 \
  -p 9090:9090 \
  -e AMQP_URL="amqp://guest:guest@rabbitmq:5672/parlor" \
  ghcr.io/realliance/parlor-room:main

# Check health
curl http://localhost:8080/health
curl http://localhost:9090/metrics
```

#### Building from Source

```bash
# Clone the repository
git clone https://github.com/realliance/parlor-room.git
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
export AMQP_URL="amqp://guest:guest@localhost:5672/parlor"
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
- `
