# Matchmaking Microservice Execution Plan

## Overview

Build a matchmaking microservice that handles player/bot queuing via AMQP, manages lobbies with different configurations, implements dynamic wait times, and uses **Weng-Lin (OpenSkill)** rating system for skill-based matching via the `skillratings` crate.

**Queue Entry Methods:**

- **Active Queuing**: Both humans and bots can send `QueueRequest` messages to join lobbies
- **Bot Backfilling**: Automatic bot addition to general lobbies when wait times expire

## Module Design Structure

```
src/
├── main.rs                          # Application entry point
├── lib.rs                           # Library root, re-exports
├── config/
│   ├── mod.rs                       # Configuration management
│   ├── amqp.rs                      # AMQP-specific config
│   ├── lobby.rs                     # Lobby configuration
│   └── rating.rs                    # Rating system config
├── amqp/
│   ├── mod.rs                       # AMQP module root
│   ├── connection.rs                # Connection management & pooling
│   ├── handlers.rs                  # Message handlers
│   ├── publisher.rs                 # Event publishing
│   └── messages.rs                  # Message type definitions
├── lobby/
│   ├── mod.rs                       # Lobby module root
│   ├── manager.rs                   # LobbyManager implementation
│   ├── provider.rs                  # LobbyProvider trait & implementations
│   ├── instance.rs                  # Individual lobby instances
│   └── matching.rs                  # Lobby matching algorithms
├── rating/
│   ├── mod.rs                       # Rating module root
│   ├── calculator.rs                # RatingCalculator trait & implementations
│   ├── weng_lin.rs                  # Weng-Lin specific implementation
│   └── storage.rs                   # Rating persistence interface
├── bot/
│   ├── mod.rs                       # Bot module root
│   ├── provider.rs                  # BotProvider trait & implementations
│   ├── backfill.rs                  # Automatic backfilling logic
│   └── auth.rs                      # Bot authentication
├── wait_time/
│   ├── mod.rs                       # Wait time module root
│   ├── provider.rs                  # WaitTimeProvider trait & implementations
│   ├── calculator.rs                # Dynamic wait time calculations
│   └── statistics.rs                # Statistical tracking
├── metrics/
│   ├── mod.rs                       # Metrics module root
│   ├── collector.rs                 # Metrics collection
│   └── health.rs                    # Health check endpoints
├── error.rs                         # Error type definitions
├── types.rs                         # Common type definitions
└── utils.rs                         # Utility functions

tests/
├── integration/
│   ├── amqp_flow.rs                 # End-to-end AMQP message flow tests
│   ├── lobby_lifecycle.rs           # Complete lobby workflows
│   └── bot_integration.rs           # Bot queuing + backfill scenarios
├── load/
│   ├── concurrent_queuing.rs        # High concurrency stress tests
│   ├── lobby_scaling.rs             # Multiple lobby performance
│   └── rating_performance.rs        # Rating calculation benchmarks
└── fixtures/
    ├── mod.rs                       # Test data fixtures
    ├── players.rs                   # Mock player data
    └── messages.rs                  # Mock AMQP messages
```

## Testing Strategy by Component

### **1. AMQP Module (`src/amqp/`)**

#### Unit Tests (in each file):

```rust
// src/amqp/connection.rs
#[cfg(test)]
mod tests {
    // Test connection retry logic
    // Test connection pool management
    // Test connection failure scenarios
}

// src/amqp/handlers.rs
#[cfg(test)]
mod tests {
    // Test message deserialization
    // Test handler routing logic
    // Test error handling for malformed messages
    // Mock AMQP channels for isolated testing
}

// src/amqp/publisher.rs
#[cfg(test)]
mod tests {
    // Test event serialization
    // Test retry logic on publish failures
    // Test deduplication logic
}
```

#### Integration Tests:

```rust
// tests/integration/amqp_flow.rs
#[tokio::test]
async fn test_queue_request_to_game_starting_flow() {
    // End-to-end: QueueRequest -> PlayerJoinedLobby -> GameStarting
}

#[tokio::test]
async fn test_bot_vs_human_queue_priority() {
    // Test human priority in general lobbies
}
```

### **2. Lobby Module (`src/lobby/`)**

#### Unit Tests:

```rust
// src/lobby/manager.rs
#[cfg(test)]
mod tests {
    // Test lobby creation and cleanup
    // Test lobby matching algorithm
    // Test concurrent access safety
    // Mock rating calculator and bot provider
}

// src/lobby/instance.rs
#[cfg(test)]
mod tests {
    // Test player addition/removal
    // Test lobby state transitions
    // Test capacity enforcement
    // Property-based tests for invariants
}

// src/lobby/matching.rs
#[cfg(test)]
mod tests {
    // Test skill-based matching logic
    // Test edge cases (no suitable lobbies)
    // Test rating tolerance calculations
}
```

#### Property-Based Tests:

```rust
// src/lobby/instance.rs
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn lobby_never_exceeds_capacity(players in vec(any::<Player>(), 0..10)) {
            // Ensure lobby capacity is never violated
        }

        #[test]
        fn lobby_state_transitions_are_valid(actions in vec(any::<LobbyAction>(), 0..20)) {
            // Test state machine invariants
        }
    }
}
```

### **3. Rating Module (`src/rating/`)**

#### Unit Tests:

```rust
// src/rating/weng_lin.rs
#[cfg(test)]
mod tests {
    // Test rating calculations with known inputs/outputs
    // Test edge cases (new players, extreme ratings)
    // Test serialization/deserialization of ratings
    // Mock skillratings crate for deterministic testing
}

// src/rating/calculator.rs
#[cfg(test)]
mod tests {
    // Test trait implementations
    // Test error handling for invalid inputs
}
```

#### Benchmark Tests:

```rust
// benches/rating_performance.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn rating_calculation_benchmark(c: &mut Criterion) {
    c.bench_function("4_player_rating_calculation", |b| {
        b.iter(|| {
            // Benchmark rating calculation performance
        })
    });
}
```

### **4. Bot Module (`src/bot/`)**

#### Unit Tests:

```rust
// src/bot/provider.rs
#[cfg(test)]
mod tests {
    // Test bot selection algorithms
    // Test bot authentication logic
    // Test bot availability tracking
    // Mock external bot services
}

// src/bot/backfill.rs
#[cfg(test)]
mod tests {
    // Test backfill trigger conditions
    // Test bot skill matching
    // Test bot removal on human join
}
```

#### Integration Tests:

```rust
// tests/integration/bot_integration.rs
#[tokio::test]
async fn test_active_bot_queuing_flow() {
    // Test bot queue request handling
}

#[tokio::test]
async fn test_automatic_backfill_timing() {
    // Test wait time triggers and bot addition
}

#[tokio::test]
async fn test_mixed_lobby_composition() {
    // Test lobbies with active bots + backfilled bots + humans
}
```

### **5. Wait Time Module (`src/wait_time/`)**

#### Unit Tests:

```rust
// src/wait_time/calculator.rs
#[cfg(test)]
mod tests {
    // Test statistical calculations (mean, std dev)
    // Test min/max bounds enforcement
    // Test edge cases (no historical data)
}

// src/wait_time/statistics.rs
#[cfg(test)]
mod tests {
    // Test running statistics updates
    // Test data persistence and retrieval
    // Test statistical accuracy with known datasets
}
```

#### Statistical Tests:

```rust
// src/wait_time/statistics.rs
#[cfg(test)]
mod statistical_tests {
    #[test]
    fn test_running_mean_accuracy() {
        // Test against known statistical properties
    }

    #[test]
    fn test_standard_deviation_calculation() {
        // Verify statistical correctness
    }
}
```

### **6. Load Testing (`tests/load/`)**

#### Concurrent Access Tests:

```rust
// tests/load/concurrent_queuing.rs
#[tokio::test]
async fn test_1000_concurrent_queue_requests() {
    // Spawn 1000 concurrent queue requests
    // Verify system stability and response times
}

#[tokio::test]
async fn test_lobby_manager_under_load() {
    // Test multiple lobbies being created/destroyed simultaneously
}
```

#### Performance Tests:

```rust
// tests/load/rating_performance.rs
#[tokio::test]
async fn test_rating_calculation_latency() {
    // Verify <100ms processing time requirement
}
```

### **7. End-to-End Tests (`tests/integration/`)**

#### Complete Workflow Tests:

```rust
// tests/integration/lobby_lifecycle.rs
#[tokio::test]
async fn test_complete_general_lobby_workflow() {
    // 1. Human joins queue
    // 2. Bot joins queue
    // 3. Wait time expires
    // 4. Backfill bots added
    // 5. Game starts
    // 6. Rating updates calculated
}

#[tokio::test]
async fn test_allbot_lobby_workflow() {
    // 4 bots queue, immediate start
}
```

## Testing Infrastructure

### **Mock Implementations:**

```rust
// tests/fixtures/mod.rs
pub struct MockAmqpConnection {
    // Mock AMQP for testing without RabbitMQ
}

pub struct MockBotProvider {
    // Deterministic bot generation for testing
}

pub struct MockRatingStorage {
    // In-memory rating storage for tests
}
```

### **Test Configuration:**

```rust
// tests/fixtures/mod.rs
pub struct TestConfig {
    pub enable_slow_tests: bool,
    pub concurrent_test_limit: usize,
    pub mock_external_services: bool,
}
```

### **Test Categories:**

#### Fast Tests (< 1s each):

- All unit tests
- Property-based tests
- Mocked integration tests

#### Slow Tests (1s-30s each):

- Real AMQP integration tests
- Load tests with actual concurrency
- End-to-end workflows

#### Stress Tests (30s+ each):

- High concurrency scenarios
- Memory leak detection
- Performance regression tests

## Continuous Integration Testing

### **Test Stages:**

1. **Fast Tests**: Run on every commit
2. **Integration Tests**: Run on PR creation
3. **Load Tests**: Run nightly
4. **Stress Tests**: Run weekly

### **Coverage Requirements:**

- **Unit Test Coverage**: >90%
- **Integration Test Coverage**: >80% of public APIs
- **Critical Path Coverage**: 100% (queue request handling, rating calculations)

## Architecture Components

### 1. Core Infrastructure Setup

**Step 1.1: Project Structure & Dependencies**

- Initialize Rust project with proper directory structure
- Add dependencies: `amqprs`, `tokio`, `serde`, `anyhow`, `uuid`, `chrono`, `skillratings`
- Set up error handling with `anyhow` crate
- Create basic logging setup with `tracing`
- **Add testing dependencies**: `tokio-test`, `proptest`, `criterion`, `mockall`

**Step 1.2: AMQP Connection Management**

- Create AMQP connection pool/manager
- Implement connection retry logic with exponential backoff
- Set up message serialization/deserialization with serde
- Create message routing configuration

### 2. Message Queue Layer

**Step 2.1: Define Message Types**

- `QueueRequest`: **Any player or bot** wanting to join a lobby
  - Fields: `player_id`, `player_type` (Human/Bot), `lobby_type`, `current_rating`, `uncertainty`, `timestamp`
  - Source: Human players, bot services, or external bot controllers
- `PlayerJoinedLobby`: Emitted when player/bot joins lobby
  - Fields: `lobby_id`, `player_id`, `player_type`, `current_players`, `timestamp`
- `PlayerLeftLobby`: Emitted when player/bot leaves/disconnects
  - Fields: `lobby_id`, `player_id`, `reason`, `remaining_players`, `timestamp`
- `GameStarting`: Emitted when lobby is full and game begins
  - Fields: `lobby_id`, `players`, `rating_changes_preview`, `game_id`, `timestamp`

**Step 2.2: AMQP Message Handlers**

- Implement incoming queue request handler for **both humans and bots**
- Create outbound event publishers for each event type
- Add message validation and error handling
- Implement dead letter queue for failed messages
- Handle bot authentication/validation for active bot queuing

### 3. Lobby Management System

**Step 3.1: Lobby Traits and Interfaces**

- Define `LobbyProvider` trait for future database integration
- Define `Lobby` trait with methods: `add_player()`, `remove_player()`, `is_full()`, `should_start()`
- Create `LobbyConfiguration` struct with capacity, wait times, bot-fill rules, bot priority settings

**Step 3.2: Static Lobby Provider Implementation**

- Implement `StaticLobbyProvider` with two lobby types:
  - `AllBotLobby`: 4 bot capacity, **accepts bot queue requests**, immediate start when full
  - `GeneralLobby`: 4 player capacity, **accepts both human and bot queue requests**, human priority, bot backfill
- Create lobby factory methods
- Implement lobby lifecycle management (create, destroy, cleanup)

**Step 3.3: Lobby Instance Management**

- Create `LobbyManager` to handle multiple active lobby instances
- Implement lobby matching algorithm (find suitable lobby or create new one)
- **Handle queue requests from both humans and bots**
- Add lobby cleanup for abandoned/stale lobbies
- Create lobby state persistence in memory (HashMap with lobby_id)
- Implement priority queuing (humans first in general lobbies, but bots can still actively join)

### 4. Wait Time Calculation System

**Step 4.1: Wait Time Statistics Provider**

- Define `WaitTimeProvider` trait for future external data integration
- Implement `InternalWaitTimeProvider` that tracks:
  - Running average of wait times per lobby type
  - Standard deviation calculation
  - Sample size and confidence intervals
  - **Separate tracking for human vs bot queue times**

**Step 4.2: Dynamic Wait Time Logic**

- Implement wait time calculation: `average + 1 * std_deviation`
- Create minimum/maximum wait time bounds (e.g., 30s min, 5min max)
- Add wait time tracking per lobby instance
- Implement timeout triggers for **automatic bot backfill** (separate from active bot queuing)

### 5. Rating System Integration (Using `skillratings` crate)

**Step 5.1: Rating System Implementation**

- **Use `skillratings` crate with Weng-Lin (OpenSkill) algorithm**
- Create `RatingCalculator` trait for different rating systems
- Implement `WengLinRatingCalculator` with:
  - Multi-player ranking support (1st, 2nd, 3rd, 4th place)
  - Rating change calculation before game starts using `weng_lin_multi_team`
  - Configurable parameters (beta, kappa, etc.)
  - Support for `WengLinRating` struct (rating + uncertainty)

**Step 5.2: Matchmaking Score Integration**

- Add rating-based lobby matching (similar skill levels)
- Implement rating range tolerance (e.g., ±3 uncertainty units)
- **Consider both actively queued bots and humans for skill matching**
- Create rating change preview for `GameStarting` events
- Add rating persistence interface (trait for future database integration)
- Use `expected_score` function for lobby balance prediction

### 6. Bot Integration System

**Step 6.1: Bot Provider Interface**

- Define `BotProvider` trait with two methods:
  - `get_backfill_bot()`: For automatic backfilling
  - `validate_bot_request()`: For active bot queue requests
- Implement `MockBotProvider` that returns bot instances with `WengLinRating`
- Create bot selection algorithm (pick bots with similar rating to humans)
- Add bot availability checking and reservation
- **Handle bot authentication for active queue requests**

**Step 6.2: Dual Bot Integration Logic**

- **Active Bot Queuing**: Handle `QueueRequest` messages from bots
  - Validate bot credentials/permissions
  - Apply same matchmaking logic as humans
  - Route to appropriate lobby based on skill and lobby type
- **Automatic Bot Backfill**: When wait time expires in general lobbies
  - Request suitable bots from `BotProvider`
  - Match bot skill level to existing lobby members
  - Add bots automatically without queue messages
- **Bot Management**: Handle bot removal if humans join during backfill period

### 7. Event System and Monitoring

**Step 7.1: Event Publishing**

- Create event publisher with retry logic
- Implement event ordering and deduplication
- Add event correlation IDs for tracking
- Create event schema validation
- **Track source of players (active queue vs backfill)**

**Step 7.2: Metrics and Monitoring**

- Add lobby utilization metrics (human vs bot ratios)
- Track average wait times and success rates
- Monitor rating distribution and game balance
- **Separate metrics for active bot queuing vs backfill effectiveness**
- Create health check endpoints

### 8. Configuration and Deployment

**Step 8.1: Configuration Management**

- Create configuration structs for all components
- Implement environment variable configuration
- Add configuration validation on startup
- Create default configuration presets
- **Configure bot queue permissions and rate limits**

**Step 8.2: Testing Strategy**

- Unit tests for rating calculations using `skillratings`
- Integration tests for AMQP message flow
- **Test both active bot queuing and backfill scenarios**
- Load testing for concurrent lobby management
- End-to-end testing scenarios
- **Implement comprehensive test coverage as outlined above**

**Step 8.3: Deployment Preparation**

- Create Dockerfile with multi-stage build
- Add health check endpoints
- Implement graceful shutdown handling
- Create deployment scripts and documentation

## Implementation Priority

### Phase 1: Core Foundation (Steps 1-2)

- Basic project setup with AMQP integration
- Message types and basic handlers for **both humans and bots**
- **Set up testing infrastructure and CI pipeline**

### Phase 2: Lobby System (Step 3)

- Static lobby implementation
- **Active queuing support for both humans and bots**
- Basic matchmaking without rating system
- **Unit tests for lobby logic**

### Phase 3: Advanced Features (Steps 4-5)

- Wait time calculation
- **Weng-Lin rating system integration using `skillratings` crate**
- **Property-based tests for rating calculations**

### Phase 4: Bot Integration (Step 6)

- **Dual bot system: active queuing + backfill logic**
- **Comprehensive bot integration tests**

### Phase 5: Production Ready (Steps 7-8)

- Monitoring, testing, deployment
- **Load testing and performance optimization**

## Key Technical Decisions

1. **Rating System**: **Use `skillratings` crate with Weng-Lin (OpenSkill) algorithm** for multi-player games
2. **Wait Time**: Dynamic calculation based on historical data (mean + std deviation)
3. **Bot Integration**: **Dual approach - active queuing via AMQP + automatic backfilling**
4. **Bot Matching**: Select bots with similar rating to maintain game balance
5. **Lobby Lifecycle**: Create new lobbies when needed, clean up empty ones
6. **Error Handling**: Use `anyhow` throughout, avoid `unwrap()` calls
7. **Async Runtime**: Tokio-based with proper async/await patterns
8. **Testing Strategy**: **Comprehensive testing with mocks, property-based tests, and performance benchmarks**

## Bot Integration Details

### Two Bot Entry Methods:

#### 1. Active Bot Queuing (via AMQP):

- Bots send `QueueRequest` messages just like humans
- Applied to both AllBot and General lobbies
- Subject to same skill-based matchmaking
- Requires bot authentication/validation

#### 2. Automatic Bot Backfilling:

- Only applies to General lobbies
- Triggered when human wait time expires
- Bots selected to match skill level of waiting humans
- No queue message required

### Lobby Behavior:

- **AllBot Lobby**: Only accepts bot queue requests, starts immediately when full
- **General Lobby**:
  - Accepts both human and bot queue requests
  - Prioritizes humans in queue order
  - Automatically backfills with bots after wait timeout
  - Can have mix of actively queued bots and backfilled bots

## Rating System Details

### Why Weng-Lin (OpenSkill) over Elo-MMR:

- **Production-ready**: Well-tested crate vs research implementation
- **Multi-player native**: Designed for 4+ player competitions
- **Simpler integration**: Clean API vs complex research code
- **Active maintenance**: Recent updates vs static research project
- **Better documentation**: 100% documented vs research-focused

### Example Usage:

```rust
use skillratings::{
    weng_lin::{weng_lin_multi_team, WengLinConfig, WengLinRating},
    MultiTeamOutcome,
};

// Calculate rating changes for 4-player lobby
let players = vec![player1_rating, player2_rating, player3_rating, player4_rating];
let results = vec![
    (&players[0], MultiTeamOutcome::new(1)), // 1st place
    (&players[1], MultiTeamOutcome::new(2)), // 2nd place
    (&players[2], MultiTeamOutcome::new(3)), // 3rd place
    (&players[3], MultiTeamOutcome::new(4)), // 4th place
];
let new_ratings = weng_lin_multi_team(&results, &WengLinConfig::new());
```

## Success Metrics

- Average lobby fill time < 2 minutes for general lobbies
- Game balance: Rating spread within lobbies < 6 uncertainty units
- System throughput: Handle 1000+ concurrent players (humans + bots)
- **Bot queue success rate: 95%+ for active bot requests**
- Bot backfill accuracy: 90%+ of games start within calculated wait time
- Message processing latency < 100ms for queue requests
- **Test Coverage: >90% unit test coverage, >80% integration coverage**
