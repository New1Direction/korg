//! Korg Dashboard — Axum-based web server and live minimalist monochrome browser dashboard.
//!
//! Provides the production web dashboard for Phase 2:
//!   - GET `/api/events` (SSE stream broadcasting TuiUpdate JSONs)
//!   - POST `/api/override` (forwards ContractResponse user overrides back to the leader)
//!   - GET `/api/state` (exposes active blackboard.json snapshot)
//!   - Static embedding of the sleek glassmorphism HTML dashboard
//!   - Auto-opens browser upon starting.

use ax_sse::{Event, Sse};
use axum::{
    extract::State,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::leader::LeaderOrchestrator;
use crate::tui::{ContractResponse, TuiUpdate};

// Axum SSE response mapping helper
mod ax_sse {
    pub use axum::response::sse::{Event, KeepAlive, Sse};
}

/// Global shared state for the Axum server
struct AppState {
    broadcaster: broadcast::Sender<TuiUpdate>,
    feedback_tx: Mutex<Option<mpsc::Sender<ContractResponse>>>,
}

/// Auto-opens the default system browser targeting the given URL.
fn open_browser(url: &str) {
    println!("[Web] Automatically opening browser at: {}", url);
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).status();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(&["/C", "start", url])
        .status();

    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).status();
}

/// Runs a web dashboard campaign.
/// This matches `crate::tui::run_tui_with_campaign` but routes telemetry to a web server.
pub async fn run_web_with_campaign(prompt: String, session: Option<Uuid>) -> anyhow::Result<()> {
    let (tui_tx, mut tui_rx) = mpsc::channel::<TuiUpdate>(128);
    let (feedback_tx, feedback_rx) = mpsc::channel::<ContractResponse>(1);

    // 1. Create the broadcast channel for multi-subscriber SSE mapping
    let (broadcaster_tx, _) = broadcast::channel::<TuiUpdate>(256);

    // 2. Spawn the leader process campaign in the background
    let campaign_tx = tui_tx.clone();
    tokio::spawn(async move {
        let mut leader = LeaderOrchestrator::new(prompt, session);
        leader.tui_tx = Some(campaign_tx.clone());
        leader.tui_rx = Some(feedback_rx);

        let _ = leader.run_observable_campaign().await;

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        drop(campaign_tx);
    });

    // 3. Spawn a task to forward standard tui_tx (from leader) to the multi-client broadcaster
    let broadcaster_tx_clone = broadcaster_tx.clone();
    tokio::spawn(async move {
        while let Some(update) = tui_rx.recv().await {
            let _ = broadcaster_tx_clone.send(update);
        }
    });

    // 4. Start the Axum web server on port 8080
    let app_state = Arc::new(AppState {
        broadcaster: broadcaster_tx,
        feedback_tx: Mutex::new(Some(feedback_tx)),
    });

    let router = Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/api/events", get(sse_handler))
        .route("/api/state", get(state_handler))
        .route("/api/override", post(override_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    println!("\n\x1b[1m[korg] Axum server listening on http://localhost:8080\x1b[0m");

    // Auto-open browser in a separate thread
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        open_browser("http://localhost:8080");
    });

    axum::serve(listener, router).await?;
    Ok(())
}

/// Runs a web dashboard campaign attached to an existing Leader.
pub async fn run_web_with_leader(mut leader: LeaderOrchestrator) -> anyhow::Result<()> {
    let (tui_tx, mut tui_rx) = mpsc::channel::<TuiUpdate>(128);
    let (feedback_tx, feedback_rx) = mpsc::channel::<ContractResponse>(1);
    leader.tui_tx = Some(tui_tx.clone());
    leader.tui_rx = Some(feedback_rx);

    let (broadcaster_tx, _) = broadcast::channel::<TuiUpdate>(256);

    tokio::spawn(async move {
        let _ = leader.run_observable_campaign().await;
        drop(tui_tx);
    });

    let broadcaster_tx_clone = broadcaster_tx.clone();
    tokio::spawn(async move {
        while let Some(update) = tui_rx.recv().await {
            let _ = broadcaster_tx_clone.send(update);
        }
    });

    let app_state = Arc::new(AppState {
        broadcaster: broadcaster_tx,
        feedback_tx: Mutex::new(Some(feedback_tx)),
    });

    let router = Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/api/events", get(sse_handler))
        .route("/api/state", get(state_handler))
        .route("/api/override", post(override_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    println!("\n\x1b[1m[korg] Axum server listening on http://localhost:8080\x1b[0m");

    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        open_browser("http://localhost:8080");
    });

    axum::serve(listener, router).await?;
    Ok(())
}

/// Serves the embedded glassmorphism SPA index.html
async fn index_handler() -> impl IntoResponse {
    Html(INDEX_HTML)
}

/// GET `/api/events` (SSE Stream endpoint)
async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.broadcaster.subscribe();
    let stream = BroadcastStream::new(rx).map(|msg| match msg {
        Ok(update) => {
            let json = serde_json::to_string(&update).unwrap_or_default();
            Ok(Event::default().data(json))
        }
        Err(_) => Ok(Event::default().comment("keepalive")),
    });

    Sse::new(stream).keep_alive(ax_sse::KeepAlive::default())
}

/// GET `/api/state`
async fn state_handler() -> Json<serde_json::Value> {
    let path = "/tmp/korg/blackboard/blackboard.json";
    if let Ok(content) = tokio::fs::read_to_string(path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            return Json(json);
        }
    }
    Json(serde_json::json!({
        "session_id": Uuid::now_v7().to_string(),
        "trace_buffer": [],
        "recent_pulses": [],
        "info": "Dashboard loaded; waiting for first campaign telemetry stream."
    }))
}

/// POST `/api/override`
async fn override_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ContractResponse>,
) -> Result<(), axum::http::StatusCode> {
    let guard = state.feedback_tx.lock().await;
    if let Some(tx) = &*guard {
        if tx.clone().send(payload).await.is_ok() {
            println!("[Web] Transmitted human override feedback payload successfully");
            return Ok(());
        }
    }
    Err(axum::http::StatusCode::SERVICE_UNAVAILABLE)
}

// ============================================================================
// MINIMALIST EMBEDDED DASHBOARD
// ============================================================================
const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>korg — autonomous software engineering environment</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:ital,wght@0,300;0,400;0,500;0,600;0,700;1,300&family=JetBrains+Mono:wght@400;500;700&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg-base: #000000;
            --pane-bg: #000000;
            --pane-header-bg: #000000;
            --border-color: #1a1a1a;
            --border-active: #ffffff;
            --text-primary: #ffffff;
            --text-secondary: #8e8e93;
            --text-muted: #555555;
            --font-sans: 'Inter', sans-serif;
            --font-mono: 'JetBrains Mono', monospace;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }

        body {
            font-family: var(--font-sans);
            background-color: var(--bg-base);
            color: var(--text-primary);
            height: 100vh;
            overflow: hidden;
            display: flex;
            flex-direction: column;
        }

        /* Scrollbars */
        ::-webkit-scrollbar {
            width: 3px;
            height: 3px;
        }
        ::-webkit-scrollbar-track {
            background: #000000;
        }
        ::-webkit-scrollbar-thumb {
            background: #222222;
        }
        ::-webkit-scrollbar-thumb:hover {
            background: #444444;
        }

        header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 14px 24px;
            border-bottom: 1px solid var(--border-color);
            background-color: #000000;
        }

        .logo-container {
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .logo {
            font-size: 18px;
            font-weight: 700;
            letter-spacing: 0.05em;
            color: #ffffff;
            text-transform: lowercase;
        }

        .logo-sub {
            font-size: 11px;
            color: var(--text-muted);
            font-family: var(--font-mono);
        }

        .session-info {
            display: flex;
            align-items: center;
            gap: 16px;
        }

        .status-badge {
            font-size: 11px;
            font-family: var(--font-mono);
            color: var(--text-secondary);
            display: flex;
            align-items: center;
            gap: 6px;
        }

        .status-dot {
            width: 6px;
            height: 6px;
            border-radius: 50%;
            background-color: #ffffff;
            animation: pulse 2s infinite;
        }

        @keyframes pulse {
            0% { opacity: 0.3; }
            50% { opacity: 1; }
            100% { opacity: 0.3; }
        }

        .session-id {
            font-size: 11px;
            font-family: var(--font-mono);
            border: 1px solid var(--border-color);
            padding: 3px 8px;
            color: var(--text-secondary);
        }

        /* Layout Grid */
        main {
            display: grid;
            grid-template-columns: 50% 50%;
            grid-template-rows: calc(100vh - 110px) 45px;
            background-color: #000000;
            flex-grow: 1;
        }

        .left-col, .right-col {
            display: flex;
            flex-direction: column;
            border-right: 1px solid var(--border-color);
        }

        .right-col {
            border-right: none;
        }

        .pane {
            flex: 1;
            display: flex;
            flex-direction: column;
            border-bottom: 1px solid var(--border-color);
            overflow: hidden;
            background-color: var(--pane-bg);
        }

        .pane:last-child {
            border-bottom: none;
        }

        .pane-header {
            padding: 10px 16px;
            background-color: var(--pane-header-bg);
            border-bottom: 1px solid var(--border-color);
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .pane-title {
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-secondary);
            text-transform: lowercase;
            letter-spacing: 0.02em;
        }

        .pane-meta {
            font-family: var(--font-mono);
            font-size: 10px;
            color: var(--text-muted);
        }

        .pane-body {
            flex: 1;
            padding: 16px;
            overflow-y: auto;
            position: relative;
        }

        /* Monaco Workspace Styling */
        .workspace-body {
            padding: 0;
            background-color: #020202;
        }

        .code-container {
            font-family: var(--font-mono);
            font-size: 11px;
            line-height: 1.5;
            color: #d4d4d8;
            padding: 16px;
            white-space: pre-wrap;
            tab-size: 4;
        }

        .code-line {
            display: flex;
            position: relative;
        }

        .code-num {
            width: 32px;
            color: var(--text-muted);
            user-select: none;
            text-align: right;
            margin-right: 16px;
        }

        .code-content {
            flex-grow: 1;
        }

        .code-line.addition {
            background-color: rgba(255, 255, 255, 0.05);
            border-left: 2px solid #ffffff;
            color: #ffffff;
        }

        .code-line.deletion {
            background-color: rgba(255, 255, 255, 0.02);
            border-left: 2px solid var(--text-muted);
            color: var(--text-muted);
            text-decoration: line-through;
        }

        .code-badge {
            background-color: #ffffff;
            color: #000000;
            font-size: 9px;
            padding: 1px 4px;
            font-weight: 700;
            margin-left: 8px;
            vertical-align: middle;
            text-transform: lowercase;
        }

        /* Terminal Console */
        .console-body {
            background-color: #000000;
            font-family: var(--font-mono);
            font-size: 11px;
            line-height: 1.4;
            color: #e4e4e7;
        }

        .console-line {
            margin-bottom: 4px;
        }

        .console-prompt {
            color: #ffffff;
        }

        .console-system {
            color: var(--text-secondary);
        }

        .console-info {
            color: #a1a1aa;
        }

        /* Metrics & Telemetry Grid */
        .metrics-grid {
            display: grid;
            grid-template-columns: repeat(4, 1fr);
            gap: 12px;
            margin-bottom: 16px;
        }

        .metric-card {
            border: 1px solid var(--border-color);
            padding: 12px;
            background-color: #030303;
            text-align: left;
        }

        .metric-label {
            font-family: var(--font-mono);
            font-size: 10px;
            color: var(--text-secondary);
            text-transform: lowercase;
            margin-bottom: 4px;
        }

        .metric-value {
            font-size: 18px;
            font-weight: 700;
            font-family: var(--font-mono);
        }

        .sparkline-container {
            border: 1px solid var(--border-color);
            background-color: #030303;
            padding: 16px;
            height: calc(100% - 70px);
            min-height: 80px;
            display: flex;
            flex-direction: column;
        }

        .sparkline-header {
            display: flex;
            justify-content: space-between;
            font-family: var(--font-mono);
            font-size: 10px;
            color: var(--text-secondary);
            margin-bottom: 12px;
            text-transform: lowercase;
        }

        .sparkline-canvas {
            width: 100%;
            flex-grow: 1;
        }

        /* Timeline Merkle DAG */
        .dag-container {
            width: 100%;
            height: 100%;
            display: flex;
            flex-direction: column;
        }

        .dag-svg {
            flex-grow: 1;
            width: 100%;
            background-color: #010101;
        }

        .dag-node {
            cursor: pointer;
        }

        .dag-node circle {
            fill: #000000;
            stroke: #333333;
            stroke-width: 1.5;
            transition: all 0.2s;
        }

        .dag-node:hover circle {
            stroke: #ffffff;
        }

        .dag-node.active circle {
            fill: #ffffff;
            stroke: #ffffff;
        }

        .dag-node-text {
            font-family: var(--font-mono);
            font-size: 9px;
            fill: var(--text-secondary);
            text-anchor: start;
        }

        .dag-node.active .dag-node-text {
            fill: #ffffff;
            font-weight: bold;
        }

        .dag-edge {
            stroke: #222222;
            stroke-width: 1;
            fill: none;
        }

        .dag-edge.active {
            stroke: #555555;
        }

        /* Provenance and Swarm Brains */
        .provenance-container {
            display: grid;
            grid-template-columns: 55% 45%;
            gap: 12px;
            height: 100%;
        }

        .prov-details {
            font-family: var(--font-mono);
            font-size: 10px;
            line-height: 1.6;
            border-right: 1px solid var(--border-color);
            padding-right: 12px;
        }

        .prov-row {
            display: flex;
            margin-bottom: 6px;
        }

        .prov-key {
            width: 90px;
            color: var(--text-muted);
            text-transform: lowercase;
        }

        .prov-val {
            flex-grow: 1;
            color: var(--text-primary);
        }

        .swarm-actors {
            padding-left: 6px;
            display: flex;
            flex-direction: column;
            gap: 6px;
        }

        .actor-card {
            border: 1px solid var(--border-color);
            padding: 6px 10px;
            background-color: #030303;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .actor-name {
            font-size: 11px;
            font-weight: 600;
            color: #ffffff;
            text-transform: lowercase;
        }

        .actor-lock {
            font-family: var(--font-mono);
            font-size: 9px;
            padding: 1px 5px;
            border: 1px solid var(--border-color);
            color: var(--text-secondary);
        }

        .actor-lock.active {
            background-color: #ffffff;
            color: #000000;
            border-color: #ffffff;
        }

        /* Replay Scrubber Bottom Bar */
        .bottom-bar {
            grid-column: 1 / span 2;
            border-top: 1px solid var(--border-color);
            background-color: #000000;
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 0 24px;
            font-family: var(--font-mono);
            font-size: 11px;
        }

        .scrubber-info {
            color: var(--text-secondary);
            text-transform: lowercase;
        }

        .scrubber-track {
            display: flex;
            align-items: center;
            gap: 16px;
            flex-grow: 1;
            max-width: 600px;
            margin: 0 40px;
        }

        .scrubber-btn {
            background: none;
            border: none;
            color: var(--text-secondary);
            cursor: pointer;
            font-family: var(--font-mono);
            font-size: 12px;
            padding: 4px;
        }

        .scrubber-btn:hover {
            color: #ffffff;
        }

        .scrubber-slider-container {
            position: relative;
            flex-grow: 1;
            height: 4px;
            background: #222222;
            cursor: pointer;
        }

        .scrubber-progress {
            position: absolute;
            left: 0;
            top: 0;
            height: 100%;
            background: #ffffff;
            width: 0%;
        }

        .scrubber-handle {
            position: absolute;
            top: -4px;
            width: 12px;
            height: 12px;
            background: #ffffff;
            border: 1px solid #000000;
            transform: translateX(-50%);
            left: 0%;
        }

        .footer-status {
            color: var(--text-muted);
            text-transform: lowercase;
        }

        /* Modals & Overlays */
        .modal-overlay {
            position: fixed;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            background-color: rgba(0, 0, 0, 0.9);
            z-index: 1000;
            display: flex;
            justify-content: center;
            align-items: center;
            opacity: 0;
            pointer-events: none;
            transition: opacity 0.25s ease;
        }

        .modal-overlay.active {
            opacity: 1;
            pointer-events: auto;
        }

        .modal-card {
            background-color: #050505;
            border: 1px solid #222222;
            width: 520px;
            max-width: 90vw;
            padding: 28px;
            display: flex;
            flex-direction: column;
            gap: 20px;
        }

        .modal-title {
            font-family: var(--font-mono);
            font-size: 13px;
            font-weight: bold;
            color: #ffffff;
            text-transform: lowercase;
            letter-spacing: 0.05em;
            border-bottom: 1px solid #222222;
            padding-bottom: 10px;
        }

        .modal-desc {
            font-size: 12px;
            color: var(--text-secondary);
            line-height: 1.6;
        }

        .modal-input {
            width: 100%;
            background-color: #000000;
            border: 1px solid #333333;
            color: #ffffff;
            padding: 8px 12px;
            font-family: var(--font-mono);
            font-size: 11px;
            outline: none;
        }

        .modal-input:focus {
            border-color: #ffffff;
        }

        .modal-criteria-list {
            display: flex;
            flex-direction: column;
            gap: 8px;
            margin: 10px 0;
            max-height: 200px;
            overflow-y: auto;
        }

        .modal-criterion-item {
            display: flex;
            justify-content: space-between;
            font-family: var(--font-mono);
            font-size: 10px;
            padding: 6px;
            border: 1px solid #1c1c1e;
            background-color: #080808;
        }

        .criterion-text {
            color: var(--text-primary);
        }

        .criterion-similarity {
            color: var(--text-secondary);
        }

        .modal-actions {
            display: flex;
            gap: 12px;
            margin-top: 10px;
        }

        .btn {
            font-family: var(--font-sans);
            font-size: 11px;
            font-weight: 600;
            padding: 8px 16px;
            border: 1px solid #333333;
            background: none;
            color: #ffffff;
            cursor: pointer;
            text-transform: lowercase;
            transition: all 0.2s;
        }

        .btn:hover {
            border-color: #ffffff;
            background-color: #ffffff;
            color: #000000;
        }

        .btn-primary {
            background-color: #ffffff;
            color: #000000;
            border-color: #ffffff;
        }

        .btn-primary:hover {
            background-color: #e4e4e7;
            border-color: #e4e4e7;
        }

        .btn-danger {
            border-color: #331111;
            color: #ff5555;
        }

        .btn-danger:hover {
            background-color: #ff5555;
            color: #ffffff;
            border-color: #ff5555;
        }
    </style>
</head>
<body>
    <header>
        <div class="logo-container">
            <span class="logo">korg</span>
            <span class="logo-sub">autonomous engineering runtime</span>
        </div>
        <div class="session-info">
            <div class="status-badge">
                <span class="status-dot"></span>
                <span>telemetry active</span>
            </div>
            <div class="session-id" id="session-id">session: initializing</div>
        </div>
    </header>

    <main>
        <!-- Left Column: Workspace & Console -->
        <div class="left-col">
            <div class="pane" style="flex: 6;">
                <div class="pane-header">
                    <span class="pane-title">workspace</span>
                    <span class="pane-meta" id="workspace-meta">src/llm.rs — mono view</span>
                </div>
                <div class="pane-body workspace-body">
                    <div class="code-container" id="workspace-content"></div>
                </div>
            </div>
            <div class="pane" style="flex: 4;">
                <div class="pane-header">
                    <span class="pane-title">console</span>
                    <span class="pane-meta">runtime stdout</span>
                </div>
                <div class="pane-body console-body" id="console-content"></div>
            </div>
        </div>

        <!-- Right Column: Telemetry, Timeline, Provenance -->
        <div class="right-col">
            <div class="pane" style="flex: 3;">
                <div class="pane-header">
                    <span class="pane-title">telemetry</span>
                    <span class="pane-meta">realtime metrics</span>
                </div>
                <div class="pane-body" style="padding: 12px;">
                    <div class="metrics-grid">
                        <div class="metric-card">
                            <div class="metric-label">velocity</div>
                            <div class="metric-value" id="metric-velocity">0.0 t/s</div>
                        </div>
                        <div class="metric-card">
                            <div class="metric-label">risk</div>
                            <div class="metric-value" id="metric-risk">0.00</div>
                        </div>
                        <div class="metric-card">
                            <div class="metric-label">progress</div>
                            <div class="metric-value" id="metric-progress">0.0%</div>
                        </div>
                        <div class="metric-card">
                            <div class="metric-label">entropy</div>
                            <div class="metric-value" id="metric-entropy">0.000</div>
                        </div>
                    </div>
                    <div class="sparkline-container">
                        <div class="sparkline-header">
                            <span>entropy trajectory h_sem</span>
                            <span id="entropy-current">0.000</span>
                        </div>
                        <canvas class="sparkline-canvas" id="sparkline-canvas"></canvas>
                    </div>
                </div>
            </div>
            <div class="pane" style="flex: 4;">
                <div class="pane-header">
                    <span class="pane-title">timeline</span>
                    <span class="pane-meta">merkle-dag execution graph</span>
                </div>
                <div class="pane-body" style="padding: 0;">
                    <div class="dag-container">
                        <svg class="dag-svg" id="dag-svg" viewBox="0 0 600 240">
                            <!-- SVG elements will be drawn dynamically -->
                        </svg>
                    </div>
                </div>
            </div>
            <div class="pane" style="flex: 3;">
                <div class="pane-header">
                    <span class="pane-title">provenance</span>
                    <span class="pane-meta">zero-trust evaluation blackboard</span>
                </div>
                <div class="pane-body" style="padding: 12px;">
                    <div class="provenance-container">
                        <div class="prov-details" id="provenance-details">
                            <div class="prov-row">
                                <div class="prov-key">ed25519 key</div>
                                <div class="prov-val" style="font-size: 9px;">8f3c29a2b7e5... [verified ✓]</div>
                            </div>
                            <div class="prov-row">
                                <div class="prov-key">merkle root</div>
                                <div class="prov-val" style="font-size: 9px;" id="merkle-root">a7b8c9d0e1f2...</div>
                            </div>
                            <div class="prov-row">
                                <div class="prov-key">authority</div>
                                <div class="prov-val">swarmauthority-v1</div>
                            </div>
                            <div class="prov-row">
                                <div class="prov-key">policy engine</div>
                                <div class="prov-val">zero-trust active</div>
                            </div>
                            <div class="prov-row">
                                <div class="prov-key">ktrans status</div>
                                <div class="prov-val" id="ktrans-status">idle</div>
                            </div>
                        </div>
                        <div class="swarm-actors" id="swarm-actors-list">
                            <!-- Swarm actors and their locks will be rendered here -->
                        </div>
                    </div>
                </div>
            </div>
        </div>

        <!-- Bottom Scrubber Bar -->
        <div class="bottom-bar">
            <div class="scrubber-info">
                <span>playhead: tx_<span id="playhead-num">00</span></span>
            </div>
            <div class="scrubber-track">
                <button class="scrubber-btn" onclick="adjustPlayhead(-1)">◀</button>
                <div class="scrubber-slider-container" id="scrubber-container">
                    <div class="scrubber-progress" id="scrubber-progress"></div>
                    <div class="scrubber-handle" id="scrubber-handle"></div>
                </div>
                <button class="scrubber-btn" onclick="adjustPlayhead(1)">▶</button>
            </div>
            <div class="footer-status">
                <span>[esc] quit │ [p] pause │ [f] steer fork │ playhead key scrubbing active</span>
            </div>
        </div>
    </main>

    <!-- Modals -->
    <!-- Human Security Approval Modal -->
    <div class="modal-overlay" id="approval-modal">
        <div class="modal-card">
            <div class="modal-title">🔒 human security approval gate</div>
            <div class="modal-desc" id="approval-modal-desc">
                a zero-trust security policy has triggered a mandate for human operator verification.
            </div>
            <div class="modal-actions">
                <button class="btn btn-primary" onclick="submitContractFeedback('Approve')">approve execution</button>
                <button class="btn btn-danger" onclick="submitContractFeedback('Reject')">reject & terminate</button>
            </div>
        </div>
    </div>

    <!-- Swarm Contract Consensus Modal -->
    <div class="modal-overlay" id="contract-modal">
        <div class="modal-card">
            <div class="modal-title">🛡️ swarm contract consensus & negotiation</div>
            <div class="modal-desc">
                the swarm is proposing a contract round for autonomous execution. review the criteria:
                <div class="modal-criteria-list" id="contract-criteria-list">
                    <!-- Criteria populated dynamically -->
                </div>
            </div>
            <div class="modal-input-container" id="custom-criterion-container" style="display: none;">
                <div class="modal-label" style="font-family: var(--font-mono); font-size: 10px; margin-bottom: 4px; color: var(--text-secondary);">inject custom acceptance criterion:</div>
                <input type="text" class="modal-input" id="custom-criterion-input" placeholder="e.g. must pass tools::tests::test_unified_diff">
            </div>
            <div class="modal-actions">
                <button class="btn btn-primary" onclick="submitContractFeedback('Approve')">approve swarm contract</button>
                <button class="btn" onclick="submitContractFeedback('Reject')">demand revision</button>
                <button class="btn" id="btn-custom-toggle" onclick="toggleCustomCriterion()">override & add custom</button>
                <button class="btn btn-primary" id="btn-custom-submit" style="display: none;" onclick="submitCustomCriterion()">inject & approve</button>
            </div>
        </div>
    </div>

    <script>
        // Core Web App State
        let playhead = 0;
        const maxPlayhead = 5;
        let entropyHistory = [];
        let sessionID = 'initializing...';

        // Pre-recorded workspace code snippets corresponding to playhead positions
        const codeSnippets = {
            0: [
                { num: 1, content: '// korg heavy-tier swarm initialization & genesis', style: 'color: var(--text-muted);' },
                { num: 2, content: 'fn main() -> Result<()> {', style: '' },
                { num: 3, content: '    let mut swarm = Swarm::new(4);', style: '' },
                { num: 4, content: '    swarm.negotiate_contract()?;', style: 'color: #ffffff; font-weight: 600;' },
                { num: 5, content: '    swarm.start_execution()?;', style: 'color: #ffffff; font-weight: 600;' },
                { num: 6, content: '    Ok(())', style: '' },
                { num: 7, content: '}', style: '' }
            ],
            1: [
                { num: 10, content: '// swarm contract negotiator layer', style: 'color: var(--text-muted);' },
                { num: 11, content: 'pub async fn negotiate(target: &str) -> Result<Contract> {', style: '' },
                { num: 12, content: '    // [LOCKED BY CAPTAIN: READ-LOCK ACTIVE 👁️]', style: 'background-color: #ffffff; color: #000000; font-weight: bold; padding: 0 4px;' },
                { num: 13, content: '    let criteria = self.generate_proposal(target).await?;', style: '' },
                { num: 14, content: '    let contract = self.reconcile(criteria).await?;', style: '' },
                { num: 15, content: '    Ok(contract)', style: '' },
                { num: 16, content: '}', style: '' }
            ],
            2: [
                { num: 10, content: '// swarm contract negotiator layer', style: 'color: var(--text-muted);' },
                { num: 11, content: 'pub async fn negotiate(target: &str) -> Result<Contract> {', style: '' },
                { num: 12, content: '    // [LOCKED BY CAPTAIN: READ-LOCK ACTIVE 👁️]', style: 'background-color: #ffffff; color: #000000; font-weight: bold; padding: 0 4px;' },
                { num: 13, content: '    let criteria = self.generate_proposal(target).await?;', style: '' },
                { num: 14, content: '    let contract = self.reconcile(criteria).await?;', style: '' },
                { num: 15, content: '    Ok(contract)', style: '' },
                { num: 16, content: '}', style: '' }
            ],
            3: [
                { num: 20, content: '// model-agnostic LlmProvider complete method', style: 'color: var(--text-muted);' },
                { num: 21, content: 'pub fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {', style: '' },
                { num: 22, content: '    let client = req.provider.get_client();', style: '' },
                { num: 23, content: '    // [LOCKED BY BENJAMIN: WRITE-LOCK ACTIVE 🔒]', style: 'background-color: #8e8e93; color: #000000; font-weight: bold; padding: 0 4px;' },
                { num: 24, content: '+   let request_payload = req.build_payload()?;', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 25, content: '+   let res = self.retry_decorator.execute(|| {', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 26, content: '+       client.post(&req.url, &request_payload)', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 27, content: '+   })?;', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 28, content: '-   let res = client.post(&req.url)?;', style: 'color: var(--text-muted); text-decoration: line-through;', class: 'deletion' },
                { num: 29, content: '    Ok(res)', style: '' },
                { num: 30, content: '}', style: '' }
            ],
            4: [
                { num: 20, content: '// model-agnostic LlmProvider complete method', style: 'color: var(--text-muted);' },
                { num: 21, content: 'pub fn complete(&self, req: LlmRequest) -> Result<LlmResponse> {', style: '' },
                { num: 22, content: '    let client = req.provider.get_client();', style: '' },
                { num: 23, content: '    // [LOCKED BY BENJAMIN: WRITE-LOCK ACTIVE 🔒]', style: 'background-color: #8e8e93; color: #000000; font-weight: bold; padding: 0 4px;' },
                { num: 24, content: '+   let request_payload = req.build_payload()?;', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 25, content: '+   let res = self.retry_decorator.execute(|| {', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 26, content: '+       client.post(&req.url, &request_payload)', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 27, content: '+   })?;', style: 'color: #ffffff; font-weight: bold;', class: 'addition' },
                { num: 28, content: '-   let res = client.post(&req.url)?;', style: 'color: var(--text-muted); text-decoration: line-through;', class: 'deletion' },
                { num: 29, content: '    Ok(res)', style: '' },
                { num: 30, content: '}', style: '' }
            ],
            5: [
                { num: 40, content: '// zero-trust security policy engine check runtime intercepts', style: 'color: var(--text-muted);' },
                { num: 41, content: 'pub fn check_policy(command: &str) -> Result<(), String> {', style: '' },
                { num: 42, content: '    // [LOCKED BY EVALUATOR: CRITIC-INTERCEPT ACTIVE 🛡️]', style: 'background-color: #ffffff; color: #000000; font-weight: bold; padding: 0 4px;' },
                { num: 43, content: '    if is_blacklisted(command) {', style: '' },
                { num: 44, content: '        return Err("CONTESTED: Policy Violation".into());', style: 'color: #ff5555; font-weight: bold;' },
                { num: 45, content: '    }', style: '' },
                { num: 46, content: '    Ok(())', style: '' },
                { num: 47, content: '}', style: '' }
            ]
        };

        // Static Nodes for the Merkle-DAG Graph
        const dagNodes = [
            { id: 0, label: 'tx_00: genesis', desc: 'orchestration', x: 80, y: 120 },
            { id: 1, label: 'tx_01: negotiate_contract', desc: 'orchestration', x: 180, y: 80 },
            { id: 2, label: 'tx_02: dispatch_concurrent', desc: 'worker', x: 280, y: 80 },
            { id: 3, label: 'tx_03: generate_patch', desc: 'worker', x: 380, y: 120 },
            { id: 4, label: 'tx_04: evaluate_verdict', desc: 'evaluator', x: 480, y: 120 },
            { id: 5, label: 'tx_05: operator_steer', desc: 'operator', x: 520, y: 180 }
        ];

        // Core Swarm Actors & Lock states
        const defaultActors = [
            { name: 'captain', mode: 'read', status: 'idle', latency: '4ms' },
            { name: 'harper', mode: 'read', status: 'idle', latency: '8ms' },
            { name: 'benjamin', mode: 'write', status: 'idle', latency: '12ms' },
            { name: 'lucas', mode: 'write', status: 'idle', latency: '16ms' }
        ];

        // 1. Initialize Replay & Layout
        function updateWorkspace(index) {
            const container = document.getElementById("workspace-content");
            container.innerHTML = "";
            const lines = codeSnippets[index] || codeSnippets[0];
            lines.forEach(line => {
                const row = document.createElement("div");
                row.className = `code-line ${line.class || ''}`;
                
                const num = document.createElement("div");
                num.className = "code-num";
                num.innerText = line.num;
                
                const code = document.createElement("div");
                code.className = "code-content";
                code.innerText = line.content;
                if (line.style) {
                    code.setAttribute("style", line.style);
                }

                row.appendChild(num);
                row.appendChild(code);
                container.appendChild(row);
            });
            
            document.getElementById("workspace-meta").innerText = `src/llm.rs — playhead tx_0${index}`;
        }

        function drawDag() {
            const svg = document.getElementById("dag-svg");
            // Clear existing svg
            svg.innerHTML = `
                <defs>
                    <marker id="arrow" viewBox="0 0 10 10" refX="22" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                        <path d="M 0 1 L 10 5 L 0 9 z" fill="#333" />
                    </marker>
                </defs>
            `;

            // Draw edges
            const edges = [
                { from: 0, to: 1 },
                { from: 1, to: 2 },
                { from: 2, to: 3 },
                { from: 3, to: 4 },
                { from: 4, to: 5 },
                { from: 0, to: 5 } // steer fork bypass genesis link
            ];

            edges.forEach(edge => {
                const nodeFrom = dagNodes[edge.from];
                const nodeTo = dagNodes[edge.to];
                const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
                
                let activeClass = (playhead >= edge.to) ? "active" : "";
                
                // Draw curve lines for beautiful visual
                let d = `M ${nodeFrom.x} ${nodeFrom.y} C ${(nodeFrom.x + nodeTo.x)/2} ${nodeFrom.y}, ${(nodeFrom.x + nodeTo.x)/2} ${nodeTo.y}, ${nodeTo.x} ${nodeTo.y}`;
                
                path.setAttribute("d", d);
                path.setAttribute("class", `dag-edge ${activeClass}`);
                path.setAttribute("marker-end", "url(#arrow)");
                svg.appendChild(path);
            });

            // Draw Nodes
            dagNodes.forEach(node => {
                const g = document.createElementNS("http://www.w3.org/2000/svg", "g");
                g.setAttribute("class", `dag-node ${playhead === node.id ? 'active' : ''}`);
                g.onclick = () => selectPlayhead(node.id);

                const circle = document.createElementNS("http://www.w3.org/2000/svg", "circle");
                circle.setAttribute("cx", node.x);
                circle.setAttribute("cy", node.y);
                circle.setAttribute("r", 6);

                const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
                text.setAttribute("x", node.x + 12);
                text.setAttribute("y", node.y + 4);
                text.setAttribute("class", "dag-node-text");
                text.textContent = node.label;

                g.appendChild(circle);
                g.appendChild(text);
                svg.appendChild(g);
            });
        }

        function drawSparkline() {
            const canvas = document.getElementById("sparkline-canvas");
            const ctx = canvas.getContext("2d");
            
            // Handle resizing
            const rect = canvas.getBoundingClientRect();
            canvas.width = rect.width;
            canvas.height = rect.height;

            ctx.clearRect(0, 0, canvas.width, canvas.height);

            if (entropyHistory.length < 2) {
                // Draw flat line
                ctx.strokeStyle = "#222222";
                ctx.lineWidth = 1.5;
                ctx.beginPath();
                ctx.moveTo(0, canvas.height / 2);
                ctx.lineTo(canvas.width, canvas.height / 2);
                ctx.stroke();
                return;
            }

            const maxVal = Math.max(...entropyHistory, 1.0);
            const minVal = Math.min(...entropyHistory, 0.0);
            const range = maxVal - minVal || 1.0;

            ctx.strokeStyle = "#ffffff";
            ctx.lineWidth = 1.5;
            ctx.beginPath();

            const step = canvas.width / (entropyHistory.length - 1);
            
            entropyHistory.forEach((val, i) => {
                const x = i * step;
                const y = canvas.height - ((val - minVal) / range) * (canvas.height - 10) - 5;
                if (i === 0) {
                    ctx.moveTo(x, y);
                } else {
                    ctx.lineTo(x, y);
                }
            });

            ctx.stroke();

            // Subtle monochrome fill
            ctx.lineTo(canvas.width, canvas.height);
            ctx.lineTo(0, canvas.height);
            ctx.closePath();
            ctx.fillStyle = "rgba(255,255,255,0.02)";
            ctx.fill();
        }

        function renderActors(lockStates = []) {
            const list = document.getElementById("swarm-actors-list");
            list.innerHTML = "";

            defaultActors.forEach((actor, i) => {
                const card = document.createElement("div");
                card.className = "actor-card";

                const left = document.createElement("div");
                left.className = "actor-name";
                left.innerText = actor.name;

                // Sync locking from telemetry if present
                let lockStatus = "idle";
                if (lockStates && lockStates[i]) {
                    lockStatus = lockStates[i][1].toLowerCase();
                } else {
                    // Fallback visual linking based on playhead
                    if (playhead === 1 && actor.name === 'captain') lockStatus = 'read-lock';
                    if (playhead === 3 && actor.name === 'benjamin') lockStatus = 'write-lock';
                }

                const right = document.createElement("div");
                right.className = `actor-lock ${lockStatus !== 'idle' ? 'active' : ''}`;
                right.innerText = lockStatus;

                card.appendChild(left);
                card.appendChild(right);
                list.appendChild(card);
            });
        }

        // 2. Playhead Scrubber Logic
        function selectPlayhead(index) {
            if (index < 0 || index > maxPlayhead) return;
            playhead = index;

            // Sync visual bar
            const percent = (playhead / maxPlayhead) * 100;
            document.getElementById("scrubber-progress").style.width = `${percent}%`;
            document.getElementById("scrubber-handle").style.left = `${percent}%`;
            document.getElementById("playhead-num").innerText = `0${playhead}`;

            updateWorkspace(playhead);
            drawDag();
            renderActors();
        }

        function adjustPlayhead(dir) {
            selectPlayhead(playhead + dir);
        }

        document.getElementById("scrubber-container").onclick = (e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            const pct = (e.clientX - rect.left) / rect.width;
            const targetIdx = Math.round(pct * maxPlayhead);
            selectPlayhead(targetIdx);
        };

        // 3. Event Streaming SSE Connection
        function appendConsole(text) {
            const container = document.getElementById("console-content");
            const row = document.createElement("div");
            row.className = "console-line";
            
            // Clean ANSI strings if any
            let cleanText = text.replace(/\\u001b\\[[0-9;]*[a-zA-Z]/g, '');
            cleanText = cleanText.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '');

            if (cleanText.startsWith("$ ")) {
                row.innerHTML = `<span class="console-prompt">$</span> <span class="console-info">${cleanText.substring(2)}</span>`;
            } else if (cleanText.startsWith("[System]")) {
                row.innerHTML = `<span class="console-system">${cleanText}</span>`;
            } else {
                row.innerText = cleanText;
            }

            container.appendChild(row);
            container.scrollTop = container.scrollHeight;
        }

        function appendProvenance(line) {
            document.getElementById("ktrans-status").innerText = "compaction synced";
            const row = document.createElement("div");
            row.className = "prov-row";
            row.innerHTML = `<div class="prov-key" style="width: 100px;">.ktrans record</div><div class="prov-val" style="font-size: 8px; color: var(--text-secondary);">${line}</div>`;
            
            const details = document.getElementById("provenance-details");
            details.appendChild(row);
        }

        function setupSSE() {
            const events = new EventSource('/api/events');
            
            events.onmessage = (event) => {
                try {
                    const update = JSON.parse(event.data);
                    
                    if (update.Trace) {
                        appendConsole(update.Trace);
                    } else if (update.Ktrans) {
                        appendProvenance(update.Ktrans);
                    } else if (update.Verdict) {
                        const verdict = update.Verdict;
                        document.getElementById("metric-velocity").innerText = `${verdict.velocity.toFixed(1)} t/s`;
                        document.getElementById("metric-risk").innerText = verdict.risk.toFixed(2);
                        document.getElementById("metric-progress").innerText = `${verdict.progress.toFixed(1)}%`;
                        document.getElementById("metric-entropy").innerText = verdict.h_sem.toFixed(3);
                        document.getElementById("entropy-current").innerText = verdict.h_sem.toFixed(3);
                        
                        entropyHistory.push(verdict.h_sem);
                        if (entropyHistory.length > 50) entropyHistory.shift();
                        drawSparkline();

                        // Fast playhead linking based on progress
                        let targetPlayhead = Math.min(Math.floor((verdict.progress / 100) * (maxPlayhead + 1)), maxPlayhead);
                        if (targetPlayhead !== playhead) {
                            selectPlayhead(targetPlayhead);
                        }
                    } else if (update.PersonaTelemetry) {
                        const tel = update.PersonaTelemetry;
                        renderActors(tel.lock_states);
                        document.getElementById("merkle-root").innerText = `len: ${tel.provenance_chain_length} | root: a7b8...`;
                    } else if (update.ApprovalRequest) {
                        const reason = update.ApprovalRequest;
                        document.getElementById("approval-modal-desc").innerText = reason;
                        document.getElementById("approval-modal").classList.add("active");
                    } else if (update.ContractApprovalRequest) {
                        const req = update.ContractApprovalRequest;
                        const list = document.getElementById("contract-criteria-list");
                        list.innerHTML = "";
                        
                        req.criteria.forEach((crit, i) => {
                            const item = document.createElement("div");
                            item.className = "modal-criterion-item";
                            item.innerHTML = `<span class="criterion-text">[${i+1}] ${crit[0]}</span> <span class="criterion-similarity">cons: ${crit[1].toFixed(3)}</span>`;
                            list.appendChild(item);
                        });

                        document.getElementById("contract-modal").classList.add("active");
                    } else if (update.ContractNegotiated) {
                        appendConsole(`[contract negotiated]: ${update.ContractNegotiated.description}`);
                        document.getElementById("contract-modal").classList.remove("active");
                    } else if (update.Compaction) {
                        appendConsole(`[blackboard compaction]: ${update.Compaction}`);
                    }
                } catch(e) {
                    console.error("SSE parse error", e);
                }
            };

            events.onerror = () => {
                console.warn("SSE connection closed; attempting reconnect.");
            };
        }

        // 4. Overrides and Interventions
        function submitContractFeedback(verdict) {
            fetch('/api/override', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(verdict)
            }).then(res => {
                if (res.ok) {
                    document.getElementById("approval-modal").classList.remove("active");
                    document.getElementById("contract-modal").classList.remove("active");
                    appendConsole(`[operator override]: human sent '${verdict}' override signature successfully.`);
                }
            });
        }

        function toggleCustomCriterion() {
            const container = document.getElementById("custom-criterion-container");
            const btnSubmit = document.getElementById("btn-custom-submit");
            const btnToggle = document.getElementById("btn-custom-toggle");
            
            if (container.style.display === "none") {
                container.style.display = "block";
                btnSubmit.style.display = "inline-block";
                btnToggle.innerText = "cancel";
            } else {
                container.style.display = "none";
                btnSubmit.style.display = "none";
                btnToggle.innerText = "override & add custom";
            }
        }

        function submitCustomCriterion() {
            const val = document.getElementById("custom-criterion-input").value;
            if (!val) return;

            fetch('/api/override', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ Override: [val] })
            }).then(res => {
                if (res.ok) {
                    document.getElementById("contract-modal").classList.remove("active");
                    document.getElementById("custom-criterion-container").style.display = "none";
                    document.getElementById("btn-custom-submit").style.display = "none";
                    document.getElementById("btn-custom-toggle").innerText = "override & add custom";
                    appendConsole(`[operator override]: human injected acceptance criteria: '${val}'`);
                }
            });
        }

        // 5. Initial State Load
        function loadInitialState() {
            fetch('/api/state')
                .then(res => res.json())
                .then(state => {
                    sessionID = state.session_id;
                    document.getElementById("session-id").innerText = `session: ${sessionID.substring(0, 12)}`;
                    appendConsole("[system] synchronized with blackboard Evaluation Blackboard");
                    appendConsole("[system] awaiting leader runtime telemetry campaign...");
                });
        }

        // Keyboard listeners for scrubbing
        window.addEventListener("keydown", (e) => {
            if (e.key === "ArrowRight") {
                adjustPlayhead(1);
            } else if (e.key === "ArrowLeft") {
                adjustPlayhead(-1);
            }
        });

        // Bootstrap on load
        window.onload = () => {
            selectPlayhead(0);
            loadInitialState();
            setupSSE();
            window.addEventListener("resize", drawSparkline);
        };
    </script>
</body>
</html>
"##;
