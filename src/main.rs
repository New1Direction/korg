//! korg — Korg Heavy-Tier Agent Harness
//!
//! Global CLI reference implementation of the Grok 4.20 Heavy / Korg architecture.
//!
//! Install once with `cargo install --path . --force` (binary name: korg).
//!
//! Then run from anywhere:
//!   korg campaign
//!   korg leader --demo
//!   korg leader --replay latest
//!
//! This crate closely follows the pseudocode in:
//!   wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md
//!
//! Run as a worker (when invoked via the korg binary):
//!   korg worker --id my-worker-01

use anyhow::Result;
use clap::{Parser, Subcommand};

mod acp;
mod blackboard;
mod embeddings;
mod evaluator;
mod harness;
mod leader;
mod personas;
mod tui;

use acp::AcpClient;
use harness::SingleWorkerHarness;
use leader::LeaderOrchestrator;

#[derive(Parser)]
#[command(name = "korg")]
#[command(
    version,
    about = "Korg Heavy-Tier Agent Harness (Rust reference implementation)"
)]
#[command(
    long_about = "A production-grade reference implementation of the Korg / Grok 4.20 Heavy architecture.

Features full ACP v1.17 messaging (signed, framed), multi-persona concurrent workers,
live Evaluator guardrails with 5 adversarial rubrics + semantic entropy (Candle optional),
multi-round Arena, human approval gates, persistent signed .ktrans, compaction + fast recovery,
and real-time streaming.

Run `korg campaign` for the full observable Heavy-Tier demo."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run as a single worker (demonstrates the core harness loop + persona routing)
    Worker {
        /// Stable worker identifier
        #[arg(long, default_value = "worker-01")]
        id: String,

        /// ACP endpoint (stdio, unix socket, or ws://)
        #[arg(long, default_value = "stdio")]
        endpoint: String,
    },

    /// Run the full Grok Build-style campaign with real workers, interactive approvals, and persistent blackboard
    Leader {
        #[arg(long, default_value = "stdio")]
        endpoint: String,

        /// Resume from an existing session (uses the current blackboard state as base_snapshot)
        #[arg(long)]
        session: Option<String>,

        /// Shorthand for --session <latest> (resume last campaign)
        #[arg(long)]
        resume: bool,

        /// Non-interactive demo mode: auto-approve everything and print a rich
        /// observable summary of SwarmTelemetryPulse → TraceEvent → Evaluator verdicts → Leader actions.
        #[arg(long)]
        demo: bool,

        /// Replay a previous campaign from its persisted .ktrans artifacts.
        /// Accepts a session UUID or "latest".
        #[arg(long)]
        replay: Option<String>,

        /// When used with --replay or as a standalone monitor, also listen on stdin
        /// for live `AcpMessage::CampaignKtrans` events and print a real-time "LIVE STREAM" ticker.
        #[arg(long)]
        live_stream: bool,

        /// Launch inside the Ratatui TUI dashboard
        #[arg(long)]
        tui: bool,
    },

    /// Fully observable end-to-end Heavy-Tier campaign (recommended for demos).
    /// Equivalent to `leader --demo` but with extra pretty printing.
    Campaign {
        /// Optional session id
        #[arg(long)]
        session: Option<String>,

        /// Launch inside the Ratatui TUI dashboard instead of plain CLI output
        #[arg(long)]
        tui: bool,
    },

    /// Launch the interactive Ratatui TUI dashboard (live ticker, approvals, .ktrans stream, etc.)
    Tui {
        /// Run the TUI without starting a new campaign (for monitoring only)
        #[arg(long)]
        monitor_only: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Worker { id, endpoint } => {
            println!("Starting SingleWorkerHarness (id={})", id);
            println!("Connecting to ACP endpoint: {}", endpoint);

            let mut client =
                AcpClient::connect(&endpoint, &id, vec!["code".into(), "exec".into()]).await?;
            let mut harness = SingleWorkerHarness::new(id);

            harness.run(&mut client).await?;
        }
        Commands::Leader {
            endpoint: _,
            session,
            resume,
            demo,
            replay,
            live_stream,
            tui,
            ..
        } => {
            let sid = if resume {
                None
            } else {
                session.and_then(|s| uuid::Uuid::parse_str(&s).ok())
            };

            let mut leader = LeaderOrchestrator::new(
                "Refactor authentication module with improved audit logging and rate limiting"
                    .to_string(),
                sid,
            );

            println!("[Leader] Session: {}", leader.session_id());
            println!("[Leader] Base snapshot: {}", leader.base_snapshot());

            if let Some(replay_arg) = replay {
                let replay_sid = if replay_arg == "latest" {
                    None
                } else {
                    uuid::Uuid::parse_str(&replay_arg).ok()
                };
                if live_stream {
                    // Run replay + live listener concurrently (simplified: run replay first, then enter live mode)
                    leader.replay_campaign(replay_sid)?;
                    leader.run_live_ktrans_monitor().await?;
                } else {
                    leader.replay_campaign(replay_sid)?;
                }
            } else if tui {
                println!("Launching Ratatui dashboard for Leader mode...");
                crate::tui::run_tui_with_leader(leader).await?;
            } else if demo {
                leader.run_observable_campaign().await?;
            } else {
                leader.run_full_campaign().await?;
            }
        }

        Commands::Campaign { session, tui } => {
            let sid = session.and_then(|s| uuid::Uuid::parse_str(&s).ok());

            if tui {
                println!("Launching Ratatui dashboard with live campaign...");
                crate::tui::run_tui_with_campaign(sid).await?;
            } else {
                let mut leader = LeaderOrchestrator::new(
                    "Implement production-grade semantic evaluation guardrail with 5 adversarial rubrics".to_string(),
                    sid,
                );

                println!("\n=== OBSERVABLE HEAVY-TIER CAMPAIGN (end-to-end telemetry → Evaluator loop) ===\n");
                leader.run_observable_campaign().await?;
            }
        }

        Commands::Tui { monitor_only } => {
            if monitor_only {
                println!("Starting Korg TUI in monitor-only mode (no new campaign)...");
            } else {
                println!("Starting Korg TUI + live observable campaign...");
            }
            // Launch the Ratatui dashboard with a real live campaign.
            crate::tui::run_tui_with_campaign(None).await?;
        }
    }

    Ok(())
}
