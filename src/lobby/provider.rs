//! Lobby provider traits and implementations
//!
//! This module defines the interface for creating and configuring lobbies,
//! along with the static implementation for Phase 2.

use crate::error::{MatchmakingError, Result};
use crate::types::{LobbyType, PlayerType};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for a specific lobby type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyConfiguration {
    /// Type of lobby (AllBot or General)
    pub lobby_type: LobbyType,
    /// Maximum number of players in the lobby
    pub capacity: usize,
    /// Base wait time before considering backfill (only for General lobbies)
    pub base_wait_time_seconds: u64,
    /// Whether to allow bot backfilling (only for General lobbies)
    pub allow_bot_backfill: bool,
    /// Whether to prioritize humans over bots in queue order
    pub prioritize_humans: bool,
    /// Minimum number of human players required (for General lobbies)
    pub min_human_players: usize,
    /// Whether lobby should start immediately when full
    pub immediate_start_when_full: bool,
}

impl LobbyConfiguration {
    /// Create configuration for AllBot lobby
    pub fn all_bot() -> Self {
        Self {
            lobby_type: LobbyType::AllBot,
            capacity: 4,
            base_wait_time_seconds: 0, // Immediate start
            allow_bot_backfill: false, // Not applicable for AllBot
            prioritize_humans: false,  // No humans allowed
            min_human_players: 0,
            immediate_start_when_full: true,
        }
    }

    /// Create configuration for General lobby (mixed human/bot)
    pub fn general() -> Self {
        Self {
            lobby_type: LobbyType::General,
            capacity: 4,
            base_wait_time_seconds: 120, // 2 minutes
            allow_bot_backfill: true,
            prioritize_humans: true,
            min_human_players: 1, // At least one human
            immediate_start_when_full: true,
        }
    }

    /// Check if a player type is allowed in this lobby configuration
    pub fn allows_player_type(&self, player_type: PlayerType) -> bool {
        match (self.lobby_type, player_type) {
            (LobbyType::AllBot, PlayerType::Bot) => true,
            (LobbyType::AllBot, PlayerType::Human) => false,
            (LobbyType::General, _) => true,
        }
    }

    /// Get the wait time as a Duration
    pub fn wait_time(&self) -> Duration {
        Duration::from_secs(self.base_wait_time_seconds)
    }
}

/// Trait for providing lobby configurations and managing lobby types
pub trait LobbyProvider: Send + Sync {
    /// Get configuration for a specific lobby type
    fn get_lobby_config(&self, lobby_type: LobbyType) -> Result<LobbyConfiguration>;

    /// Get all available lobby types
    fn available_lobby_types(&self) -> Vec<LobbyType>;

    /// Validate if a lobby configuration is valid
    fn validate_config(&self, config: &LobbyConfiguration) -> Result<()>;

    /// Get default configuration for a lobby type if not explicitly configured
    fn default_config_for_type(&self, lobby_type: LobbyType) -> LobbyConfiguration;
}

/// Static lobby provider implementation for Phase 2
///
/// This provider manages two predefined lobby types:
/// - AllBot: 4 bots, immediate start
/// - General: 4 players (human + bot), with backfill after wait time
#[derive(Debug, Clone)]
pub struct StaticLobbyProvider {
    all_bot_config: LobbyConfiguration,
    general_config: LobbyConfiguration,
}

impl StaticLobbyProvider {
    /// Create a new static lobby provider with default configurations
    pub fn new() -> Self {
        Self {
            all_bot_config: LobbyConfiguration::all_bot(),
            general_config: LobbyConfiguration::general(),
        }
    }

    /// Create with custom configurations
    pub fn with_configs(
        all_bot_config: LobbyConfiguration,
        general_config: LobbyConfiguration,
    ) -> Result<Self> {
        // Validate configurations
        let provider = Self {
            all_bot_config: all_bot_config.clone(),
            general_config: general_config.clone(),
        };

        provider.validate_config(&all_bot_config)?;
        provider.validate_config(&general_config)?;

        Ok(provider)
    }

    /// Update the AllBot lobby configuration
    pub fn update_all_bot_config(&mut self, config: LobbyConfiguration) -> Result<()> {
        if config.lobby_type != LobbyType::AllBot {
            return Err(MatchmakingError::ConfigurationError {
                message: "Configuration must be for AllBot lobby type".to_string(),
            }
            .into());
        }

        self.validate_config(&config)?;
        self.all_bot_config = config;
        Ok(())
    }

    /// Update the General lobby configuration
    pub fn update_general_config(&mut self, config: LobbyConfiguration) -> Result<()> {
        if config.lobby_type != LobbyType::General {
            return Err(MatchmakingError::ConfigurationError {
                message: "Configuration must be for General lobby type".to_string(),
            }
            .into());
        }

        self.validate_config(&config)?;
        self.general_config = config;
        Ok(())
    }
}

impl Default for StaticLobbyProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LobbyProvider for StaticLobbyProvider {
    fn get_lobby_config(&self, lobby_type: LobbyType) -> Result<LobbyConfiguration> {
        match lobby_type {
            LobbyType::AllBot => Ok(self.all_bot_config.clone()),
            LobbyType::General => Ok(self.general_config.clone()),
        }
    }

    fn available_lobby_types(&self) -> Vec<LobbyType> {
        vec![LobbyType::AllBot, LobbyType::General]
    }

    fn validate_config(&self, config: &LobbyConfiguration) -> Result<()> {
        // Validate capacity
        if config.capacity == 0 {
            return Err(MatchmakingError::ConfigurationError {
                message: "Lobby capacity must be greater than 0".to_string(),
            }
            .into());
        }

        if config.capacity > 8 {
            return Err(MatchmakingError::ConfigurationError {
                message: "Lobby capacity cannot exceed 8 players".to_string(),
            }
            .into());
        }

        // Validate AllBot specific rules
        if config.lobby_type == LobbyType::AllBot {
            if config.min_human_players > 0 {
                return Err(MatchmakingError::ConfigurationError {
                    message: "AllBot lobbies cannot require human players".to_string(),
                }
                .into());
            }

            if config.allow_bot_backfill {
                return Err(MatchmakingError::ConfigurationError {
                    message: "AllBot lobbies should not use bot backfill".to_string(),
                }
                .into());
            }
        }

        // Validate General lobby specific rules
        if config.lobby_type == LobbyType::General {
            if config.min_human_players > config.capacity {
                return Err(MatchmakingError::ConfigurationError {
                    message: "Minimum human players cannot exceed lobby capacity".to_string(),
                }
                .into());
            }
        }

        Ok(())
    }

    fn default_config_for_type(&self, lobby_type: LobbyType) -> LobbyConfiguration {
        match lobby_type {
            LobbyType::AllBot => LobbyConfiguration::all_bot(),
            LobbyType::General => LobbyConfiguration::general(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lobby_configuration_all_bot() {
        let config = LobbyConfiguration::all_bot();
        assert_eq!(config.lobby_type, LobbyType::AllBot);
        assert_eq!(config.capacity, 4);
        assert!(!config.allows_player_type(PlayerType::Human));
        assert!(config.allows_player_type(PlayerType::Bot));
        assert!(config.immediate_start_when_full);
    }

    #[test]
    fn test_lobby_configuration_general() {
        let config = LobbyConfiguration::general();
        assert_eq!(config.lobby_type, LobbyType::General);
        assert_eq!(config.capacity, 4);
        assert!(config.allows_player_type(PlayerType::Human));
        assert!(config.allows_player_type(PlayerType::Bot));
        assert!(config.allow_bot_backfill);
        assert!(config.prioritize_humans);
    }

    #[test]
    fn test_static_lobby_provider_creation() {
        let provider = StaticLobbyProvider::new();
        let lobby_types = provider.available_lobby_types();
        assert_eq!(lobby_types.len(), 2);
        assert!(lobby_types.contains(&LobbyType::AllBot));
        assert!(lobby_types.contains(&LobbyType::General));
    }

    #[test]
    fn test_get_lobby_config() {
        let provider = StaticLobbyProvider::new();

        let all_bot_config = provider.get_lobby_config(LobbyType::AllBot).unwrap();
        assert_eq!(all_bot_config.lobby_type, LobbyType::AllBot);

        let general_config = provider.get_lobby_config(LobbyType::General).unwrap();
        assert_eq!(general_config.lobby_type, LobbyType::General);
    }

    #[test]
    fn test_config_validation() {
        let provider = StaticLobbyProvider::new();

        // Valid configurations should pass
        let valid_config = LobbyConfiguration::all_bot();
        assert!(provider.validate_config(&valid_config).is_ok());

        // Invalid capacity should fail
        let mut invalid_config = LobbyConfiguration::all_bot();
        invalid_config.capacity = 0;
        assert!(provider.validate_config(&invalid_config).is_err());

        // AllBot with human requirements should fail
        let mut invalid_config = LobbyConfiguration::all_bot();
        invalid_config.min_human_players = 1;
        assert!(provider.validate_config(&invalid_config).is_err());
    }

    #[test]
    fn test_custom_configuration_update() {
        let mut provider = StaticLobbyProvider::new();

        // Update AllBot config
        let mut new_config = LobbyConfiguration::all_bot();
        new_config.capacity = 6;
        assert!(provider.update_all_bot_config(new_config).is_ok());

        let updated_config = provider.get_lobby_config(LobbyType::AllBot).unwrap();
        assert_eq!(updated_config.capacity, 6);

        // Try to update with wrong lobby type
        let mut wrong_config = LobbyConfiguration::general();
        wrong_config.lobby_type = LobbyType::General;
        assert!(provider.update_all_bot_config(wrong_config).is_err());
    }
}
