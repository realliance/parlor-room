# Parlor Room - Majong Matchmaking

A rating-based matchmaking for bot and human-bot mahjong games

## Features

### Core Matchmaking

- **Multi-lobby Support**: All Bot (immediate 4-bot games) and General (mixed human/bot) lobbies
- **Rank Based Player Matching**: Rating-based matching with configurable tolerance based on OpenSkill.
- **Dynamic Wait Time Calculation**: Statistical analysis of historical wait times to mitigate human waits.
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

### Game Starting Event Format

When a lobby is full and a game is ready to start, the service publishes a `GameStarting` event with the following structure:

```jsonc
{
  "lobby_id": "550e8400-e29b-41d4-a716-446655440000",
  "game_id": "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
  "players": [
    {
      "id": "player_123",
      "player_type": "Human",
      "rating": {
        "rating": 1500.0,
        "uncertainty": 350.0
      },
      "joined_at": "2024-01-15T10:30:45.123456Z"
    }
    // + 3 more players
  ],
  "rating_scenarios": {
    "scenarios": [
      {
        "player_id": "player_123",
        "rank": 1,
        "current_rating": {
          "rating": 1500.0,
          "uncertainty": 350.0
        },
        "predicted_rating": {
          "rating": 1532.5,
          "uncertainty": 320.0
        },
        "rating_delta": 32.5
      }
      // Precalculated rating placement scenarios (16 total)
    ],
    "player_ranges": [
      {
        "player_id": "player_123",
        "current_rating": {
          "rating": 1500.0,
          "uncertainty": 350.0
        },
        "best_case_rating": {
          "rating": 1532.5,
          "uncertainty": 320.0
        },
        "worst_case_rating": {
          "rating": 1468.2,
          "uncertainty": 320.0
        },
        "max_gain": 32.5,
        "max_loss": -31.8
      }
      // + 3 more player score overviews
    ]
  },
  "timestamp": "2024-01-15T10:30:55.123456Z"
}
```

### Player Left Lobby Event Format

When a player leaves a lobby (due to disconnect, quit, timeout, etc.), the service publishes a `PlayerLeftLobby` event:

```jsonc
{
  "lobby_id": "550e8400-e29b-41d4-a716-446655440000",
  "player_id": "player_456",
  "reason": "Disconnect",
  "remaining_players": [
    {
      "id": "player_123",
      "player_type": "Human",
      "rating": {
        "rating": 1500.0,
        "uncertainty": 350.0
      },
      "joined_at": "2024-01-15T10:30:45.123456Z"
    }
    // + remaining players still in lobby
  ],
  "timestamp": "2024-01-15T10:32:10.456789Z"
}
```

**Leave Reasons:**

- `Disconnect` - Player connection lost
- `UserQuit` - Player manually left
- `Timeout` - Player inactive too long
- `SystemError` - Internal system error
- `BotReplacement` - Bot was replaced by human player

### Event Publishing

The service publishes events to configured AMQP exchanges:

- `PlayerJoinedLobby`: When a player joins a lobby
- `PlayerLeftLobby`: When a player leaves
- `GameStarting`: When a lobby is full and game begins
