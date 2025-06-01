//! Bot authentication and authorization
//!
//! This module handles authentication and authorization for bots sending
//! queue requests, ensuring only valid bots can participate in matchmaking.

use crate::bot::provider::BotProvider;
use crate::error::{MatchmakingError, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, warn};

/// Trait for bot authentication services
#[async_trait]
pub trait BotAuthenticator: Send + Sync {
    /// Authenticate a bot's queue request
    async fn authenticate_bot(&self, bot_id: &str, auth_token: &str) -> Result<bool>;

    /// Check if a bot is authorized for a specific lobby type
    async fn is_authorized_for_lobby(
        &self,
        bot_id: &str,
        lobby_type: crate::types::LobbyType,
    ) -> Result<bool>;

    /// Validate bot credentials and return bot details if valid
    async fn validate_and_get_bot(
        &self,
        bot_id: &str,
        auth_token: &str,
    ) -> Result<Option<crate::types::Player>>;
}

/// Default bot authenticator that uses a BotProvider for authentication
pub struct DefaultBotAuthenticator {
    bot_provider: Arc<dyn BotProvider>,
}

impl DefaultBotAuthenticator {
    /// Create a new default bot authenticator
    pub fn new(bot_provider: Arc<dyn BotProvider>) -> Self {
        Self { bot_provider }
    }
}

#[async_trait]
impl BotAuthenticator for DefaultBotAuthenticator {
    async fn authenticate_bot(&self, bot_id: &str, auth_token: &str) -> Result<bool> {
        debug!("Authenticating bot: {}", bot_id);

        match self
            .bot_provider
            .validate_bot_request(bot_id, auth_token)
            .await
        {
            Ok(is_valid) => {
                if is_valid {
                    debug!("Bot {} authenticated successfully", bot_id);
                } else {
                    warn!("Bot {} authentication failed: invalid credentials", bot_id);
                }
                Ok(is_valid)
            }
            Err(e) => {
                warn!("Bot {} authentication error: {}", bot_id, e);
                Err(e)
            }
        }
    }

    async fn is_authorized_for_lobby(
        &self,
        bot_id: &str,
        lobby_type: crate::types::LobbyType,
    ) -> Result<bool> {
        // For now, all authenticated bots are authorized for all lobby types
        // In the future, this could be extended to support bot-specific permissions

        // First check if bot exists
        match self.bot_provider.get_bot(bot_id).await? {
            Some(_) => {
                debug!(
                    "Bot {} is authorized for lobby type {:?}",
                    bot_id, lobby_type
                );
                Ok(true)
            }
            None => {
                warn!("Bot {} not found, not authorized for any lobby", bot_id);
                Ok(false)
            }
        }
    }

    async fn validate_and_get_bot(
        &self,
        bot_id: &str,
        auth_token: &str,
    ) -> Result<Option<crate::types::Player>> {
        // First authenticate
        if !self.authenticate_bot(bot_id, auth_token).await? {
            return Ok(None);
        }

        // Then get bot details
        self.bot_provider.get_bot(bot_id).await
    }
}

/// Mock bot authenticator for testing
pub struct MockBotAuthenticator {
    always_allow: bool,
    valid_tokens: std::collections::HashMap<String, String>,
}

impl MockBotAuthenticator {
    /// Create a mock authenticator that always allows authentication
    pub fn allow_all() -> Self {
        Self {
            always_allow: true,
            valid_tokens: std::collections::HashMap::new(),
        }
    }

    /// Create a mock authenticator that denies all authentication
    pub fn deny_all() -> Self {
        Self {
            always_allow: false,
            valid_tokens: std::collections::HashMap::new(),
        }
    }

    /// Create a mock authenticator with specific valid tokens
    pub fn with_tokens(tokens: std::collections::HashMap<String, String>) -> Self {
        Self {
            always_allow: false,
            valid_tokens: tokens,
        }
    }

    /// Add a valid token for a bot
    pub fn add_valid_token(&mut self, bot_id: String, token: String) {
        self.valid_tokens.insert(bot_id, token);
    }
}

#[async_trait]
impl BotAuthenticator for MockBotAuthenticator {
    async fn authenticate_bot(&self, bot_id: &str, auth_token: &str) -> Result<bool> {
        if self.always_allow {
            return Ok(true);
        }

        Ok(self
            .valid_tokens
            .get(bot_id)
            .map_or(false, |token| token == auth_token))
    }

    async fn is_authorized_for_lobby(
        &self,
        _bot_id: &str,
        _lobby_type: crate::types::LobbyType,
    ) -> Result<bool> {
        Ok(self.always_allow || !self.valid_tokens.is_empty())
    }

    async fn validate_and_get_bot(
        &self,
        bot_id: &str,
        auth_token: &str,
    ) -> Result<Option<crate::types::Player>> {
        if !self.authenticate_bot(bot_id, auth_token).await? {
            return Ok(None);
        }

        // Return a mock bot player
        Ok(Some(crate::types::Player {
            id: bot_id.to_string(),
            player_type: crate::types::PlayerType::Bot,
            rating: crate::types::PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            joined_at: crate::utils::current_timestamp(),
        }))
    }
}

/// Helper function to validate bot queue request
pub async fn validate_bot_queue_request(
    authenticator: &dyn BotAuthenticator,
    request: &crate::types::QueueRequest,
) -> Result<()> {
    // Only validate bot requests
    if request.player_type != crate::types::PlayerType::Bot {
        return Ok(());
    }

    // Check if auth token is provided
    let auth_token =
        request
            .auth_token
            .as_ref()
            .ok_or_else(|| MatchmakingError::InvalidQueueRequest {
                reason: "Bot requests must include authentication token".to_string(),
            })?;

    // Authenticate the bot
    let is_authenticated = authenticator
        .authenticate_bot(&request.player_id, auth_token)
        .await?;

    if !is_authenticated {
        return Err(MatchmakingError::BotAuthenticationFailed {
            bot_id: request.player_id.clone(),
        }
        .into());
    }

    // Check authorization for the specific lobby type
    let is_authorized = authenticator
        .is_authorized_for_lobby(&request.player_id, request.lobby_type)
        .await?;

    if !is_authorized {
        return Err(MatchmakingError::InvalidQueueRequest {
            reason: format!(
                "Bot {} is not authorized for lobby type {:?}",
                request.player_id, request.lobby_type
            ),
        }
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::provider::MockBotProvider;
    use crate::types::{LobbyType, PlayerType, QueueRequest};

    #[tokio::test]
    async fn test_default_authenticator_with_mock_provider() {
        let provider = Arc::new(MockBotProvider::with_test_bots());
        let authenticator = DefaultBotAuthenticator::new(provider);

        // Test valid authentication
        let is_valid = authenticator
            .authenticate_bot("testbot1", "token1")
            .await
            .unwrap();
        assert!(is_valid);

        // Test invalid token
        let is_invalid = authenticator
            .authenticate_bot("testbot1", "wrong_token")
            .await
            .unwrap();
        assert!(!is_invalid);

        // Test non-existent bot
        let is_nonexistent = authenticator
            .authenticate_bot("nonexistent", "token")
            .await
            .unwrap();
        assert!(!is_nonexistent);
    }

    #[tokio::test]
    async fn test_lobby_authorization() {
        let provider = Arc::new(MockBotProvider::with_test_bots());
        let authenticator = DefaultBotAuthenticator::new(provider);

        // Test authorization for existing bot
        let is_authorized = authenticator
            .is_authorized_for_lobby("testbot1", LobbyType::AllBot)
            .await
            .unwrap();
        assert!(is_authorized);

        let is_authorized_general = authenticator
            .is_authorized_for_lobby("testbot1", LobbyType::General)
            .await
            .unwrap();
        assert!(is_authorized_general);

        // Test authorization for non-existent bot
        let is_not_authorized = authenticator
            .is_authorized_for_lobby("nonexistent", LobbyType::AllBot)
            .await
            .unwrap();
        assert!(!is_not_authorized);
    }

    #[tokio::test]
    async fn test_validate_and_get_bot() {
        let provider = Arc::new(MockBotProvider::with_test_bots());
        let authenticator = DefaultBotAuthenticator::new(provider);

        // Test valid bot retrieval
        let bot = authenticator
            .validate_and_get_bot("testbot1", "token1")
            .await
            .unwrap();
        assert!(bot.is_some());
        assert_eq!(bot.unwrap().id, "testbot1");

        // Test invalid credentials
        let no_bot = authenticator
            .validate_and_get_bot("testbot1", "wrong_token")
            .await
            .unwrap();
        assert!(no_bot.is_none());
    }

    #[tokio::test]
    async fn test_mock_authenticator_allow_all() {
        let authenticator = MockBotAuthenticator::allow_all();

        let is_valid = authenticator
            .authenticate_bot("any_bot", "any_token")
            .await
            .unwrap();
        assert!(is_valid);

        let is_authorized = authenticator
            .is_authorized_for_lobby("any_bot", LobbyType::AllBot)
            .await
            .unwrap();
        assert!(is_authorized);
    }

    #[tokio::test]
    async fn test_mock_authenticator_deny_all() {
        let authenticator = MockBotAuthenticator::deny_all();

        let is_valid = authenticator
            .authenticate_bot("any_bot", "any_token")
            .await
            .unwrap();
        assert!(!is_valid);

        let is_authorized = authenticator
            .is_authorized_for_lobby("any_bot", LobbyType::AllBot)
            .await
            .unwrap();
        assert!(!is_authorized);
    }

    #[tokio::test]
    async fn test_mock_authenticator_with_tokens() {
        let mut tokens = std::collections::HashMap::new();
        tokens.insert("valid_bot".to_string(), "valid_token".to_string());

        let authenticator = MockBotAuthenticator::with_tokens(tokens);

        // Test valid bot/token
        let is_valid = authenticator
            .authenticate_bot("valid_bot", "valid_token")
            .await
            .unwrap();
        assert!(is_valid);

        // Test invalid token
        let is_invalid = authenticator
            .authenticate_bot("valid_bot", "wrong_token")
            .await
            .unwrap();
        assert!(!is_invalid);

        // Test invalid bot
        let is_unknown = authenticator
            .authenticate_bot("unknown_bot", "any_token")
            .await
            .unwrap();
        assert!(!is_unknown);
    }

    #[tokio::test]
    async fn test_validate_bot_queue_request() {
        let mut tokens = std::collections::HashMap::new();
        tokens.insert("valid_bot".to_string(), "valid_token".to_string());
        let authenticator = MockBotAuthenticator::with_tokens(tokens);

        // Test valid bot request
        let valid_request = QueueRequest {
            player_id: "valid_bot".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: crate::types::PlayerRating::default(),
            timestamp: crate::utils::current_timestamp(),
            auth_token: Some("valid_token".to_string()),
        };

        assert!(validate_bot_queue_request(&authenticator, &valid_request)
            .await
            .is_ok());

        // Test bot request without token
        let no_token_request = QueueRequest {
            player_id: "valid_bot".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: crate::types::PlayerRating::default(),
            timestamp: crate::utils::current_timestamp(),
            auth_token: None,
        };

        assert!(
            validate_bot_queue_request(&authenticator, &no_token_request)
                .await
                .is_err()
        );

        // Test bot request with invalid token
        let invalid_token_request = QueueRequest {
            player_id: "valid_bot".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: crate::types::PlayerRating::default(),
            timestamp: crate::utils::current_timestamp(),
            auth_token: Some("wrong_token".to_string()),
        };

        assert!(
            validate_bot_queue_request(&authenticator, &invalid_token_request)
                .await
                .is_err()
        );

        // Test human request (should pass through without validation)
        let human_request = QueueRequest {
            player_id: "human1".to_string(),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: crate::types::PlayerRating::default(),
            timestamp: crate::utils::current_timestamp(),
            auth_token: None,
        };

        assert!(validate_bot_queue_request(&authenticator, &human_request)
            .await
            .is_ok());
    }
}
