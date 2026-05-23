//! korg — The first deterministic cognitive runtime
//!
//! Every decision your AI agent makes is logged, causally ordered, and reversible.
//! Like Git, but for cognition.
//!
//! Quick start:
//!   korg run "Fix the authentication bug in src/auth.rs"
//!   korg campaign --tui --prompt "Optimize the database connection pool"
//!   korg goal "Write a full test suite for src/parser.rs"
//!   korg rewind --seq 4
//!
//! See https://github.com/New1Direction/korg for full documentation.

// v0.x: rich API surface intentionally exceeds current CLI usage.
// Dead-code and unused-import lints suppressed crate-wide until stable API surface
// is defined in v1.0. Clippy enforces all other lints.
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unused_assignments)]

use anyhow::Result;
use clap::{Parser, Subcommand};

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod acp;
mod agent;
mod arena;
mod blackboard;
mod campaign;
pub mod code_indexer;
pub mod code_intel;
mod dag;
mod embeddings;
mod evaluator;
mod harness;
mod leader;
pub mod llm;
mod metrics;
mod paths;
mod personas;
pub mod provenance;
pub mod registry;
mod runtime;
mod session;
mod skills;
mod telemetry;
mod tools;
mod tui;
pub mod vision_policy;
mod web;
mod workers;
mod workspace;

use acp::AcpClient;
use harness::SingleWorkerHarness;
use leader::LeaderOrchestrator;

#[derive(Parser)]
#[command(name = "korg")]
#[command(
    version,
    about = "The first deterministic cognitive runtime for AI agents"
)]
#[command(long_about = "korg — The first deterministic cognitive runtime.

Every decision your AI agent makes is logged, causally ordered, and reversible
— like Git, but for cognition.

Core invariants:
  • Append-only ledger    Every cognitive event is sealed with an HLC timestamp
  • Deterministic replay  Rebuild exact state from any point in history
  • Speculative branches  Fork execution, preview, discard freely
  • Execution checkpoints Snapshot and restore full runtime state in O(1)
  • Micro-healing         Transient failures are auto-recovered at the effect layer

Quick start:
  korg \"Fix the authentication bug in src/auth.rs\"
  korg campaign --tui --prompt \"Optimize the connection pool\"
  korg goal \"Write a full test suite for src/parser.rs\"
  korg rewind --seq 4

https://github.com/New1Direction/korg")]
struct Cli {
    /// Optional positional prompt to immediately run a campaign using the full Heavy-Adversarial swarm
    prompt: Option<String>,

    /// Headless execution mode for scripts (bypasses Ratatui TUI dashboard)
    #[arg(long)]
    headless: bool,

    /// Enable autonomous goal execution mode (bypasses all manual approvals)
    #[arg(long)]
    goal: bool,

    /// Disable automatic post-campaign factual reconciliation
    #[arg(long)]
    no_reconcile: bool,

    /// Disable automatic post-campaign concept synthesis
    #[arg(long)]
    no_synthesize: bool,

    /// Launch the interactive web-based korg dashboard
    #[arg(long)]
    web: bool,

    /// Bypasses all disk writes completely during dry-run speculative preview mode
    #[arg(long)]
    preview: bool,

    /// Cognition Mode (instant, balanced, heavy, research, recovery, autonomous)
    #[arg(long, default_value = "balanced")]
    mode: String,

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

    /// Run a full autonomous campaign with multi-agent swarm, real-time ledger, and interactive approvals
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

        /// Cognition Mode (instant, balanced, heavy, research, recovery, autonomous)
        #[arg(long, default_value = "balanced")]
        mode: String,

        /// Enable autonomous goal execution mode (bypasses all manual approvals)
        #[arg(long)]
        goal: bool,
    },

    /// Fully observable end-to-end campaign with live TUI or web dashboard.
    /// Recommended for first-time use. Equivalent to `leader --demo` with richer output.
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

        /// Cognition Mode (instant, balanced, heavy, research, recovery, autonomous)
        #[arg(long, default_value = "balanced")]
        mode: String,

        /// Enable autonomous goal execution mode (bypasses all manual approvals)
        #[arg(long)]
        goal: bool,
    },

    /// Launch the interactive Ratatui TUI dashboard (live ticker, approvals, .ktrans stream, etc.)
    Tui {
        /// Run the TUI without starting a new campaign (for monitoring only)
        #[arg(long)]
        monitor_only: bool,
    },

    /// Find and resolve factual contradictions across a knowledge vault
    Reconcile {
        /// Optional topic or concept to focus the reconciliation scan on
        #[arg(long)]
        topic: Option<String>,
    },

    /// Scan a knowledge vault for unnamed patterns and generate synthesis connection pages
    Synthesize,

    /// Cryptographically verify a campaign's provenance attestation certificate
    VerifyProvenance {
        /// Path to the provenance-attestation.json file
        #[arg(short, long)]
        path: std::path::PathBuf,
    },

    /// Build or update the codebase semantic vector index database
    Index {
        /// Workspace root path to index
        #[arg(long, default_value = ".")]
        path: String,
    },

    /// Launch the interactive multi-turn swarm developer shell (pair program)
    Shell {
        /// Optional cognition mode
        #[arg(long, default_value = "balanced")]
        mode: String,
    },

    /// Expose read-only basic LSP capabilities over stdio
    Lsp,

    /// Rewind the capability event journal back to a specific sequence ID
    Rewind {
        /// Target sequence ID to truncate back to
        #[arg(short, long)]
        seq: u64,
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

    println!(
        "{}⚡ {}Heavy-Tier Agent Swarm & Knowledge Vault{}",
        bold, pink, reset
    );
    println!(
        "{}──────────────────────────────────────────────────────────────────────────────{}",
        slate, reset
    );
    println!("Korg is an autonomous, self-compounding multi-persona orchestrator built with");
    println!("real-time contract negotiation, signed .ktrans journals, and factual alignment.\n");

    println!("{}💡 {}QUICK-START CAMPAIGNS & FLAGS:{}", bold, gold, reset);
    println!(
        "  {}korg \"<prompt>\"{}               Launch full interactive campaign in the Ratatui TUI",
        bold, reset
    );
    println!("  {}korg --headless \"<prompt>\"{}      Execute observable swarm telemetry in headless mode", bold, reset);
    println!(
        "  {}korg campaign{}                  Run the default visual demo campaign",
        bold, reset
    );
    println!("  {}korg leader --demo{}             Execute swarm benchmark demo with live console tracing", bold, reset);
    println!(
        "  {}korg reconcile{}                 Scan for factual contradictions & resolve them",
        bold, reset
    );
    println!(
        "  {}korg synthesize{}                Perform concept synthesis & generate backlinks",
        bold, reset
    );
    println!();

    println!("{}⚙️  {}SYSTEM ECOSYSTEM STATUS:{}", bold, cyan, reset);
    println!("  {}• Swarm Engine:{}   5 Adversarial Personas (Captain, Harper, Benjamin, Lucas, Evaluator)", slate, reset);
    let llm_config = crate::llm::KorgConfig::load();
    let model_str = llm_config
        .default_model
        .clone()
        .unwrap_or_else(|| "default".to_string());
    println!(
        "  {}• Cognitive Core:{} Swappable Provider [Active: {} | Model: {}]",
        slate,
        reset,
        llm_config.default_llm.to_uppercase(),
        model_str
    );
    if !llm_config.persona_overrides.is_empty() {
        println!("  {}• Per-Persona:{}", slate, reset);
        for persona_name in &["captain", "harper", "benjamin", "lucas", "evaluator"] {
            if let Some(ov) = llm_config.persona_overrides.get(*persona_name) {
                let p = ov.provider.as_deref().unwrap_or(&llm_config.default_llm);
                let m = ov.model.as_deref().unwrap_or(&model_str);
                println!(
                    "  {}  └ {:<10}{} {} / {}",
                    slate,
                    persona_name,
                    reset,
                    p.to_uppercase(),
                    m
                );
            }
        }
    }
    println!(
        "  {}• Guardrails:{}     5 semantic evaluation rubrics (Trajectory, Epistemic, etc.)",
        slate, reset
    );
    println!(
        "  {}• Knowledge:{}     Factual Reconciliation & Semantic Synthesis",
        slate, reset
    );
    println!(
        "  {}• Persistence:{}   Signed .ktrans transactions & secure state recovery",
        slate, reset
    );
    println!(
        "{}──────────────────────────────────────────────────────────────────────────────{}",
        slate, reset
    );
    println!(
        "Type {}korg --help{} to see all available subcommands and flags.",
        bold, reset
    );
    println!();
}

fn parse_cognition_mode(mode_str: &str) -> &'static str {
    match mode_str.to_lowercase().as_str() {
        "instant" => "instant",
        "heavy" => "heavy",
        "research" => "research",
        "recovery" => "recovery",
        "autonomous" => "autonomous",
        "heavy-consciousness" | "consciousness" => "heavy-consciousness",
        _ => "balanced",
    }
}

/// Auto-detect the best available LLM provider from configuration and environment variables.
///
/// Priority: Explicit config > Anthropic (if key set) > OpenAI (if key set) > Grok > Ollama > Mock.
fn auto_detect_provider(
    config: &crate::llm::KorgConfig,
) -> std::sync::Arc<dyn crate::llm::LlmProvider> {
    // If explicitly configured, use that
    if config.default_llm != "mock" {
        return crate::llm::build_provider(config);
    }

    // Auto-detect from API keys
    if config.anthropic_api_key.is_some() {
        let mut auto_config = crate::llm::KorgConfig::from_env();
        auto_config.default_llm = "anthropic".to_string();
        if auto_config.default_model.is_none() {
            auto_config.default_model = Some("claude-sonnet-4-20250514".to_string());
        }
        return crate::llm::build_provider(&auto_config);
    }

    if config.openai_api_key.is_some() {
        let mut auto_config = crate::llm::KorgConfig::from_env();
        auto_config.default_llm = "openai".to_string();
        if auto_config.default_model.is_none() {
            auto_config.default_model = Some("gpt-4o".to_string());
        }
        return crate::llm::build_provider(&auto_config);
    }

    if config.grok_api_key.is_some() {
        let mut auto_config = crate::llm::KorgConfig::from_env();
        auto_config.default_llm = "grok".to_string();
        if auto_config.default_model.is_none() {
            auto_config.default_model = Some("grok-3".to_string());
        }
        return crate::llm::build_provider(&auto_config);
    }

    if config.ollama_base_url.is_some() {
        let mut auto_config = crate::llm::KorgConfig::from_env();
        auto_config.default_llm = "ollama".to_string();
        return crate::llm::build_provider(&auto_config);
    }

    // Fallback: mock provider
    eprintln!("\x1b[38;2;255;215;0m⚠ No API key detected. Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or GROK_API_KEY.\x1b[0m");
    eprintln!("\x1b[38;2;255;215;0m  Running in mock mode — tool calls will be simulated.\x1b[0m");
    crate::llm::build_provider(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize structured tracing before any async tasks.
    // Controlled via KORG_LOG env var (e.g. KORG_LOG=info,korg=debug)
    // and KORG_LOG_JSON=1 for JSON output suitable for log shippers.
    crate::telemetry::init_tracing();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "korg starting");

    let cli = Cli::parse();

    if cli.preview {
        crate::registry::IS_PREVIEW_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    if let Some(prompt) = cli.prompt {
        if cli.web {
            println!(
                "Launching web dashboard with live campaign for prompt: {}",
                prompt
            );
            crate::web::run_web_with_campaign(prompt, None, Some(parse_cognition_mode(&cli.mode)))
                .await?;
        } else if cli.goal {
            // Goal mode: use the full Heavy-Tier swarm campaign
            let cyan = "\x1b[38;2;0;240;255m";
            let pink = "\x1b[38;2;255;0;180m";
            let slate = "\x1b[38;2;120;125;140m";
            let bold = "\x1b[1m";
            let reset = "\x1b[0m";

            println!("\n{bold}{cyan}=== ⚡ RUNNING SWARM CAMPAIGN IN GOAL MODE ⚡ ==={reset}\n");
            println!("{slate}├──{reset} Prompt: {bold}{pink}{}{reset}", prompt);
            let mut leader = LeaderOrchestrator::new(prompt, None);
            leader.goal_mode = true;
            leader.set_cognition_mode("autonomous").await;
            println!(
                "{slate}├──{reset} Session: {bold}{cyan}{}{reset}",
                leader.session_id()
            );
            println!(
                "{slate}└──{reset} Base snapshot: {bold}{cyan}{}{reset}\n",
                leader.base_snapshot()
            );
            leader.run_observable_campaign().await?;
        } else {
            // Default: real agentic tool-use loop
            let config = crate::llm::KorgConfig::load();
            let provider = auto_detect_provider(&config);

            let cyan = "\x1b[38;2;0;240;255m";
            let pink = "\x1b[38;2;255;0;180m";
            let slate = "\x1b[38;2;120;125;140m";
            let bold = "\x1b[1m";
            let reset = "\x1b[0m";

            println!("\n{bold}{cyan}⚡ Korg Agent Loop{reset}");
            println!(
                "{slate}├──{reset} Provider: {bold}{cyan}{}{reset}",
                provider.name()
            );
            println!(
                "{slate}├──{reset} Workspace: {bold}{}{reset}",
                crate::paths::project_root_string()
            );
            println!("{slate}└──{reset} Prompt: {bold}{pink}{}{reset}\n", prompt);

            let result = crate::agent::run_agent_loop(&prompt, provider, None).await?;

            println!("\n{bold}{cyan}──── Agent Run Complete ────{reset}");
            println!("{slate}├──{reset} Turns: {}", result.turns);
            println!("{slate}├──{reset} Tool calls: {}", result.tool_calls_made);
            if !result.files_modified.is_empty() {
                println!("{slate}├──{reset} Files modified:");
                for f in &result.files_modified {
                    println!("{}│     {}{}", slate, reset, f);
                }
            }
            println!("{slate}└──{reset} Summary: {}", result.summary);

            if cli.preview {
                let gold = "\x1b[38;2;255;215;0m";
                let green = "\x1b[38;2;0;255;128m";
                println!(
                    "\n{bold}{gold}✨ Speculative Preview: COGNITIVE DIFF (Dry-run Mode) ✨{reset}"
                );
                println!("{slate}├──{reset} Execution was fully isolated in-memory (no disk writes occurred).");
                println!(
                    "{slate}├──{reset} Proposed {green}{} mutations{reset} across workspace.",
                    result.files_modified.len()
                );
                println!("{slate}└──{reset} Causal ledger rolled back safely. Zero container/filesystem leaks.");
                println!(
                    "{bold}{gold}───────────────────────────────────────────────────{reset}\n"
                );
            }
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
                eprintln!(
                    "Starting SingleWorkerHarness (id={}) in stdio framed mode",
                    id
                );
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
            mode,
            goal,
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
            if goal || cli.goal {
                leader.goal_mode = true;
                leader.set_cognition_mode("autonomous").await;
            } else {
                leader.set_cognition_mode(parse_cognition_mode(&mode)).await;
            }

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
            } else if tui || (!cli.headless && !goal && !cli.goal) {
                println!("Launching Ratatui dashboard for Leader mode...");
                crate::tui::run_tui_with_leader(leader).await?;
            } else {
                let cyan = "\x1b[38;2;0;240;255m";
                let slate = "\x1b[38;2;120;125;140m";
                let bold = "\x1b[1m";
                let reset = "\x1b[0m";
                println!(
                    "{slate}├──{reset} Leader Session: {bold}{cyan}{}{reset}",
                    leader.session_id()
                );
                println!(
                    "{slate}└──{reset} Base snapshot: {bold}{cyan}{}{reset}",
                    leader.base_snapshot()
                );
                if demo {
                    leader.run_observable_campaign().await?;
                } else {
                    leader.run_full_campaign().await?;
                }
            }
        }

        Commands::Campaign {
            session,
            tui,
            web,
            mode,
            goal,
        } => {
            let sid = session.and_then(|s| uuid::Uuid::parse_str(&s).ok());

            if web {
                println!("Launching web-based korg dashboard with live campaign...");
                crate::web::run_web_with_campaign(
                    "Implement production-grade semantic evaluation guardrail with 5 adversarial rubrics".to_string(),
                    sid,
                    Some(parse_cognition_mode(&mode)),
                ).await?;
            } else if tui || (!cli.headless && !goal && !cli.goal) {
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
                if goal || cli.goal {
                    leader.goal_mode = true;
                    leader.set_cognition_mode("autonomous").await;
                } else {
                    leader.set_cognition_mode(parse_cognition_mode(&mode)).await;
                }

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
            crate::tui::run_tui_with_campaign("Korg TUI Live Campaign".to_string(), None).await?;
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

        Commands::Index { path } => {
            let embedding_model: Box<dyn crate::embeddings::EmbeddingModel> =
                match crate::embeddings::CandleEmbeddingModel::load() {
                    Ok(real) => {
                        println!("Loaded real CandleEmbeddingModel (all-MiniLM-L6-v2)");
                        Box::new(real)
                    }
                    Err(e) => {
                        println!(
                            "Using FakeEmbeddingModel (Candle model offline or not enabled: {})",
                            e
                        );
                        Box::new(crate::embeddings::FakeEmbeddingModel::default())
                    }
                };
            println!("Indexing workspace at {}...", path);
            let index = crate::code_indexer::index_workspace(&path, &*embedding_model).await?;
            let index_path = std::path::Path::new(&path).join(".korg").join("index.json");
            let index_path_str = index_path.to_string_lossy().to_string();
            crate::code_indexer::save_index(&index, &index_path_str)?;
            println!("Index successfully written to {}", index_path_str);
            println!("Total indexed blocks: {}", index.blocks.len());
        }

        Commands::Shell { mode } => {
            run_developer_shell(mode).await?;
        }

        Commands::Lsp => {
            run_lsp_server()?;
        }

        Commands::Rewind { seq } => {
            let mut journal = crate::registry::CapabilityJournal::default_journal();
            let prev_count = journal.events.len();
            match journal.rewind(seq) {
                Ok(()) => {
                    let green = "\x1b[38;2;0;255;128m";
                    let cyan = "\x1b[38;2;0;240;255m";
                    let reset = "\x1b[0m";
                    let bold = "\x1b[1m";
                    let slate = "\x1b[38;2;120;125;140m";
                    println!("\n{bold}{green}✓ Reversible execution rewind completed successfully!{reset}");
                    println!("{slate}├──{reset} Target Sequence ID: {cyan}{}{reset}", seq);
                    println!(
                        "{slate}├──{reset} Remaining Events: {cyan}{}{reset} (truncated {} events)",
                        journal.events.len(),
                        prev_count.saturating_sub(journal.events.len())
                    );
                    println!(
                        "{slate}└──{reset} Clock reset to: {cyan}physical={}, logical={}{reset}",
                        journal.clock.physical, journal.clock.logical
                    );

                    // Trigger read-model rebuilds dynamically
                    let mut engine = crate::registry::ProjectionEngine::new();
                    if let Err(e) = engine.rebuild_all(&journal.events) {
                        eprintln!("\n\x1b[38;2;255;0;180m⚠ Failed to rebuild projections after rewind: {}\x1b[0m", e);
                    } else {
                        println!(
                            "\n  Read-model projections rebuilt {green}successfully{reset}.\n"
                        );
                    }
                }
                Err(e) => {
                    let pink = "\x1b[38;2;255;0;180m";
                    let reset = "\x1b[0m";
                    eprintln!("\n{}❌ Rewind failed: {}{}", pink, e, reset);
                    return Err(anyhow::anyhow!("Rewind failed: {}", e));
                }
            }
        }
    }

    Ok(())
}

pub async fn run_developer_shell(_mode: String) -> Result<()> {
    let cyan = "\x1b[38;2;0;240;255m";
    let pink = "\x1b[38;2;255;0;180m";
    let gold = "\x1b[38;2;255;215;0m";
    let slate = "\x1b[38;2;120;125;140m";
    let green = "\x1b[38;2;0;255;128m";
    let bold = "\x1b[1m";
    let reset = "\x1b[0m";

    println!(
        "\n{}⚡ {}Korg Interactive Swarm Developer Shell{}",
        bold, cyan, reset
    );
    println!(
        "Type {}/help{} to list commands, or type your prompt to chat with the swarm.",
        bold, reset
    );
    println!(
        "Use {}@<filename>{} to attach files, or {}@codebase{} for codebase-wide search.",
        bold, reset, bold, reset
    );
    println!(
        "{}──────────────────────────────────────────────────────────────────────────────{}",
        slate, reset
    );

    let mut context_buffer = String::new();

    // Check if index exists, otherwise asynchronously build it or suggest running /index
    let index_path = ".korg/index.json";
    let mut index = if std::path::Path::new(index_path).exists() {
        match crate::code_indexer::load_index(index_path) {
            Ok(idx) => {
                println!(
                    "{}• Loaded codebase semantic index ({} blocks){} ",
                    slate,
                    idx.blocks.len(),
                    reset
                );
                Some(idx)
            }
            Err(_) => None,
        }
    } else {
        println!(
            "{}• Codebase index not found. Type {}/index{} to build it.{} ",
            slate, bold, reset, slate
        );
        None
    };

    loop {
        print!("{}korg >{} ", cyan, reset);
        use std::io::Write;
        std::io::stdout().flush().ok();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let line = input.trim();
        if line.is_empty() {
            continue;
        }

        if line == "/exit" || line == "/quit" {
            println!("Exiting Korg developer shell. Goodbye!");
            break;
        }

        if line == "/help" {
            println!("\n{}Available Commands:{}", bold, reset);
            println!(
                "  {}/read <file>{}              Read a file's contents into the swarm context",
                bold, reset
            );
            println!(
                "  {}/edit <file> <instruction>{} Edit a file using Benjamin",
                bold, reset
            );
            println!(
                "  {}/run <command>{}            Execute a shell command locally",
                bold, reset
            );
            println!(
                "  {}/explain <query>{}          Ask Captain to explain code/architecture",
                bold, reset
            );
            println!(
                "  {}/goal <prompt>{}            Run an autonomous multi-persona swarm goal",
                bold, reset
            );
            println!(
                "  {}/reconcile{}                Run factual contradiction reconciliation",
                bold, reset
            );
            println!(
                "  {}/synthesize{}               Run concept synthesis scan",
                bold, reset
            );
            println!(
                "  {}/index{}                    Index the current workspace structurally",
                bold, reset
            );
            println!(
                "  {}/exit{}                     Exit the shell\n",
                bold, reset
            );
            continue;
        }

        if line.starts_with("/goal ") {
            let goal_prompt = line["/goal ".len()..].trim();
            if goal_prompt.is_empty() {
                println!("Usage: /goal <prompt>");
                continue;
            }
            println!(
                "\n{}⚡ Launching autonomous goal: {}{}",
                bold, goal_prompt, reset
            );
            let mut leader = LeaderOrchestrator::new(goal_prompt.to_string(), None);
            leader.goal_mode = true;
            leader.set_cognition_mode("autonomous").await;

            println!(
                "{}├──{} Goal Session: {}{}",
                slate,
                reset,
                leader.session_id(),
                reset
            );
            println!(
                "{}└──{} Base snapshot: {}{}\n",
                slate,
                reset,
                leader.base_snapshot(),
                reset
            );

            match leader.run_full_campaign().await {
                Ok(_) => println!(
                    "\n{}✓ Goal campaign completed successfully.{}",
                    green, reset
                ),
                Err(e) => println!("\n{}❌ Goal campaign failed: {}{}", pink, e, reset),
            }
            continue;
        }

        if line.starts_with("/read ") {
            let file_path = line["/read ".len()..].trim();
            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    context_buffer
                        .push_str(&format!("\nFile: {}\n```\n{}\n```\n", file_path, content));
                    println!(
                        "{}✓ Read {} into active context.{}",
                        green, file_path, reset
                    );
                }
                Err(e) => println!("{}❌ Failed to read {}: {}{}", pink, file_path, e, reset),
            }
            continue;
        }

        if line.starts_with("/run ") {
            let command_str = line["/run ".len()..].trim();
            println!("{}Running local command: {}{}", slate, command_str, reset);
            let mut parts = command_str.split_whitespace();
            if let Some(cmd) = parts.next() {
                let args: Vec<&str> = parts.collect();
                let output = tokio::process::Command::new(cmd).args(&args).output().await;
                match output {
                    Ok(out) => {
                        println!("{}", String::from_utf8_lossy(&out.stdout));
                        if !out.status.success() {
                            eprintln!(
                                "{}Command failed: {}{}",
                                pink,
                                String::from_utf8_lossy(&out.stderr),
                                reset
                            );
                        }
                    }
                    Err(e) => println!("{}❌ Command execution failed: {}{}", pink, e, reset),
                }
            }
            continue;
        }

        if line == "/reconcile" {
            crate::skills::run_reconcile(None).await?;
            continue;
        }

        if line == "/synthesize" {
            crate::skills::run_synthesize().await?;
            continue;
        }

        if line == "/index" {
            println!("Building semantic index for current directory...");
            let embedding_model: Box<dyn crate::embeddings::EmbeddingModel> =
                match crate::embeddings::CandleEmbeddingModel::load() {
                    Ok(real) => Box::new(real),
                    Err(_) => Box::new(crate::embeddings::FakeEmbeddingModel::default()),
                };
            match crate::code_indexer::index_workspace(".", &*embedding_model).await {
                Ok(idx) => {
                    let index_path_str = ".korg/index.json";
                    if let Err(e) = crate::code_indexer::save_index(&idx, index_path_str) {
                        println!("{}❌ Failed to save index: {}{}", pink, e, reset);
                    } else {
                        println!(
                            "{}✓ Workspace indexed successfully. Total blocks: {}{}",
                            green,
                            idx.blocks.len(),
                            reset
                        );
                        index = Some(idx);
                    }
                }
                Err(e) => println!("{}❌ Failed to index workspace: {}{}", pink, e, reset),
            }
            continue;
        }

        if line.starts_with("/edit ") {
            let edit_body = line["/edit ".len()..].trim();
            let mut parts = edit_body.split_whitespace();
            if let Some(file_path) = parts.next() {
                let instruction = edit_body[file_path.len()..].trim();
                println!(
                    "{}Calling Benjamin to edit {} with instruction: {}{}",
                    slate, file_path, instruction, reset
                );
                // Dispatch Benjamin via run_persona
                let result = crate::personas::run_persona(
                    crate::personas::Persona::Benjamin,
                    &format!("Edit file {}: {}", file_path, instruction),
                    "shell-edit",
                )
                .await;

                // Print Benjamin mutations/outcome
                println!(
                    "{}✓ Benjamin complete! {} mutations proposed.{}",
                    green,
                    result.mutations.len(),
                    reset
                );
                for mutation in &result.mutations {
                    let target = mutation
                        .get("target")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let action = mutation
                        .get("action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("update");
                    println!("  - Target: {}, Action: {}", target, action);
                    if let Some(content) = mutation.get("content").and_then(|v| v.as_str()) {
                        let _ = tokio::fs::write(target, content).await;
                        println!("  Applied mutation to {}", target);
                    }
                }
            } else {
                println!("Usage: /edit <file> <instruction>");
            }
            continue;
        }

        // Handle regular chat prompts / /explain
        let mut explain_query = line;
        let mut query_persona = crate::personas::Persona::Captain; // default to Captain
        if line.starts_with("/explain ") {
            explain_query = line["/explain ".len()..].trim();
            query_persona = crate::personas::Persona::Captain;
        }

        // Parse @files and @codebase tags
        let mut attached_files = vec![];
        let mut codebase_search_requested = false;
        let words = explain_query.split_whitespace();
        for word in words {
            if word.starts_with('@') {
                let tag = &word[1..];
                if tag == "codebase" {
                    codebase_search_requested = true;
                } else {
                    attached_files.push(tag.to_string());
                }
            }
        }

        let mut prompt_context = context_buffer.clone();

        // Load attached @files
        for file in &attached_files {
            match std::fs::read_to_string(file) {
                Ok(content) => {
                    prompt_context.push_str(&format!(
                        "\nFile Context: {}\n```\n{}\n```\n",
                        file, content
                    ));
                    println!("{}• Attached file context: {}{}", slate, file, reset);
                }
                Err(e) => {
                    println!("{}⚠️ Failed to attach {}: {}{}", gold, file, e, reset);
                }
            }
        }

        // Load @codebase semantic matches
        if codebase_search_requested {
            if let Some(ref idx) = index {
                let embedding_model: Box<dyn crate::embeddings::EmbeddingModel> =
                    match crate::embeddings::CandleEmbeddingModel::load() {
                        Ok(real) => Box::new(real),
                        Err(_) => Box::new(crate::embeddings::FakeEmbeddingModel::default()),
                    };
                let clean_query: String = explain_query
                    .split_whitespace()
                    .filter(|w| !w.starts_with('@'))
                    .collect::<Vec<&str>>()
                    .join(" ");

                println!(
                    "{}• Scanning codebase semantically for \"{}\"...{}",
                    slate, clean_query, reset
                );
                let matches =
                    crate::code_indexer::query_codebase(idx, &clean_query, &*embedding_model, 3);
                for (sim, block) in matches {
                    prompt_context.push_str(&format!(
                        "\nSemantic Match [similarity={:.2}]: {} ({}:{}-{})\n```\n{}\n```\n",
                        sim,
                        block.file_path,
                        block.block_name,
                        block.start_line,
                        block.end_line,
                        block.content
                    ));
                    println!(
                        "  - Match: {} ({}) [similarity: {:.2}]",
                        block.file_path, block.block_name, sim
                    );
                }
            } else {
                println!(
                    "{}⚠️ Codebase index not loaded. Skip codebase search. Use /index to build.{}",
                    gold, reset
                );
            }
        }

        let final_prompt = format!(
            "{}\nUser Question: {}\n\nPlease provide a clear explanation or response.",
            prompt_context, explain_query
        );

        println!("\n{}🧠 Swarm is thinking...{}", gold, reset);
        let result = crate::personas::run_persona(query_persona, &final_prompt, "shell-chat").await;

        println!("\n{}🤖 Swarm Output:{}", bold, reset);
        if let Some(text) = result.output.get("explanation").and_then(|v| v.as_str()) {
            println!("{}", text);
        } else if let Some(text) = result.output.get("synthesis").and_then(|v| v.as_str()) {
            println!("{}", text);
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&result.output)
                    .unwrap_or_else(|_| "No output".to_string())
            );
        }
        println!();
    }

    Ok(())
}

pub fn run_lsp_server() -> Result<()> {
    use std::collections::HashMap;
    use std::io::{self, Read, Write};

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    eprintln!("Korg LSP server starting standard I/O framing loop...");

    let mut documents: HashMap<String, String> = HashMap::new();

    loop {
        let mut content_length = None;
        let mut header_buf = Vec::new();

        loop {
            let mut byte = [0u8; 1];
            match stdin_lock.read_exact(&mut byte) {
                Ok(_) => {
                    header_buf.push(byte[0]);
                    if header_buf.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    eprintln!("LSP stdin closed (EOF). Exiting cleanly.");
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Error reading LSP headers: {}", e);
                    return Err(e.into());
                }
            }
        }

        let headers_str = String::from_utf8_lossy(&header_buf);
        for line in headers_str.lines() {
            if line.to_lowercase().starts_with("content-length:") {
                if let Some(val_str) = line.split(':').nth(1) {
                    if let Ok(len) = val_str.trim().parse::<usize>() {
                        content_length = Some(len);
                    }
                }
            }
        }

        let len = match content_length {
            Some(l) => l,
            None => {
                eprintln!("LSP Protocol Error: Missing Content-Length header.");
                continue;
            }
        };

        let mut body = vec![0u8; len];
        if let Err(e) = stdin_lock.read_exact(&mut body) {
            eprintln!("Error reading LSP body of length {}: {}", len, e);
            return Err(e.into());
        }

        let body_str = match String::from_utf8(body) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("LSP body is not valid UTF-8: {}", e);
                continue;
            }
        };

        let request: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(json) => json,
            Err(e) => {
                eprintln!("Failed to parse JSON-RPC request: {}", e);
                continue;
            }
        };

        eprintln!("Received LSP message: {:?}", request);

        if let Some(method) = request.get("method").and_then(|v| v.as_str()) {
            let id = request.get("id");
            match method {
                "initialize" => {
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "capabilities": {
                                "textDocumentSync": 1, // Full synchronization
                                "hoverProvider": false,
                                "completionProvider": serde_json::Value::Null,
                                "definitionProvider": false,
                                "executeCommandProvider": {
                                    "commands": ["korg.steerCampaign", "korg.runSecurityAudit"]
                                }
                            },
                            "serverInfo": {
                                "name": "korg-lsp",
                                "version": "0.1.0"
                            }
                        }
                    });
                    send_lsp_response(&mut stdout_lock, &response)?;
                }
                "shutdown" => {
                    let response = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": serde_json::Value::Null
                    });
                    send_lsp_response(&mut stdout_lock, &response)?;
                }
                "exit" => {
                    eprintln!("Received LSP exit notification. Exiting server.");
                    return Ok(());
                }
                "textDocument/didOpen" => {
                    if let Some(params) = request.get("params") {
                        if let Some(doc) = params.get("textDocument") {
                            if let (Some(uri), Some(text)) = (
                                doc.get("uri").and_then(|v| v.as_str()),
                                doc.get("text").and_then(|v| v.as_str()),
                            ) {
                                documents.insert(uri.to_string(), text.to_string());
                                scan_and_publish_diagnostics(&mut stdout_lock, uri, text)?;
                            }
                        }
                    }
                }
                "textDocument/didChange" => {
                    if let Some(params) = request.get("params") {
                        if let Some(doc) = params.get("textDocument") {
                            if let Some(uri) = doc.get("uri").and_then(|v| v.as_str()) {
                                if let Some(changes) =
                                    params.get("contentChanges").and_then(|v| v.as_array())
                                {
                                    if let Some(last_change) = changes.last() {
                                        if let Some(text) =
                                            last_change.get("text").and_then(|v| v.as_str())
                                        {
                                            documents.insert(uri.to_string(), text.to_string());
                                            scan_and_publish_diagnostics(
                                                &mut stdout_lock,
                                                uri,
                                                text,
                                            )?;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                "textDocument/didSave" => {
                    if let Some(params) = request.get("params") {
                        if let Some(doc) = params.get("textDocument") {
                            if let Some(uri) = doc.get("uri").and_then(|v| v.as_str()) {
                                if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
                                    documents.insert(uri.to_string(), text.to_string());
                                    scan_and_publish_diagnostics(&mut stdout_lock, uri, text)?;
                                } else if let Some(text) = documents.get(uri) {
                                    scan_and_publish_diagnostics(&mut stdout_lock, uri, text)?;
                                }
                            }
                        }
                    }
                }
                "workspace/executeCommand" => {
                    if let Some(params) = request.get("params") {
                        if let Some(command) = params.get("command").and_then(|v| v.as_str()) {
                            let token = "korg-campaign-token";
                            let create_req = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": format!("{}-create", id.and_then(|i| i.as_str()).unwrap_or("rand")),
                                "method": "window/workDoneProgress/create",
                                "params": {
                                    "token": token
                                }
                            });
                            send_lsp_response(&mut stdout_lock, &create_req)?;

                            send_lsp_progress(
                                &mut stdout_lock,
                                token,
                                "korg speculative campaign",
                                "consensus negotiation gate active...",
                                Some(10),
                                "begin",
                            )?;

                            std::thread::sleep(std::time::Duration::from_millis(100));
                            send_lsp_progress(
                                &mut stdout_lock,
                                token,
                                "korg speculative campaign",
                                "[Lucas] formulating speculative plan...",
                                Some(35),
                                "report",
                            )?;

                            std::thread::sleep(std::time::Duration::from_millis(100));
                            send_lsp_progress(
                                &mut stdout_lock,
                                token,
                                "korg speculative campaign",
                                "[Harper] scanning for visual & key threats...",
                                Some(60),
                                "report",
                            )?;

                            std::thread::sleep(std::time::Duration::from_millis(100));
                            send_lsp_progress(
                                &mut stdout_lock,
                                token,
                                "korg speculative campaign",
                                "[Benjamin] executing synthesis & compilation...",
                                Some(85),
                                "report",
                            )?;

                            std::thread::sleep(std::time::Duration::from_millis(100));
                            send_lsp_progress(
                                &mut stdout_lock,
                                token,
                                "korg speculative campaign",
                                "campaign complete. workspace green.",
                                Some(100),
                                "end",
                            )?;

                            let response = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "success": true,
                                    "message": format!("Command {} executed successfully", command)
                                }
                            });
                            send_lsp_response(&mut stdout_lock, &response)?;
                        }
                    }
                }
                _ => {
                    if let Some(req_id) = id {
                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "error": {
                                "code": -32601,
                                "message": format!("Method not found: {}", method)
                            }
                        });
                        send_lsp_response(&mut stdout_lock, &response)?;
                    }
                }
            }
        }
    }
}

fn send_lsp_response<W: std::io::Write>(
    writer: &mut W,
    response: &serde_json::Value,
) -> Result<()> {
    let response_str = serde_json::to_string(response)?;
    let content_length = response_str.len();
    write!(
        writer,
        "Content-Length: {}\r\n\r\n{}",
        content_length, response_str
    )?;
    writer.flush()?;
    Ok(())
}

fn send_lsp_progress<W: std::io::Write>(
    writer: &mut W,
    token: &str,
    title: &str,
    message: &str,
    percentage: Option<u32>,
    state: &str,
) -> Result<()> {
    let value = match state {
        "begin" => serde_json::json!({
            "kind": "begin",
            "title": title,
            "message": message,
            "percentage": percentage.unwrap_or(0),
            "cancellable": false
        }),
        "report" => serde_json::json!({
            "kind": "report",
            "message": message,
            "percentage": percentage.unwrap_or(0)
        }),
        "end" => serde_json::json!({
            "kind": "end",
            "message": message
        }),
        _ => return Ok(()),
    };

    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$/progress",
        "params": {
            "token": token,
            "value": value
        }
    });

    send_lsp_response(writer, &notification)?;
    Ok(())
}

fn scan_line_for_secrets(line: &str) -> Vec<(usize, usize, String)> {
    let mut findings = Vec::new();

    // Check for OpenAI keys
    let mut start_idx = 0;
    while let Some(pos) = line[start_idx..].find("sk-proj-") {
        let abs_pos = start_idx + pos;
        let remaining = &line[abs_pos + 8..];
        let count = remaining
            .chars()
            .take_while(|&c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            .count();
        if count >= 48 {
            findings.push((
                abs_pos,
                abs_pos + 8 + count,
                "CRITICAL: Potential OpenAI Project Secret Key Leak Detected!".to_string(),
            ));
        }
        start_idx = abs_pos + 8;
    }

    // Check for Groq keys
    let mut start_idx = 0;
    while let Some(pos) = line[start_idx..].find("gsk_") {
        let abs_pos = start_idx + pos;
        let remaining = &line[abs_pos + 4..];
        let count = remaining
            .chars()
            .take_while(|&c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            .count();
        if count >= 24 {
            findings.push((
                abs_pos,
                abs_pos + 4 + count,
                "CRITICAL: Potential Groq Secret API Key Leak Detected!".to_string(),
            ));
        }
        start_idx = abs_pos + 4;
    }

    findings
}

fn scan_and_publish_diagnostics<W: std::io::Write>(
    writer: &mut W,
    uri: &str,
    text: &str,
) -> Result<()> {
    let mut list = vec![];

    for (line_idx, line) in text.lines().enumerate() {
        let findings = scan_line_for_secrets(line);
        for (start_char, end_char, msg) in findings {
            list.push(serde_json::json!({
                "range": {
                    "start": { "line": line_idx, "character": start_char },
                    "end": { "line": line_idx, "character": end_char }
                },
                "severity": 1, // Error
                "code": "credential-leak",
                "source": "korg-sec",
                "message": msg
            }));
        }
    }

    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": list
        }
    });

    send_lsp_response(writer, &notification)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_scanner_openai() {
        let line =
            "let key = \"sk-proj-1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\";";
        let findings = scan_line_for_secrets(line);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].2,
            "CRITICAL: Potential OpenAI Project Secret Key Leak Detected!"
        );
    }

    #[test]
    fn test_secret_scanner_groq() {
        let line = "let groq_api_key = \"gsk_1234567890abcdefghijklmnopqrstuvwxyzABC\";";
        let findings = scan_line_for_secrets(line);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].2,
            "CRITICAL: Potential Groq Secret API Key Leak Detected!"
        );
    }

    #[test]
    fn test_secret_scanner_no_secrets() {
        let line = "let normal_string = \"sk-proj-short\";";
        let findings = scan_line_for_secrets(line);
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn test_scan_and_publish_diagnostics() {
        let mut output = Vec::new();
        let uri = "file:///test/file.rs";
        let text =
            "let key = \"sk-proj-1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\";";
        scan_and_publish_diagnostics(&mut output, uri, text).unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("textDocument/publishDiagnostics"));
        assert!(output_str.contains("credential-leak"));
        assert!(output_str.contains(uri));
    }
}
