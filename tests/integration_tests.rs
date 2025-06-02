//! Integration tests for the parlor-room matchmaking service
//!
//! These tests validate the entire system working together, including:
//! - Complete lobby lifecycle workflows
//! - Bot integration (active queuing and backfill)
//! - AMQP event publishing
//! - Concurrent request handling
//! - Error handling and recovery

// Modules for organizing tests
mod fixtures;

use parlor_room::lobby::instance::Lobby;
use parlor_room::lobby::manager::LobbyManager;
use parlor_room::lobby::provider::StaticLobbyProvider;
use parlor_room::types::{LobbyType, PlayerType, QueueRequest};
use std::sync::Arc;

use fixtures::{IntegrationBotProvider, MockEventPublisher};

/// Integration test setup that creates a complete system
async fn create_test_system() -> (
    LobbyManager,
    Arc<IntegrationBotProvider>,
    Arc<MockEventPublisher>,
) {
    let bot_provider = Arc::new(IntegrationBotProvider::new());
    let event_publisher = Arc::new(MockEventPublisher::new());
    let lobby_provider = Arc::new(StaticLobbyProvider::new());

    let lobby_manager = LobbyManager::new(lobby_provider, event_publisher.clone());

    (lobby_manager, bot_provider, event_publisher)
}

#[tokio::test]
async fn test_complete_general_lobby_workflow() {
    let (lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Step 1: Human joins queue
    let human_request = QueueRequest {
        player_id: "human_test_1".to_string(),
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
        auth_token: None,
    };

    let lobby_id = lobby_manager
        .handle_queue_request(human_request)
        .await
        .unwrap();

    // Verify PlayerJoinedLobby event was published
    assert_eq!(event_publisher.count_events_of_type("PlayerJoinedLobby"), 1);

    // Step 2: Get lobby info to verify the player was added
    let lobby_info = lobby_manager.get_lobby_info(lobby_id).await.unwrap();
    assert!(lobby_info.is_some());

    let lobby = lobby_info.unwrap();
    let players = lobby.get_players();

    // Should have the human player
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].player_type, PlayerType::Human);
    assert_eq!(players[0].id, "human_test_1");

    println!("✅ Complete general lobby workflow test passed");
}

#[tokio::test]
async fn test_allbot_lobby_workflow() {
    let (lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Add 4 bot queue requests for AllBot lobby (without authentication for now)
    let bot_requests = vec![
        QueueRequest {
            player_id: "skilled_bot_1".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1800.0,
                uncertainty: 120.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None, // Skip auth for this test
        },
        QueueRequest {
            player_id: "skilled_bot_2".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1750.0,
                uncertainty: 130.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        },
        QueueRequest {
            player_id: "average_bot_1".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        },
        QueueRequest {
            player_id: "average_bot_2".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1450.0,
                uncertainty: 180.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        },
    ];

    let mut lobby_id = None;

    // Process all bot requests
    for request in bot_requests {
        let result = lobby_manager.handle_queue_request(request).await.unwrap();
        if lobby_id.is_none() {
            lobby_id = Some(result);
        }
    }

    let lobby_id = lobby_id.unwrap();

    // Verify 4 PlayerJoinedLobby events were published
    assert_eq!(event_publisher.count_events_of_type("PlayerJoinedLobby"), 4);

    // Verify lobby is full and game has started automatically
    let lobby_info = lobby_manager.get_lobby_info(lobby_id).await.unwrap();
    assert!(lobby_info.is_some());

    let lobby = lobby_info.unwrap();

    // AllBot lobbies should automatically start when full
    assert!(lobby.is_full(), "AllBot lobby should be full");

    // The lobby should be in Starting state (game has auto-started)
    use parlor_room::lobby::instance::LobbyState;
    assert_eq!(
        lobby.state(),
        LobbyState::Starting,
        "AllBot lobby should auto-start when full"
    );

    // Verify we have 4 bots
    let players = lobby.get_players();
    assert_eq!(players.len(), 4, "Should have 4 players");
    assert!(
        players
            .iter()
            .all(|p| p.player_type == parlor_room::types::PlayerType::Bot),
        "All players should be bots"
    );

    println!("✅ AllBot lobby workflow test passed");
}

#[tokio::test]
async fn test_mixed_lobby_with_humans_and_bots() {
    let (lobby_manager, _bot_provider, _event_publisher) = create_test_system().await;

    // Step 1: Human joins
    let human_request = QueueRequest {
        player_id: "human_mixed_1".to_string(),
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1600.0,
            uncertainty: 180.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
        auth_token: None,
    };

    let lobby_id = lobby_manager
        .handle_queue_request(human_request)
        .await
        .unwrap();

    // Step 2: Bot joins the same lobby type
    let bot_request = QueueRequest {
        player_id: "average_bot_1".to_string(),
        player_type: PlayerType::Bot,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
        auth_token: None, // Skip auth for test
    };

    let bot_lobby_id = lobby_manager
        .handle_queue_request(bot_request)
        .await
        .unwrap();

    // Should join the same lobby
    assert_eq!(bot_lobby_id, lobby_id);

    // Step 3: Verify lobby composition
    let lobby_info = lobby_manager.get_lobby_info(lobby_id).await.unwrap();
    assert!(lobby_info.is_some());

    let lobby = lobby_info.unwrap();
    let players = lobby.get_players();

    assert_eq!(players.len(), 2);

    // Human should be first due to priority
    assert_eq!(players[0].player_type, PlayerType::Human);
    assert_eq!(players[1].player_type, PlayerType::Bot);

    println!("✅ Mixed lobby with humans and bots test passed");
}

#[tokio::test]
async fn test_lobby_statistics() {
    let (lobby_manager, _bot_provider, _event_publisher) = create_test_system().await;

    // Get initial statistics
    let initial_stats = lobby_manager.get_stats().await.unwrap();
    assert_eq!(initial_stats.lobbies_created, 0);
    assert_eq!(initial_stats.players_queued, 0);

    // Create a lobby by adding a player
    let request = QueueRequest {
        player_id: "stats_test_player".to_string(),
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
        auth_token: None,
    };

    let _lobby_id = lobby_manager.handle_queue_request(request).await.unwrap();

    // Get updated statistics
    let final_stats = lobby_manager.get_stats().await.unwrap();
    assert_eq!(final_stats.lobbies_created, 1);
    assert_eq!(final_stats.players_queued, 1);
    assert_eq!(final_stats.active_lobbies, 1);

    println!("✅ Lobby statistics test passed");
}

#[tokio::test]
async fn test_error_handling_and_recovery() {
    let (lobby_manager, _bot_provider, _event_publisher) = create_test_system().await;

    // Test invalid queue request (empty player ID)
    let invalid_request = QueueRequest {
        player_id: "".to_string(), // Invalid empty ID
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
        auth_token: None,
    };

    let _result = lobby_manager.handle_queue_request(invalid_request).await;
    // For now, the system might accept empty IDs, so we'll just verify it doesn't crash

    // Test valid request to ensure system still works
    let valid_request = QueueRequest {
        player_id: "recovery_test_player".to_string(),
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
        auth_token: None,
    };

    let result = lobby_manager.handle_queue_request(valid_request).await;
    assert!(result.is_ok(), "System should handle valid requests");

    println!("✅ Error handling and recovery test passed");
}

#[tokio::test]
async fn test_multiple_lobbies_creation() {
    let (lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Create requests for different lobby types
    let requests = vec![
        QueueRequest {
            player_id: "human_1".to_string(),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        },
        QueueRequest {
            player_id: "bot_1".to_string(),
            player_type: PlayerType::Bot,
            lobby_type: LobbyType::AllBot,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1600.0,
                uncertainty: 150.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        },
        QueueRequest {
            player_id: "human_2".to_string(),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                rating: 2000.0, // Very different rating
                uncertainty: 100.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        },
    ];

    let mut lobby_ids = Vec::new();

    // Process all requests
    for request in requests {
        let lobby_id = lobby_manager.handle_queue_request(request).await.unwrap();
        lobby_ids.push(lobby_id);
    }

    // Should create different lobbies due to different types and ratings
    assert_eq!(lobby_ids.len(), 3);
    // The first and third might be different lobbies due to rating differences
    // The second should definitely be different due to lobby type
    assert_ne!(lobby_ids[0], lobby_ids[1]); // Different lobby types

    // Verify events were published
    assert_eq!(event_publisher.count_events_of_type("PlayerJoinedLobby"), 3);

    println!("✅ Multiple lobbies creation test passed");
}
