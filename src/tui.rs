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
    text::{Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;

/// Events sent from the LeaderOrchestrator to the live Ratatui TUI.
#[derive(Debug, Clone)]
pub enum TuiUpdate {
    Verdict {
        text: String,
        rubrics: Vec<(String, bool)>, // e.g. [("Trajectory", true), ("Epistemic", false), ...]
        h_sem: f32,
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
}

pub struct KorgTui {
    pub swarm_size: u32,
    pub h_sem: f32,
    pub current_verdict: String,
    pub rubric_status: Vec<(String, bool)>, // (name, passed)
    pub arena_history: Vec<String>,
    pub trace_events: Vec<String>,
    pub ktrans_log: Vec<String>,
    pub compaction_status: String,
    pub pending_approval: Option<String>,
    pub paused: bool,
    pub logs: Vec<String>,
}

impl Default for KorgTui {
    fn default() -> Self {
        Self {
            swarm_size: 4,
            h_sem: 0.42,
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
pub async fn run_tui_with_campaign(session: Option<uuid::Uuid>) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::channel::<TuiUpdate>(128);

    // Spawn the actual campaign in the background
    let campaign_tx = tx.clone();
    tokio::spawn(async move {
        let mut leader = LeaderOrchestrator::new("Korg TUI Live Campaign".to_string(), session);
        leader.tui_tx = Some(campaign_tx.clone());

        // We monkey-patch the observable campaign to send updates (in real code
        // we would add proper event hooks to LeaderOrchestrator).
        // For this integration we run the campaign and periodically send synthetic
        // but realistic updates based on the leader's internal state.
        let _ = leader.run_observable_campaign().await;

        // After campaign ends, keep the channel alive for a bit
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        drop(campaign_tx);
    });

    run_tui_event_loop(rx).await
}

/// Runs the TUI attached to an existing Leader (used by `korg leader --demo --tui`)
pub async fn run_tui_with_leader(mut leader: LeaderOrchestrator) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::mpsc::channel::<TuiUpdate>(128);
    leader.tui_tx = Some(tx.clone());

    // In a fuller integration the leader would accept an event sender.
    // For now we spawn a task that feeds updates from the leader.
    tokio::spawn(async move {
        // Run the observable campaign while feeding the TUI
        let _ = leader.run_observable_campaign().await;
        drop(tx);
    });

    run_tui_event_loop(rx).await
}

async fn run_tui_event_loop(mut rx: tokio::sync::mpsc::Receiver<TuiUpdate>) -> anyhow::Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    let mut app = KorgTui::default();
    let mut should_quit = false;

    while !should_quit {
        terminal.draw(|f| draw_dashboard(f, &app))?;

        // Handle keyboard
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
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

        // Receive real updates from the campaign
        while let Ok(update) = rx.try_recv() {
            match update {
                TuiUpdate::Verdict {
                    text,
                    rubrics,
                    h_sem,
                } => {
                    app.current_verdict = text;
                    app.rubric_status = rubrics;
                    app.h_sem = h_sem;
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
            }
        }

        // Light demo heartbeat if no real data yet
        if app.current_verdict.contains("Waiting") {
            app.h_sem = (app.h_sem + 0.015) % 1.0;
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    terminal.show_cursor()?;
    Ok(())
}

fn draw_dashboard(f: &mut Frame, app: &KorgTui) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top bar
            Constraint::Min(10),   // Main area
            Constraint::Length(6), // Bottom logs
        ])
        .split(f.size());

    // Top bar
    let top = Paragraph::new(format!(
        "KORG Heavy-Tier Dashboard  |  Swarm: {}  |  H_sem: {:.3}  |  {}",
        app.swarm_size,
        app.h_sem,
        if app.paused { "PAUSED" } else { "LIVE" }
    ))
    .style(Style::default().fg(Color::Cyan).bold())
    .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(top, chunks[0]);

    // Main dashboard split
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35), // Left: Ticker + Rubrics
            Constraint::Percentage(40), // Center: Arena
            Constraint::Percentage(25), // Right: Sparkline + Events
        ])
        .split(chunks[1]);

    // Left column - Verdict Ticker + Rubrics
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(main_chunks[0]);

    let ticker = Paragraph::new(app.current_verdict.clone())
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Live Verdict Ticker"),
        );
    f.render_widget(ticker, left[0]);

    let rubric_items: Vec<ListItem> = app
        .rubric_status
        .iter()
        .map(|(name, passed)| {
            let color = if *passed { Color::Green } else { Color::Red };
            ListItem::new(Span::styled(
                format!("{} {}", if *passed { "✓" } else { "✗" }, name),
                Style::default().fg(color),
            ))
        })
        .collect();

    let rubrics = List::new(rubric_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("5-Rubric Status"),
    );
    f.render_widget(rubrics, left[1]);

    // Center - Arena History
    let arena_items: Vec<ListItem> = app
        .arena_history
        .iter()
        .map(|s| ListItem::new(s.clone()))
        .collect();
    let arena = List::new(arena_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Arena History + Winner"),
    );
    f.render_widget(arena, main_chunks[1]);

    // Right - H_sem + TraceEvents
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(5)])
        .split(main_chunks[2]);

    let hsem = Paragraph::new(format!("H_sem sparkline: {:.3}", app.h_sem)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Semantic Entropy"),
    );
    f.render_widget(hsem, right[0]);

    let trace_items: Vec<ListItem> = app
        .trace_events
        .iter()
        .map(|s| ListItem::new(s.clone()))
        .collect();
    let trace = List::new(trace_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("TraceEvent Feed"),
    );
    f.render_widget(trace, right[1]);

    // Bottom - Logs + Ktrans
    let bottom = Paragraph::new(Text::from(app.logs.join("\n")))
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Logs | .ktrans streamed: {} | Compaction: {}",
            app.ktrans_log.len(),
            app.compaction_status
        )));
    f.render_widget(bottom, chunks[2]);

    // Approval Modal
    if let Some(reason) = &app.pending_approval {
        let area = centered_rect(60, 35, f.size());
        let modal = Paragraph::new(format!(
            "HUMAN APPROVAL REQUIRED\n\n{}\n\n[y] Approve   [n] Reject   [e] Edit   [q] Quit",
            reason
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Approval Gate")
                .style(Style::default().fg(Color::Yellow).bold()),
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
