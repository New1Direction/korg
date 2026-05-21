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

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod acp;
mod blackboard;
mod embeddings;
mod evaluator;
mod harness;
mod leader;
mod personas;
mod skills;
mod tools;
mod tui;
pub mod llm;
mod web;
pub mod provenance;
pub mod vision_policy;

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
    /// Optional positional prompt to immediately run a campaign using the full Heavy-Adversarial swarm
    prompt: Option<String>,

    /// Headless execution mode for scripts (bypasses Ratatui TUI dashboard)
    #[arg(long)]
    headless: bool,

    /// Disable automatic post-campaign factual reconciliation
    #[arg(long)]
    no_reconcile: bool,

    /// Disable automatic post-campaign concept synthesis
    #[arg(long)]
    no_synthesize: bool,

    /// Launch the interactive web-based korg dashboard
    #[arg(long)]
    web: bool,

    #[command(subcommand)]
    command: Option<Commands>,
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

        /// Launch inside the web-based korg dashboard
        #[arg(long)]
        web: bool,
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

        /// Launch inside the web-based korg dashboard
        #[arg(long)]
        web: bool,
    },

    /// Launch the interactive Ratatui TUI dashboard (live ticker, approvals, .ktrans stream, etc.)
    Tui {
        /// Run the TUI without starting a new campaign (for monitoring only)
        #[arg(long)]
        monitor_only: bool,
    },

    /// Find and resolve factual contradictions across the vault (Yvaeh mode)
    Reconcile {
        /// Optional topic or concept to focus the reconciliation scan on
        #[arg(long)]
        topic: Option<String>,
    },

    /// Scan the vault for unnamed patterns and generate synthesis connection pages (Yvaeh mode)
    Synthesize,

    /// Cryptographically verify a campaign's provenance attestation certificate
    VerifyProvenance {
        /// Path to the provenance-attestation.json file
        #[arg(short, long)]
        path: std::path::PathBuf,
    },
}

fn print_welcome_banner() {
    let cyan = "\x1b[38;2;0;240;255m";
    let pink = "\x1b[38;2;255;0;180m";
    let gold = "\x1b[38;2;255;215;0m";
    let slate = "\x1b[38;2;120;125;140m";
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";

    println!("{}{}", bold, cyan);
    println!("    __  ___  ____    ____    ______");
    println!("   / /_/ /  / __ \\  / __ \\  / ____/");
    println!("  / ,< /   / / / / / /_/ / / / __  ");
    println!(" / /| /   / /_/ / / _, _/ / /_/ /  ");
    println!("/_/ |_|   \\____/ /_/ |_|  \\____/   ");
    println!("{}", reset);

    println!("{}⚡ {}Heavy-Tier Agent Swarm & Knowledge Vault{}", bold, pink, reset);
    println!("{}──────────────────────────────────────────────────────────────────────────────{}", slate, reset);
    println!("Korg is an autonomous, self-compounding multi-persona orchestrator built with");
    println!("real-time contract negotiation, signed .ktrans journals, and factual alignment.\n");

    println!("{}💡 {}QUICK-START CAMPAIGNS & FLAGS:{}", bold, gold, reset);
    println!("  {}korg \"<prompt>\"{}               Launch full interactive campaign in the Ratatui TUI", bold, reset);
    println!("  {}korg --headless \"<prompt>\"{}      Execute observable swarm telemetry in headless mode", bold, reset);
    println!("  {}korg campaign{}                  Run the default visual demo campaign", bold, reset);
    println!("  {}korg leader --demo{}             Execute swarm benchmark demo with live console tracing", bold, reset);
    println!("  {}korg reconcile{}                 Scan for factual contradictions & resolve them", bold, reset);
    println!("  {}korg synthesize{}                Perform concept synthesis & generate backlinks", bold, reset);
    println!();

    println!("{}⚙️  {}SYSTEM ECOSYSTEM STATUS:{}", bold, cyan, reset);
    println!("  {}• Swarm Engine:{}   5 Adversarial Personas (Captain, Harper, Benjamin, Lucas, Evaluator)", slate, reset);
    let llm_config = crate::llm::KorgConfig::load();
    let model_str = llm_config.default_model.clone().unwrap_or_else(|| "default".to_string());
    println!("  {}• Cognitive Core:{} Swappable Provider [Active: {} | Model: {}]", slate, reset, llm_config.default_llm.to_uppercase(), model_str);
    println!("  {}• Guardrails:{}     5 semantic evaluation rubrics (Trajectory, Epistemic, etc.)", slate, reset);
    println!("  {}• Knowledge:{}     Factual Reconciliation & Semantic Synthesis (Yvaeh mode)", slate, reset);
    println!("  {}• Persistence:{}   Signed .ktrans transactions & secure state recovery", slate, reset);
    println!("{}──────────────────────────────────────────────────────────────────────────────{}", slate, reset);
    println!("Type {}korg --help{} to see all available subcommands and flags.", bold, reset);
    println!();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(prompt) = cli.prompt {
        if cli.web {
            println!("Launching web dashboard with live campaign for prompt: {}", prompt);
            crate::web::run_web_with_campaign(prompt, None).await?;
        } else if cli.headless {
            let cyan = "\x1b[38;2;0;240;255m";
            let pink = "\x1b[38;2;255;0;180m";
            let slate = "\x1b[38;2;120;125;140m";
            let bold = "\x1b[1m";
            let reset = "\x1b[0m";

            println!("\n{bold}{cyan}=== ⚡ RUNNING SWARM CAMPAIGN IN HEADLESS MODE ⚡ ==={reset}\n");
            println!("{slate}├──{reset} Prompt: {bold}{pink}{}{reset}", prompt);
            let mut leader = LeaderOrchestrator::new(prompt, None);
            println!("{slate}├──{reset} Session: {bold}{cyan}{}{reset}", leader.session_id());
            println!("{slate}└──{reset} Base snapshot: {bold}{cyan}{}{reset}\n", leader.base_snapshot());
            leader.run_observable_campaign().await?;
        } else {
            println!("Launching Ratatui dashboard with live campaign for prompt: {}", prompt);
            crate::tui::run_tui_with_campaign(prompt, None).await?;
        }

        // Automatic post-campaign reconciliation and synthesis steps
        if !cli.no_reconcile {
            crate::skills::run_reconcile(None).await?;
        }
        if !cli.no_synthesize {
            crate::skills::run_synthesize().await?;
        }

        return Ok(());
    }

    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            print_welcome_banner();
            return Ok(());
        }
    };

    match command {
        Commands::Worker { id, endpoint } => {
            if endpoint == "stdio" {
                // Phase A: Real signed ACP envelope path used by the leader
                eprintln!("Starting SingleWorkerHarness (id={}) in stdio framed mode", id);
                crate::harness::SingleWorkerHarness::run_as_stdio_worker(id).await?;
            } else {
                println!("Starting SingleWorkerHarness (id={})", id);
                println!("Connecting to ACP endpoint: {}", endpoint);

                let mut client =
                    AcpClient::connect(&endpoint, &id, vec!["code".into(), "exec".into()]).await?;
                let mut harness = SingleWorkerHarness::new(id);

                harness.run(&mut client).await?;
            }
        }
        Commands::Leader {
            endpoint: _,
            session,
            resume,
            demo,
            replay,
            live_stream,
            tui,
            web,
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
            } else if web {
                println!("Launching web-based korg dashboard for leader mode...");
                crate::web::run_web_with_leader(leader).await?;
            } else if tui || !cli.headless {
                println!("Launching Ratatui dashboard for Leader mode...");
                crate::tui::run_tui_with_leader(leader).await?;
            } else {
                let cyan = "\x1b[38;2;0;240;255m";
                let slate = "\x1b[38;2;120;125;140m";
                let bold = "\x1b[1m";
                let reset = "\x1b[0m";
                println!("{slate}├──{reset} Leader Session: {bold}{cyan}{}{reset}", leader.session_id());
                println!("{slate}└──{reset} Base snapshot: {bold}{cyan}{}{reset}", leader.base_snapshot());
                if demo {
                    leader.run_observable_campaign().await?;
                } else {
                    leader.run_full_campaign().await?;
                }
            }
        }

        Commands::Campaign { session, tui, web } => {
            let sid = session.and_then(|s| uuid::Uuid::parse_str(&s).ok());

            if web {
                println!("Launching web-based korg dashboard with live campaign...");
                crate::web::run_web_with_campaign(
                    "Implement production-grade semantic evaluation guardrail with 5 adversarial rubrics".to_string(),
                    sid,
                ).await?;
            } else if tui || !cli.headless {
                println!("Launching Ratatui dashboard with live campaign...");
                crate::tui::run_tui_with_campaign(
                    "Implement production-grade semantic evaluation guardrail with 5 adversarial rubrics".to_string(),
                    sid,
                ).await?;
            } else {
                let mut leader = LeaderOrchestrator::new(
                    "Implement production-grade semantic evaluation guardrail with 5 adversarial rubrics".to_string(),
                    sid,
                );

                let cyan = "\x1b[38;2;0;240;255m";
                let bold = "\x1b[1m";
                let reset = "\x1b[0m";
                println!("\n{bold}{cyan}=== ⚡ OBSERVABLE HEAVY-TIER CAMPAIGN (end-to-end telemetry → Evaluator loop) ==={reset}\n");
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
            crate::tui::run_tui_with_campaign(
                "Korg TUI Live Campaign".to_string(),
                None,
            ).await?;
        }

        Commands::Reconcile { topic } => {
            crate::skills::run_reconcile(topic).await?;
        }

        Commands::Synthesize => {
            crate::skills::run_synthesize().await?;
        }

        Commands::VerifyProvenance { path } => {
            crate::provenance::verify_cli_command(&path)?;
        }
    }

    Ok(())
}
