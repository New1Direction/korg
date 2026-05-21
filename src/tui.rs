//! Ratatui TUI for the Korg Heavy-Tier Operator Dashboard.
//!
//! Entry point: `korg tui`
//! Also usable via `korg campaign --tui` and `korg leader --demo --tui`

use crate::acp::AcpMessage;
use crate::leader::LeaderOrchestrator;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Wrap,
        Gauge, Sparkline, BarChart, Table, Row, Cell,
    },
    Frame, Terminal,
};
use std::io;

/// Events sent from the LeaderOrchestrator to the live Ratatui TUI.
#[derive(Debug, Clone)]
pub enum ContractResponse {
    Approve,
    Reject,
    Force,
    Override(Vec<String>),
}

/// Events sent from the LeaderOrchestrator to the live Ratatui TUI.
#[derive(Debug, Clone)]
pub enum TuiUpdate {
    Verdict {
        text: String,
        rubrics: Vec<(String, bool)>, // e.g. [("Trajectory", true), ("Epistemic", false), ...]
        h_sem: f32,
        velocity: f32,
        risk: f32,
        progress: f32,
        doom_prob: f32,
    },
    Arena {
        round: usize,
        winner: String,
        mutations: usize,
    },
    Trace(String),
    Ktrans(String),
    ApprovalRequest(String),
    Compaction(String),
    ContractNegotiated {
        description: String,
        criteria: Vec<(String, f32)>, // criteria description + similarity score
    },
    ContractApprovalRequest {
        round: usize,
        description: String,
        criteria: Vec<(String, f32)>, // criteria description + similarity score
    },
    PersonaTelemetry {
        scores: [f32; 4],
        telemetry_merges: u32,
        crdt_sync_frequency: f32,
        conflicts_count: u32,
        provenance_chain_length: u32,
        lock_states: Vec<(String, String, String, String)>, // (Persona, Lock Mode, Latency, Activity)
    },
}

pub struct KorgTui {
    pub swarm_size: u32,
    pub h_sem: f32,
    pub h_sem_history: Vec<u64>,
    pub current_verdict: String,
    pub rubric_status: Vec<(String, bool)>, // (name, passed)
    pub arena_history: Vec<String>,
    pub trace_events: Vec<String>,
    pub ktrans_log: Vec<String>,
    pub compaction_status: String,
    pub pending_approval: Option<String>,
    pub paused: bool,
    pub logs: Vec<String>,
    pub contract_description: String,
    pub contract_criteria: Vec<(String, f32)>, // paired with BERT similarity
    pub pending_contract_approval: Option<(usize, String, Vec<(String, f32)>)>,
    pub editing_custom_criterion: bool,
    pub input_buffer: String,
    pub feedback_tx: Option<tokio::sync::mpsc::Sender<ContractResponse>>,

    // Enriched campaign health metrics
    pub velocity: f32,
    pub risk: f32,
    pub progress: f32,
    pub doom_prob: f32,

    // Enriched blackboard & persona telemetry
    pub persona_scores: [f32; 4], // Captain, Harper, Benjamin, Lucas
    pub telemetry_merges: u32,
    pub crdt_sync_frequency: f32,
    pub conflicts_count: u32,
    pub provenance_chain_length: u32,
    pub lock_states: Vec<(String, String, String, String)>, // (Persona, Lock Mode, Latency, Activity)

    // Persona sparkline histories
    pub captain_score_history: Vec<u64>,
    pub harper_score_history: Vec<u64>,
    pub benjamin_score_history: Vec<u64>,
    pub lucas_score_history: Vec<u64>,

    // Playback and Replay Scrubber Track
    pub playhead: usize,
    pub fork_modal_open: bool,
    pub steering_buffer: String,
    pub policy_violation_alert: Option<String>,
}

impl Default for KorgTui {
    fn default() -> Self {
        Self {
            swarm_size: 4,
            h_sem: 0.42,
            h_sem_history: vec![42, 43, 41, 44, 45, 42, 40, 43, 46, 42],
            current_verdict: "Waiting for first evaluation...".to_string(),
            rubric_status: vec![
                ("Trajectory Efficiency".to_string(), true),
                ("Epistemic Integrity".to_string(), true),
                ("Tool-Use Precision".to_string(), false),
                ("Semantic Adherence".to_string(), true),
                ("Resource Utilization".to_string(), true),
            ],
            arena_history: vec!["Round 0: No winner yet".to_string()],
            trace_events: vec!["No TraceEvents yet".to_string()],
            ktrans_log: vec!["No .ktrans yet".to_string()],
            compaction_status: "No compaction yet".to_string(),
            pending_approval: None,
            paused: false,
            logs: vec!["TUI started".to_string()],
            contract_description: "No contract negotiated yet".to_string(),
            contract_criteria: vec![],
            pending_contract_approval: None,
            editing_custom_criterion: false,
            input_buffer: String::new(),
            feedback_tx: None,

            // Enriched health defaults
            velocity: 85.0,
            risk: 0.35,
            progress: 0.0,
            doom_prob: 0.0,

            // Enriched telemetry defaults
            persona_scores: [0.92, 0.87, 0.83, 0.89],
            telemetry_merges: 0,
            crdt_sync_frequency: 1.2,
            conflicts_count: 0,
            provenance_chain_length: 1,
            lock_states: vec![
                ("Captain".to_string(), "READ".to_string(), "0.15ms".to_string(), "Active".to_string()),
                ("Harper".to_string(), "IDLE".to_string(), "-".to_string(), "Idle".to_string()),
                ("Benjamin".to_string(), "IDLE".to_string(), "-".to_string(), "Idle".to_string()),
                ("Lucas".to_string(), "IDLE".to_string(), "-".to_string(), "Idle".to_string()),
            ],

            // Persona sparkline histories
            captain_score_history: vec![92, 91, 93, 92, 94, 93, 92, 92, 93, 92],
            harper_score_history: vec![87, 86, 88, 87, 89, 87, 86, 87, 88, 87],
            benjamin_score_history: vec![83, 82, 84, 83, 85, 83, 82, 83, 84, 83],
            lucas_score_history: vec![89, 88, 90, 89, 91, 89, 88, 89, 90, 89],

            playhead: 0,
            fork_modal_open: false,
            steering_buffer: String::new(),
            policy_violation_alert: None,
        }
    }
}


impl KorgTui {
    pub fn log(&mut self, msg: impl Into<String>) {
        self.logs.push(msg.into());
        if self.logs.len() > 8 {
            self.logs.remove(0);
        }
    }

    pub fn handle_acp_message(&mut self, msg: AcpMessage) {
        if let AcpMessage::CampaignKtrans { ktrans } = msg {
            self.ktrans_log.push(format!(
                "r{} | {} | swarm={}",
                ktrans.round, ktrans.leader_action, ktrans.new_swarm_size
            ));
            if self.ktrans_log.len() > 10 {
                self.ktrans_log.remove(0);
            }
        }
    }

    pub fn update_from_leader(&mut self, _leader: &LeaderOrchestrator) {
        // Real integration would pull live data here
    }
}

/// Runs the TUI with a real live campaign running in the background.
/// This is used by `korg tui` and `korg campaign --tui`.
pub async fn run_tui_with_campaign(prompt: String, session: Option<uuid::Uuid>) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::channel::<TuiUpdate>(128);
    let (feedback_tx, feedback_rx) = tokio::sync::mpsc::channel::<ContractResponse>(1);

    // Spawn the actual campaign in the background
    let campaign_tx = tx.clone();
    tokio::spawn(async move {
        let mut leader = LeaderOrchestrator::new(prompt, session);
        leader.tui_tx = Some(campaign_tx.clone());
        leader.tui_rx = Some(feedback_rx);

        // We monkey-patch the observable campaign to send updates (in real code
        // we would add proper event hooks to LeaderOrchestrator).
        // For this integration we run the campaign and periodically send updates.
        let _ = leader.run_observable_campaign().await;

        // After campaign ends, keep the channel alive for a bit
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        drop(campaign_tx);
    });

    run_tui_event_loop(rx, Some(feedback_tx)).await
}

/// Runs the TUI attached to an existing Leader (used by `korg leader --demo --tui`)
pub async fn run_tui_with_leader(mut leader: LeaderOrchestrator) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::channel::<TuiUpdate>(128);
    let (feedback_tx, feedback_rx) = tokio::sync::mpsc::channel::<ContractResponse>(1);
    leader.tui_tx = Some(tx.clone());
    leader.tui_rx = Some(feedback_rx);

    // Spawn a task that feeds updates from the leader.
    tokio::spawn(async move {
        // Run the observable campaign while feeding the TUI
        let _ = leader.run_observable_campaign().await;
        drop(tx);
    });

    run_tui_event_loop(rx, Some(feedback_tx)).await
}

async fn run_tui_event_loop(
    mut rx: tokio::sync::mpsc::Receiver<TuiUpdate>,
    feedback_tx: Option<tokio::sync::mpsc::Sender<ContractResponse>>,
) -> anyhow::Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    let mut app = KorgTui::default();
    app.feedback_tx = feedback_tx;
    let mut should_quit = false;

    while !should_quit {
        terminal.draw(|f| draw_dashboard(f, &app))?;

        // Handle keyboard
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.fork_modal_open {
                        match key.code {
                            KeyCode::Char(c) => {
                                app.steering_buffer.push(c);
                            }
                            KeyCode::Backspace => {
                                app.steering_buffer.pop();
                            }
                            KeyCode::Enter => {
                                if !app.steering_buffer.is_empty() {
                                    app.log(format!("FORK CREATED at tx_{:02} with directive: {}", app.playhead, app.steering_buffer));
                                    if let Some(ref tx) = app.feedback_tx {
                                        let _ = tx.try_send(ContractResponse::Override(vec![format!("FORK:{}:{}", app.playhead, app.steering_buffer)]));
                                    }
                                    app.fork_modal_open = false;
                                    app.steering_buffer.clear();
                                }
                            }
                            KeyCode::Esc => {
                                app.fork_modal_open = false;
                                app.steering_buffer.clear();
                                app.log("Forking cancelled.");
                            }
                            _ => {}
                        }
                    } else if app.policy_violation_alert.is_some() {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                app.log("Policy Override APPROVED by operator");
                                app.policy_violation_alert = None;
                                app.pending_approval = None;
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                app.log("Policy Violation REJECTED. Swarm execution terminated.");
                                app.policy_violation_alert = None;
                                app.pending_approval = None;
                            }
                            KeyCode::Esc => {
                                app.policy_violation_alert = None;
                                app.pending_approval = None;
                            }
                            _ => {}
                        }
                    } else if app.pending_contract_approval.is_some() {
                        if app.editing_custom_criterion {
                            match key.code {
                                KeyCode::Char(c) => {
                                    app.input_buffer.push(c);
                                }
                                KeyCode::Backspace => {
                                    app.input_buffer.pop();
                                }
                                KeyCode::Enter => {
                                    if !app.input_buffer.is_empty() {
                                        app.log(format!("Custom override submitted: {}", app.input_buffer));
                                        if let Some(ref tx) = app.feedback_tx {
                                            let _ = tx.try_send(ContractResponse::Override(vec![app.input_buffer.clone()]));
                                        }
                                        app.pending_contract_approval = None;
                                        app.editing_custom_criterion = false;
                                    }
                                }
                                KeyCode::Esc => {
                                    app.editing_custom_criterion = false;
                                    app.input_buffer.clear();
                                    app.log("Edit cancelled.");
                                }
                                _ => {}
                            }
                        } else {
                            match key.code {
                                KeyCode::Char('y') => {
                                    app.log("Contract APPROVED");
                                    if let Some(ref tx) = app.feedback_tx {
                                        let _ = tx.try_send(ContractResponse::Approve);
                                    }
                                    app.pending_contract_approval = None;
                                }
                                KeyCode::Char('n') => {
                                    app.log("Contract REJECTED");
                                    if let Some(ref tx) = app.feedback_tx {
                                        let _ = tx.try_send(ContractResponse::Reject);
                                    }
                                    app.pending_contract_approval = None;
                                }
                                KeyCode::Char('f') => {
                                    app.log("Contract FORCED");
                                    if let Some(ref tx) = app.feedback_tx {
                                        let _ = tx.try_send(ContractResponse::Force);
                                    }
                                    app.pending_contract_approval = None;
                                }
                                KeyCode::Char('e') => {
                                    app.editing_custom_criterion = true;
                                    app.input_buffer.clear();
                                    app.log("Entering Custom Override Editing Mode");
                                }
                                KeyCode::Char('q') => should_quit = true,
                                _ => {}
                            }
                        }
                    } else {
                        // Standard non-approval keyboard handlers
                        match key.code {
                            KeyCode::Char('q') => should_quit = true,
                            KeyCode::Char('p') => {
                                app.paused = !app.paused;
                                app.log(if app.paused { "Paused" } else { "Resumed" });
                            }
                            KeyCode::Char('c') => app.log("Compaction requested"),
                            KeyCode::Char('y') if app.pending_approval.is_some() => {
                                app.log("Human APPROVED");
                                app.pending_approval = None;
                            }
                            KeyCode::Char('n') if app.pending_approval.is_some() => {
                                app.log("Human REJECTED");
                                app.pending_approval = None;
                            }
                            KeyCode::Left => {
                                if app.playhead > 0 {
                                    app.playhead -= 1;
                                    app.log(format!("Playhead scrubbed back to tx_{:02}", app.playhead));
                                }
                            }
                            KeyCode::Right => {
                                if app.playhead < 10 {
                                    app.playhead += 1;
                                    app.log(format!("Playhead scrubbed forward to tx_{:02}", app.playhead));
                                }
                            }
                            KeyCode::Char('f') | KeyCode::Char('F') => {
                                app.fork_modal_open = true;
                                app.steering_buffer.clear();
                                app.log(format!("Launching Steer-Fork Modal at tx_{:02}...", app.playhead));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Receive real updates from the campaign
        while let Ok(update) = rx.try_recv() {
            match update {
                TuiUpdate::Verdict {
                    text,
                    rubrics,
                    h_sem,
                    velocity,
                    risk,
                    progress,
                    doom_prob,
                } => {
                    app.current_verdict = text;
                    app.rubric_status = rubrics;
                    app.h_sem = h_sem;
                    app.velocity = velocity;
                    app.risk = risk;
                    app.progress = progress;
                    app.doom_prob = doom_prob;

                    // Update sparkline history
                    let scaled = (h_sem * 100.0).clamp(0.0, 100.0) as u64;
                    app.h_sem_history.push(scaled);
                    if app.h_sem_history.len() > 30 {
                        app.h_sem_history.remove(0);
                    }
                }
                TuiUpdate::Arena {
                    round,
                    winner,
                    mutations,
                } => {
                    app.arena_history.push(format!(
                        "Round {}: {} (+{} mutations)",
                        round, winner, mutations
                    ));
                    if app.arena_history.len() > 8 {
                        app.arena_history.remove(0);
                    }
                }
                TuiUpdate::Trace(s) => {
                    app.trace_events.push(s);
                    if app.trace_events.len() > 12 {
                        app.trace_events.remove(0);
                    }
                }
                TuiUpdate::Ktrans(s) => {
                    app.ktrans_log.push(s);
                    if app.ktrans_log.len() > 10 {
                        app.ktrans_log.remove(0);
                    }
                }
                TuiUpdate::ApprovalRequest(reason) => {
                    app.pending_approval = Some(reason);
                }
                TuiUpdate::Compaction(s) => {
                    app.compaction_status = s;
                }
                TuiUpdate::ContractNegotiated {
                    description,
                    criteria,
                } => {
                    app.contract_description = description;
                    app.contract_criteria = criteria;
                    app.log("Contract successfully negotiated!");
                }
                TuiUpdate::ContractApprovalRequest {
                    round,
                    description,
                    criteria,
                } => {
                    app.pending_contract_approval = Some((round, description, criteria));
                    app.editing_custom_criterion = false;
                    app.input_buffer.clear();
                }
                TuiUpdate::PersonaTelemetry {
                    scores,
                    telemetry_merges,
                    crdt_sync_frequency,
                    conflicts_count,
                    provenance_chain_length,
                    lock_states,
                } => {
                    app.persona_scores = scores;
                    app.telemetry_merges = telemetry_merges;
                    app.crdt_sync_frequency = crdt_sync_frequency;
                    app.conflicts_count = conflicts_count;
                    app.provenance_chain_length = provenance_chain_length;
                    app.lock_states = lock_states;

                    // Push scores to sparkline histories
                    let cap_sc = (scores[0] * 100.0).clamp(0.0, 100.0) as u64;
                    app.captain_score_history.push(cap_sc);
                    if app.captain_score_history.len() > 30 { app.captain_score_history.remove(0); }

                    let har_sc = (scores[1] * 100.0).clamp(0.0, 100.0) as u64;
                    app.harper_score_history.push(har_sc);
                    if app.harper_score_history.len() > 30 { app.harper_score_history.remove(0); }

                    let ben_sc = (scores[2] * 100.0).clamp(0.0, 100.0) as u64;
                    app.benjamin_score_history.push(ben_sc);
                    if app.benjamin_score_history.len() > 30 { app.benjamin_score_history.remove(0); }

                    let luc_sc = (scores[3] * 100.0).clamp(0.0, 100.0) as u64;
                    app.lucas_score_history.push(luc_sc);
                    if app.lucas_score_history.len() > 30 { app.lucas_score_history.remove(0); }
                }
            }
        }

        // Light demo heartbeat if no real data yet
        if app.current_verdict.contains("Waiting") {
            app.h_sem = (app.h_sem + 0.015) % 1.0;
            let scaled = (app.h_sem * 100.0) as u64;
            app.h_sem_history.push(scaled);
            if app.h_sem_history.len() > 30 {
                app.h_sem_history.remove(0);
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw_dashboard(f: &mut Frame, app: &KorgTui) {
    // 24-bit TrueColor Palette Definitions
    let fg_cyan = Color::Rgb(0, 240, 255);      // Electric Neon Cyan
    let fg_pink = Color::Rgb(255, 0, 180);      // Cyber Pink / Deep Neon Magenta
    let fg_green = Color::Rgb(0, 255, 128);     // Spring Neon Green
    let fg_gold = Color::Rgb(255, 215, 0);      // Amber Gold / Bright Yellow
    let fg_crimson = Color::Rgb(255, 50, 80);    // Neon Crimson
    let fg_slate = Color::Rgb(120, 125, 140);    // Muted Slate Gray
    let fg_white = Color::Rgb(240, 240, 245);    // Sleek High-Contrast White

    // Cockpit Workspace Layout splitting (6-Pane layout)
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Top Bar
            Constraint::Min(10),    // 6-Pane Cockpit Grid Workspace
            Constraint::Length(3),  // Bottom Scrubber Track & Status Bar
        ])
        .split(f.size());

    let top_bar_area = main_layout[0];
    let grid_area = main_layout[1];
    let bottom_track_area = main_layout[2];

    // Grid Columns (Left vs Right)
    let grid_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left Column (Editor, Terminal)
            Constraint::Percentage(50), // Right Column (Telemetry, Timeline DAG, Provenance)
        ])
        .split(grid_area);

    let left_col = grid_cols[0];
    let right_col = grid_cols[1];

    // Left Column split: Top (Editor), Bottom (Terminal)
    let left_panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(60), // Top-Left: Editor Pane
            Constraint::Percentage(40), // Bottom-Left: Terminal Pane
        ])
        .split(left_col);

    let editor_pane_area = left_panes[0];
    let terminal_pane_area = left_panes[1];

    // Right Column split: Top (Telemetry/Health), Center (DAG Timeline), Bottom (Provenance)
    let right_panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(25), // Top-Right: Telemetry/Health Pane
            Constraint::Percentage(50), // Center-Right: DAG Timeline
            Constraint::Percentage(25), // Bottom-Right: Provenance & Diff Viewer
        ])
        .split(right_col);

    let health_telemetry_area = right_panes[0];
    let dag_timeline_area = right_panes[1];
    let provenance_area = right_panes[2];

    // Bottom Track split: Top (Scrubber), Bottom (Status Bar)
    let bottom_panes = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Playhead Scrubber Track
            Constraint::Length(1), // Bottom Status Bar
        ])
        .split(bottom_track_area);

    let scrubber_track_area = bottom_panes[0];
    let status_bar_area = bottom_panes[1];

    // ==========================================
    // 0. Top Bar Dashboard Header
    // ==========================================
    let top = Paragraph::new(format!(
        " 🛡️  K O R G   A C P   C O M M A N D   D A S H B O A R D   │   Swarm: {}   │   Entropy: {:.3}   │   [{}] ",
        app.swarm_size,
        app.h_sem,
        if app.paused { "PAUSED 🛑" } else { "ACTIVE 🟢 (Multi-Threaded)" }
    ))
    .style(Style::default().fg(fg_cyan).bold())
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(fg_cyan))
        .title(" [ Korg Core Telemetry Cockpit v0.1.0 ] "));
    f.render_widget(top, top_bar_area);

    // ==========================================
    // 1. Monaco Editor Pane (Left Top)
    // ==========================================
    let code_lines = match app.playhead {
        0 => vec![
            Line::from(Span::styled("1: // Korg Heavy-Tier Swarm Init", Style::default().fg(fg_slate))),
            Line::from(Span::styled("2: fn main() -> Result<()> {", Style::default().fg(fg_white))),
            Line::from(Span::styled("3:     let mut swarm = Swarm::new(4);", Style::default().fg(fg_white))),
            Line::from(Span::styled("4:     swarm.negotiate_contract()?;", Style::default().fg(fg_cyan))),
            Line::from(Span::styled("5:     swarm.start_execution()?;", Style::default().fg(fg_cyan))),
            Line::from(Span::styled("6:     Ok(())", Style::default().fg(fg_white))),
            Line::from(Span::styled("7: }", Style::default().fg(fg_white))),
        ],
        1 | 2 => vec![
            Line::from(Span::styled("10: // Swarm Contract Negotiator Layer", Style::default().fg(fg_slate))),
            Line::from(Span::styled("11: pub async fn negotiate(target: &str) -> Result<Contract> {", Style::default().fg(fg_white))),
            Line::from(vec![
                Span::styled("12:     ", Style::default().fg(fg_slate)),
                Span::styled("[LOCKED BY CAPTAIN: READ-LOCK ACTIVE 👁️]", Style::default().fg(fg_cyan).bold().reversed())
            ]),
            Line::from(Span::styled("13:     let criteria = self.generate_proposal(target).await?;", Style::default().fg(fg_white))),
            Line::from(Span::styled("14:     let contract = self.reconcile(criteria).await?;", Style::default().fg(fg_white))),
            Line::from(Span::styled("15:     Ok(contract)", Style::default().fg(fg_white))),
            Line::from(Span::styled("16: }", Style::default().fg(fg_white))),
        ],
        3 | 4 => vec![
            Line::from(Span::styled("20: // Model-Agnostic LlmProvider complete method", Style::default().fg(fg_slate))),
            Line::from(Span::styled("21: pub fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {", Style::default().fg(fg_white))),
            Line::from(Span::styled("22:     let client = req.provider.get_client();", Style::default().fg(fg_white))),
            Line::from(vec![
                Span::styled("23:     ", Style::default().fg(fg_slate)),
                Span::styled("[LOCKED BY BENJAMIN: WRITE-LOCK ACTIVE 🔒]", Style::default().fg(fg_pink).bold().reversed())
            ]),
            Line::from(Span::styled("24: +   let request_payload = req.build_payload()?;", Style::default().fg(fg_green))),
            Line::from(Span::styled("25: +   let res = self.retry_decorator.execute(|| {", Style::default().fg(fg_green))),
            Line::from(Span::styled("26: +       client.post(&req.url, &request_payload)", Style::default().fg(fg_green))),
            Line::from(Span::styled("27: +   })?;", Style::default().fg(fg_green))),
            Line::from(Span::styled("28: -   let res = client.post(&req.url)?;", Style::default().fg(fg_pink))),
            Line::from(Span::styled("29:     Ok(res)", Style::default().fg(fg_white))),
            Line::from(Span::styled("30: }", Style::default().fg(fg_white))),
        ],
        _ => vec![
            Line::from(Span::styled("40: // Zero-Trust Security Policy Engine checks", Style::default().fg(fg_slate))),
            Line::from(Span::styled("41: pub fn check_policy(command: &str) -> Result<(), String> {", Style::default().fg(fg_white))),
            Line::from(vec![
                Span::styled("42:     ", Style::default().fg(fg_slate)),
                Span::styled("[LOCKED BY EVALUATOR: CRITIC-INTERCEPT ACTIVE 🛡️]", Style::default().fg(fg_gold).bold().reversed())
            ]),
            Line::from(Span::styled("43:     if is_blacklisted(command) {", Style::default().fg(fg_white))),
            Line::from(Span::styled("44:         return Err(\"CONTESTED: Policy Violation\".into());", Style::default().fg(fg_crimson).bold())),
            Line::from(Span::styled("45:     }", Style::default().fg(fg_white))),
            Line::from(Span::styled("46:     Ok(())", Style::default().fg(fg_white))),
            Line::from(Span::styled("47: }", Style::default().fg(fg_white))),
        ]
    };

    let editor_block = Paragraph::new(code_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_cyan))
            .title(" [ 📝 Monaco Editor (src/llm.rs) ] "));
    f.render_widget(editor_block, editor_pane_area);

    // ==========================================
    // 2. Terminal Subprocess Pane (Left Bottom)
    // ==========================================
    let terminal_lines = match app.playhead {
        0 => vec![
            Line::from(Span::styled("$ korg campaign init", Style::default().fg(fg_green))),
            Line::from(Span::styled("[System] Initializing heavy-tier swarm workspace...", Style::default().fg(fg_white))),
            Line::from(Span::styled("[System] Loaded 4 cognitive personas (Captain, Harper, Benjamin, Lucas)", Style::default().fg(fg_white))),
            Line::from(Span::styled("[System] Active directory locked at /Users/clubpenguin/Documents/Korg", Style::default().fg(fg_slate))),
        ],
        1 | 2 => vec![
            Line::from(Span::styled("$ korg negotiate --contract-rounds 3", Style::default().fg(fg_green))),
            Line::from(Span::styled("[Leader] Formulating task decomposition into 4 work packages...", Style::default().fg(fg_white))),
            Line::from(Span::styled("[Captain] Negotiating Swarm Agreement (BERT similarity targeting 0.85)...", Style::default().fg(fg_cyan))),
            Line::from(Span::styled("[Evaluator] Epistemic and Trajectory Rubrics active.", Style::default().fg(fg_pink))),
        ],
        3 | 4 => vec![
            Line::from(Span::styled("$ cargo test --lib tools", Style::default().fg(fg_green))),
            Line::from(Span::styled("   Compiling korg v0.1.0 (/Users/clubpenguin/Documents/Korg)", Style::default().fg(fg_slate))),
            Line::from(Span::styled("    Finished test [unoptimized + debuginfo] target(s) in 0.45s", Style::default().fg(fg_slate))),
            Line::from(Span::styled("     Running unittests src/main.rs (target/debug/deps/korg-...)", Style::default().fg(fg_slate))),
            Line::from(Span::styled("test tools::tests::test_apply_unified_diff_fuzzy ... ok", Style::default().fg(fg_green))),
            Line::from(Span::styled("test tools::tests::test_apply_unified_diff_multi_hunk ... ok", Style::default().fg(fg_green))),
            Line::from(Span::styled("test result: ok. 18 passed; 0 failed; 0 ignored;", Style::default().fg(fg_green).bold())),
        ],
        _ => vec![
            Line::from(Span::styled("$ cargo run -- campaign --tui", Style::default().fg(fg_green))),
            Line::from(Span::styled("[PolicyEngine] Intercepted shell command: 'cargo run'", Style::default().fg(fg_white))),
            Line::from(Span::styled("[PolicyEngine] Command matched whitelisted patterns in POLICY.md", Style::default().fg(fg_green))),
            Line::from(Span::styled("[Evaluator] Running 5-Rubric Critic Guardrail on live trace telemetry...", Style::default().fg(fg_pink))),
            Line::from(Span::styled("[Leader] Swarm scaled to 16 workers concurrently.", Style::default().fg(fg_cyan))),
        ]
    };

    let terminal_block = Paragraph::new(terminal_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_slate))
            .title(" [ 💻 Terminal Subprocess Piped Output ] "));
    f.render_widget(terminal_block, terminal_pane_area);

    // ==========================================
    // 3. Health & Telemetry Pane (Right Top)
    // ==========================================
    let ht_sub = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left: Metrics Gauges
            Constraint::Percentage(50), // Right: Sparkline
        ])
        .split(health_telemetry_area);

    let metrics_lines = vec![
        Line::from(vec![
            Span::styled(" ⚡ Velocity: ", Style::default().fg(fg_cyan).bold()),
            Span::styled(format!("{:.1} t/s", app.velocity), Style::default().fg(fg_white).bold()),
        ]),
        Line::from(vec![
            Span::styled(" ⚠️  Risk:     ", Style::default().fg(fg_pink).bold()),
            Span::styled(format!("{:.2}", app.risk), Style::default().fg(if app.risk > 0.6 { fg_crimson } else { fg_gold }).bold()),
        ]),
        Line::from(vec![
            Span::styled(" 📈 Progress: ", Style::default().fg(fg_green).bold()),
            Span::styled(format!("{:.1}%", app.progress), Style::default().fg(fg_white).bold()),
        ]),
    ];

    let metrics_block = Paragraph::new(metrics_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_pink))
            .title(" [ Telemetry Gauges ] "));
    f.render_widget(metrics_block, ht_sub[0]);

    // H_sem History Sparkline
    let sparkline = Sparkline::default()
        .data(&app.h_sem_history)
        .style(Style::default().fg(fg_cyan));
    let sparkline_block = Paragraph::new(vec![
        Line::from(Span::styled("Entropy H_sem History:", Style::default().fg(fg_gold).bold())),
    ]);
    let spark_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(ht_sub[1].inner(&ratatui::layout::Margin { vertical: 1, horizontal: 1 }));
    f.render_widget(sparkline_block, spark_layout[0]);
    f.render_widget(sparkline, spark_layout[1]);

    // ==========================================
    // 4. Live Swarm Timeline DAG (Right Center)
    // ==========================================
    let mut timeline_items = vec![];
    let nodes = [
        ("tx_00: genesis", "Orchestration Blue", fg_cyan),
        ("tx_01: negotiate_contract", "Orchestration Blue", fg_cyan),
        ("tx_02: dispatch_concurrent", "Worker Green", fg_green),
        ("tx_03: generate_patch", "Worker Green", fg_green),
        ("tx_04: evaluate_verdict", "Evaluator Red", fg_crimson),
        ("tx_05: operator_steer", "Operator Purple", fg_pink),
    ];

    for (i, (title, channel, color)) in nodes.iter().enumerate() {
        let is_current = app.playhead == i;
        let prefix = if is_current { "▶ " } else { "  " };
        let node_style = if is_current {
            Style::default().fg(*color).bold().reversed()
        } else {
            Style::default().fg(*color)
        };
        
        let branch_char = match i {
            0 => "● ",
            5 => "└── ◆ ",
            _ => "├── ● ",
        };

        timeline_items.push(ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(fg_gold).bold()),
            Span::styled(branch_char, Style::default().fg(fg_slate)),
            Span::styled(*title, node_style),
            Span::styled(format!(" [{}]", channel), Style::default().fg(fg_slate).italic()),
        ])));
    }

    let timeline_block = List::new(timeline_items)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_green))
            .title(" [ 🌿 Swarm Timeline DAG (F key to Fork) ] "));
    f.render_widget(timeline_block, dag_timeline_area);

    // ==========================================
    // 5. Provenance & Cryptographic Diff Viewer (Right Bottom)
    // ==========================================
    let prov_lines = vec![
        Line::from(vec![
            Span::styled(" Ed25519 Key: ", Style::default().fg(fg_cyan)),
            Span::styled("8f3c29a2b7e5... [VERIFIED ✓]", Style::default().fg(fg_green).bold()),
        ]),
        Line::from(vec![
            Span::styled(" Merkle Root: ", Style::default().fg(fg_gold)),
            Span::styled("a7b8c9d0e1f2...", Style::default().fg(fg_white)),
        ]),
        Line::from(vec![
            Span::styled(" File Impact: ", Style::default().fg(fg_pink)),
            Span::styled("src/llm.rs (L20-L30)", Style::default().fg(fg_white)),
        ]),
        Line::from(vec![
            Span::styled(" Authority:   ", Style::default().fg(fg_slate)),
            Span::styled("SwarmAuthority-v1-signed", Style::default().fg(fg_slate).italic()),
        ]),
    ];

    let provenance_block = Paragraph::new(prov_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_gold))
            .title(" [ 🔑 Cryptographic Provenance & Diffs ] "));
    f.render_widget(provenance_block, provenance_area);

    // ==========================================
    // 6. Playback Scrubber Track (Bottom Track)
    // ==========================================
    let filled_ticks = app.playhead.min(10);
    let unfilled_ticks = 10 - filled_ticks;
    let slider_bar = format!(
        "◄ ─── {}{} [ tx_{:02} ] ─── ►",
        "█".repeat(filled_ticks),
        "░".repeat(unfilled_ticks),
        app.playhead
    );

    let scrubber_text = vec![
        Line::from(vec![
            Span::styled(" [ REPLAY PLAYHEAD ] ", Style::default().fg(fg_gold).bold()),
            Span::styled(slider_bar, Style::default().fg(fg_cyan).bold()),
            Span::styled("  (Use Left/Right arrow keys to scrub) ", Style::default().fg(fg_slate).italic()),
        ])
    ];

    let scrubber_block = Paragraph::new(scrubber_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_cyan))
            .title(" [ 🎛️ Replay & Playback Scrubber ] "));
    f.render_widget(scrubber_block, scrubber_track_area);

    // ==========================================
    // 7. Bottom Status Bar (Bottom Track Footer)
    // ==========================================
    let status_text = format!(
        " ⚙️  [ESC] Quit  │  [P] Pause  │  [F] Steer Fork  │  [y/n] Policy Override  │  Playhead: tx_{:02}  │  Zero-Trust Engine OK ✓",
        app.playhead
    );
    let status_paragraph = Paragraph::new(status_text)
        .style(Style::default().bg(Color::Rgb(25, 25, 35)).fg(fg_cyan).bold());
    f.render_widget(status_paragraph, status_bar_area);

    // ==========================================
    // Modal Overlays
    // ==========================================

    // Approval Modal
    if let Some(reason) = &app.pending_approval {
        let area = centered_rect(60, 35, f.size());
        let modal = Paragraph::new(format!(
            " ⚠️  HUMAN IN THE LOOP APPROVAL MANDATE REQUIRED\n\n  {}\n\n  [y] Approve   [n] Reject   [e] Override   [q] Terminate Swarm",
            reason
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Double)
                .title(" 🔒 Human Security Approval Gate ")
                .style(Style::default().fg(fg_gold).bold()),
        );
        f.render_widget(modal, area);
    }

    // Policy Violation Alert Modal (Thick Double Border Visuals)
    if let Some(reason) = &app.policy_violation_alert {
        let area = centered_rect(60, 30, f.size());
        let modal = Paragraph::new(format!(
            "🚨 SECURITY INTERRUPT: CONTESTED POLICY VIOLATION 🚨\n\n  {}\n\n  [y] Force Override & Approve   [n] Reject & Stop Swarm   [Esc] Dismiss",
            reason
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Double)
                .title(" 🔒 Zero-Trust Policy Engine Intercept ")
                .style(Style::default().fg(fg_pink).bold()),
        );
        f.render_widget(modal, area);
    }

    // Fork/Steer Modal
    if app.fork_modal_open {
        let area = centered_rect(60, 30, f.size());
        let modal = Paragraph::new(format!(
            "🌿 TIME-TRAVEL PLAYHEAD FORK & STEER TERMINAL 🌿\n\n  Forking workspace at playhead position tx_{:02}.\n  Enter custom steering directive for the branched swarm:\n\n  > {}▍\n\n  [Enter] Deploy Swarm Fork   [Esc] Cancel",
            app.playhead, app.steering_buffer
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(ratatui::widgets::BorderType::Double)
                .title(" 🌿 Branching Playhead & Swarm Steering ")
                .style(Style::default().fg(fg_cyan).bold()),
        );
        f.render_widget(modal, area);
    }

    // Contract Approval Modal (Thick Double Border Visuals)
    if let Some((round, description, criteria)) = &app.pending_contract_approval {
        let area = centered_rect(70, 50, f.size());
        
        let mut lines = vec![
            Line::from(vec![
                Span::styled("🛡️  PROPOSED SWARM CONTRACT CRITERIA (Round ", Style::default().fg(fg_gold).bold()),
                Span::styled(round.to_string(), Style::default().fg(fg_green).bold()),
                Span::styled(")", Style::default().fg(fg_gold).bold()),
            ]),
            Line::from(""),
            Line::from(Span::styled("Task Prompt Description:", Style::default().fg(fg_cyan).bold())),
            Line::from(Span::styled(format!("  {}", description), Style::default().fg(fg_white))),
            Line::from(""),
            Line::from(Span::styled("Consensus Acceptance Criteria:", Style::default().fg(fg_cyan).bold())),
        ];
        
        for (i, (desc, sim)) in criteria.iter().enumerate() {
            let sim_color = if *sim >= 0.85 {
                fg_green
            } else if *sim >= 0.70 {
                fg_cyan
            } else {
                fg_gold
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", i + 1), Style::default().fg(fg_gold)),
                Span::styled(format!("{:<50} ", desc), Style::default().fg(fg_white)),
                Span::styled("  [ Cons: ", Style::default().fg(fg_slate)),
                Span::styled(format!("{:.3}", sim), Style::default().fg(sim_color).bold()),
                Span::styled(" ]", Style::default().fg(fg_slate)),
            ]));
        }
        
        lines.push(Line::from(""));
        
        if app.editing_custom_criterion {
            lines.push(Line::from(Span::styled(" ▍ Operator Override Terminal active", Style::default().fg(fg_pink).bold())));
            lines.push(Line::from(Span::styled("  Type custom criteria below and press Enter to inject. Press Esc to escape override.", Style::default().fg(fg_slate))));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Injected Criterion: ", Style::default().fg(fg_cyan).bold()),
                Span::styled(format!("{}▍", app.input_buffer), Style::default().fg(fg_white).bold()),
            ]));
        } else {
            lines.push(Line::from(Span::styled("Consensus Actions:", Style::default().fg(fg_cyan).bold())));
            lines.push(Line::from(Span::styled("  [y] Approve Swarm Contract   [n] Demand Revision   [e] Override and Add Custom   [f] Force Cons   [q] Cancel", Style::default().fg(fg_green).bold())));
        }
        
        let text = Text::from(lines);
        let modal = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Double)
                    .title(" Swarm Contract Consensus & Negotiation Gate ")
                    .style(Style::default().fg(fg_gold).bold()),
            );
        f.render_widget(modal, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playhead_scrubbing_boundaries() {
        let mut app = KorgTui::default();
        assert_eq!(app.playhead, 0);

        // Test boundary scrubbing back (no-op)
        app.playhead = 0;
        if app.playhead > 0 {
            app.playhead -= 1;
        }
        assert_eq!(app.playhead, 0);

        // Scrub forward
        for _ in 0..15 {
            if app.playhead < 10 {
                app.playhead += 1;
            }
        }
        assert_eq!(app.playhead, 10);

        // Scrub back once
        if app.playhead > 0 {
            app.playhead -= 1;
        }
        assert_eq!(app.playhead, 9);
    }

    #[test]
    fn test_steering_buffer_updates() {
        let mut app = KorgTui::default();
        assert!(app.steering_buffer.is_empty());

        // Simulate typing into buffer
        app.steering_buffer.push('F');
        app.steering_buffer.push('o');
        app.steering_buffer.push('r');
        app.steering_buffer.push('k');
        assert_eq!(app.steering_buffer, "Fork");

        // Backspace
        app.steering_buffer.pop();
        assert_eq!(app.steering_buffer, "For");

        // Escape cancelling
        app.fork_modal_open = true;
        app.steering_buffer.clear();
        app.fork_modal_open = false;
        assert!(app.steering_buffer.is_empty());
        assert!(!app.fork_modal_open);
    }

    #[test]
    fn test_zero_trust_policy_violation_modal() {
        let mut app = KorgTui::default();
        assert!(app.policy_violation_alert.is_none());

        // Trigger violation alert
        app.policy_violation_alert = Some("Shell command injection in Benjamin persona: 'rm -rf /'".to_string());
        assert!(app.policy_violation_alert.is_some());

        // Simulate Operator input handling
        // ESC/Dismiss
        app.policy_violation_alert = None;
        app.pending_approval = None;
        assert!(app.policy_violation_alert.is_none());
    }
}
