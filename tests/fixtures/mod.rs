//! Test fixtures and mock implementations for integration testing

use async_trait::async_trait;
use parlor_room::amqp::publisher::EventPublisher;
use parlor_room::bot::provider::{BotProvider, BotSelectionCriteria};
use parlor_room::error::Result;
use parlor_room::types::{Player, PlayerRating, PlayerType};
use parlor_room::utils::current_timestamp;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock event publisher that captures published events for testing
#[derive(Debug, Default)]
pub struct MockEventPublisher {
    published_events: Arc<Mutex<Vec<parlor_room::types::AmqpMessage>>>,
}

impl MockEventPublisher {
    pub fn new() -> Self {
        Self {
            published_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get all published events (for testing)
    pub fn get_published_events(&self) -> Vec<parlor_room::types::AmqpMessage> {
        self.published_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    /// Clear published events
    pub fn clear_events(&self) {
        if let Ok(mut events) = self.published_events.lock() {
            events.clear();
        }
    }

    /// Count events of specific type
    pub fn count_events_of_type(&self, event_type: &str) -> usize {
        self.get_published_events()
            .iter()
            .filter(|event| match event {
                parlor_room::types::AmqpMessage::PlayerJoinedLobby(_) => {
                    event_type == "PlayerJoinedLobby"
                }
                parlor_room::types::AmqpMessage::PlayerLeftLobby(_) => {
                    event_type == "PlayerLeftLobby"
                }
                parlor_room::types::AmqpMessage::GameStarting(_) => event_type == "GameStarting",
                _ => false,
            })
            .count()
    }
}

#[async_trait]
impl EventPublisher for MockEventPublisher {
    async fn publish_player_joined_lobby(
        &self,
        event: parlor_room::types::PlayerJoinedLobby,
    ) -> Result<()> {
        if let Ok(mut events) = self.published_events.lock() {
            events.push(parlor_room::types::AmqpMessage::PlayerJoinedLobby(event));
        }
        Ok(())
    }

    async fn publish_player_left_lobby(
        &self,
        event: parlor_room::types::PlayerLeftLobby,
    ) -> Result<()> {
        if let Ok(mut events) = self.published_events.lock() {
            events.push(parlor_room::types::AmqpMessage::PlayerLeftLobby(event));
        }
        Ok(())
    }

    async fn publish_game_starting(&self, event: parlor_room::types::GameStarting) -> Result<()> {
        if let Ok(mut events) = self.published_events.lock() {
            events.push(parlor_room::types::AmqpMessage::GameStarting(event));
        }
        Ok(())
    }
}

/// Integration test bot provider with configurable behavior
#[derive(Debug)]
pub struct IntegrationBotProvider {
    bots: Arc<Mutex<HashMap<String, Player>>>,
    reserved_bots: Arc<Mutex<Vec<String>>>,
    auth_tokens: Arc<Mutex<HashMap<String, String>>>,
    backfill_delay_ms: u64,
}

impl IntegrationBotProvider {
    pub fn new() -> Self {
        let provider = Self {
            bots: Arc::new(Mutex::new(HashMap::new())),
            reserved_bots: Arc::new(Mutex::new(Vec::new())),
            auth_tokens: Arc::new(Mutex::new(HashMap::new())),
            backfill_delay_ms: 10, // Small delay to simulate real-world latency
        };

        // Add realistic test bots
        provider.setup_test_bots();
        provider
    }

    /// Set up realistic test bots with varied ratings
    fn setup_test_bots(&self) {
        let test_bots = vec![
            ("skilled_bot_1", 1800.0, 120.0, "token_skilled_1"),
            ("skilled_bot_2", 1750.0, 130.0, "token_skilled_2"),
            ("average_bot_1", 1500.0, 200.0, "token_average_1"),
            ("average_bot_2", 1450.0, 180.0, "token_average_2"),
            ("average_bot_3", 1550.0, 190.0, "token_average_3"),
            ("beginner_bot_1", 1200.0, 250.0, "token_beginner_1"),
            ("beginner_bot_2", 1150.0, 280.0, "token_beginner_2"),
            ("expert_bot_1", 2000.0, 100.0, "token_expert_1"),
            ("newbie_bot_1", 1500.0, 350.0, "token_newbie_1"), // High uncertainty
            ("consistent_bot_1", 1600.0, 80.0, "token_consistent_1"), // Low uncertainty
        ];

        if let (Ok(mut bots), Ok(mut tokens)) = (self.bots.lock(), self.auth_tokens.lock()) {
            for (bot_id, rating, uncertainty, token) in test_bots {
                let bot = Player {
                    id: bot_id.to_string(),
                    player_type: PlayerType::Bot,
                    rating: PlayerRating {
                        rating,
                        uncertainty,
                    },
                    joined_at: current_timestamp(),
                };
                bots.insert(bot_id.to_string(), bot);
                tokens.insert(bot_id.to_string(), token.to_string());
            }
        }
    }

    /// Set backfill delay for testing (simulates network latency)
    pub fn set_backfill_delay(&mut self, delay_ms: u64) {
        self.backfill_delay_ms = delay_ms;
    }

    /// Get number of available bots
    pub fn available_count(&self) -> usize {
        if let (Ok(bots), Ok(reserved)) = (self.bots.lock(), self.reserved_bots.lock()) {
            bots.len() - reserved.len()
        } else {
            0
        }
    }
}

impl Default for IntegrationBotProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BotProvider for IntegrationBotProvider {
    async fn select_backfill_bots(&self, criteria: BotSelectionCriteria) -> Result<Vec<Player>> {
        // Simulate network delay
        if self.backfill_delay_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(self.backfill_delay_ms)).await;
        }

        let (bots, reserved) = {
            let bots_guard = self.bots.lock().map_err(|_| {
                parlor_room::error::MatchmakingError::InternalError {
                    message: "Failed to acquire bots lock".to_string(),
                }
            })?;
            let reserved_guard = self.reserved_bots.lock().map_err(|_| {
                parlor_room::error::MatchmakingError::InternalError {
                    message: "Failed to acquire reserved lock".to_string(),
                }
            })?;
            (bots_guard.clone(), reserved_guard.clone())
        };

        // Filter and select bots
        let mut suitable_bots: Vec<Player> = bots
            .values()
            .filter(|bot| {
                // Skip reserved and excluded bots
                if reserved.contains(&bot.id) || criteria.exclude_bots.contains(&bot.id) {
                    return false;
                }

                // Check rating and uncertainty ranges
                let rating = bot.rating.rating;
                let uncertainty = bot.rating.uncertainty;

                rating >= criteria.rating_range.0
                    && rating <= criteria.rating_range.1
                    && uncertainty >= criteria.uncertainty_range.0
                    && uncertainty <= criteria.uncertainty_range.1
            })
            .cloned()
            .collect();

        // Sort by preferred rating if specified
        if let Some(preferred_rating) = criteria.preferred_rating {
            suitable_bots.sort_by(|a, b| {
                let diff_a = (a.rating.rating - preferred_rating).abs();
                let diff_b = (b.rating.rating - preferred_rating).abs();
                diff_a
                    .partial_cmp(&diff_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Return requested count
        suitable_bots.truncate(criteria.count);
        Ok(suitable_bots)
    }

    async fn validate_bot_request(&self, bot_id: &str, auth_token: &str) -> Result<bool> {
        let tokens = self.auth_tokens.lock().map_err(|_| {
            parlor_room::error::MatchmakingError::InternalError {
                message: "Failed to acquire tokens lock".to_string(),
            }
        })?;

        Ok(tokens
            .get(bot_id)
            .map_or(false, |token| token == auth_token))
    }

    async fn get_bot(&self, bot_id: &str) -> Result<Option<Player>> {
        let bots =
            self.bots
                .lock()
                .map_err(|_| parlor_room::error::MatchmakingError::InternalError {
                    message: "Failed to acquire bots lock".to_string(),
                })?;

        Ok(bots.get(bot_id).cloned())
    }

    async fn reserve_bots(&self, bot_ids: Vec<String>) -> Result<()> {
        let mut reserved = self.reserved_bots.lock().map_err(|_| {
            parlor_room::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved lock".to_string(),
            }
        })?;

        for bot_id in bot_ids {
            if !reserved.contains(&bot_id) {
                reserved.push(bot_id);
            }
        }
        Ok(())
    }

    async fn release_bots(&self, bot_ids: Vec<String>) -> Result<()> {
        let mut reserved = self.reserved_bots.lock().map_err(|_| {
            parlor_room::error::MatchmakingError::InternalError {
                message: "Failed to acquire reserved lock".to_string(),
            }
        })?;

        reserved.retain(|id| !bot_ids.contains(id));
        Ok(())
    }

    async fn available_bot_count(&self) -> Result<usize> {
        Ok(self.available_count())
    }

    async fn is_bot_available(&self, bot_id: &str) -> Result<bool> {
        let (bots, reserved) = {
            let bots_guard = self.bots.lock().map_err(|_| {
                parlor_room::error::MatchmakingError::InternalError {
                    message: "Failed to acquire bots lock".to_string(),
                }
            })?;
            let reserved_guard = self.reserved_bots.lock().map_err(|_| {
                parlor_room::error::MatchmakingError::InternalError {
                    message: "Failed to acquire reserved lock".to_string(),
                }
            })?;
            (
                bots_guard.contains_key(bot_id),
                reserved_guard.contains(&bot_id.to_string()),
            )
        };

        Ok(bots && !reserved)
    }
}

/// Create test players with specific characteristics
pub fn create_test_players() -> Vec<Player> {
    vec![
        Player {
            id: "human_beginner_1".to_string(),
            player_type: PlayerType::Human,
            rating: PlayerRating {
                rating: 1200.0,
                uncertainty: 300.0,
            },
            joined_at: current_timestamp(),
        },
        Player {
            id: "human_average_1".to_string(),
            player_type: PlayerType::Human,
            rating: PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            joined_at: current_timestamp(),
        },
        Player {
            id: "human_skilled_1".to_string(),
            player_type: PlayerType::Human,
            rating: PlayerRating {
                rating: 1800.0,
                uncertainty: 150.0,
            },
            joined_at: current_timestamp(),
        },
        Player {
            id: "human_expert_1".to_string(),
            player_type: PlayerType::Human,
            rating: PlayerRating {
                rating: 2000.0,
                uncertainty: 100.0,
            },
            joined_at: current_timestamp(),
        },
    ]
}

/// Create test queue requests
pub fn create_test_queue_requests() -> Vec<parlor_room::types::QueueRequest> {
    let players = create_test_players();

    players
        .into_iter()
        .map(|player| {
            parlor_room::types::QueueRequest {
                player_id: player.id,
                player_type: player.player_type,
                lobby_type: parlor_room::types::LobbyType::General,
                current_rating: player.rating,
                timestamp: current_timestamp(),
                auth_token: None, // Humans don't need auth tokens
            }
        })
        .collect()
}

/// Create bot queue requests with authentication
pub fn create_bot_queue_requests() -> Vec<parlor_room::types::QueueRequest> {
    vec![
        parlor_room::types::QueueRequest {
            player_id: "skilled_bot_1".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: parlor_room::types::LobbyType::AllBot,
            current_rating: PlayerRating {
                rating: 1800.0,
                uncertainty: 120.0,
            },
            timestamp: current_timestamp(),
            auth_token: Some("token_skilled_1".to_string()),
        },
        parlor_room::types::QueueRequest {
            player_id: "average_bot_1".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: parlor_room::types::LobbyType::General,
            current_rating: PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            timestamp: current_timestamp(),
            auth_token: Some("token_average_1".to_string()),
        },
    ]
}
