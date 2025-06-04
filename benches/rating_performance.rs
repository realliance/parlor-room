//! Performance benchmarks for rating calculations

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use parlor_room::lobby::manager::LobbyManager;
use parlor_room::lobby::provider::StaticLobbyProvider;
use parlor_room::rating::{RatingCalculator, WengLinRatingCalculator};
use parlor_room::types::{LobbyType, PlayerRating, PlayerType, QueueRequest};
use std::sync::Arc;

// Mock event publisher for benchmarks
#[derive(Debug, Clone)]
struct BenchEventPublisher;

#[async_trait::async_trait]
impl parlor_room::amqp::publisher::EventPublisher for BenchEventPublisher {
    async fn publish_player_joined_lobby(
        &self,
        _event: parlor_room::types::PlayerJoinedLobby,
    ) -> parlor_room::error::Result<()> {
        Ok(())
    }

    async fn publish_player_left_lobby(
        &self,
        _event: parlor_room::types::PlayerLeftLobby,
    ) -> parlor_room::error::Result<()> {
        Ok(())
    }

    async fn publish_game_starting(
        &self,
        _event: parlor_room::types::GameStarting,
    ) -> parlor_room::error::Result<()> {
        Ok(())
    }
}

fn create_bench_system() -> LobbyManager {
    let lobby_provider = Arc::new(StaticLobbyProvider::new());
    let event_publisher = Arc::new(BenchEventPublisher);

    LobbyManager::new(lobby_provider, event_publisher)
}

fn bench_rating_calculations(c: &mut Criterion) {
    let calculator = WengLinRatingCalculator::new(
        parlor_room::rating::weng_lin::ExtendedWengLinConfig::default(),
    )
    .unwrap();

    // Create test players with different ratings
    let players = vec![
        (
            "player1".to_string(),
            parlor_room::types::PlayerRating {
                rating: 1500.0,
                uncertainty: 200.0,
            },
        ),
        (
            "player2".to_string(),
            parlor_room::types::PlayerRating {
                rating: 1600.0,
                uncertainty: 180.0,
            },
        ),
        (
            "player3".to_string(),
            parlor_room::types::PlayerRating {
                rating: 1400.0,
                uncertainty: 220.0,
            },
        ),
        (
            "player4".to_string(),
            parlor_room::types::PlayerRating {
                rating: 1550.0,
                uncertainty: 190.0,
            },
        ),
    ];

    c.bench_function("rating_calculation_4_players", |b| {
        b.iter(|| {
            let rankings = vec![
                ("player1".to_string(), 1),
                ("player2".to_string(), 2),
                ("player3".to_string(), 3),
                ("player4".to_string(), 4),
            ];
            black_box(calculator.calculate_rating_changes(&players, &rankings))
        })
    });
}

fn bench_single_queue_request(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("single_queue_request", |b| {
        b.iter(|| {
            rt.block_on(async {
                let lobby_manager = create_bench_system();

                let request = QueueRequest {
                    player_id: "bench_player".to_string(),
                    player_type: PlayerType::Human,
                    lobby_type: LobbyType::General,
                    current_rating: PlayerRating {
                        rating: 1500.0,
                        uncertainty: 200.0,
                    },
                    timestamp: parlor_room::utils::current_timestamp(),
                };

                black_box(lobby_manager.handle_queue_request(request).await)
            })
        })
    });
}

fn bench_lobby_statistics(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("lobby_statistics", |b| {
        b.iter(|| {
            rt.block_on(async {
                let lobby_manager = create_bench_system();

                // Add some load first
                for i in 0..5 {
                    let request = QueueRequest {
                        player_id: format!("player_{}", i),
                        player_type: PlayerType::Human,
                        lobby_type: LobbyType::General,
                        current_rating: PlayerRating {
                            rating: 1500.0 + (i as f64 * 10.0),
                            uncertainty: 200.0,
                        },
                        timestamp: parlor_room::utils::current_timestamp(),
                    };
                    let _ = lobby_manager.handle_queue_request(request).await;
                }

                black_box(lobby_manager.get_stats().await)
            })
        })
    });
}

criterion_group!(
    benches,
    bench_rating_calculations,
    bench_single_queue_request,
    bench_lobby_statistics
);
criterion_main!(benches);
