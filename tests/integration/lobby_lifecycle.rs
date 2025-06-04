//! Complete lobby lifecycle integration tests
//!
//! These tests validate the entire flow from queue requests through
//! lobby management, wait times, rating calculations, and bot integration.

use parlor_room::bot::auth::DefaultBotAuthenticator;
use parlor_room::bot::backfill::{BackfillConfig, DefaultBackfillManager};
use parlor_room::lobby::manager::LobbyManager;
use parlor_room::lobby::provider::StaticLobbyProvider;
use parlor_room::rating::calculator::WengLinRatingCalculator;
use parlor_room::rating::storage::InMemoryRatingStorage;
use parlor_room::types::{LobbyType, Player, PlayerType, QueueRequest};
use parlor_room::wait_time::provider::InternalWaitTimeProvider;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

// Import test fixtures
use crate::fixtures::{
    create_bot_queue_requests, create_test_players, create_test_queue_requests,
    IntegrationBotProvider, MockEventPublisher,
};

/// Integration test setup that creates a complete system
async fn create_test_system() -> (
    LobbyManager,
    Arc<IntegrationBotProvider>,
    Arc<MockEventPublisher>,
) {
    let bot_provider = Arc::new(IntegrationBotProvider::new());
    let event_publisher = Arc::new(MockEventPublisher::new());
    let lobby_provider = Arc::new(StaticLobbyProvider::new());
    let wait_time_provider = Arc::new(InternalWaitTimeProvider::new());
    let rating_storage = Arc::new(InMemoryRatingStorage::new());
    let rating_calculator = Arc::new(WengLinRatingCalculator::new());
    let bot_authenticator = Arc::new(DefaultBotAuthenticator::new(bot_provider.clone()));
    let backfill_config = BackfillConfig::default();
    let backfill_manager = Arc::new(
        DefaultBackfillManager::new(
            backfill_config,
            bot_provider.clone(),
            wait_time_provider.clone(),
            rating_calculator.clone(),
        )
        .unwrap(),
    );

    let lobby_manager = LobbyManager::new(
        lobby_provider,
        event_publisher.clone(),
        wait_time_provider,
        rating_calculator,
        bot_provider.clone(),
        bot_authenticator,
        backfill_manager,
    );

    (lobby_manager, bot_provider, event_publisher)
}

#[tokio::test]
async fn test_complete_general_lobby_workflow() {
    let (mut lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Step 1: Human joins queue
    let human_request = QueueRequest {
        player_id: "lifecycle_human_1".to_string(),
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
    };

    let lobby_id = lobby_manager
        .handle_queue_request(human_request)
        .await
        .unwrap();
    assert!(lobby_id.is_some());
    let lobby_id = lobby_id.unwrap();

    // Verify PlayerJoinedLobby event was published
    assert_eq!(event_publisher.count_events_of_type("PlayerJoinedLobby"), 1);

    // Step 2: Wait for potential backfill (simulate wait time expiry)
    let mut lobby = lobby_manager.get_lobby_mut(&lobby_id).unwrap();

    // Manually trigger wait timeout to force backfill
    lobby.set_wait_timeout(Some(
        parlor_room::utils::current_timestamp() - chrono::Duration::seconds(10),
    ));

    // Step 3: Trigger backfill processing
    lobby_manager.process_backfill().await.unwrap();

    // Step 4: Verify lobby is now full or nearly full
    let lobby = lobby_manager.get_lobby(&lobby_id).unwrap();
    let players = lobby.get_players();

    // Should have added bots through backfill
    assert!(players.len() > 1, "Backfill should have added bots");

    // Verify we have at least one human and some bots
    let human_count = players
        .iter()
        .filter(|p| p.player_type == PlayerType::Human)
        .count();
    let bot_count = players
        .iter()
        .filter(|p| p.player_type == PlayerType::Bot)
        .count();

    assert_eq!(human_count, 1, "Should have exactly one human");
    assert!(bot_count > 0, "Should have bots from backfill");

    // Step 5: If lobby is full, it should be ready to start
    if lobby.is_full() {
        assert!(lobby.should_start(), "Full lobby should be ready to start");

        // Process game start
        lobby_manager.process_ready_lobbies().await.unwrap();

        // Verify GameStarting event was published
        assert_eq!(event_publisher.count_events_of_type("GameStarting"), 1);
    }

    println!("✅ Complete general lobby workflow test passed");
}

#[tokio::test]
async fn test_allbot_lobby_workflow() {
    let (mut lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Add 4 bot queue requests for AllBot lobby
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
        },
    ];

    let mut lobby_id = None;

    // Process all bot requests
    for request in bot_requests {
        let result = lobby_manager.handle_queue_request(request).await.unwrap();
        if lobby_id.is_none() {
            lobby_id = result;
        }
    }

    let lobby_id = lobby_id.unwrap();

    // Verify 4 PlayerJoinedLobby events were published
    assert_eq!(event_publisher.count_events_of_type("PlayerJoinedLobby"), 4);

    // Verify lobby is full and ready to start
    let lobby = lobby_manager.get_lobby(&lobby_id).unwrap();
    assert!(lobby.is_full(), "AllBot lobby should be full");
    assert!(
        lobby.should_start(),
        "Full AllBot lobby should be ready to start"
    );

    // Process ready lobbies
    lobby_manager.process_ready_lobbies().await.unwrap();

    // Verify GameStarting event was published
    assert_eq!(event_publisher.count_events_of_type("GameStarting"), 1);

    println!("✅ AllBot lobby workflow test passed");
}

#[tokio::test]
async fn test_mixed_lobby_with_humans_and_active_bots() {
    let (mut lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Step 1: Human joins
    let human_request = QueueRequest {
        player_id: "lifecycle_human_1".to_string(),
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
    };

    let lobby_id = lobby_manager
        .handle_queue_request(human_request)
        .await
        .unwrap()
        .unwrap();

    // Step 2: Bot actively joins the same lobby type
    let bot_request = QueueRequest {
        player_id: "average_bot_1".to_string(),
        player_type: PlayerType::Bot,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
    };

    let bot_lobby_id = lobby_manager
        .handle_queue_request(bot_request)
        .await
        .unwrap();

    // Should join the same lobby
    assert_eq!(bot_lobby_id, Some(lobby_id));

    // Step 3: Verify lobby composition
    let lobby = lobby_manager.get_lobby(&lobby_id).unwrap();
    let players = lobby.get_players();

    assert_eq!(players.len(), 2);

    // Human should be first due to priority
    assert_eq!(players[0].player_type, PlayerType::Human);
    assert_eq!(players[1].player_type, PlayerType::Bot);

    // Step 4: Trigger backfill to fill remaining slots
    let mut lobby = lobby_manager.get_lobby_mut(&lobby_id).unwrap();
    lobby.set_wait_timeout(Some(
        parlor_room::utils::current_timestamp() - chrono::Duration::seconds(10),
    ));

    lobby_manager.process_backfill().await.unwrap();

    // Step 5: Verify final lobby state
    let lobby = lobby_manager.get_lobby(&lobby_id).unwrap();
    let final_players = lobby.get_players();

    // Should now be full or nearly full
    assert!(
        final_players.len() >= 3,
        "Should have more players after backfill"
    );

    // Verify mix of actively queued and backfilled bots
    let human_count = final_players
        .iter()
        .filter(|p| p.player_type == PlayerType::Human)
        .count();
    let bot_count = final_players
        .iter()
        .filter(|p| p.player_type == PlayerType::Bot)
        .count();

    assert_eq!(human_count, 1);
    assert!(bot_count >= 2); // 1 actively queued + backfilled bots

    println!("✅ Mixed lobby with humans and active bots test passed");
}

#[tokio::test]
async fn test_lobby_cleanup_and_statistics() {
    let (mut lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Create multiple lobbies
    let requests = create_test_queue_requests();
    let mut lobby_ids = Vec::new();

    for request in requests.into_iter().take(2) {
        if let Some(lobby_id) = lobby_manager.handle_queue_request(request).await.unwrap() {
            lobby_ids.push(lobby_id);
        }
    }

    // Get initial statistics
    let initial_stats = lobby_manager.get_statistics();
    assert!(initial_stats.active_lobbies >= 1);

    // Process one lobby to completion
    if let Some(lobby_id) = lobby_ids.first() {
        let mut lobby = lobby_manager.get_lobby_mut(lobby_id).unwrap();

        // Add more players to fill it
        for i in 1..=3 {
            let player = Player {
                id: format!("fill_player_{}", i),
                player_type: PlayerType::Human,
                rating: parlor_room::types::PlayerRating {
                    rating: 1500.0,
                    uncertainty: 200.0,
                },
                joined_at: parlor_room::utils::current_timestamp(),
            };

            let _ = lobby.add_player(player);
        }

        // Mark as starting and then game started
        let _ = lobby.mark_starting();
        let _ = lobby.mark_game_started();
    }

    // Trigger cleanup
    lobby_manager.cleanup_stale_lobbies().await.unwrap();

    // Verify events were published appropriately
    let events = event_publisher.get_published_events();
    assert!(!events.is_empty(), "Should have published events");

    // Get final statistics
    let final_stats = lobby_manager.get_statistics();
    assert!(final_stats.total_players_matched > 0);

    println!("✅ Lobby cleanup and statistics test passed");
}

#[tokio::test]
async fn test_concurrent_queue_requests() {
    let (lobby_manager, _bot_provider, event_publisher) = create_test_system().await;
    let lobby_manager = Arc::new(tokio::sync::Mutex::new(lobby_manager));

    // Create concurrent queue requests
    let concurrent_requests = (0..10)
        .map(|i| QueueRequest {
            player_id: format!("concurrent_player_{}", i),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1500.0 + (i as f64 * 10.0),
                uncertainty: 200.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
        })
        .collect::<Vec<_>>();

    // Process requests concurrently
    let handles: Vec<_> = concurrent_requests
        .into_iter()
        .map(|request| {
            let lobby_manager = lobby_manager.clone();
            tokio::spawn(async move {
                let mut manager = lobby_manager.lock().await;
                manager.handle_queue_request(request).await
            })
        })
        .collect();

    // Wait for all to complete
    let results: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // Verify all requests were processed successfully
    for result in results {
        assert!(result.is_ok(), "All concurrent requests should succeed");
    }

    // Verify appropriate number of events were published
    let event_count = event_publisher.count_events_of_type("PlayerJoinedLobby");
    assert_eq!(event_count, 10, "Should have 10 PlayerJoinedLobby events");

    println!("✅ Concurrent queue requests test passed");
}

#[tokio::test]
async fn test_rating_based_matchmaking() {
    let (mut lobby_manager, _bot_provider, event_publisher) = create_test_system().await;

    // Create players with very different ratings
    let requests = vec![
        QueueRequest {
            player_id: "beginner_player".to_string(),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1200.0,
                uncertainty: 300.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
        },
        QueueRequest {
            player_id: "expert_player".to_string(),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                rating: 2000.0,
                uncertainty: 100.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
        },
    ];

    let mut lobby_ids = Vec::new();

    // Process the requests
    for request in requests {
        if let Some(lobby_id) = lobby_manager.handle_queue_request(request).await.unwrap() {
            lobby_ids.push(lobby_id);
        }
    }

    // Should create separate lobbies due to rating difference
    assert_eq!(
        lobby_ids.len(),
        2,
        "Should create separate lobbies for different skill levels"
    );
    assert_ne!(lobby_ids[0], lobby_ids[1], "Lobbies should be different");

    // Verify each lobby gets appropriate backfill bots
    for lobby_id in lobby_ids {
        let mut lobby = lobby_manager.get_lobby_mut(&lobby_id).unwrap();

        // Trigger backfill
        lobby.set_wait_timeout(Some(
            parlor_room::utils::current_timestamp() - chrono::Duration::seconds(10),
        ));
    }

    lobby_manager.process_backfill().await.unwrap();

    // Verify backfilled bots match the skill level of humans in each lobby
    for lobby_id in lobby_ids {
        let lobby = lobby_manager.get_lobby(&lobby_id).unwrap();
        let players = lobby.get_players();

        if players.len() > 1 {
            // Calculate rating spread
            let ratings: Vec<f64> = players.iter().map(|p| p.rating.rating).collect();
            let min_rating = ratings.iter().cloned().fold(f64::INFINITY, f64::min);
            let max_rating = ratings.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let spread = max_rating - min_rating;

            // Rating spread should be reasonable (within tolerance)
            assert!(
                spread <= 500.0,
                "Rating spread should be reasonable: {} (lobby: {})",
                spread,
                lobby_id
            );
        }
    }

    println!("✅ Rating-based matchmaking test passed");
}

#[tokio::test]
async fn test_error_handling_and_recovery() {
    let (mut lobby_manager, _bot_provider, _event_publisher) = create_test_system().await;

    // Test invalid queue request
    let invalid_request = QueueRequest {
        player_id: "".to_string(), // Invalid empty ID
        player_type: PlayerType::Human,
        lobby_type: LobbyType::General,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
    };

    let result = lobby_manager.handle_queue_request(invalid_request).await;
    assert!(result.is_err(), "Should reject invalid queue request");

    // Test bot request without authentication
    let unauth_bot_request = QueueRequest {
        player_id: "unauth_bot".to_string(),
        player_type: PlayerType::Bot,
        lobby_type: LobbyType::AllBot,
        current_rating: parlor_room::types::PlayerRating {
            rating: 1500.0,
            uncertainty: 200.0,
        },
        timestamp: parlor_room::utils::current_timestamp(),
    };

    let result = lobby_manager.handle_queue_request(unauth_bot_request).await;
    assert!(
        result.is_err(),
        "Should reject bot request without auth token"
    );

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
    };

    let result = lobby_manager.handle_queue_request(valid_request).await;
    assert!(
        result.is_ok(),
        "System should recover and handle valid requests"
    );

    println!("✅ Error handling and recovery test passed");
}
