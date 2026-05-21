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
    cognition_mode: Arc<std::sync::Mutex<crate::leader::CognitionMode>>,
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
pub async fn run_web_with_campaign(
    prompt: String,
    session: Option<Uuid>,
    mode: Option<crate::leader::CognitionMode>,
) -> anyhow::Result<()> {
    let (tui_tx, mut tui_rx) = mpsc::channel::<TuiUpdate>(128);
    let (feedback_tx, feedback_rx) = mpsc::channel::<ContractResponse>(1);

    // 1. Create the broadcast channel for multi-subscriber SSE mapping
    let (broadcaster_tx, _) = broadcast::channel::<TuiUpdate>(256);

    let cognition_mode_arc = Arc::new(std::sync::Mutex::new(mode.unwrap_or(crate::leader::CognitionMode::Balanced)));

    // 2. Spawn the leader process campaign in the background
    let campaign_tx = tui_tx.clone();
    let cognition_mode_leader = cognition_mode_arc.clone();
    tokio::spawn(async move {
        let mut leader = LeaderOrchestrator::new(prompt, session);
        leader.tui_tx = Some(campaign_tx.clone());
        leader.tui_rx = Some(feedback_rx);
        leader.cognition_mode = cognition_mode_leader;

        let _ = leader.run_observable_campaign().await;

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        drop(campaign_tx);
    });

    // 3. Spawn a task to forward standard tui_tx (from leader) to the multi-client broadcaster
    let broadcaster_tx_clone = broadcaster_tx.clone();
    tokio::spawn(async move {
        while let Some(update) = tui_rx.recv().await {
            let mut update = update;
            let config = crate::llm::KorgConfig::load();
            if !config.security_vision.allow_raw_screenshots {
                if let TuiUpdate::Ktrans(ref mut s) = update {
                    if let Ok(mut ktrans) = serde_json::from_str::<serde_json::Value>(s) {
                        if let Some(attachments) = ktrans.get_mut("vision_attachments").and_then(|a| a.as_array_mut()) {
                            for att in attachments {
                                let verdict = att.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                                if verdict == "REDACTED" || verdict == "BLOCKED" {
                                    if let Some(data) = att.get_mut("data_base64") {
                                        *data = serde_json::Value::String(crate::vision_policy::BLACKOUT_PNG_BASE64.to_string());
                                    }
                                }
                            }
                        }
                        if let Ok(serialized) = serde_json::to_string(&ktrans) {
                            *s = serialized;
                        }
                    }
                }
            }
            let _ = broadcaster_tx_clone.send(update);
        }
    });

    // 4. Start the Axum web server on port 8080
    let app_state = Arc::new(AppState {
        broadcaster: broadcaster_tx,
        feedback_tx: Mutex::new(Some(feedback_tx)),
        cognition_mode: cognition_mode_arc,
    });

    let router = Router::new()
        .route("/", get(landing_handler))
        .route("/cockpit", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/api/events", get(sse_handler))
        .route("/api/state", get(state_handler))
        .route("/api/override", post(override_handler))
        .route("/api/mode", post(mode_handler))
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
    let cognition_mode_arc = leader.cognition_mode.clone();

    tokio::spawn(async move {
        let _ = leader.run_observable_campaign().await;
        drop(tui_tx);
    });

    let broadcaster_tx_clone = broadcaster_tx.clone();
    tokio::spawn(async move {
        while let Some(update) = tui_rx.recv().await {
            let mut update = update;
            let config = crate::llm::KorgConfig::load();
            if !config.security_vision.allow_raw_screenshots {
                if let TuiUpdate::Ktrans(ref mut s) = update {
                    if let Ok(mut ktrans) = serde_json::from_str::<serde_json::Value>(s) {
                        if let Some(attachments) = ktrans.get_mut("vision_attachments").and_then(|a| a.as_array_mut()) {
                            for att in attachments {
                                let verdict = att.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                                if verdict == "REDACTED" || verdict == "BLOCKED" {
                                    if let Some(data) = att.get_mut("data_base64") {
                                        *data = serde_json::Value::String(crate::vision_policy::BLACKOUT_PNG_BASE64.to_string());
                                    }
                                }
                            }
                        }
                        if let Ok(serialized) = serde_json::to_string(&ktrans) {
                            *s = serialized;
                        }
                    }
                }
            }
            let _ = broadcaster_tx_clone.send(update);
        }
    });

    let app_state = Arc::new(AppState {
        broadcaster: broadcaster_tx,
        feedback_tx: Mutex::new(Some(feedback_tx)),
        cognition_mode: cognition_mode_arc,
    });

    let router = Router::new()
        .route("/", get(landing_handler))
        .route("/cockpit", get(index_handler))
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

/// Serves the premium monochrome landing page
async fn landing_handler() -> impl IntoResponse {
    Html(LANDING_HTML)
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
async fn state_handler(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let mode = {
        let m = state.cognition_mode.lock().unwrap();
        format!("{:?}", *m)
    };
    let path = "/tmp/korg/blackboard/blackboard.json";
    if let Ok(content) = tokio::fs::read_to_string(path).await {
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(obj) = json.as_object_mut() {
                obj.insert("cognition_mode".to_string(), serde_json::json!(mode));
            }
            return Json(json);
        }
    }
    Json(serde_json::json!({
        "session_id": Uuid::now_v7().to_string(),
        "trace_buffer": [],
        "recent_pulses": [],
        "cognition_mode": mode,
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

#[derive(serde::Deserialize)]
struct ModeRequest {
    mode: String,
}

/// POST `/api/mode`
async fn mode_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ModeRequest>,
) -> impl IntoResponse {
    let mode = match payload.mode.to_lowercase().as_str() {
        "instant" => crate::leader::CognitionMode::Instant,
        "heavy" => crate::leader::CognitionMode::Heavy,
        "research" => crate::leader::CognitionMode::Research,
        "recovery" => crate::leader::CognitionMode::Recovery,
        "autonomous" => crate::leader::CognitionMode::Autonomous,
        _ => crate::leader::CognitionMode::Balanced,
    };
    
    {
        let mut m = state.cognition_mode.lock().unwrap();
        *m = mode;
    }
    
    println!("[Web API] Dynamic Cognition Mode updated to: {:?}", mode);
    
    // Broadcast trace event to live console log stream
    let _ = state.broadcaster.send(TuiUpdate::Trace(format!(
        "[cognition-mode] Dynamically switched active mode to: {:?}",
        mode
    )));
    
    axum::http::StatusCode::OK
}

// ============================================================================
// PREMIUM MONOCHROME LANDING PAGE
// ============================================================================
const LANDING_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>korg — autonomous engineering runtime</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600&family=Outfit:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;700&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg-base: #000000;
            --pane-bg: #09090b;
            --border-color: #27272a;
            --border-active: #ffffff;
            --text-primary: #ffffff;
            --text-secondary: #a1a1aa;
            --text-muted: #52525b;
            --font-sans: 'Inter', sans-serif;
            --font-heading: 'Outfit', sans-serif;
            --font-mono: 'JetBrains Mono', monospace;
            --accent-glow: rgba(255, 255, 255, 0.08);
            --accent-color: #ffffff;
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
            min-height: 100vh;
            overflow-x: hidden;
            display: flex;
            flex-direction: column;
            position: relative;
        }

        body::before {
            content: "";
            position: absolute;
            top: 0;
            left: 0;
            right: 0;
            height: 600px;
            background: radial-gradient(circle at 50% -100px, rgba(255, 255, 255, 0.12) 0%, rgba(255, 255, 255, 0.03) 40%, rgba(255, 255, 255, 0) 70%);
            pointer-events: none;
            z-index: 0;
        }

        body::after {
            content: "";
            position: absolute;
            inset: 0;
            background-image: radial-gradient(rgba(255, 255, 255, 0.05) 1px, transparent 1px);
            background-size: 24px 24px;
            pointer-events: none;
            z-index: 0;
            opacity: 0.7;
        }

        header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 24px 40px;
            border-bottom: 1px solid var(--border-color);
            background-color: rgba(0, 0, 0, 0.8);
            backdrop-filter: blur(12px);
            position: sticky;
            top: 0;
            z-index: 100;
        }

        .logo-container {
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .logo {
            font-family: var(--font-heading);
            font-size: 24px;
            font-weight: 800;
            letter-spacing: -0.04em;
            color: #ffffff;
            text-transform: lowercase;
        }

        .logo-sub {
            font-size: 11px;
            color: var(--text-muted);
            font-family: var(--font-mono);
            border-left: 1px solid var(--border-color);
            padding-left: 12px;
            letter-spacing: 0.05em;
        }

        .header-status {
            display: flex;
            align-items: center;
            gap: 8px;
            font-size: 11px;
            font-family: var(--font-mono);
            color: var(--text-secondary);
            border: 1px solid var(--border-color);
            padding: 6px 12px;
            border-radius: 4px;
            background: rgba(9, 9, 11, 0.5);
        }

        .status-dot {
            width: 6px;
            height: 6px;
            border-radius: 50%;
            background-color: #10b981;
            box-shadow: 0 0 8px #10b981;
            animation: pulse 2s infinite;
        }

        @keyframes pulse {
            0% { opacity: 0.4; }
            50% { opacity: 1; }
            100% { opacity: 0.4; }
        }

        main {
            flex-grow: 1;
            display: flex;
            flex-direction: column;
            align-items: center;
            padding: 80px 20px;
            max-width: 1200px;
            margin: 0 auto;
            width: 100%;
            position: relative;
            z-index: 1;
        }

        .hero-section {
            text-align: center;
            max-width: 850px;
            margin-bottom: 60px;
            animation: fadeIn 1s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .hero-badge {
            display: inline-flex;
            align-items: center;
            gap: 8px;
            font-family: var(--font-mono);
            font-size: 11px;
            background: rgba(255, 255, 255, 0.03);
            border: 1px solid rgba(255, 255, 255, 0.08);
            padding: 6px 16px;
            border-radius: 100px;
            margin-bottom: 24px;
            color: #ffffff;
            transition: all 0.3s ease;
        }

        .hero-badge:hover {
            border-color: rgba(255, 255, 255, 0.2);
            background: rgba(255, 255, 255, 0.05);
        }

        .hero-badge-dot {
            width: 6px;
            height: 6px;
            border-radius: 50%;
            background-color: #ffffff;
            box-shadow: 0 0 6px #ffffff;
        }

        .hero-title {
            font-family: var(--font-heading);
            font-size: 56px;
            font-weight: 800;
            letter-spacing: -0.03em;
            line-height: 1.05;
            margin-bottom: 24px;
            background: linear-gradient(180deg, #ffffff 0%, #a1a1aa 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }

        .hero-subtitle {
            font-size: 17px;
            color: var(--text-secondary);
            line-height: 1.6;
            font-weight: 300;
            margin-bottom: 36px;
            max-width: 700px;
            margin-left: auto;
            margin-right: auto;
        }

        .hero-ctas {
            display: flex;
            justify-content: center;
            gap: 16px;
        }

        .btn-primary {
            font-family: var(--font-sans);
            font-size: 13px;
            font-weight: 600;
            padding: 12px 28px;
            background-color: #ffffff;
            color: #000000;
            border: 1px solid #ffffff;
            border-radius: 6px;
            cursor: pointer;
            text-decoration: none;
            display: inline-flex;
            align-items: center;
            gap: 8px;
            transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
            box-shadow: 0 4px 12px rgba(255, 255, 255, 0.1);
        }

        .btn-primary:hover {
            background-color: #e4e4e7;
            border-color: #e4e4e7;
            transform: translateY(-1px);
            box-shadow: 0 8px 20px rgba(255, 255, 255, 0.15);
        }

        .btn-secondary {
            font-family: var(--font-sans);
            font-size: 13px;
            font-weight: 600;
            padding: 12px 28px;
            background-color: rgba(9, 9, 11, 0.5);
            color: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: 6px;
            cursor: pointer;
            text-decoration: none;
            display: inline-flex;
            align-items: center;
            gap: 8px;
            transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .btn-secondary:hover {
            border-color: rgba(255, 255, 255, 0.3);
            background-color: rgba(255, 255, 255, 0.05);
            transform: translateY(-1px);
        }

        /* Interactive Simulator */
        .simulator-section {
            width: 100%;
            max-width: 900px;
            margin-bottom: 100px;
            animation: fadeInUp 1s cubic-bezier(0.16, 1, 0.3, 1) 0.15s;
            animation-fill-mode: both;
        }

        .terminal-window {
            border: 1px solid var(--border-color);
            background-color: rgba(9, 9, 11, 0.7);
            border-radius: 8px;
            overflow: hidden;
            box-shadow: 0 20px 40px rgba(0, 0, 0, 0.8), 0 0 0 1px rgba(255, 255, 255, 0.05);
            backdrop-filter: blur(12px);
            display: flex;
            flex-direction: column;
            height: 380px;
        }

        .terminal-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 12px 20px;
            background-color: rgba(0, 0, 0, 0.4);
            border-bottom: 1px solid var(--border-color);
        }

        .terminal-dots {
            display: flex;
            gap: 6px;
        }

        .terminal-dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
        }
        .terminal-dot.red { background-color: #ef4444; }
        .terminal-dot.yellow { background-color: #f59e0b; }
        .terminal-dot.green { background-color: #10b981; }

        .terminal-title {
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-muted);
            letter-spacing: 0.05em;
        }

        .terminal-body {
            padding: 24px;
            font-family: var(--font-mono);
            font-size: 12px;
            line-height: 1.6;
            color: var(--text-secondary);
            overflow-y: auto;
            flex-grow: 1;
        }

        .terminal-controls {
            display: flex;
            gap: 12px;
            padding: 12px 20px;
            background-color: rgba(0, 0, 0, 0.3);
            border-top: 1px solid var(--border-color);
        }

        .sim-btn {
            font-family: var(--font-mono);
            font-size: 11px;
            padding: 6px 14px;
            background-color: rgba(255, 255, 255, 0.03);
            border: 1px solid var(--border-color);
            color: var(--text-secondary);
            border-radius: 4px;
            cursor: pointer;
            transition: all 0.2s;
        }

        .sim-btn:hover {
            border-color: #ffffff;
            color: #ffffff;
            background-color: rgba(255, 255, 255, 0.05);
        }

        .sim-btn.active {
            border-color: #ffffff;
            color: #000000;
            background-color: #ffffff;
        }

        /* Portals Grid */
        .portals-grid {
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 24px;
            width: 100%;
            margin-bottom: 100px;
            animation: fadeInUp 1s cubic-bezier(0.16, 1, 0.3, 1) 0.3s;
            animation-fill-mode: both;
        }

        .portal-card {
            border: 1px solid var(--border-color);
            background-color: rgba(9, 9, 11, 0.4);
            border-radius: 8px;
            padding: 32px;
            display: flex;
            flex-direction: column;
            gap: 20px;
            cursor: pointer;
            transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
            position: relative;
            text-decoration: none;
            color: inherit;
            backdrop-filter: blur(8px);
        }

        .portal-card::before {
            content: "";
            position: absolute;
            inset: 0;
            border-radius: 8px;
            padding: 1px;
            background: linear-gradient(to bottom, rgba(255, 255, 255, 0.08), rgba(255, 255, 255, 0));
            -webkit-mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
            -webkit-mask-composite: xor;
            mask-composite: exclude;
            pointer-events: none;
        }

        .portal-card:hover {
            border-color: rgba(255, 255, 255, 0.25);
            transform: translateY(-4px);
            background-color: rgba(12, 12, 16, 0.6);
            box-shadow: 0 20px 40px rgba(0, 0, 0, 0.5), 0 0 30px rgba(255, 255, 255, 0.02);
        }

        .portal-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .portal-icon {
            font-size: 24px;
            transition: transform 0.3s ease;
        }

        .portal-card:hover .portal-icon {
            transform: scale(1.1) rotate(5deg);
        }

        .portal-tag {
            font-family: var(--font-mono);
            font-size: 10px;
            color: var(--text-muted);
            border: 1px solid var(--border-color);
            padding: 4px 10px;
            border-radius: 100px;
            background: rgba(0, 0, 0, 0.3);
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }

        .portal-card:hover .portal-tag {
            border-color: rgba(255, 255, 255, 0.2);
            color: var(--text-secondary);
        }

        .portal-title {
            font-family: var(--font-heading);
            font-size: 20px;
            font-weight: 700;
            color: #ffffff;
            letter-spacing: -0.01em;
        }

        .portal-desc {
            font-size: 13px;
            color: var(--text-secondary);
            line-height: 1.6;
            flex-grow: 1;
            font-weight: 300;
        }

        .portal-action {
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-muted);
            text-transform: uppercase;
            transition: color 0.2s;
            display: inline-flex;
            align-items: center;
            gap: 6px;
        }

        .portal-card:hover .portal-action {
            color: #ffffff;
        }

        /* Matrix Specification Grid */
        .matrix-section {
            width: 100%;
            border-top: 1px solid var(--border-color);
            padding-top: 80px;
            animation: fadeInUp 1s cubic-bezier(0.16, 1, 0.3, 1) 0.4s;
            animation-fill-mode: both;
        }

        .matrix-title {
            font-family: var(--font-heading);
            font-size: 28px;
            font-weight: 700;
            color: #ffffff;
            margin-bottom: 12px;
            text-align: center;
            letter-spacing: -0.02em;
        }

        .matrix-subtitle {
            font-size: 14px;
            color: var(--text-secondary);
            text-align: center;
            margin-bottom: 48px;
            max-width: 600px;
            margin-left: auto;
            margin-right: auto;
            font-weight: 300;
        }

        .matrix-grid {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 32px;
        }

        .matrix-card {
            display: flex;
            flex-direction: column;
            gap: 12px;
            border: 1px solid var(--border-color);
            background: rgba(9, 9, 11, 0.2);
            padding: 24px;
            border-radius: 8px;
            transition: border-color 0.3s;
        }

        .matrix-card:hover {
            border-color: rgba(255, 255, 255, 0.15);
        }

        .matrix-card-title {
            font-family: var(--font-mono);
            font-size: 13px;
            font-weight: bold;
            color: #ffffff;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .matrix-card-title::before {
            content: "";
            display: inline-block;
            width: 6px;
            height: 6px;
            background-color: #ffffff;
            border-radius: 50%;
        }

        .matrix-card-text {
            font-size: 13px;
            color: var(--text-secondary);
            line-height: 1.6;
            font-weight: 300;
        }

        /* Footer */
        footer {
            border-top: 1px solid var(--border-color);
            padding: 40px;
            text-align: center;
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-muted);
            background-color: #000000;
            letter-spacing: 0.05em;
        }

        /* Modals style */
        .modal-overlay {
            position: fixed;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            background-color: rgba(0, 0, 0, 0.85);
            backdrop-filter: blur(8px);
            z-index: 1000;
            display: flex;
            justify-content: center;
            align-items: center;
            opacity: 0;
            pointer-events: none;
            transition: opacity 0.3s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .modal-overlay.active {
            opacity: 1;
            pointer-events: auto;
        }

        .modal-card {
            background-color: #09090b;
            border: 1px solid var(--border-color);
            border-radius: 12px;
            width: 600px;
            max-width: 90vw;
            padding: 36px;
            display: flex;
            flex-direction: column;
            gap: 24px;
            box-shadow: 0 30px 60px rgba(0, 0, 0, 0.8), 0 0 0 1px rgba(255, 255, 255, 0.05);
            transform: scale(0.95) translateY(10px);
            transition: transform 0.3s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .modal-overlay.active .modal-card {
            transform: scale(1) translateY(0);
        }

        .modal-title {
            font-family: var(--font-heading);
            font-size: 22px;
            font-weight: 700;
            color: #ffffff;
            letter-spacing: -0.01em;
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .modal-desc {
            font-size: 13px;
            color: var(--text-secondary);
            line-height: 1.6;
            font-weight: 300;
        }

        .terminal-box {
            display: flex;
            align-items: center;
            justify-content: space-between;
            background-color: #000000;
            border: 1px solid var(--border-color);
            border-radius: 6px;
            padding: 14px 18px;
            font-family: var(--font-mono);
            font-size: 12px;
            color: #ffffff;
        }

        .terminal-prompt {
            color: var(--text-muted);
            user-select: none;
            margin-right: 8px;
        }

        .terminal-command {
            flex-grow: 1;
            color: #ffffff;
        }

        .copy-btn {
            background: none;
            border: 1px solid var(--border-color);
            color: var(--text-secondary);
            font-family: var(--font-mono);
            font-size: 10px;
            padding: 4px 10px;
            border-radius: 4px;
            cursor: pointer;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            transition: all 0.2s;
        }

        .copy-btn:hover {
            border-color: #ffffff;
            color: #ffffff;
            background-color: rgba(255, 255, 255, 0.05);
        }

        .cli-details {
            display: flex;
            flex-direction: column;
            gap: 10px;
            font-family: var(--font-mono);
            font-size: 11px;
            border-top: 1px solid var(--border-color);
            padding-top: 18px;
        }

        .cli-detail-row {
            display: flex;
            justify-content: space-between;
        }

        .cli-detail-key {
            color: #ffffff;
            font-weight: bold;
        }

        .cli-detail-val {
            color: var(--text-secondary);
        }

        .modal-actions {
            display: flex;
            justify-content: flex-end;
            margin-top: 8px;
        }

        .btn-modal-close {
            font-family: var(--font-sans);
            font-size: 12px;
            font-weight: 600;
            padding: 10px 20px;
            background-color: #ffffff;
            color: #000000;
            border: 1px solid #ffffff;
            border-radius: 6px;
            cursor: pointer;
            transition: all 0.2s;
        }

        .btn-modal-close:hover {
            background-color: #e4e4e7;
            border-color: #e4e4e7;
        }

        /* Provenance Explorer Modal Layout */
        .modal-dag-layout {
            display: flex;
            flex-direction: column;
            gap: 20px;
        }

        .modal-dag-visual {
            border: 1px solid var(--border-color);
            background-color: #020203;
            border-radius: 6px;
            padding: 24px;
            display: flex;
            justify-content: center;
            align-items: center;
        }

        .mini-dag-svg {
            width: 100%;
            height: 80px;
        }

        .mini-edge {
            stroke: #27272a;
            stroke-width: 2;
            stroke-dasharray: 4 4;
        }

        .mini-node {
            cursor: pointer;
        }

        .mini-node circle {
            fill: #09090b;
            stroke: #27272a;
            stroke-width: 2;
            transition: all 0.2s;
        }

        .mini-node:hover circle {
            stroke: #ffffff;
            filter: drop-shadow(0 0 6px rgba(255, 255, 255, 0.3));
        }

        .mini-node.active circle {
            fill: #ffffff;
            stroke: #ffffff;
            filter: drop-shadow(0 0 8px rgba(255, 255, 255, 0.5));
        }

        .mini-node text {
            font-family: var(--font-mono);
            font-size: 10px;
            fill: var(--text-muted);
            text-anchor: middle;
            user-select: none;
            font-weight: 500;
        }

        .mini-node.active text {
            fill: #ffffff;
            font-weight: bold;
        }

        .modal-dag-properties {
            border: 1px solid var(--border-color);
            background-color: #040405;
            border-radius: 6px;
            padding: 20px;
            font-family: var(--font-mono);
        }

        .properties-header {
            font-size: 11px;
            color: var(--text-muted);
            text-transform: uppercase;
            letter-spacing: 0.05em;
            margin-bottom: 14px;
            border-bottom: 1px solid var(--border-color);
            padding-bottom: 8px;
        }

        .prop-table {
            display: flex;
            flex-direction: column;
            gap: 10px;
            font-size: 11px;
        }

        .prop-row {
            display: flex;
            align-items: flex-start;
        }

        .prop-key {
            width: 140px;
            color: var(--text-secondary);
            font-weight: bold;
        }

        .prop-val {
            flex-grow: 1;
            color: #ffffff;
            word-break: break-all;
        }

        /* Animations */
        @keyframes fadeIn {
            from { opacity: 0; transform: translateY(-10px); }
            to { opacity: 1; transform: translateY(0); }
        }

        @keyframes fadeInUp {
            from { opacity: 0; transform: translateY(20px); }
            to { opacity: 1; transform: translateY(0); }
        }
    </style>
</head>
<body>
    <header>
        <div class="logo-container">
            <span class="logo">korg</span>
            <span class="logo-sub">autonomous engineering runtime</span>
        </div>
        <div class="header-status">
            <span class="status-dot"></span>
            <span class="status-text">provenance node active</span>
        </div>
    </header>

    <main>
        <div class="hero-section">
            <div class="hero-badge">
                <span class="hero-badge-dot"></span>
                <span>korg v0.1.0 is now officially public</span>
            </div>
            <h1 class="hero-title">the autonomous software engineering runtime.</h1>
            <p class="hero-subtitle">
                A zero-trust multi-persona swarm environment speaking ACP, powered by content-addressed Merkle-DAG ledgers, adversarial sandbox verification, and enterprise-grade multi-modal vision policy firewalls.
            </p>
            <div class="hero-ctas">
                <a href="/cockpit" class="btn-primary">
                    <span>launch dashboard</span>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </a>
                <a href="#matrix" class="btn-secondary" onclick="document.getElementById('matrix').scrollIntoView({behavior: 'smooth'}); return false;">
                    <span>view specification</span>
                </a>
            </div>
        </div>

        <!-- Interactive Simulator -->
        <div class="simulator-section">
            <div class="terminal-window">
                <div class="terminal-header">
                    <div class="terminal-dots">
                        <span class="terminal-dot red"></span>
                        <span class="terminal-dot yellow"></span>
                        <span class="terminal-dot green"></span>
                    </div>
                    <span class="terminal-title">korg_interactive_simulation.sh</span>
                    <span style="width: 40px;"></span>
                </div>
                <div class="terminal-body" id="term-output">
                    <!-- Typing simulation content goes here -->
                </div>
                <div class="terminal-controls">
                    <button class="sim-btn active" id="btn-sim-run" onclick="startCampaignSim()">Run Swarm Sandbox</button>
                    <button class="sim-btn" id="btn-sim-policy" onclick="startPolicySim()">Verify OCR Redaction</button>
                    <button class="sim-btn" id="btn-sim-dag" onclick="startDagSim()">Inspect Merkle replay</button>
                </div>
            </div>
        </div>

        <div class="portals-grid">
            <a href="/cockpit" class="portal-card">
                <div class="portal-header">
                    <span class="portal-icon">⚡</span>
                    <span class="portal-tag">live dashboard</span>
                </div>
                <h3 class="portal-title">enter swarm dashboard</h3>
                <p class="portal-desc">Observe live multi-persona agent execution streams, check real-time OCR visual intercepts, and authorize manual plan overrides.</p>
                <span class="portal-action">
                    <span>launch stream</span>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </span>
            </a>
            
            <div class="portal-card" onclick="openCliModal()">
                <div class="portal-header">
                    <span class="portal-icon">🖥️</span>
                    <span class="portal-tag">cli engine</span>
                </div>
                <h3 class="portal-title">run campaign via cli</h3>
                <p class="portal-desc">Initiate highly isolated autonomous campaigns directly from your terminal. Full local workspace isolation and rollback support.</p>
                <span class="portal-action">
                    <span>reveal schema</span>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </span>
            </div>
            
            <div class="portal-card" onclick="openDagModal()">
                <div class="portal-header">
                    <span class="portal-icon">⛓️</span>
                    <span class="portal-tag">provenance</span>
                </div>
                <h3 class="portal-title">verify provenance trace</h3>
                <p class="portal-desc">Audit the cryptographic attestation chain. Verify content-addressed Merkle hashes and ed25519 system signature paths.</p>
                <span class="portal-action">
                    <span>execute verification</span>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </span>
            </div>
        </div>

        <div class="matrix-section" id="matrix">
            <h2 class="matrix-title">runtime specification matrix</h2>
            <p class="matrix-subtitle">Every building block of Korg is engineered for deterministic, high-assurance software synthesis.</p>
            <div class="matrix-grid">
                <div class="matrix-card">
                    <div class="matrix-card-title">merkle-dag ledger</div>
                    <p class="matrix-card-text">Every execution tick serializes the codebase and active blackboard state into content-addressed blobs. Replayable and cryptographically tamper-proof.</p>
                </div>
                <div class="matrix-card">
                    <div class="matrix-card-title">zero-trust visual policy</div>
                    <p class="matrix-card-text">Real-time visual pattern/OCR checking on captured screenshots prevents prod secrets leaks. Supports blur, blackout, and manual operator overrides.</p>
                </div>
                <div class="matrix-card">
                    <div class="matrix-card-title">adversarial arenas</div>
                    <p class="matrix-card-text">Multi-persona worker swarms validate changes across five adversarial rubrics before committing, utilizing semantic entropy and semantic merges.</p>
                </div>
                <div class="matrix-card">
                    <div class="matrix-card-title">functional sandboxing</div>
                    <p class="matrix-card-text">Isolates runtime actions using temporary git worktrees. Eliminates local state contamination and ensures clean rollback on plan validation failures.</p>
                </div>
            </div>
        </div>
    </main>

    <footer>
        korg v0.1.0 — autonomous software engineering runtime — cryptographically secure
    </footer>

    <!-- CLI Guide Modal -->
    <div class="modal-overlay" id="cli-modal" onclick="if(event.target === this) closeCliModal()">
        <div class="modal-card">
            <div class="modal-title">
                <span>🖥️ run campaign via cli</span>
            </div>
            <p class="modal-desc">Execute Korg campaigns directly from your system shell. Copy the command below to start an interactive visual campaign:</p>
            <div class="terminal-box">
                <span class="terminal-prompt">$</span> 
                <span class="terminal-command" id="cmd-text">korg campaign --web --prompt "Refactor authentication layer"</span>
                <button class="copy-btn" onclick="copyCliCommand()">copy</button>
            </div>
            <div class="cli-details">
                <div class="cli-detail-row">
                    <span class="cli-detail-key">--web</span>
                    <span class="cli-detail-val">Launches real-time event visualization in the browser</span>
                </div>
                <div class="cli-detail-row">
                    <span class="cli-detail-key">--headless</span>
                    <span class="cli-detail-val">Runs campaign purely inside stdout without GUI hooks</span>
                </div>
                <div class="cli-detail-row">
                    <span class="cli-detail-key">--tui</span>
                    <span class="cli-detail-val">Launches Ratatui-based interactive terminal dashboard</span>
                </div>
            </div>
            <div class="modal-actions">
                <button class="btn-modal-close" onclick="closeCliModal()">close</button>
            </div>
        </div>
    </div>

    <!-- Provenance Modal -->
    <div class="modal-overlay" id="dag-modal" onclick="if(event.target === this) closeDagModal()">
        <div class="modal-card">
            <div class="modal-title">
                <span>⛓️ provenance trace audit verifier</span>
            </div>
            <p class="modal-desc">Cryptographically verify the content-addressed chain of custody from genesis state through compilation release.</p>
            
            <div class="modal-dag-layout">
                <div class="modal-dag-visual">
                    <svg width="420" height="80" id="mini-dag-svg">
                        <line x1="50" y1="40" x2="130" y2="40" class="mini-edge"></line>
                        <line x1="130" y1="40" x2="210" y2="40" class="mini-edge"></line>
                        <line x1="210" y1="40" x2="290" y2="40" class="mini-edge"></line>
                        <line x1="290" y1="40" x2="370" y2="40" class="mini-edge"></line>
                        <g class="mini-node active" id="mn-0" onclick="selectMiniNode(0)"><circle cx="50" cy="40" r="14"></circle><text x="50" y="44">tx_0</text></g>
                        <g class="mini-node" id="mn-1" onclick="selectMiniNode(1)"><circle cx="130" cy="40" r="14"></circle><text x="130" y="44">tx_1</text></g>
                        <g class="mini-node" id="mn-2" onclick="selectMiniNode(2)"><circle cx="210" cy="40" r="14"></circle><text x="210" y="44">tx_2</text></g>
                        <g class="mini-node" id="mn-3" onclick="selectMiniNode(3)"><circle cx="290" cy="40" r="14"></circle><text x="290" y="44">tx_3</text></g>
                        <g class="mini-node" id="mn-4" onclick="selectMiniNode(4)"><circle cx="370" cy="40" r="14"></circle><text x="370" y="44">tx_4</text></g>
                    </svg>
                </div>
                <div class="modal-dag-properties">
                    <h4 class="properties-header">node attributes</h4>
                    <div class="prop-table" id="prop-table-body">
                        <!-- Filled dynamically -->
                    </div>
                </div>
            </div>

            <div class="modal-actions">
                <button class="btn-modal-close" onclick="closeDagModal()">close</button>
            </div>
        </div>
    </div>

    <script>
        // Modal functions
        function openCliModal() { document.getElementById("cli-modal").classList.add("active"); }
        function closeCliModal() { document.getElementById("cli-modal").classList.remove("active"); }
        function openDagModal() { document.getElementById("dag-modal").classList.add("active"); selectMiniNode(0); }
        function closeDagModal() { document.getElementById("dag-modal").classList.remove("active"); }
        
        function copyCliCommand() {
            const text = document.getElementById("cmd-text").innerText;
            navigator.clipboard.writeText(text).then(() => {
                const btn = document.querySelector(".copy-btn");
                btn.innerText = "copied!";
                setTimeout(() => { btn.innerText = "copy"; }, 2000);
            });
        }

        // Mini DAG mock database
        const miniDagDb = [
            {
                tx: "tx_00 (genesis)",
                type: "SYSTEM_GENESIS",
                hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                signature: "ed25519::verified [8f3c29a2b7e5c4...]",
                state_root: "a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2 [verified ✓]",
                status: "attested & finalized"
            },
            {
                tx: "tx_01 (plan)",
                type: "PLAN_FORMULATION",
                hash: "6d2d46e3ea406fb2b18ea24bfbd54f97155e8c1cf9e1d8820cf67ef8fc8a385f",
                signature: "ed25519::verified [4a7d3b2e5f1c9a...]",
                state_root: "f8e7d6c5b4a39281706f5e4d3c2b1a0f [verified ✓]",
                status: "approved by operator"
            },
            {
                tx: "tx_02 (code)",
                type: "WORKSPACE_SYNTHESIS",
                hash: "5f82c4f1e312a02b1f8d4239824bfbd54f97155e8c1cf9e1d8820cf67ef8fc8a3",
                signature: "ed25519::verified [3c2b9a8d7e5f4a...]",
                state_root: "b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9 [verified ✓]",
                status: "adversarial suite green"
            },
            {
                tx: "tx_03 (policy)",
                type: "POLICY_REDISTRIBUTION",
                hash: "4e91a7c3b2e5f1c9a8b7d6e5c4b3a2b1e3f4a5b6c7d8e9a0b1c2d3e4f5a6b7c8",
                signature: "ed25519::contested [9a8c7b6d5e4f3a...]",
                state_root: "d8e7c6b5a4938271605f4e3d2c1b0a9f [redacted ✓]",
                status: "intercepted & redacted"
            },
            {
                tx: "tx_04 (release)",
                type: "RELEASE_COMMIT",
                hash: "9f3c2b8a7d5e4f3c2b1a0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a",
                signature: "ed25519::verified [3c2b7a9f8e7d6c...]",
                state_root: "e3f2d1c0b9a876543210fedcba987654 [verified ✓]",
                status: "production build active"
            }
        ];

        function selectMiniNode(idx) {
            document.querySelectorAll(".mini-node").forEach(n => n.classList.remove("active"));
            document.getElementById(`mn-${idx}`).classList.add("active");
            
            const data = miniDagDb[idx];
            const body = document.getElementById("prop-table-body");
            body.innerHTML = `
                <div class="prop-table">
                    <div class="prop-row">
                        <div class="prop-key">Transaction ID</div>
                        <div class="prop-val">${data.tx}</div>
                    </div>
                    <div class="prop-row">
                        <div class="prop-key">Event Type</div>
                        <div class="prop-val" style="color: #ffffff; font-weight: bold;">${data.type}</div>
                    </div>
                    <div class="prop-row">
                        <div class="prop-key">Merkle Hash</div>
                        <div class="prop-val" style="color: var(--text-secondary); font-family: var(--font-mono);">${data.hash}</div>
                    </div>
                    <div class="prop-row">
                        <div class="prop-key">Attestation</div>
                        <div class="prop-val" style="color: #10b981;">${data.signature}</div>
                    </div>
                    <div class="prop-row">
                        <div class="prop-key">State Root</div>
                        <div class="prop-val">${data.state_root}</div>
                    </div>
                    <div class="prop-row">
                        <div class="prop-key">Status</div>
                        <div class="prop-val" style="color: ${data.tx.includes('tx_03') ? '#f59e0b' : '#ffffff'}; font-weight: 500;">${data.status}</div>
                    </div>
                </div>
            `;
        }

        // Terminal Interactive Simulator Logic
        let simInterval = null;
        const termElement = document.getElementById("term-output");

        const simulatorScripts = {
            run: [
                { type: "input", text: "korg campaign --web --prompt \"Refactor authentication database connection pool\"" },
                { type: "output", text: "[korg] Initializing campaign environment...", color: "#a1a1aa" },
                { type: "output", text: "[korg] Creating transient isolation sandbox (git worktree)...", color: "#a1a1aa" },
                { type: "output", text: "[korg] Sandbox created at: /tmp/korg-worktree-a8f3", color: "#52525b" },
                { type: "output", text: "[korg] Spawning autonomous swarm (3 personas active):", color: "#ffffff" },
                { type: "output", text: "   ▸ [architect] Designing execution layout...", color: "#a1a1aa" },
                { type: "output", text: "   ▸ [coder] Generating patch for src/db.rs...", color: "#a1a1aa" },
                { type: "output", text: "   ▸ [tester] Synthesizing adversarial verification suite...", color: "#a1a1aa" },
                { type: "output", text: "[korg] Patch formulated. Running adversarial test suite...", color: "#ffffff" },
                { type: "output", text: "   ✔ Compile check: GREEN (took 1.2s)", color: "#10b981" },
                { type: "output", text: "   ✔ Unit tests (8/8): GREEN", color: "#10b981" },
                { type: "output", text: "   ✔ Adversarial Security Scan: CLEAN", color: "#10b981" },
                { type: "output", text: "[korg] Swarm verification complete. Generating Merkle-DAG attestation...", color: "#ffffff" },
                { type: "output", text: "[korg] Attestation tx_02 written to cryptographic ledger.", color: "#10b981" },
                { type: "output", text: "[korg] Campaign successfully finalized! Ready for deployment.", color: "#ffffff" }
            ],
            policy: [
                { type: "input", text: "korg campaign --verify-vision-policy" },
                { type: "output", text: "[policy-engine] Booting zero-trust visual intercept interceptor...", color: "#a1a1aa" },
                { type: "output", text: "[policy-engine] Monitoring active workspace GUI state...", color: "#a1a1aa" },
                { type: "output", text: "[policy-engine] Screenshot triggered by tester persona.", color: "#ffffff" },
                { type: "output", text: "[policy-engine] Processing screenshot_382.png through vision firewall...", color: "#ffffff" },
                { type: "output", text: "   ▸ Scanning metadata and OCR layers...", color: "#52525b" },
                { type: "output", text: "   ⚠ VIOLATION DETECTED: Found string pattern 'DATABASE_PASSWORD=********' in visual OCR buffer!", color: "#ef4444" },
                { type: "output", text: "   ⚠ FAIL-SECURE POLICY ACTIVATED: Triggering zero-trust filter.", color: "#ef4444" },
                { type: "output", text: "[policy-engine] Redacting raw screenshot in memory...", color: "#ffffff" },
                { type: "output", text: "   ▸ Method: Grayscale Overlay + Total Blur redaction", color: "#a1a1aa" },
                { type: "output", text: "   ✔ Screenshot redacted. Safe base64 broadcast emitted.", color: "#10b981" },
                { type: "output", text: "[policy-engine] Attestation tx_03 recorded: OCR_VIOLATION_AUTO_REDACTED", color: "#10b981" },
                { type: "output", text: "[policy-engine] No raw sensitive credentials escaped the sandbox.", color: "#ffffff" }
            ],
            dag: [
                { type: "input", text: "korg dag log --tx tx_04" },
                { type: "output", text: "[korg-dag] Content-Addressed Merkle ledger audit trace:", color: "#ffffff" },
                { type: "output", text: "--------------------------------------------------------", color: "#52525b" },
                { type: "output", text: "Transaction: tx_04", color: "#ffffff" },
                { type: "output", text: "Parent Hash: e3b0c44298fc1c149afbf4c8996fb92427ae41e4...", color: "#a1a1aa" },
                { type: "output", text: "State Root:  9f3c2b8a7d5e4f3c2b1a0d9e8f7a6b5c4d3e2f1a...", color: "#a1a1aa" },
                { type: "output", text: "Signature:   ed25519::verified [attester: leader_primary]", color: "#10b981" },
                { type: "output", text: "Payload Type: RELEASE_COMMIT", color: "#ffffff" },
                { type: "output", text: "Diff Attestation:", color: "#a1a1aa" },
                { type: "output", text: "   + modified: src/web.rs (monochrome layout upgrade)", color: "#10b981" },
                { type: "output", text: "   + verified: adversarial-arena compiler passes", color: "#10b981" },
                { type: "output", text: "Cryptographic Attestation Chain: VALID", color: "#10b981" },
                { type: "output", text: "   Genesis (tx_00) ➔ Plan (tx_01) ➔ Synthesis (tx_02) ➔ Intercept (tx_03) ➔ Release (tx_04)", color: "#ffffff" }
            ]
        };

        function setSimButtonActive(simId) {
            document.querySelectorAll(".sim-btn").forEach(btn => btn.classList.remove("active"));
            if (simId === "run") document.getElementById("btn-sim-run").classList.add("active");
            if (simId === "policy") document.getElementById("btn-sim-policy").classList.add("active");
            if (simId === "dag") document.getElementById("btn-sim-dag").classList.add("active");
        }

        function runSimScript(script) {
            if (simInterval) clearInterval(simInterval);
            termElement.innerHTML = "";
            let lineIndex = 0;
            
            function printNextLine() {
                if (lineIndex >= script.length) return;
                
                const line = script[lineIndex];
                const div = document.createElement("div");
                
                if (line.type === "input") {
                    div.innerHTML = `<span style="color: var(--text-muted); user-select: none;">$</span> <span style="color: #ffffff;"></span>`;
                    termElement.appendChild(div);
                    
                    // Typewriter effect for input line
                    let charIndex = 0;
                    const textSpan = div.querySelector("span:nth-child(2)");
                    
                    const typeInterval = setInterval(() => {
                        if (charIndex < line.text.length) {
                            textSpan.textContent += line.text[charIndex];
                            charIndex++;
                        } else {
                            clearInterval(typeInterval);
                            lineIndex++;
                            setTimeout(printNextLine, 500);
                        }
                    }, 20);
                } else {
                    div.style.color = line.color || "var(--text-secondary)";
                    div.textContent = line.text;
                    termElement.appendChild(div);
                    termElement.scrollTop = termElement.scrollHeight;
                    lineIndex++;
                    setTimeout(printNextLine, 400);
                }
            }
            
            printNextLine();
        }

        function startCampaignSim() {
            setSimButtonActive("run");
            runSimScript(simulatorScripts.run);
        }

        function startPolicySim() {
            setSimButtonActive("policy");
            runSimScript(simulatorScripts.policy);
        }

        function startDagSim() {
            setSimButtonActive("dag");
            runSimScript(simulatorScripts.dag);
        }

        // Run default simulation on page load
        window.addEventListener("DOMContentLoaded", () => {
            startCampaignSim();
        });
    </script>
</body>
</html>
"##;

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

        .btn-warning {
            border-color: #332200;
            color: #ffaa00;
        }

        .btn-warning:hover {
            background-color: #ffaa00;
            color: #000000;
            border-color: #ffaa00;
        }

        /* PREMIUM MONOCHROME COGNITION MODE SELECTOR STYLE */
        .cognition-selector-container {
            display: flex;
            align-items: center;
            gap: 10px;
            background-color: #0c0c0e;
            border: 1px solid #27272a;
            padding: 3px 6px;
            border-radius: 6px;
        }

        .mode-pulse-dot {
            width: 8px;
            height: 8px;
            border-radius: 50%;
            background-color: #a1a1aa; /* silver */
            margin-left: 6px;
            transition: background-color 0.4s ease, box-shadow 0.4s ease;
        }

        /* Pulsating depth pulses for different modes */
        .mode-pulse-dot.instant {
            background-color: #38bdf8;
            box-shadow: 0 0 8px #38bdf8;
            animation: pulse-blue 2s infinite;
        }
        .mode-pulse-dot.balanced {
            background-color: #a1a1aa;
            box-shadow: 0 0 6px #a1a1aa;
            animation: pulse-silver 2s infinite;
        }
        .mode-pulse-dot.heavy {
            background-color: #a855f7;
            box-shadow: 0 0 10px #a855f7;
            animation: pulse-purple 2s infinite;
        }
        .mode-pulse-dot.research {
            background-color: #22c55e;
            box-shadow: 0 0 8px #22c55e;
            animation: pulse-green 2s infinite;
        }
        .mode-pulse-dot.recovery {
            background-color: #ef4444;
            box-shadow: 0 0 12px #ef4444;
            animation: pulse-red 2s infinite;
        }
        .mode-pulse-dot.autonomous {
            background-color: #f59e0b;
            box-shadow: 0 0 10px #f59e0b;
            animation: pulse-gold 2s infinite;
        }

        @keyframes pulse-blue {
            0% { opacity: 0.4; } 50% { opacity: 1; } 100% { opacity: 0.4; }
        }
        @keyframes pulse-silver {
            0% { opacity: 0.4; } 50% { opacity: 1; } 100% { opacity: 0.4; }
        }
        @keyframes pulse-purple {
            0% { opacity: 0.4; } 50% { opacity: 1; } 100% { opacity: 0.4; }
        }
        @keyframes pulse-green {
            0% { opacity: 0.4; } 50% { opacity: 1; } 100% { opacity: 0.4; }
        }
        @keyframes pulse-red {
            0% { opacity: 0.4; } 50% { opacity: 1; } 100% { opacity: 0.4; }
        }
        @keyframes pulse-gold {
            0% { opacity: 0.4; } 50% { opacity: 1; } 100% { opacity: 0.4; }
        }

        .cognition-selector {
            display: flex;
            gap: 2px;
        }

        .mode-btn {
            background: transparent;
            border: 1px solid transparent;
            color: #71717a;
            font-family: var(--font-sans);
            font-size: 11px;
            font-weight: 500;
            padding: 4px 10px;
            border-radius: 4px;
            cursor: pointer;
            text-transform: lowercase;
            transition: all 0.2s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .mode-btn:hover {
            color: #ffffff;
            background-color: #18181b;
        }

        .mode-btn.active {
            color: #ffffff;
            background-color: #27272a;
            border-color: #3f3f46;
        }
    </style>
</head>
<body>
    <header>
        <div class="logo-container">
            <span class="logo">korg</span>
            <span class="logo-sub">autonomous engineering runtime</span>
        </div>

        <div class="cognition-selector-container">
            <div class="mode-pulse-dot balanced" id="mode-pulse"></div>
            <div class="cognition-selector">
                <button class="mode-btn" data-mode="instant" onclick="setCognitionMode('instant')">instant</button>
                <button class="mode-btn active" data-mode="balanced" onclick="setCognitionMode('balanced')">balanced</button>
                <button class="mode-btn" data-mode="heavy" onclick="setCognitionMode('heavy')">heavy</button>
                <button class="mode-btn" data-mode="research" onclick="setCognitionMode('research')">research</button>
                <button class="mode-btn" data-mode="recovery" onclick="setCognitionMode('recovery')">recovery</button>
                <button class="mode-btn" data-mode="autonomous" onclick="setCognitionMode('autonomous')">autonomous</button>
            </div>
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
                <button class="btn btn-primary" id="btn-approve-raw" onclick="submitContractFeedback('Approve')">approve execution</button>
                <button class="btn btn-warning" id="btn-approve-redacted" style="display: none;" onclick="submitContractFeedback('Force')">approve redacted</button>
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

    <!-- Playhead Fork Modal -->
    <div class="modal-overlay" id="fork-modal">
        <div class="modal-card">
            <div class="modal-title">🍴 playhead steering & workspace fork</div>
            <div class="modal-desc">
                you are about to fork the swarm execution back to transaction <span id="fork-modal-tx" style="font-weight: bold; color: #fff;">tx_00</span>.
                <p style="margin-top: 8px; font-size: 11px; color: var(--text-secondary);">
                    this will physically revert your workspace codebase (via git tree) and logically rehydrate the blackboard to this point.
                </p>
                <div class="modal-input-container" style="margin-top: 12px;">
                    <div class="modal-label" style="font-family: var(--font-mono); font-size: 10px; margin-bottom: 4px; color: var(--text-secondary);">provide steering directive for the new branch:</div>
                    <input type="text" class="modal-input" id="fork-directive-input" placeholder="e.g., focus on robust parser rules">
                </div>
            </div>
            <div class="modal-actions">
                <button class="btn btn-primary" onclick="submitFork()">execute fork</button>
                <button class="btn" onclick="closeForkModal()">cancel</button>
            </div>
        </div>
    </div>

    <script>
        // Core Web App State
        let playhead = 0;
        let maxPlayhead = 5;
        let entropyHistory = [];
        let sessionID = 'initializing...';

        // Dynamic Nodes and Edges for the Merkle-DAG Graph
        let dagNodes = [
            { id: 0, label: 'tx_00: genesis', desc: 'orchestration', x: 80, y: 120, tx_hash: 'genesis', parent_hashes: [], state_merkle_root: 'sha256:genesis', codebase_merkle_root: 'sha256:genesis' },
            { id: 1, label: 'tx_01: negotiate_contract', desc: 'orchestration', x: 180, y: 80 },
            { id: 2, label: 'tx_02: dispatch_concurrent', desc: 'worker', x: 280, y: 80 },
            { id: 3, label: 'tx_03: generate_patch', desc: 'worker', x: 380, y: 120 },
            { id: 4, label: 'tx_04: evaluate_verdict', desc: 'evaluator', x: 480, y: 120 },
            { id: 5, label: 'tx_05: operator_steer', desc: 'operator', x: 520, y: 180 }
        ];

        let edges = [
            { from: 0, to: 1 },
            { from: 1, to: 2 },
            { from: 2, to: 3 },
            { from: 3, to: 4 },
            { from: 4, to: 5 },
            { from: 0, to: 5 }
        ];

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

        // Static Nodes replaced by global dynamic nodes

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

            edges.forEach(edge => {
                const nodeFrom = dagNodes.find(n => n.id === edge.from);
                const nodeTo = dagNodes.find(n => n.id === edge.to);
                if (!nodeFrom || !nodeTo) return;
                
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

            // Update provenance roots from dynamic transaction nodes if present
            const selectedNode = dagNodes.find(n => n.id === playhead);
            if (selectedNode && selectedNode.state_merkle_root) {
                const shortRoot = selectedNode.state_merkle_root.length > 12 
                    ? selectedNode.state_merkle_root.substring(0, 12) + "..."
                    : selectedNode.state_merkle_root;
                document.getElementById("merkle-root").innerText = shortRoot;
                document.getElementById("merkle-root").title = selectedNode.state_merkle_root;
                
                // Draw dynamic "steering fork" action in provenance pane
                const actionRowId = "provenance-fork-action-row";
                let actionRow = document.getElementById(actionRowId);
                if (!actionRow) {
                    actionRow = document.createElement("div");
                    actionRow.className = "prov-row";
                    actionRow.id = actionRowId;
                    document.getElementById("provenance-details").appendChild(actionRow);
                }
                
                if (playhead > 0) {
                    actionRow.innerHTML = `
                        <div class="prov-key">steering fork</div>
                        <div class="prov-val">
                            <a href="#" onclick="openForkModal(${playhead}); return false;" style="color: #ff5555; font-weight: bold; text-decoration: underline;">Fork from Round ${playhead}</a>
                        </div>
                    `;
                } else {
                    actionRow.innerHTML = `
                        <div class="prov-key">steering fork</div>
                        <div class="prov-val" style="color: var(--text-muted);">genesis cannot be forked</div>
                    `;
                }
            }
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
                        if (update.Trace.includes("[cognition-mode]")) {
                            const parts = update.Trace.split("to:");
                            if (parts.length > 1) {
                                const modeName = parts[1].trim();
                                updateModeUI(modeName);
                            }
                        } else if (update.Trace.includes("[cognition-escalation]")) {
                            updateModeUI("heavy");
                        }
                    } else if (update.Ktrans) {
                        appendProvenance(update.Ktrans);
                        try {
                            const ktrans = JSON.parse(update.Ktrans);
                            if (ktrans && typeof ktrans.round === 'number') {
                                if (sessionID === 'initializing...' || sessionID !== ktrans.session_id) {
                                    sessionID = ktrans.session_id;
                                    dagNodes = [];
                                    edges = [];
                                    maxPlayhead = ktrans.round > 5 ? ktrans.round : 5;
                                }
                                
                                let existingNode = dagNodes.find(n => n.id === ktrans.round);
                                if (!existingNode) {
                                    let x = 80 + ktrans.round * 100;
                                    let y = 120;
                                    
                                    if (ktrans.round % 2 !== 0 && ktrans.round > 0) {
                                        y = 80;
                                    } else if (ktrans.round % 3 === 0 && ktrans.round > 0) {
                                        y = 160;
                                    }
                                    
                                    let label = `tx_0${ktrans.round}: ${ktrans.leader_action || ktrans.arena_winner || 'steer'}`;
                                    let desc = ktrans.arena_winner || 'orchestration';
                                    
                                    dagNodes.push({
                                        id: ktrans.round,
                                        label: label,
                                        desc: desc,
                                        x: x,
                                        y: y,
                                        tx_hash: ktrans.tx_hash,
                                        parent_hashes: ktrans.parent_hashes || [],
                                        state_merkle_root: ktrans.state_merkle_root,
                                        codebase_merkle_root: ktrans.codebase_merkle_root
                                    });
                                    
                                    if (ktrans.round > maxPlayhead) {
                                        maxPlayhead = ktrans.round;
                                    }
                                    
                                    if (ktrans.parent_hashes && ktrans.parent_hashes.length > 0) {
                                        ktrans.parent_hashes.forEach(pHash => {
                                            let parentNode = dagNodes.find(n => n.tx_hash === pHash);
                                            if (parentNode) {
                                                edges.push({ from: parentNode.id, to: ktrans.round });
                                            } else {
                                                edges.push({ from: ktrans.round - 1, to: ktrans.round });
                                            }
                                        });
                                    } else if (ktrans.round > 0) {
                                        edges.push({ from: ktrans.round - 1, to: ktrans.round });
                                    }
                                    
                                    drawDag();
                                }
                            }
                        } catch (e) {
                            console.error("Error parsing dynamic Ktrans transaction:", e);
                        }
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
                        const isSecurity = reason.includes("Security Policy Blocked!");
                        document.getElementById("approval-modal-desc").innerText = reason;
                        
                        const titleEl = document.querySelector("#approval-modal .modal-title");
                        const btnRaw = document.getElementById("btn-approve-raw");
                        const btnRedacted = document.getElementById("btn-approve-redacted");
                        
                        if (isSecurity) {
                            titleEl.innerHTML = "🚨 zero-trust security policy intercept";
                            btnRaw.innerText = "force override & approve raw";
                            btnRedacted.style.display = "inline-block";
                        } else {
                            titleEl.innerHTML = "🔒 human security approval gate";
                            btnRaw.innerText = "approve execution";
                            btnRedacted.style.display = "none";
                        }
                        
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

        let activeForkTx = null;

        function openForkModal(txId) {
            activeForkTx = txId;
            document.getElementById("fork-modal-tx").innerText = `tx_0${txId}`;
            document.getElementById("fork-directive-input").value = "";
            document.getElementById("fork-modal").classList.add("active");
        }

        function closeForkModal() {
            document.getElementById("fork-modal").classList.remove("active");
            activeForkTx = null;
        }

        function submitFork() {
            if (activeForkTx === null) return;
            const directive = document.getElementById("fork-directive-input").value || "focus on robustness";
            const forkStr = `FORK:${activeForkTx}:${directive}`;
            
            fetch('/api/override', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ Override: [forkStr] })
            }).then(res => {
                if (res.ok) {
                    closeForkModal();
                    appendConsole(`[operator fork]: triggered playhead steering fork at tx_0${activeForkTx} with directive: '${directive}'`);
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
                    
                    if (state.cognition_mode) {
                        updateModeUI(state.cognition_mode);
                    }
                    
                    appendConsole("[system] synchronized with blackboard Evaluation Blackboard");
                    appendConsole("[system] awaiting leader runtime telemetry campaign...");
                });
        }

        function updateModeUI(mode) {
            if (!mode) return;
            const lowercaseMode = mode.toLowerCase();
            
            document.querySelectorAll(".mode-btn").forEach(btn => {
                if (btn.getAttribute("data-mode") === lowercaseMode) {
                    btn.classList.add("active");
                } else {
                    btn.classList.remove("active");
                }
            });
            
            const pulse = document.getElementById("mode-pulse");
            if (pulse) {
                pulse.className = "mode-pulse-dot";
                pulse.classList.add(lowercaseMode);
            }
        }

        function setCognitionMode(mode) {
            updateModeUI(mode);
            fetch('/api/mode', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify({ mode: mode })
            })
            .then(res => {
                if (!res.ok) {
                    console.error("Failed to update cognition mode");
                }
            })
            .catch(err => console.error("Error setting cognition mode:", err));
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
