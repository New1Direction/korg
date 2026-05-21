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
pub mod code_indexer;

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

        /// Cognition Mode (instant, balanced, heavy, research, recovery, autonomous)
        #[arg(long, default_value = "balanced")]
        mode: String,

        /// Enable autonomous goal execution mode (bypasses all manual approvals)
        #[arg(long)]
        goal: bool,
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

fn parse_cognition_mode(mode_str: &str) -> crate::leader::CognitionMode {
    match mode_str.to_lowercase().as_str() {
        "instant" => crate::leader::CognitionMode::Instant,
        "heavy" => crate::leader::CognitionMode::Heavy,
        "research" => crate::leader::CognitionMode::Research,
        "recovery" => crate::leader::CognitionMode::Recovery,
        "autonomous" => crate::leader::CognitionMode::Autonomous,
        _ => crate::leader::CognitionMode::Balanced,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(prompt) = cli.prompt {
        if cli.web {
            println!("Launching web dashboard with live campaign for prompt: {}", prompt);
            crate::web::run_web_with_campaign(prompt, None, Some(parse_cognition_mode(&cli.mode))).await?;
        } else if cli.headless || cli.goal {
            let cyan = "\x1b[38;2;0;240;255m";
            let pink = "\x1b[38;2;255;0;180m";
            let slate = "\x1b[38;2;120;125;140m";
            let bold = "\x1b[1m";
            let reset = "\x1b[0m";

            println!("\n{bold}{cyan}=== ⚡ RUNNING SWARM CAMPAIGN IN HEADLESS MODE ⚡ ==={reset}\n");
            println!("{slate}├──{reset} Prompt: {bold}{pink}{}{reset}", prompt);
            let mut leader = LeaderOrchestrator::new(prompt, None);
            if cli.goal {
                leader.goal_mode = true;
                *leader.cognition_mode.lock().unwrap() = crate::leader::CognitionMode::Autonomous;
            } else {
                *leader.cognition_mode.lock().unwrap() = parse_cognition_mode(&cli.mode);
            }
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
                *leader.cognition_mode.lock().unwrap() = crate::leader::CognitionMode::Autonomous;
            } else {
                *leader.cognition_mode.lock().unwrap() = parse_cognition_mode(&mode);
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
                println!("{slate}├──{reset} Leader Session: {bold}{cyan}{}{reset}", leader.session_id());
                println!("{slate}└──{reset} Base snapshot: {bold}{cyan}{}{reset}", leader.base_snapshot());
                if demo {
                    leader.run_observable_campaign().await?;
                } else {
                    leader.run_full_campaign().await?;
                }
            }
        }

        Commands::Campaign { session, tui, web, mode, goal } => {
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
                    *leader.cognition_mode.lock().unwrap() = crate::leader::CognitionMode::Autonomous;
                } else {
                    *leader.cognition_mode.lock().unwrap() = parse_cognition_mode(&mode);
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

        Commands::Index { path } => {
            let embedding_model: Box<dyn crate::embeddings::EmbeddingModel> = match crate::embeddings::CandleEmbeddingModel::load() {
                Ok(real) => {
                    println!("Loaded real CandleEmbeddingModel (all-MiniLM-L6-v2)");
                    Box::new(real)
                }
                Err(e) => {
                    println!("Using FakeEmbeddingModel (Candle model offline or not enabled: {})", e);
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

    println!("\n{}⚡ {}Korg Interactive Swarm Developer Shell{}", bold, cyan, reset);
    println!("Type {}/help{} to list commands, or type your prompt to chat with the swarm.", bold, reset);
    println!("Use {}@<filename>{} to attach files, or {}@codebase{} for codebase-wide search.", bold, reset, bold, reset);
    println!("{}──────────────────────────────────────────────────────────────────────────────{}", slate, reset);

    let mut context_buffer = String::new();

    // Check if index exists, otherwise asynchronously build it or suggest running /index
    let index_path = ".korg/index.json";
    let mut index = if std::path::Path::new(index_path).exists() {
        match crate::code_indexer::load_index(index_path) {
            Ok(idx) => {
                println!("{}• Loaded codebase semantic index ({} blocks){} ", slate, idx.blocks.len(), reset);
                Some(idx)
            }
            Err(_) => None,
        }
    } else {
        println!("{}• Codebase index not found. Type {}/index{} to build it.{} ", slate, bold, reset, slate);
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
            println!("  {}/read <file>{}              Read a file's contents into the swarm context", bold, reset);
            println!("  {}/edit <file> <instruction>{} Edit a file using Benjamin", bold, reset);
            println!("  {}/run <command>{}            Execute a shell command locally", bold, reset);
            println!("  {}/explain <query>{}          Ask Captain to explain code/architecture", bold, reset);
            println!("  {}/goal <prompt>{}            Run an autonomous multi-persona swarm goal", bold, reset);
            println!("  {}/reconcile{}                Run Yvaeh factual reconciliation", bold, reset);
            println!("  {}/synthesize{}               Run Yvaeh concept synthesis", bold, reset);
            println!("  {}/index{}                    Index the current workspace structurally", bold, reset);
            println!("  {}/exit{}                     Exit the shell\n", bold, reset);
            continue;
        }

        if line.starts_with("/goal ") {
            let goal_prompt = line["/goal ".len()..].trim();
            if goal_prompt.is_empty() {
                println!("Usage: /goal <prompt>");
                continue;
            }
            println!("\n{}⚡ Launching autonomous goal: {}{}", bold, goal_prompt, reset);
            let mut leader = LeaderOrchestrator::new(goal_prompt.to_string(), None);
            leader.goal_mode = true;
            *leader.cognition_mode.lock().unwrap() = crate::leader::CognitionMode::Autonomous;

            println!("{}├──{} Goal Session: {}{}", slate, reset, leader.session_id(), reset);
            println!("{}└──{} Base snapshot: {}{}\n", slate, reset, leader.base_snapshot(), reset);

            match leader.run_full_campaign().await {
                Ok(_) => println!("\n{}✓ Goal campaign completed successfully.{}", green, reset),
                Err(e) => println!("\n{}❌ Goal campaign failed: {}{}", pink, e, reset),
            }
            continue;
        }

        if line.starts_with("/read ") {
            let file_path = line["/read ".len()..].trim();
            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    context_buffer.push_str(&format!("\nFile: {}\n```\n{}\n```\n", file_path, content));
                    println!("{}✓ Read {} into active context.{}", green, file_path, reset);
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
                let output = tokio::process::Command::new(cmd)
                    .args(&args)
                    .output()
                    .await;
                match output {
                    Ok(out) => {
                        println!("{}", String::from_utf8_lossy(&out.stdout));
                        if !out.status.success() {
                            eprintln!("{}Command failed: {}{}", pink, String::from_utf8_lossy(&out.stderr), reset);
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
            let embedding_model: Box<dyn crate::embeddings::EmbeddingModel> = match crate::embeddings::CandleEmbeddingModel::load() {
                Ok(real) => Box::new(real),
                Err(_) => Box::new(crate::embeddings::FakeEmbeddingModel::default()),
            };
            match crate::code_indexer::index_workspace(".", &*embedding_model).await {
                Ok(idx) => {
                    let index_path_str = ".korg/index.json";
                    if let Err(e) = crate::code_indexer::save_index(&idx, index_path_str) {
                        println!("{}❌ Failed to save index: {}{}", pink, e, reset);
                    } else {
                        println!("{}✓ Workspace indexed successfully. Total blocks: {}{}", green, idx.blocks.len(), reset);
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
                println!("{}Calling Benjamin to edit {} with instruction: {}{}", slate, file_path, instruction, reset);
                // Dispatch Benjamin via run_persona
                let result = crate::personas::run_persona(
                    crate::personas::Persona::Benjamin,
                    &format!("Edit file {}: {}", file_path, instruction),
                    "shell-edit",
                ).await;
                
                // Print Benjamin mutations/outcome
                println!("{}✓ Benjamin complete! {} mutations proposed.{}", green, result.mutations.len(), reset);
                for mutation in &result.mutations {
                    let target = mutation.get("target").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let action = mutation.get("action").and_then(|v| v.as_str()).unwrap_or("update");
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
                    prompt_context.push_str(&format!("\nFile Context: {}\n```\n{}\n```\n", file, content));
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
                let embedding_model: Box<dyn crate::embeddings::EmbeddingModel> = match crate::embeddings::CandleEmbeddingModel::load() {
                    Ok(real) => Box::new(real),
                    Err(_) => Box::new(crate::embeddings::FakeEmbeddingModel::default()),
                };
                let clean_query: String = explain_query.split_whitespace()
                    .filter(|w| !w.starts_with('@'))
                    .collect::<Vec<&str>>()
                    .join(" ");

                println!("{}• Scanning codebase semantically for \"{}\"...{}", slate, clean_query, reset);
                let matches = crate::code_indexer::query_codebase(idx, &clean_query, &*embedding_model, 3);
                for (sim, block) in matches {
                    prompt_context.push_str(&format!(
                        "\nSemantic Match [similarity={:.2}]: {} ({}:{}-{})\n```\n{}\n```\n",
                        sim, block.file_path, block.block_name, block.start_line, block.end_line, block.content
                    ));
                    println!("  - Match: {} ({}) [similarity: {:.2}]", block.file_path, block.block_name, sim);
                }
            } else {
                println!("{}⚠️ Codebase index not loaded. Skip codebase search. Use /index to build.{}", gold, reset);
            }
        }

        let final_prompt = format!(
            "{}\nUser Question: {}\n\nPlease provide a clear explanation or response.",
            prompt_context,
            explain_query
        );

        println!("\n{}🧠 Swarm is thinking...{}", gold, reset);
        let result = crate::personas::run_persona(query_persona, &final_prompt, "shell-chat").await;
        
        println!("\n{}🤖 Swarm Output:{}", bold, reset);
        if let Some(text) = result.output.get("explanation").and_then(|v| v.as_str()) {
            println!("{}", text);
        } else if let Some(text) = result.output.get("synthesis").and_then(|v| v.as_str()) {
            println!("{}", text);
        } else {
            println!("{}", serde_json::to_string_pretty(&result.output).unwrap_or_else(|_| "No output".to_string()));
        }
        println!();
    }

    Ok(())
}
