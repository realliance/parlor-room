//! High concurrency stress tests for queue request processing
//!
//! These tests validate system performance under high load conditions
//! and ensure thread safety and responsiveness.

use parlor_room::bot::auth::DefaultBotAuthenticator;
use parlor_room::bot::backfill::{BackfillConfig, DefaultBackfillManager};
use parlor_room::lobby::manager::LobbyManager;
use parlor_room::lobby::provider::StaticLobbyProvider;
use parlor_room::rating::calculator::WengLinRatingCalculator;
use parlor_room::rating::storage::InMemoryRatingStorage;
use parlor_room::types::{LobbyType, PlayerType, QueueRequest};
use parlor_room::wait_time::provider::InternalWaitTimeProvider;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Import test fixtures
use crate::fixtures::{IntegrationBotProvider, MockEventPublisher};

/// Create a high-performance test system optimized for load testing
async fn create_load_test_system() -> (
    Arc<tokio::sync::Mutex<LobbyManager>>,
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

    // Use aggressive backfill for faster testing
    let backfill_config = BackfillConfig::aggressive();
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

    (
        Arc::new(tokio::sync::Mutex::new(lobby_manager)),
        bot_provider,
        event_publisher,
    )
}

#[tokio::test]
async fn test_100_concurrent_queue_requests() {
    let (lobby_manager, _bot_provider, event_publisher) = create_load_test_system().await;
    let concurrent_requests = 100;

    let start_time = Instant::now();

    // Create concurrent queue requests
    let requests: Vec<_> = (0..concurrent_requests)
        .map(|i| QueueRequest {
            player_id: format!("load_test_player_{}", i),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1400.0 + (i as f64 % 400.0), // Spread ratings across range
                uncertainty: 150.0 + (i as f64 % 100.0),
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        })
        .collect();

    // Process requests concurrently
    let handles: Vec<_> = requests
        .into_iter()
        .map(|request| {
            let manager = lobby_manager.clone();
            tokio::spawn(async move {
                let mut mgr = manager.lock().await;
                mgr.handle_queue_request(request).await
            })
        })
        .collect();

    // Wait for all to complete
    let results = futures::future::join_all(handles).await;

    let duration = start_time.elapsed();

    // Verify all requests completed successfully
    let mut successful_requests = 0;
    for result in results {
        match result {
            Ok(Ok(_)) => successful_requests += 1,
            Ok(Err(e)) => eprintln!("Request failed: {}", e),
            Err(e) => eprintln!("Task failed: {}", e),
        }
    }

    // Performance assertions
    assert_eq!(
        successful_requests, concurrent_requests,
        "All requests should succeed"
    );
    assert!(
        duration < Duration::from_secs(10),
        "100 requests should complete within 10 seconds, took: {:?}",
        duration
    );

    // Verify events were published
    let event_count = event_publisher.count_events_of_type("PlayerJoinedLobby");
    assert_eq!(
        event_count, concurrent_requests,
        "Should have published event for each request"
    );

    // Calculate throughput
    let throughput = concurrent_requests as f64 / duration.as_secs_f64();
    println!(
        "✅ 100 concurrent requests test passed - Throughput: {:.1} requests/sec",
        throughput
    );
}

#[tokio::test]
async fn test_1000_concurrent_queue_requests() {
    let (lobby_manager, _bot_provider, event_publisher) = create_load_test_system().await;
    let concurrent_requests = 1000;

    let start_time = Instant::now();

    // Create concurrent queue requests with varied characteristics
    let requests: Vec<_> = (0..concurrent_requests)
        .map(|i| {
            let player_type = if i % 10 == 0 {
                PlayerType::Bot
            } else {
                PlayerType::Human
            };

            let lobby_type = if player_type == PlayerType::Bot && i % 20 == 0 {
                LobbyType::AllBot
            } else {
                LobbyType::General
            };

            let auth_token = if player_type == PlayerType::Bot {
                Some(format!("load_test_token_{}", i))
            } else {
                None
            };

            QueueRequest {
                player_id: format!("stress_test_player_{}", i),
                player_type,
                lobby_type,
                current_rating: parlor_room::types::PlayerRating {
                    rating: 1200.0 + (i as f64 % 600.0), // Wide rating distribution
                    uncertainty: 100.0 + (i as f64 % 150.0),
                },
                timestamp: parlor_room::utils::current_timestamp(),
                auth_token,
            }
        })
        .collect();

    // Process requests in batches to avoid overwhelming the system
    let batch_size = 50;
    let mut successful_requests = 0;

    for chunk in requests.chunks(batch_size) {
        let handles: Vec<_> = chunk
            .iter()
            .cloned()
            .map(|request| {
                let manager = lobby_manager.clone();
                tokio::spawn(async move {
                    let mut mgr = manager.lock().await;
                    mgr.handle_queue_request(request).await
                })
            })
            .collect();

        let batch_results = futures::future::join_all(handles).await;

        for result in batch_results {
            match result {
                Ok(Ok(_)) => successful_requests += 1,
                Ok(Err(e)) => {
                    // Some bot requests might fail due to missing auth setup
                    if !e.to_string().contains("authentication") {
                        eprintln!("Unexpected request failure: {}", e);
                    }
                }
                Err(e) => eprintln!("Task failed: {}", e),
            }
        }

        // Small delay between batches to prevent overwhelming
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let duration = start_time.elapsed();

    // Performance assertions - allow for some failures due to bot auth
    let min_successful = (concurrent_requests as f64 * 0.85) as usize; // 85% success rate
    assert!(
        successful_requests >= min_successful,
        "Should have at least 85% success rate, got {}/{}",
        successful_requests,
        concurrent_requests
    );

    assert!(
        duration < Duration::from_secs(30),
        "1000 requests should complete within 30 seconds, took: {:?}",
        duration
    );

    // Calculate throughput
    let throughput = successful_requests as f64 / duration.as_secs_f64();
    println!(
        "✅ 1000 concurrent requests test passed - Throughput: {:.1} requests/sec ({}/{} successful)",
        throughput, successful_requests, concurrent_requests
    );
}

#[tokio::test]
async fn test_rapid_fire_requests() {
    let (lobby_manager, _bot_provider, event_publisher) = create_load_test_system().await;
    let request_count = 200;
    let request_interval = Duration::from_millis(5); // Very rapid requests

    let start_time = Instant::now();

    // Send requests rapidly in sequence
    let mut handles = Vec::new();
    for i in 0..request_count {
        let request = QueueRequest {
            player_id: format!("rapid_fire_player_{}", i),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        };

        let manager = lobby_manager.clone();
        let handle = tokio::spawn(async move {
            let mut mgr = manager.lock().await;
            mgr.handle_queue_request(request).await
        });

        handles.push(handle);

        // Small delay between requests
        tokio::time::sleep(request_interval).await;
    }

    // Wait for all requests to complete
    let results = futures::future::join_all(handles).await;

    let duration = start_time.elapsed();

    // Verify results
    let successful_requests = results
        .into_iter()
        .filter(|r| matches!(r, Ok(Ok(_))))
        .count();

    assert_eq!(
        successful_requests, request_count,
        "All rapid fire requests should succeed"
    );

    // Verify events
    let event_count = event_publisher.count_events_of_type("PlayerJoinedLobby");
    assert_eq!(event_count, request_count);

    println!(
        "✅ Rapid fire requests test passed - {} requests in {:?}",
        request_count, duration
    );
}

#[tokio::test]
async fn test_concurrent_lobby_processing() {
    let (lobby_manager, _bot_provider, event_publisher) = create_load_test_system().await;

    // Create enough requests to fill multiple lobbies
    let total_requests = 80; // Should create ~20 lobbies (4 players each)

    let start_time = Instant::now();

    // Create requests that will spread across multiple lobbies
    let requests: Vec<_> = (0..total_requests)
        .map(|i| QueueRequest {
            player_id: format!("multi_lobby_player_{}", i),
            player_type: PlayerType::Human,
            lobby_type: LobbyType::General,
            current_rating: parlor_room::types::PlayerRating {
                // Group players by rating to encourage lobby formation
                rating: 1400.0 + ((i / 4) as f64 * 50.0), // Groups of 4 with similar ratings
                uncertainty: 200.0,
            },
            timestamp: parlor_room::utils::current_timestamp(),
            auth_token: None,
        })
        .collect();

    // Process requests concurrently
    let handles: Vec<_> = requests
        .into_iter()
        .map(|request| {
            let manager = lobby_manager.clone();
            tokio::spawn(async move {
                let mut mgr = manager.lock().await;
                mgr.handle_queue_request(request).await
            })
        })
        .collect();

    let results = futures::future::join_all(handles).await;

    // Verify all requests succeeded
    let successful_requests = results
        .into_iter()
        .filter(|r| matches!(r, Ok(Ok(_))))
        .count();

    assert_eq!(successful_requests, total_requests);

    // Check lobby statistics
    let stats = {
        let manager = lobby_manager.lock().await;
        manager.get_statistics()
    };

    let duration = start_time.elapsed();

    println!(
        "✅ Concurrent lobby processing test passed - {} requests created {} lobbies in {:?}",
        total_requests, stats.active_lobbies, duration
    );

    // Verify reasonable lobby creation
    assert!(stats.active_lobbies > 0, "Should have created lobbies");
    assert!(
        stats.active_lobbies <= total_requests,
        "Shouldn't exceed request count"
    );
}

#[tokio::test]
async fn test_system_under_sustained_load() {
    let (lobby_manager, _bot_provider, event_publisher) = create_load_test_system().await;

    let test_duration = Duration::from_secs(10);
    let requests_per_second = 20;
    let total_expected = test_duration.as_secs() as usize * requests_per_second;

    let start_time = Instant::now();
    let mut request_counter = 0;
    let mut handles = Vec::new();

    // Spawn a task to generate sustained load
    let load_generator = tokio::spawn({
        let manager = lobby_manager.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_millis(50)); // 20 req/sec
            let mut counter = 0;

            while start_time.elapsed() < test_duration {
                interval.tick().await;

                let request = QueueRequest {
                    player_id: format!("sustained_load_player_{}", counter),
                    player_type: PlayerType::Human,
                    lobby_type: LobbyType::General,
                    current_rating: parlor_room::types::PlayerRating {
                        rating: 1400.0 + (counter as f64 % 400.0),
                        uncertainty: 200.0,
                    },
                    timestamp: parlor_room::utils::current_timestamp(),
                    auth_token: None,
                };

                let mgr_clone = manager.clone();
                let handle = tokio::spawn(async move {
                    let mut mgr = mgr_clone.lock().await;
                    mgr.handle_queue_request(request).await
                });

                handles.push(handle);
                counter += 1;
            }

            counter
        }
    });

    // Wait for load generation to complete
    request_counter = load_generator.await.unwrap();

    // Wait for all requests to complete
    let results = futures::future::join_all(handles).await;

    let actual_duration = start_time.elapsed();

    // Verify system handled the sustained load
    let successful_requests = results
        .into_iter()
        .filter(|r| matches!(r, Ok(Ok(_))))
        .count();

    let success_rate = successful_requests as f64 / request_counter as f64;

    assert!(
        success_rate >= 0.95,
        "Success rate should be at least 95%, got {:.1}%",
        success_rate * 100.0
    );

    // Verify system stayed responsive
    assert!(
        actual_duration <= test_duration + Duration::from_secs(2),
        "System should stay responsive under load"
    );

    let throughput = successful_requests as f64 / actual_duration.as_secs_f64();

    println!(
        "✅ Sustained load test passed - {:.1} req/sec over {:?} ({:.1}% success rate)",
        throughput,
        actual_duration,
        success_rate * 100.0
    );
}
