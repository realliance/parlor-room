//! Performance benchmarks for rating calculations

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use parlor_room::types::{PlayerRating, PlayerType};
use skillratings::weng_lin::WengLinRating;

fn benchmark_rating_conversion(c: &mut Criterion) {
    let rating = PlayerRating {
        rating: 1500.0,
        uncertainty: 200.0,
    };

    c.bench_function("player_rating_to_weng_lin", |b| {
        b.iter(|| {
            let weng_lin: WengLinRating = black_box(rating.clone()).into();
            black_box(weng_lin)
        })
    });

    c.bench_function("weng_lin_to_player_rating", |b| {
        let weng_lin = WengLinRating {
            rating: 1500.0,
            uncertainty: 200.0,
        };
        b.iter(|| {
            let player_rating: PlayerRating = black_box(weng_lin).into();
            black_box(player_rating)
        })
    });
}

criterion_group!(benches, benchmark_rating_conversion);
criterion_main!(benches);
