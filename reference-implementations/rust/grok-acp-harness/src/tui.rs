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
                    if app.pending_contract_approval.is_some() {
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Top Bar
            Constraint::Length(10), // Health, Gauges & Sparklines Panel
            Constraint::Min(10),    // Core Swarm Mechanics
            Constraint::Length(9),  // Audit Timelines / Locks
            Constraint::Length(6),  // Logs + ktrans
            Constraint::Length(1),  // Sleek Bottom Status Bar
        ])
        .split(f.size());

    // Top Bar Dashboard Header
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
        .title(" [ Korg Core Telemetry Harness v0.1.0 ] "));
    f.render_widget(top, chunks[0]);

    // Health, Gauges & Sparklines Panel (chunks[1])
    let health_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // Left: Metrics
            Constraint::Percentage(35), // Center: Stacked Gauges
            Constraint::Percentage(35), // Right: Sparklines
        ])
        .split(chunks[1]);

    // Left: One-Glance Campaign Health Metrics
    let health_text = vec![
        Line::from(vec![
            Span::styled(" ⚡ Token Velocity: ", Style::default().fg(fg_cyan).bold()),
            Span::styled(format!("{:.1} tokens/sec", app.velocity), Style::default().fg(fg_white).bold()),
        ]),
        Line::from(vec![
            Span::styled(" ⚠️  Swarm Risk Index: ", Style::default().fg(fg_pink).bold()),
            Span::styled(
                format!("{:.2}", app.risk),
                Style::default().fg(if app.risk > 0.6 { fg_crimson } else { fg_gold }).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 📈 Progress Rate:    ", Style::default().fg(fg_green).bold()),
            Span::styled(format!("{:.1}%", app.progress), Style::default().fg(fg_white).bold()),
        ]),
        Line::from(vec![
            Span::styled(" 🔥 Doom-Loop Prob:   ", Style::default().fg(fg_crimson).bold()),
            Span::styled(
                format!("{:.1}%", app.doom_prob * 100.0),
                Style::default().fg(if app.doom_prob > 0.5 { fg_crimson } else { fg_green }).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled(" 📜 Telemetry Merges: ", Style::default().fg(fg_gold).bold()),
            Span::styled(format!("{}", app.telemetry_merges), Style::default().fg(fg_white)),
        ]),
    ];
    let health_block = Paragraph::new(health_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_slate))
            .title(" [ Campaign Health Metrics ] "));
    f.render_widget(health_block, health_chunks[0]);

    // Center: stacked Gauges
    let gauge_wrapper = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(fg_cyan))
        .title(" [ Real-Time Telemetry Dials ] ");
    f.render_widget(gauge_wrapper, health_chunks[1]);

    let gauge_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(health_chunks[1].inner(&ratatui::layout::Margin { vertical: 1, horizontal: 1 }));

    // H_sem Gauge
    let entropy_color = if app.h_sem < 0.3 {
        fg_green
    } else if app.h_sem < 0.6 {
        fg_gold
    } else {
        fg_crimson
    };
    let h_gauge = Gauge::default()
        .gauge_style(Style::default().fg(entropy_color).bold())
        .percent((app.h_sem * 100.0).clamp(0.0, 100.0) as u16)
        .use_unicode(true)
        .label(format!("Semantic Entropy (H_sem): {:.3}", app.h_sem));
    f.render_widget(h_gauge, gauge_chunks[0]);

    // Risk Gauge
    let risk_color = if app.risk < 0.4 {
        fg_green
    } else if app.risk < 0.7 {
        fg_gold
    } else {
        fg_crimson
    };
    let r_gauge = Gauge::default()
        .gauge_style(Style::default().fg(risk_color).bold())
        .percent((app.risk * 100.0).clamp(0.0, 100.0) as u16)
        .use_unicode(true)
        .label(format!("Swarm Risk Level: {:.2}", app.risk));
    f.render_widget(r_gauge, gauge_chunks[1]);

    // Doom Gauge
    let doom_color = if app.doom_prob < 0.3 {
        fg_green
    } else if app.doom_prob < 0.6 {
        fg_gold
    } else {
        fg_crimson
    };
    let d_gauge = Gauge::default()
        .gauge_style(Style::default().fg(doom_color).bold())
        .percent((app.doom_prob * 100.0).clamp(0.0, 100.0) as u16)
        .use_unicode(true)
        .label(format!("Doom Loop Probability: {:.1}%", app.doom_prob * 100.0));
    f.render_widget(d_gauge, gauge_chunks[2]);

    // Right: Sparklines History
    let spark_wrapper = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(fg_slate))
        .title(" [ Telemetry Evolution History ] ");
    f.render_widget(spark_wrapper, health_chunks[2]);

    let spark_inner_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left: H_sem History
            Constraint::Percentage(50), // Right: Stacked Persona Sparklines
        ])
        .split(health_chunks[2].inner(&ratatui::layout::Margin { vertical: 1, horizontal: 1 }));

    // H_sem History Sparkline
    let sparkline = Sparkline::default()
        .data(&app.h_sem_history)
        .style(Style::default().fg(fg_cyan));
    let sparkline_label = Paragraph::new(vec![
        Line::from(Span::styled("Entropy H_sem History:", Style::default().fg(fg_gold).bold())),
    ]);
    let h_sem_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(spark_inner_chunks[0]);
    f.render_widget(sparkline_label, h_sem_layout[0]);
    f.render_widget(sparkline, h_sem_layout[1]);

    // Per-persona Sparklines
    let p_spark_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(spark_inner_chunks[1]);

    // Captain
    let cap_spark = Sparkline::default().data(&app.captain_score_history).style(Style::default().fg(fg_cyan));
    let cap_chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(7), Constraint::Min(1)]).split(p_spark_layout[0]);
    f.render_widget(Paragraph::new(Span::styled("Capt: ", Style::default().fg(fg_cyan).bold())), cap_chunks[0]);
    f.render_widget(cap_spark, cap_chunks[1]);

    // Harper
    let har_spark = Sparkline::default().data(&app.harper_score_history).style(Style::default().fg(fg_pink));
    let har_chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(7), Constraint::Min(1)]).split(p_spark_layout[1]);
    f.render_widget(Paragraph::new(Span::styled("Harp: ", Style::default().fg(fg_pink).bold())), har_chunks[0]);
    f.render_widget(har_spark, har_chunks[1]);

    // Benjamin
    let ben_spark = Sparkline::default().data(&app.benjamin_score_history).style(Style::default().fg(fg_gold));
    let ben_chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(7), Constraint::Min(1)]).split(p_spark_layout[2]);
    f.render_widget(Paragraph::new(Span::styled("Benj: ", Style::default().fg(fg_gold).bold())), ben_chunks[0]);
    f.render_widget(ben_spark, ben_chunks[1]);

    // Lucas
    let luc_spark = Sparkline::default().data(&app.lucas_score_history).style(Style::default().fg(fg_green));
    let luc_chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Length(7), Constraint::Min(1)]).split(p_spark_layout[3]);
    f.render_widget(Paragraph::new(Span::styled("Lucs: ", Style::default().fg(fg_green).bold())), luc_chunks[0]);
    f.render_widget(luc_spark, luc_chunks[1]);


    // Core Swarm Mechanics (chunks[2])
    let mechanics_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Left: Persona Confidence BarChart
            Constraint::Percentage(45), // Center: Negotiated Swarm Contract List
            Constraint::Percentage(30), // Right: Live Verdict Ticker + Rubrics
        ])
        .split(chunks[2]);

    // Left: Swarm Persona Confidence Chart
    let chart_data = [
        ("Capt", (app.persona_scores[0] * 100.0) as u64),
        ("Harp", (app.persona_scores[1] * 100.0) as u64),
        ("Benj", (app.persona_scores[2] * 100.0) as u64),
        ("Lucs", (app.persona_scores[3] * 100.0) as u64),
    ];
    let bar_chart = BarChart::default()
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_slate))
            .title(" [ Persona Confidence ] "))
        .data(&chart_data)
        .bar_width(5)
        .bar_gap(1)
        .value_style(Style::default().fg(fg_gold).bold())
        .label_style(Style::default().fg(fg_cyan))
        .style(Style::default().fg(fg_pink));
    f.render_widget(bar_chart, mechanics_chunks[0]);

    // Center: Negotiated Swarm Contract Panel (Scroll Agreement Visuals)
    let mut contract_text = vec![ListItem::new(Line::from(vec![
        Span::styled(" 📜 ACTIVE SWARM AGREEMENT (CRDT-Negotiated)", Style::default().bold().fg(fg_gold)),
    ]))];
    contract_text.push(ListItem::new(Line::from(vec![
        Span::styled(format!(" Target description: {}", app.contract_description), Style::default().italic().fg(fg_cyan)),
    ])));
    contract_text.push(ListItem::new(Line::from("")));

    for (i, (desc, sim)) in app.contract_criteria.iter().enumerate() {
        let sim_pct = (*sim * 100.0).clamp(0.0, 100.0) as usize;
        let filled_blocks = (sim_pct / 10).min(10);
        let empty_blocks = 10 - filled_blocks;
        let blocks_str = format!(
            "{}{}",
            "█".repeat(filled_blocks),
            "░".repeat(empty_blocks)
        );
        let sim_color = if *sim >= 0.85 {
            fg_green
        } else if *sim >= 0.70 {
            fg_cyan
        } else {
            fg_gold
        };

        contract_text.push(ListItem::new(Line::from(vec![
            Span::styled(format!("  [✓] {}. ", i + 1), Style::default().fg(fg_green).bold()),
            Span::styled(format!("{:<40} ", desc), Style::default().fg(fg_white)),
            Span::styled(format!(" [{} {:>3}%]", blocks_str, sim_pct), Style::default().fg(sim_color).bold()),
        ])));
    }
    let contract_widget = List::new(contract_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_cyan))
            .title(" [ Swarm Contract Criteria Panel ] "),
    );
    f.render_widget(contract_widget, mechanics_chunks[1]);

    // Right: Live Verdict Ticker + Rubrics
    let right_sub_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // Top: Live Verdict Ticker
            Constraint::Percentage(50), // Bottom: Rubrics
        ])
        .split(mechanics_chunks[2]);

    let ticker = Paragraph::new(app.current_verdict.clone())
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(fg_pink))
                .title(" [ Live Verdict Ticker ] "),
        );
    f.render_widget(ticker, right_sub_chunks[0]);

    let rubric_items: Vec<ListItem> = app
        .rubric_status
        .iter()
        .map(|(name, passed)| {
            let color = if *passed { fg_green } else { fg_crimson };
            ListItem::new(Span::styled(
                format!("{} {}", if *passed { "✓" } else { "✗" }, name),
                Style::default().fg(color),
            ))
        })
        .collect();
    let rubrics = List::new(rubric_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_cyan))
            .title(" [ 5-Rubric Critic Guardrail ] "),
    );
    f.render_widget(rubrics, right_sub_chunks[1]);

    // Audit Timelines / Locks (chunks[3])
    let audit_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40), // Left: Arena History
            Constraint::Percentage(60), // Right: Locks Table
        ])
        .split(chunks[3]);

    let arena_items: Vec<ListItem> = app
        .arena_history
        .iter()
        .map(|s| ListItem::new(s.clone()))
        .collect();
    let arena = List::new(arena_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_slate))
            .title(" [ Swarm Arena History & Winners ] "),
    );
    f.render_widget(arena, audit_chunks[0]);

    // Locks Table with visual indicators
    let header_cells = ["Persona", "Lock Mode", "Latency", "Activity"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(fg_cyan).bold()));
    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Rgb(30, 30, 40)))
        .height(1);

    let rows = app.lock_states.iter().map(|(persona, lock, latency, activity)| {
        let (lock_str, lock_color) = match lock.as_str() {
            "WRITE" => ("🔒 WRITE", fg_crimson),
            "READ" => ("👁️  READ", fg_green),
            "IDLE" => ("•  IDLE", fg_slate),
            _ => ("•  IDLE", fg_slate),
        };
        let cells = vec![
            Cell::from(persona.clone()).style(Style::default().fg(fg_white).bold()),
            Cell::from(lock_str).style(Style::default().fg(lock_color).bold()),
            Cell::from(latency.clone()).style(Style::default().fg(fg_gold)),
            Cell::from(activity.clone()).style(Style::default().fg(fg_cyan)),
        ];
        Row::new(cells)
    });

    let lock_table = Table::new(rows)
        .header(header)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_cyan))
            .title(format!(
                " [ Blackboard Lock Map │ Merges: {} │ Sync: {:.1}Hz │ Conflicts: {} ] ",
                app.telemetry_merges, app.crdt_sync_frequency, app.conflicts_count
            )))
        .widths(&[
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ]);
    f.render_widget(lock_table, audit_chunks[1]);

    // Bottom - Logs + Ktrans
    let bottom = Paragraph::new(Text::from(app.logs.join("\n")))
        .wrap(Wrap { trim: true })
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(fg_slate))
            .title(format!(
                " [ Logs │ .ktrans streamed: {} │ Compaction: {} ] ",
                app.ktrans_log.len(),
                app.compaction_status
            )));
    f.render_widget(bottom, chunks[4]);

    // Sleek status bar rendering
    let status_bar_text = format!(
        " ⚙️  [ESC] Quit  │  [P] Pause  │  [E] Override Criteria  │  Session ID: {}  │  Vault Intelligence OK ✓",
        app.pending_contract_approval.as_ref().map(|_| "PENDING OPERATOR OVERRIDE INPUT").unwrap_or("CAMPAIGN RUNNING")
    );
    let status_bar = Paragraph::new(status_bar_text)
        .style(Style::default().bg(Color::Rgb(25, 25, 35)).fg(fg_cyan).bold());
    f.render_widget(status_bar, chunks[5]);

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
