# Parlor Room - Majong Matchmaking

[![CI](https://github.com/realliance/parlor-room/workflows/CI/badge.svg)](https://github.com/realliance/parlor-room/actions/workflows/ci.yml)
[![Docker](https://github.com/realliance/parlor-room/workflows/Docker%20Build%20and%20Publish/badge.svg)](https://github.com/realliance/parlor-room/actions/workflows/docker.yml)
[![Release](https://github.com/realliance/parlor-room/workflows/Release/badge.svg)](https://github.com/realliance/parlor-room/actions/workflows/release.yml)
[![Security Audit](https://github.com/realliance/parlor-room/workflows/CI/badge.svg?label=security)](https://github.com/realliance/parlor-room/actions/workflows/ci.yml)
[![Docker Image](https://ghcr-badge.deta.dev/realliance/parlor-room/latest_tag?trim=major&label=docker)](https://github.com/realliance/parlor-room/pkgs/container/parlor-room)

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](#tests)
[![Test Coverage](https://img.shields.io/badge/tests-114%2F114-brightgreen)](#tests)
[![Performance](https://img.shields.io/badge/benchmarks-optimized-blue)](#benchmarks)

A sophisticated, production-ready matchmaking for bot and human-bot mahjong games

## 🚀 Features

### Core Matchmaking

- **Multi-lobby Support**: All Bot (immediate 4-bot games) and General (mixed human/bot) lobbies
- **Smart Player Matching**: Rating-based matching with configurable tolerance
- **Dynamic Wait Time Calculation**: Statistical analysis of historical wait times
- **Bot Backfilling**: Automatic bot addition with rating compatibility

### Production Architecture

- **Message Queue Integration**: Scalable event-driven architecture
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

#### Using Docker (Recommended)

Pull and run the latest image from GitHub Container Registry:

```bash
# Pull the latest image
docker pull ghcr.io/realliance/parlor-room:latest

# Run with basic configuration
docker run -d \
  --name parlor-room \
  -p 8080:8080 \
  -p 9090:9090 \
  -e AMQP_URL="amqp://guest:guest@rabbitmq:5672/%2f" \
  ghcr.io/realliance/parlor-room:latest

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

### Docker Compose Example

Create a `docker-compose.yml` file:

```yaml
version: "3.8"

services:
  rabbitmq:
    image: rabbitmq:3.12-management
    environment:
      RABBITMQ_DEFAULT_USER: guest
      RABBITMQ_DEFAULT_PASS: guest
    ports:
      - "5672:5672"
      - "15672:15672"
    healthcheck:
      test: rabbitmq-diagnostics -q ping
      interval: 30s
      timeout: 30s
      retries: 3

  parlor-room:
    image: ghcr.io/realliance/parlor-room:latest
    depends_on:
      rabbitmq:
        condition: service_healthy
    environment:
      AMQP_URL: "amqp://guest:guest@rabbitmq:5672/%2f"
      LOG_LEVEL: "info"
      HEALTH_PORT: "8080"
      METRICS_PORT: "9090"
    ports:
      - "8080:8080" # Health endpoints
      - "9090:9090" # Metrics endpoints
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s

  # Optional: Prometheus for metrics collection
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9091:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    command:
      - "--config.file=/etc/prometheus/prometheus.yml"
      - "--storage.tsdb.path=/prometheus"
```

Example `prometheus.yml`:

```yaml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: "parlor-room"
    static_configs:
      - targets: ["parlor-room:9090"]
    scrape_interval: 15s
    metrics_path: /metrics
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

## 🚀 CI/CD Pipeline

This project includes comprehensive GitHub Actions workflows:

### Continuous Integration

The CI pipeline (`.github/workflows/ci.yml`) automatically:

- **Code Quality**: Runs `cargo fmt`, `cargo clippy`, and compilation checks
- **Testing**: Executes all unit and integration tests with RabbitMQ service
- **Security**: Performs security audits with `cargo audit`
- **Coverage**: Generates code coverage reports and uploads to Codecov
- **Benchmarks**: Runs performance benchmarks
- **Multi-platform Builds**: Builds for `x86_64-unknown-linux-gnu` and `x86_64-unknown-linux-musl`
- **Docker Validation**: Validates Dockerfile builds successfully

### Docker Image Publishing

The Docker workflow (`.github/workflows/docker.yml`) automatically:

- **Multi-platform Images**: Builds for `linux/amd64` and `linux/arm64`
- **Registry Publishing**: Pushes to GitHub Container Registry (`ghcr.io`)
- **Security Scanning**: Scans images with Trivy vulnerability scanner
- **Image Testing**: Validates image functionality with smoke tests
- **Tagging Strategy**:
  - `main` branch → `ghcr.io/realliance/parlor-room:main`
  - Tags → `ghcr.io/realliance/parlor-room:v1.0.0`
  - PR branches → `ghcr.io/realliance/parlor-room:pr-123`

### Release Automation

The release workflow (`.github/workflows/release.yml`) automatically:

- **Multi-platform Binaries**: Builds for Linux, macOS, and Windows
- **Release Creation**: Auto-generates releases with changelogs
- **Binary Publishing**: Uploads platform-specific binaries
- **Checksums**: Generates and publishes SHA256 checksums
- **Docker Latest**: Updates `latest` tag for stable releases
- **Cargo Publishing**: Publishes to crates.io (optional)

### Workflow Triggers

- **CI**: Runs on all pushes and pull requests to `main`/`develop`
- **Docker**: Builds images on pushes to `main`/`develop` and tags
- **Release**: Triggers on version tags (`v*`)

### Security Features

- **Dependency Scanning**: Automated security audits
- **Container Scanning**: Trivy vulnerability scanning for Docker images
- **SARIF Upload**: Security findings uploaded to GitHub Security tab
- **Minimal Permissions**: Workflows use minimal required permissions

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
