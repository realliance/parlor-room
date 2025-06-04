//! Common types used throughout the matchmaking service

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use skillratings::weng_lin::WengLinRating;
use uuid::Uuid;

/// Unique identifier for players and bots
pub type PlayerId = String;

/// Unique identifier for lobbies
pub type LobbyId = Uuid;

/// Unique identifier for games
pub type GameId = Uuid;

/// Type of participant in the matchmaking system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerType {
    Human,
    Bot,
}

/// Type of lobby a player wants to join
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LobbyType {
    AllBot,
    General,
}

impl std::fmt::Display for LobbyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LobbyType::AllBot => write!(f, "AllBot"),
            LobbyType::General => write!(f, "General"),
        }
    }
}

/// Rating information for a player/bot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerRating {
    pub rating: f64,
    pub uncertainty: f64,
}

impl Default for PlayerRating {
    fn default() -> Self {
        Self {
            rating: 1500.0,
            uncertainty: 200.0,
        }
    }
}

impl From<WengLinRating> for PlayerRating {
    fn from(rating: WengLinRating) -> Self {
        Self {
            rating: rating.rating,
            uncertainty: rating.uncertainty,
        }
    }
}

impl From<PlayerRating> for WengLinRating {
    fn from(rating: PlayerRating) -> Self {
        Self {
            rating: rating.rating,
            uncertainty: rating.uncertainty,
        }
    }
}

/// Player information for matchmaking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub player_type: PlayerType,
    pub rating: PlayerRating,
    pub joined_at: DateTime<Utc>,
}

/// Reason why a player left a lobby
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LeaveReason {
    Disconnect,
    UserQuit,
    Timeout,
    SystemError,
    BotReplacement,
}

/// AMQP Message Types
/// Request to join a lobby queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueRequest {
    pub player_id: PlayerId,
    pub player_type: PlayerType,
    pub lobby_type: LobbyType,
    pub current_rating: PlayerRating,
    pub timestamp: DateTime<Utc>,
    /// Optional authentication token for bots
    pub auth_token: Option<String>,
}

/// Event emitted when a player/bot joins a lobby
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerJoinedLobby {
    pub lobby_id: LobbyId,
    pub player_id: PlayerId,
    pub player_type: PlayerType,
    pub current_players: Vec<Player>,
    pub timestamp: DateTime<Utc>,
}

/// Event emitted when a player/bot leaves a lobby
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerLeftLobby {
    pub lobby_id: LobbyId,
    pub player_id: PlayerId,
    pub reason: LeaveReason,
    pub remaining_players: Vec<Player>,
    pub timestamp: DateTime<Utc>,
}

/// Rating change scenario for different possible placements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingScenario {
    pub player_id: PlayerId,
    pub rank: u32, // 1st, 2nd, 3rd, 4th place
    pub current_rating: PlayerRating,
    pub predicted_rating: PlayerRating,
    pub rating_delta: f64, // How much rating would change (+/-)
}

/// Complete rating scenarios table for a game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingScenariosTable {
    /// All possible rating outcomes for each player at each rank
    pub scenarios: Vec<RatingScenario>,
    /// Quick lookup: min and max possible rating change for each player
    pub player_ranges: Vec<PlayerRatingRange>,
}

/// Rating range summary for a player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerRatingRange {
    pub player_id: PlayerId,
    pub current_rating: PlayerRating,
    pub best_case_rating: PlayerRating,
    pub worst_case_rating: PlayerRating,
    pub max_gain: f64,
    pub max_loss: f64,
}

/// Rating change information for a player
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingChange {
    pub player_id: PlayerId,
    pub old_rating: PlayerRating,
    pub new_rating: PlayerRating,
    pub rank: u32, // 1st, 2nd, 3rd, 4th place
}

/// Event emitted when a lobby is full and game is starting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameStarting {
    pub lobby_id: LobbyId,
    pub game_id: GameId,
    pub players: Vec<Player>,
    pub rating_scenarios: RatingScenariosTable,
    pub timestamp: DateTime<Utc>,
}

/// Union type for all AMQP messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AmqpMessage {
    QueueRequest(QueueRequest),
    PlayerJoinedLobby(PlayerJoinedLobby),
    PlayerLeftLobby(PlayerLeftLobby),
    GameStarting(GameStarting),
}
