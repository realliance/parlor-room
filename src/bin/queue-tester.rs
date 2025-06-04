//! Queue Tester CLI Tool
//!
//! Interactive command-line tool for testing queue functionality against real RabbitMQ.
//!
//! Usage:
//!   # Start Docker Compose first:
//!   docker-compose up -d
//!
//!   # Then run the queue tester:
//!   cargo run --bin queue-tester -- --help
//!   cargo run --bin queue-tester queue-human --id "player1" --lobby general --rating 1500
//!   cargo run --bin queue-tester queue-bot --id "bot1" --lobby allbot --rating 1600 --token "secret"
//!   cargo run --bin queue-tester monitor --duration 30
//!   cargo run --bin queue-tester run-scenario --scenario "four-humans"

use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use parlor_room::types::LobbyType;

#[path = "../../tests/queue_tester.rs"]
mod queue_tester;

use queue_tester::{QueueTester, TestScenarios};

#[derive(Parser)]
#[command(name = "queue-tester")]
#[command(
    about = "Interactive queue testing tool for parlor-room matchmaking against real RabbitMQ"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// AMQP URL for RabbitMQ connection
    #[arg(
        long,
        default_value = "amqp://parlor_user:parlor_pass@localhost:5672/parlor"
    )]
    amqp_url: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Queue a human player
    QueueHuman {
        /// Player ID
        #[arg(short, long)]
        id: String,
        /// Lobby type (general or allbot)
        #[arg(short, long)]
        lobby: String,
        /// Player rating
        #[arg(short, long, default_value = "1500.0")]
        rating: f64,
        /// Rating uncertainty
        #[arg(short, long, default_value = "200.0")]
        uncertainty: f64,
    },
    /// Queue a bot player
    QueueBot {
        /// Bot ID
        #[arg(short, long)]
        id: String,
        /// Lobby type (general or allbot)
        #[arg(short, long)]
        lobby: String,
        /// Bot rating
        #[arg(short, long, default_value = "1500.0")]
        rating: f64,
        /// Rating uncertainty
        #[arg(short, long, default_value = "200.0")]
        uncertainty: f64,
    },
    /// Monitor queues for activity
    Monitor {
        /// Duration to monitor in seconds
        #[arg(short, long, default_value = "10")]
        duration: u64,
    },
    /// Check current matches
    CheckMatches,
    /// Run a predefined test scenario
    RunScenario {
        /// Scenario name (four-humans, four-bots, mixed, multiple)
        #[arg(short, long)]
        scenario: String,
    },
    /// Run all test scenarios
    RunAllScenarios,
    /// Show current queue statistics
    Stats,
    /// Test RabbitMQ connection
    TestConnection,
}

fn parse_lobby_type(lobby: &str) -> Result<LobbyType> {
    match lobby.to_lowercase().as_str() {
        "general" => Ok(LobbyType::General),
        "allbot" => Ok(LobbyType::AllBot),
        _ => Err(anyhow::anyhow!(
            "Invalid lobby type. Use 'general' or 'allbot'"
        )),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Set AMQP_URL environment variable if provided
    if let Some(url) = &cli.amqp_url {
        std::env::set_var("AMQP_URL", url);
    }

    println!(
        "🔌 Connecting to RabbitMQ at: {}",
        cli.amqp_url
            .unwrap_or_else(|| "amqp://parlor_user:parlor_pass@localhost:5672/parlor".to_string())
    );

    let tester = match QueueTester::new().await {
        Ok(t) => {
            println!("✅ Connected to RabbitMQ successfully!");
            t
        }
        Err(e) => {
            eprintln!("❌ Failed to connect to RabbitMQ: {}", e);
            eprintln!("💡 Make sure Docker Compose is running: docker-compose up -d");
            std::process::exit(1);
        }
    };

    match cli.command {
        Commands::QueueHuman {
            id,
            lobby,
            rating,
            uncertainty,
        } => {
            let lobby_type = parse_lobby_type(&lobby)?;
            match tester
                .queue_human(&id, lobby_type, rating, uncertainty)
                .await
            {
                Ok(_) => {
                    println!("✅ Successfully queued human '{}'", id);
                    println!("💡 Use 'monitor' command to see when matches are formed");
                }
                Err(e) => {
                    eprintln!("❌ Failed to queue human '{}': {}", id, e);
                    std::process::exit(1);
                }
            }
        }

        Commands::QueueBot {
            id,
            lobby,
            rating,
            uncertainty,
        } => {
            let lobby_type = parse_lobby_type(&lobby)?;
            match tester.queue_bot(&id, lobby_type, rating, uncertainty).await {
                Ok(_) => {
                    println!("✅ Successfully queued bot '{}'", id);
                    println!("💡 Use 'monitor' command to see when matches are formed");
                }
                Err(e) => {
                    eprintln!("❌ Failed to queue bot '{}': {}", id, e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Monitor { duration } => {
            println!("🔍 Starting queue monitor for {} seconds...", duration);
            tester.monitor_queues(Duration::from_secs(duration)).await?;
        }

        Commands::CheckMatches => {
            let matches = tester.check_for_matches();
            if matches.is_empty() {
                println!("No matches found.");
            } else {
                println!("Found {} matches:", matches.len());
                for (i, game) in matches.iter().enumerate() {
                    println!("  Match {}: Game ID {}", i + 1, game.game_id);
                    println!(
                        "    Players: {:?}",
                        game.players
                            .iter()
                            .map(|p| format!("{}({:?})", p.id, p.player_type))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }

        Commands::RunScenario { scenario } => {
            let config = match scenario.to_lowercase().as_str() {
                "four-humans" => TestScenarios::four_humans_general(),
                "four-bots" => TestScenarios::four_bots_allbot(),
                "mixed" => TestScenarios::mixed_lobby(),
                "multiple" => TestScenarios::multiple_lobbies(),
                _ => {
                    eprintln!("❌ Unknown scenario '{}'. Available: four-humans, four-bots, mixed, multiple", scenario);
                    std::process::exit(1);
                }
            };

            println!("🧪 Running scenario: {}", config.scenario_name);
            match tester.run_test_scenario(config).await {
                Ok(success) => {
                    if success {
                        println!("✅ Scenario completed successfully!");
                    } else {
                        println!("❌ Scenario failed or timed out.");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("❌ Error running scenario: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::RunAllScenarios => {
            let scenarios = vec![
                ("four-humans", TestScenarios::four_humans_general()),
                ("four-bots", TestScenarios::four_bots_allbot()),
                ("mixed", TestScenarios::mixed_lobby()),
                ("multiple", TestScenarios::multiple_lobbies()),
            ];

            let mut passed = 0;
            let mut failed = 0;

            println!("🧪 Running all test scenarios...\n");

            for (name, config) in scenarios {
                print!("Running '{}' scenario... ", name);
                match tester.run_test_scenario(config).await {
                    Ok(success) => {
                        if success {
                            println!("✅ PASSED");
                            passed += 1;
                        } else {
                            println!("❌ FAILED (timeout)");
                            failed += 1;
                        }
                    }
                    Err(e) => {
                        println!("❌ FAILED ({})", e);
                        failed += 1;
                    }
                }

                // Small delay between scenarios to avoid interference
                tokio::time::sleep(Duration::from_millis(1000)).await;

                // Reset tester state between scenarios
                tester.reset();
            }

            println!("\n📊 Results: {} passed, {} failed", passed, failed);
            if failed > 0 {
                std::process::exit(1);
            }
        }

        Commands::Stats => {
            let stats = tester.get_stats();
            println!("📊 Queue Statistics:");
            println!("  Total requests: {}", stats.total_requests);
            println!("  Successful matches: {}", stats.successful_matches);
            println!("  Failed requests: {}", stats.failed_requests);
            println!("  Average wait time: {}ms", stats.average_wait_time_ms);
            println!("  Human players queued: {}", stats.human_count);
            println!("  Bot players queued: {}", stats.bot_count);

            let current_matches = tester.check_for_matches();
            println!("  Current matches: {}", current_matches.len());
        }

        Commands::TestConnection => {
            println!("🔌 Testing RabbitMQ connection...");
            println!("✅ Connection successful!");
            println!("💡 RabbitMQ management UI: http://localhost:15672");
            println!("   Username: parlor_user, Password: parlor_pass");
        }
    }

    Ok(())
}
