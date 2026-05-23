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
    capability_resolver: Arc<tokio::sync::Mutex<crate::registry::CapabilityResolver>>,
    runtime_coordinator: Arc<std::sync::Mutex<Option<Arc<crate::runtime::RuntimeCoordinator>>>>,
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
    mode: Option<&str>,
) -> anyhow::Result<()> {
    let (tui_tx, mut tui_rx) = mpsc::channel::<TuiUpdate>(128);
    let (feedback_tx, feedback_rx) = mpsc::channel::<ContractResponse>(1);

    // 1. Create the broadcast channel for multi-subscriber SSE mapping
    let (broadcaster_tx, _) = broadcast::channel::<TuiUpdate>(256);

    let runtime_coordinator_container = Arc::new(std::sync::Mutex::new(None));
    let capability_resolver_container = Arc::new(tokio::sync::Mutex::new(
        crate::registry::CapabilityResolver::default_resolver(),
    ));

    // Initialise the resolver's cognition mode from the caller-supplied mode argument.
    if let Some(m) = mode {
        let _ = capability_resolver_container
            .try_lock()
            .map(|mut r| r.set_cognition_mode(m));
    }

    // 2. Spawn the leader process campaign in the background
    let campaign_tx = tui_tx.clone();
    let cap_res_leader = capability_resolver_container.clone();
    let coord_leader = runtime_coordinator_container.clone();
    tokio::spawn(async move {
        let mut leader = LeaderOrchestrator::new(prompt, session);
        leader.tui_tx = Some(campaign_tx.clone());
        leader.tui_rx = Some(feedback_rx);
        leader.capability_resolver = cap_res_leader;
        *coord_leader.lock().unwrap() = Some(leader.runtime_coordinator.clone());
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
                        if let Some(attachments) = ktrans
                            .get_mut("vision_attachments")
                            .and_then(|a| a.as_array_mut())
                        {
                            for att in attachments {
                                let verdict =
                                    att.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                                if verdict == "REDACTED" || verdict == "BLOCKED" {
                                    if let Some(data) = att.get_mut("data_base64") {
                                        *data = serde_json::Value::String(
                                            crate::vision_policy::BLACKOUT_PNG_BASE64.to_string(),
                                        );
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
        capability_resolver: capability_resolver_container,
        runtime_coordinator: runtime_coordinator_container,
    });

    let router = Router::new()
        .route("/", get(landing_handler))
        .route("/dashboard", get(index_handler))
        .route("/cockpit", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/korg-frontend.js", get(wasm_js_handler))
        .route("/static/korg-frontend.js", get(wasm_js_handler))
        .route("/korg-frontend_bg.wasm", get(wasm_bytes_handler))
        .route("/static/korg-frontend_bg.wasm", get(wasm_bytes_handler))
        .route("/api/events", get(sse_handler))
        .route("/api/state", get(state_handler))
        .route("/api/screenshots", get(screenshots_handler))
        .route("/api/override", post(override_handler))
        .route("/api/mode", post(mode_handler))
        .route("/api/capabilities", get(capabilities_handler))
        .route(
            "/api/capabilities/toggle",
            post(capabilities_toggle_handler),
        )
        .route("/api/diff", get(diff_handler))
        .route("/api/input", post(input_handler))
        .route("/api/semantic_search", post(semantic_search_handler))
        .route("/api/journal", get(journal_handler))
        .route("/api/metrics", get(metrics_handler))
        .route("/api/workspaces", get(workspaces_handler))
        .route("/api/campaign/abort", post(campaign_abort_handler))
        .route(
            "/api/projections/campaign",
            get(campaign_projection_handler),
        )
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

    // Authoritatively extract runtime coordinator and capability resolver BEFORE moving leader
    let runtime_coordinator = leader.runtime_coordinator.clone();
    let capability_resolver = leader.capability_resolver.clone();
    let runtime_coordinator_container = Arc::new(std::sync::Mutex::new(Some(runtime_coordinator)));

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
                        if let Some(attachments) = ktrans
                            .get_mut("vision_attachments")
                            .and_then(|a| a.as_array_mut())
                        {
                            for att in attachments {
                                let verdict =
                                    att.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                                if verdict == "REDACTED" || verdict == "BLOCKED" {
                                    if let Some(data) = att.get_mut("data_base64") {
                                        *data = serde_json::Value::String(
                                            crate::vision_policy::BLACKOUT_PNG_BASE64.to_string(),
                                        );
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
        capability_resolver,
        runtime_coordinator: runtime_coordinator_container,
    });

    let router = Router::new()
        .route("/", get(landing_handler))
        .route("/dashboard", get(index_handler))
        .route("/cockpit", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/korg-frontend.js", get(wasm_js_handler))
        .route("/static/korg-frontend.js", get(wasm_js_handler))
        .route("/korg-frontend_bg.wasm", get(wasm_bytes_handler))
        .route("/static/korg-frontend_bg.wasm", get(wasm_bytes_handler))
        .route("/api/events", get(sse_handler))
        .route("/api/state", get(state_handler))
        .route("/api/screenshots", get(screenshots_handler))
        .route("/api/override", post(override_handler))
        .route("/api/mode", post(mode_handler))
        .route("/api/capabilities", get(capabilities_handler))
        .route(
            "/api/capabilities/toggle",
            post(capabilities_toggle_handler),
        )
        .route("/api/diff", get(diff_handler))
        .route("/api/input", post(input_handler))
        .route("/api/semantic_search", post(semantic_search_handler))
        .route("/api/journal", get(journal_handler))
        .route("/api/metrics", get(metrics_handler))
        .route("/api/workspaces", get(workspaces_handler))
        .route("/api/campaign/abort", post(campaign_abort_handler))
        .route(
            "/api/projections/campaign",
            get(campaign_projection_handler),
        )
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
    Html(LANDING_HTML)
}

async fn wasm_js_handler() -> impl IntoResponse {
    ([("content-type", "application/javascript")], "")
}

async fn wasm_bytes_handler() -> impl IntoResponse {
    ([("content-type", "application/wasm")], &[] as &[u8])
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
async fn state_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let mode = {
        let resolver = state.capability_resolver.lock().await;
        format!("{:?}", resolver.cognition_mode())
    };
    let path = crate::paths::blackboard_json();
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

/// GET `/api/screenshots`
async fn screenshots_handler() -> impl IntoResponse {
    let history = {
        let h = crate::vision_policy::VISUAL_HISTORY.lock().unwrap();
        h.clone()
    };
    Json(history)
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
struct StdinInputPayload {
    input: String,
}

async fn input_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StdinInputPayload>,
) -> axum::http::StatusCode {
    let trimmed = payload.input.trim().to_lowercase();
    let response = if trimmed == "y" || trimmed == "yes" || trimmed == "approve" {
        ContractResponse::Approve
    } else if trimmed == "n" || trimmed == "no" || trimmed == "reject" {
        ContractResponse::Reject
    } else if trimmed == "f" || trimmed == "force" {
        ContractResponse::Force
    } else {
        ContractResponse::Override(vec![payload.input.clone()])
    };

    let guard = state.feedback_tx.lock().await;
    if let Some(tx) = &*guard {
        if tx.clone().send(response).await.is_ok() {
            println!("[Web] Transmitted console input: {}", payload.input);
            return axum::http::StatusCode::OK;
        }
    }
    axum::http::StatusCode::SERVICE_UNAVAILABLE
}

async fn diff_handler() -> impl IntoResponse {
    let output = tokio::process::Command::new("git")
        .args(&["branch", "--list", "korg-branch-*"])
        .output()
        .await;

    let mut diffs = vec![];
    if let Ok(out) = output {
        let branches_str = String::from_utf8_lossy(&out.stdout);
        for line in branches_str.lines() {
            let branch = line.trim().trim_start_matches('*').trim();
            if !branch.is_empty() {
                let diff_out = tokio::process::Command::new("git")
                    .args(&["diff", "HEAD", branch])
                    .output()
                    .await;
                if let Ok(d_out) = diff_out {
                    let diff_content = String::from_utf8_lossy(&d_out.stdout).to_string();
                    if !diff_content.trim().is_empty() {
                        diffs.push(serde_json::json!({
                            "branch": branch,
                            "diff": diff_content,
                        }));
                    }
                }
            }
        }
    }

    let cwd_diff = tokio::process::Command::new("git")
        .args(&["diff", "HEAD"])
        .output()
        .await;
    if let Ok(d_out) = cwd_diff {
        let diff_content = String::from_utf8_lossy(&d_out.stdout).to_string();
        if !diff_content.trim().is_empty() {
            diffs.push(serde_json::json!({
                "branch": "working-directory",
                "diff": diff_content,
            }));
        }
    }

    Json(diffs)
}

#[derive(serde::Deserialize)]
struct ModeRequest {
    mode: String,
}

/// POST `/api/mode`
///
/// Forwards the mode change to the CapabilityResolver (single state authority).
/// The Arc<Mutex<CognitionMode>> is updated by reading back from registry active_states,
/// NOT by re-interpreting the mode string in the web layer.
async fn mode_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ModeRequest>,
) -> impl IntoResponse {
    let mode_str = payload.mode.to_lowercase();
    let cap_state = crate::registry::CapabilityState::Mode(mode_str.clone());

    let req = crate::registry::TransitionRequest {
        id: "cognition_mode".to_string(),
        target_state: cap_state,
        correlation_id: None,
    };

    let mut resolver = state.capability_resolver.lock().await;
    let response = resolver.handle_transition_request(req);

    if response.status == crate::registry::TransitionState::Applied {
        // Read the authoritative mode string back from registry active_states.
        // The web layer does NOT interpret — it mirrors what the resolver decided.
        let canonical_mode_str = match resolver.active_states.get("cognition_mode") {
            Some(crate::registry::CapabilityState::Mode(m)) => m.clone(),
            _ => mode_str.clone(),
        };
        drop(resolver);

        tracing::info!(canonical = %canonical_mode_str, "cognition_mode_updated");

        // Broadcast trace event to live console log stream
        let _ = state.broadcaster.send(TuiUpdate::Trace(format!(
            "[cognition-mode] Dynamically switched active mode to: {}",
            canonical_mode_str
        )));

        (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "mode": canonical_mode_str, "status": "applied" })),
        )
            .into_response()
    } else {
        drop(resolver);
        let errors = response.errors.join(", ");
        tracing::warn!(errors = %errors, "mode_transition_rejected");
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": errors, "plan_id": response.plan_id })),
        )
            .into_response()
    }
}

/// GET `/api/capabilities`
async fn capabilities_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let resolver = state.capability_resolver.lock().await;
    let nodes = resolver.nodes.clone();
    let active_states = resolver.active_states.clone();
    let events = resolver.journal.events.clone();

    Json(serde_json::json!({
        "nodes": nodes,
        "active_states": active_states,
        "events": events,
    }))
}

/// GET `/api/projections/campaign`
async fn campaign_projection_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let resolver = state.capability_resolver.lock().await;
    let campaign_state = resolver.get_campaign_state();
    Json(campaign_state)
}

/// POST `/api/capabilities/toggle`
async fn capabilities_toggle_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<crate::registry::TransitionRequest>,
) -> impl IntoResponse {
    let mut resolver = state.capability_resolver.lock().await;
    let response = resolver.handle_transition_request(payload);
    Json(response)
}

#[derive(serde::Deserialize)]
struct SemanticSearchRequest {
    query: String,
    top_n: Option<usize>,
}

#[derive(serde::Serialize)]
struct SemanticSearchResult {
    file_path: String,
    block_name: String,
    block_type: String,
    start_line: usize,
    end_line: usize,
    content: String,
    similarity: f32,
}

/// POST `/api/semantic_search`
async fn semantic_search_handler(
    State(_state): State<Arc<AppState>>,
    Json(payload): Json<SemanticSearchRequest>,
) -> impl IntoResponse {
    let index_path = ".korg/index.json";
    if !std::path::Path::new(index_path).exists() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Index file not found. Please run indexer." })),
        )
            .into_response();
    }

    let index = match crate::code_indexer::load_index(index_path) {
        Ok(idx) => idx,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to load index: {}", e) })),
            )
                .into_response();
        }
    };

    let embedding_model: Box<dyn crate::embeddings::EmbeddingModel> =
        match crate::embeddings::CandleEmbeddingModel::load() {
            Ok(model) => Box::new(model),
            Err(_) => Box::new(crate::embeddings::FakeEmbeddingModel::default()),
        };

    let top_n = payload.top_n.unwrap_or(5);
    let matches =
        crate::code_indexer::query_codebase(&index, &payload.query, &*embedding_model, top_n);

    let results: Vec<SemanticSearchResult> = matches
        .into_iter()
        .map(|(sim, block)| SemanticSearchResult {
            file_path: block.file_path,
            block_name: block.block_name,
            block_type: block.block_type,
            start_line: block.start_line,
            end_line: block.end_line,
            content: block.content,
            similarity: sim,
        })
        .collect();

    (axum::http::StatusCode::OK, Json(results)).into_response()
}

/// GET `/api/journal`
///
/// Returns the last 100 capability kernel events as JSONL (one event per line).
/// Suitable for streaming to log shippers, dashboards, or debugging sessions.
async fn journal_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let resolver = state.capability_resolver.lock().await;
    let jsonl = resolver.journal.to_json_lines(100);
    let total = resolver.journal.len();
    drop(resolver);

    (
        [
            ("content-type", "application/x-ndjson"),
            ("x-korg-journal-total", ""),
        ],
        format!("// total events: {}\n{}", total, jsonl),
    )
        .into_response()
}

/// GET `/api/metrics`
///
/// Returns a point-in-time snapshot of all atomic runtime counters.
/// Lock-free; safe to call at any frequency.
async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snap = crate::metrics::snapshot();

    // Extract active processes and retry budget if coordinator is available
    let (active_processes, remaining_retry_budget) = {
        let coord_guard = state.runtime_coordinator.lock().unwrap();
        if let Some(coord) = &*coord_guard {
            let active = coord.supervisor.active_count();
            let remaining = coord.retry_budget.lock().unwrap().remaining();
            (active, remaining)
        } else {
            (0, 0)
        }
    };

    let mut json_val = serde_json::to_value(snap).unwrap_or(serde_json::json!({}));
    if let Some(obj) = json_val.as_object_mut() {
        obj.insert(
            "active_processes".to_string(),
            serde_json::json!(active_processes),
        );
        obj.insert(
            "remaining_retry_budget".to_string(),
            serde_json::json!(remaining_retry_budget),
        );
    }

    Json(json_val)
}

/// GET `/api/workspaces`
///
/// Returns the current workspace manager snapshot — all known workspaces
/// with their state, persona, routing_id, and path.
async fn workspaces_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let metrics = crate::metrics::snapshot();

    // Check if coordinator is present and get workspace manager snapshot
    let coordinator_opt = {
        let guard = state.runtime_coordinator.lock().unwrap();
        guard.clone()
    };

    if let Some(coord) = coordinator_opt {
        let wm = coord.workspace_manager.lock().await;
        let list: Vec<serde_json::Value> = wm
            .snapshot_all()
            .into_iter()
            .map(|ws| serde_json::to_value(ws).unwrap_or(serde_json::Value::Null))
            .collect();

        Json(serde_json::json!({
            "session_id": coord.session_id.to_string(),
            "workspaces_created": metrics.workspaces_created,
            "workspaces_completed": metrics.workspaces_completed,
            "workspaces_destroyed": metrics.workspaces_destroyed,
            "workers_completed": metrics.workers_completed,
            "workers_crashed": metrics.workers_crashed,
            "worker_timeouts": metrics.worker_timeouts,
            "active_count": wm.active_workspaces().count(),
            "workspaces": list,
        }))
    } else {
        Json(serde_json::json!({
            "session_id": "(no-active-session)",
            "workspaces_created": metrics.workspaces_created,
            "workspaces_completed": metrics.workspaces_completed,
            "workspaces_destroyed": metrics.workspaces_destroyed,
            "workers_completed": metrics.workers_completed,
            "workers_crashed": metrics.workers_crashed,
            "worker_timeouts": metrics.worker_timeouts,
            "workspaces": Vec::<serde_json::Value>::new(),
        }))
    }
}

/// POST `/api/campaign/abort`
///
/// Forcibly aborts the currently running campaign by calling `abort()` on the coordinator.
async fn campaign_abort_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let coordinator_opt = {
        let guard = state.runtime_coordinator.lock().unwrap();
        guard.clone()
    };

    if let Some(coordinator) = coordinator_opt {
        coordinator.abort();
        tracing::warn!(session_id = %coordinator.session_id, "campaign_abort_endpoint_triggered");
        let _ = state.broadcaster.send(TuiUpdate::Trace(format!(
            "[campaign-abort] Forcibly aborted the active campaign session: {}",
            coordinator.session_id
        )));
        (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "status": "aborted",
                "session_id": coordinator.session_id.to_string(),
            })),
        )
            .into_response()
    } else {
        (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "No active campaign session to abort.",
            })),
        )
            .into_response()
    }
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
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;500;600;700;800&family=Plus+Jakarta+Sans:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;700&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg-base: #000000;
            --bg-surface: #050505;
            --bg-card: rgba(5, 5, 5, 0.7);
            --border-color: rgba(255, 255, 255, 0.04);
            --border-glow: rgba(255, 255, 255, 0.08);
            --accent-emerald: #10b981;
            --accent-emerald-glow: rgba(16, 185, 129, 0.15);
            --accent-cyan: #06b6d4;
            --accent-cyan-glow: rgba(6, 182, 212, 0.15);
            --accent-gold: #f59e0b;
            --accent-green: #10b981;
            --text-primary: #ffffff;
            --text-secondary: #a1a1aa;
            --text-muted: #52525b;
            --font-sans: 'Outfit', sans-serif;
            --font-heading: 'Plus Jakarta Sans', sans-serif;
            --font-mono: 'JetBrains Mono', monospace;
        }

        * {
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }

        html, body {
            max-width: 100%;
            overflow-x: hidden;
            width: 100%;
        }

        body {
            font-family: var(--font-sans);
            background-color: var(--bg-base);
            color: var(--text-primary);
            min-height: 100dvh;
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
            height: 900px;
            background: 
                radial-gradient(circle at 20% 10%, rgba(16, 185, 129, 0.03) 0%, transparent 40%),
                radial-gradient(circle at 80% 30%, rgba(6, 182, 212, 0.04) 0%, transparent 45%),
                radial-gradient(circle at 50% -100px, rgba(16, 185, 129, 0.05) 0%, transparent 50%);
            pointer-events: none;
            z-index: 0;
        }

        body::after {
            content: "";
            position: absolute;
            inset: 0;
            background-image: 
                linear-gradient(rgba(255, 255, 255, 0.01) 1px, transparent 1px),
                linear-gradient(90deg, rgba(255, 255, 255, 0.01) 1px, transparent 1px);
            background-size: 40px 40px;
            pointer-events: none;
            z-index: 0;
            opacity: 0.8;
        }

        .ambient-glow {
            position: absolute;
            top: -250px;
            left: -250px;
            width: 800px;
            height: 800px;
            background: radial-gradient(circle, rgba(16, 185, 129, 0.04) 0%, rgba(6, 182, 212, 0.04) 30%, transparent 70%);
            filter: blur(120px);
            pointer-events: none;
            z-index: 0;
            opacity: 0.8;
            animation: float-glow 25s infinite ease-in-out alternate;
        }

        @keyframes float-glow {
            0% {
                transform: translate(0, 0) scale(1);
                opacity: 0.6;
            }
            50% {
                transform: translate(120px, 80px) scale(1.08);
                opacity: 0.8;
            }
            100% {
                transform: translate(-80px, 150px) scale(0.95);
                opacity: 0.5;
            }
        }

        header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 16px 40px;
            border-bottom: 1px solid var(--border-color);
            background-color: rgba(0, 0, 0, 0.75);
            backdrop-filter: blur(24px);
            position: sticky;
            top: 0;
            z-index: 10000;
            width: 100%;
            box-sizing: border-box;
        }

        .logo-container {
            display: flex;
            align-items: center;
            gap: 10px;
            position: relative;
            z-index: 10001;
            flex-shrink: 0;
        }

        .logo-circle {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: 28px;
            height: 28px;
            border-radius: 50%;
            background: #000000;
            border: 1px solid rgba(255, 255, 255, 0.15);
            color: #ffffff;
            font-family: var(--font-mono);
            font-size: 14px;
            font-weight: 700;
            box-shadow: 0 0 10px rgba(255, 255, 255, 0.05);
            flex-shrink: 0;
            transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
            line-height: 1;
            padding-bottom: 1px;
        }

        .logo-container:hover .logo-circle {
            border-color: rgba(6, 182, 212, 0.8);
            box-shadow: 0 0 12px rgba(6, 182, 212, 0.4);
            transform: scale(1.08);
        }

        .logo {
            font-family: var(--font-heading);
            font-size: 24px;
            font-weight: 800;
            letter-spacing: -0.04em;
            background: linear-gradient(135deg, #ffffff 60%, #71717a 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
            text-transform: lowercase;
            position: relative;
            z-index: 10002;
            flex-shrink: 0;
        }

        .logo-sub {
            font-size: 11px;
            color: var(--text-muted);
            font-family: var(--font-mono);
            border-left: 1px solid var(--border-color);
            padding-left: 12px;
            padding-bottom: 2px; /* Ensure descenders are not cut off */
            letter-spacing: 0.05em;
            line-height: 1.4;
            margin-top: 0;
            overflow: visible;
            display: inline-block;
            vertical-align: middle;
            position: relative;
            top: -1.5px; /* Visual baseline alignment correction */
            z-index: 10002;
            flex-shrink: 0;
            white-space: nowrap;
        }

        .header-status {
            display: flex;
            align-items: center;
            gap: 8px;
            font-size: 11px;
            font-family: var(--font-mono);
            color: var(--text-secondary);
            border: 1px solid var(--border-color);
            padding: 6px 14px;
            border-radius: 6px;
            background: rgba(10, 10, 12, 0.5);
            box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
        }

        .status-dot {
            width: 6px;
            height: 6px;
            border-radius: 50%;
            background-color: var(--accent-green);
            box-shadow: 0 0 8px var(--accent-green);
            animation: pulse-active 2s infinite;
        }

        @keyframes pulse-active {
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
            margin-bottom: 28px;
            color: #d4d4d8;
            transition: all 0.3s ease;
            box-shadow: 0 0 15px rgba(255, 255, 255, 0.02);
        }

        .hero-badge:hover {
            border-color: rgba(255, 255, 255, 0.2);
            background: rgba(255, 255, 255, 0.06);
            transform: translateY(-1px);
        }

        .hero-badge-dot {
            width: 6px;
            height: 6px;
            border-radius: 50%;
            background-color: var(--accent-emerald);
            box-shadow: 0 0 6px var(--accent-emerald);
        }

        .hero-title {
            font-family: var(--font-heading);
            font-size: 64px;
            font-weight: 800;
            letter-spacing: -0.04em;
            line-height: 1.05;
            margin-bottom: 24px;
            background: linear-gradient(135deg, #ffffff 40%, #94a3b8 80%, var(--accent-emerald) 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }

        .hero-subtitle {
            font-size: 18px;
            color: var(--text-secondary);
            line-height: 1.6;
            font-weight: 300;
            margin-bottom: 40px;
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
            padding: 14px 30px;
            background: linear-gradient(135deg, #ffffff 0%, #e4e4e7 100%);
            color: #030303;
            border: 1px solid #ffffff;
            border-radius: 8px;
            cursor: pointer;
            text-decoration: none;
            display: inline-flex;
            align-items: center;
            gap: 8px;
            transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
            box-shadow: 0 10px 20px rgba(255, 255, 255, 0.05);
        }

        .btn-primary:hover {
            background: #ffffff;
            transform: translateY(-2px);
            box-shadow: 0 15px 30px rgba(255, 255, 255, 0.1);
        }

        .btn-primary svg {
            transition: transform 0.2s ease;
        }

        .btn-primary:hover svg {
            transform: translateX(4px);
        }

        .btn-secondary {
            font-family: var(--font-sans);
            font-size: 13px;
            font-weight: 600;
            padding: 14px 30px;
            background-color: rgba(255, 255, 255, 0.02);
            color: #ffffff;
            border: 1px solid var(--border-color);
            border-radius: 8px;
            cursor: pointer;
            text-decoration: none;
            display: inline-flex;
            align-items: center;
            gap: 8px;
            transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
            backdrop-filter: blur(8px);
        }

        .btn-secondary:hover {
            border-color: rgba(255, 255, 255, 0.2);
            background-color: rgba(255, 255, 255, 0.05);
            transform: translateY(-2px);
            box-shadow: 0 10px 20px rgba(0, 0, 0, 0.3);
        }

        /* Interactive Simulator */
        .simulator-section {
            width: 100%;
            max-width: 950px;
            margin-bottom: 100px;
            animation: fadeInUp 1s cubic-bezier(0.16, 1, 0.3, 1) 0.15s;
            animation-fill-mode: both;
        }

        .simulator-window {
            border: 1px solid var(--border-color);
            background: rgba(10, 10, 12, 0.65);
            border-radius: 12px;
            overflow: hidden;
            box-shadow: 
                0 30px 60px rgba(0, 0, 0, 0.7),
                0 0 40px rgba(255, 255, 255, 0.02),
                inset 0 1px 0 rgba(255, 255, 255, 0.05);
            backdrop-filter: blur(25px);
            display: flex;
            flex-direction: column;
            height: 480px;
            width: 100%;
        }

        .sim-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            background: rgba(5, 5, 5, 0.75);
            border-bottom: 1px solid var(--border-color);
            padding: 0 20px;
            height: 45px;
            user-select: none;
        }

        .sim-controls {
            display: flex;
            gap: 8px;
            align-items: center;
        }

        .sim-dot {
            width: 10px;
            height: 10px;
            border-radius: 50%;
        }
        .dot-red { background-color: #ff5f56; }
        .dot-yellow { background-color: #ffbd2e; }
        .dot-green { background-color: #27c93f; }

        .sim-tabs {
            display: flex;
            height: 100%;
            align-items: flex-end;
        }

        .sim-tab {
            background: transparent;
            border: none;
            color: var(--text-muted);
            font-family: var(--font-sans);
            font-size: 12px;
            padding: 8px 16px;
            border-top-left-radius: 6px;
            border-top-right-radius: 6px;
            cursor: pointer;
            transition: all 0.2s;
            height: 32px;
            display: inline-flex;
            align-items: center;
            border-bottom: 2px solid transparent;
        }

        .sim-tab:hover {
            color: var(--text-secondary);
            background: rgba(255, 255, 255, 0.02);
        }

        .sim-tab.active {
            color: var(--text-primary);
            background: rgba(20, 20, 23, 0.8);
            border-bottom: 2px solid var(--accent-emerald);
            font-weight: 500;
        }

        .sim-title-right {
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-muted);
        }

        .sim-workspace {
            display: flex;
            flex-grow: 1;
            overflow: hidden;
        }

        .sim-sidebar {
            width: 220px;
            background: rgba(5, 5, 5, 0.4);
            border-right: 1px solid var(--border-color);
            padding: 16px;
            display: flex;
            flex-direction: column;
            gap: 8px;
            overflow-y: auto;
        }

        .sidebar-header {
            font-family: var(--font-heading);
            font-size: 10px;
            font-weight: bold;
            color: var(--text-muted);
            text-transform: uppercase;
            letter-spacing: 0.1em;
            margin-bottom: 6px;
        }

        .sidebar-item {
            font-family: var(--font-sans);
            font-size: 12px;
            color: var(--text-secondary);
            padding: 8px 12px;
            border-radius: 6px;
            cursor: pointer;
            transition: all 0.2s;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .sidebar-item:hover {
            color: var(--text-primary);
            background: rgba(255, 255, 255, 0.04);
        }

        .sidebar-item.active {
            color: var(--text-primary);
            background: rgba(255, 255, 255, 0.08);
            font-weight: 500;
            border: 1px solid rgba(255, 255, 255, 0.15);
        }

        .sidebar-persona {
            display: flex;
            align-items: center;
            gap: 8px;
            padding: 6px 12px;
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-secondary);
        }

        .persona-indicator {
            width: 6px;
            height: 6px;
            border-radius: 50%;
        }
        .pulse-emerald { background-color: var(--accent-emerald); box-shadow: 0 0 6px var(--accent-emerald); }
        .pulse-blue { background-color: var(--accent-cyan); box-shadow: 0 0 6px var(--accent-cyan); }
        .pulse-green { background-color: #10b981; box-shadow: 0 0 6px #10b981; }

        .sim-editor {
            flex-grow: 1;
            display: flex;
            flex-direction: column;
            background: rgba(10, 10, 12, 0.85);
            overflow: hidden;
        }

        .editor-header {
            background: rgba(5, 5, 5, 0.2);
            border-bottom: 1px solid var(--border-color);
            padding: 8px 20px;
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-muted);
        }

        .editor-body {
            padding: 20px;
            overflow-y: auto;
            flex-grow: 1;
        }

        .code-output {
            font-family: var(--font-mono);
            font-size: 12px;
            line-height: 1.6;
            color: var(--text-secondary);
            white-space: pre-wrap;
            word-break: break-all;
        }

        .sim-footer-controls {
            display: flex;
            gap: 12px;
            padding: 12px 20px;
            background: rgba(5, 5, 5, 0.8);
            border-top: 1px solid var(--border-color);
        }

        .sim-action-btn {
            font-family: var(--font-mono);
            font-size: 11px;
            padding: 8px 16px;
            background: rgba(255, 255, 255, 0.02);
            border: 1px solid var(--border-color);
            color: var(--text-secondary);
            border-radius: 6px;
            cursor: pointer;
            transition: all 0.2s;
        }

        .sim-action-btn:hover {
            border-color: rgba(255, 255, 255, 0.2);
            color: var(--text-primary);
            background: rgba(255, 255, 255, 0.04);
        }

        .sim-action-btn.active {
            border-color: var(--accent-emerald);
            color: #ffffff;
            background: rgba(255, 255, 255, 0.1);
            box-shadow: 0 0 12px rgba(255, 255, 255, 0.05);
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
            background-color: var(--bg-card);
            border-radius: 12px;
            padding: 32px;
            display: flex;
            flex-direction: column;
            gap: 20px;
            cursor: pointer;
            transition: all 0.4s cubic-bezier(0.16, 1, 0.3, 1);
            position: relative;
            text-decoration: none;
            color: inherit;
            backdrop-filter: blur(20px);
            overflow: hidden;
            box-shadow: 0 10px 30px rgba(0, 0, 0, 0.3);
        }

        .portal-card::before {
            content: "";
            position: absolute;
            inset: 0;
            border-radius: 12px;
            padding: 1px;
            background: linear-gradient(to bottom, rgba(255, 255, 255, 0.1), rgba(255, 255, 255, 0));
            -webkit-mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
            -webkit-mask-composite: xor;
            mask-composite: exclude;
            pointer-events: none;
        }

        .portal-card::after {
            content: "";
            position: absolute;
            width: 160px;
            height: 160px;
            background: radial-gradient(circle, var(--accent-emerald-glow) 0%, transparent 70%);
            top: -80px;
            right: -80px;
            opacity: 0.15;
            transition: all 0.5s ease;
            pointer-events: none;
        }

        .portal-card:hover::after {
            opacity: 0.5;
            transform: scale(1.3);
        }

        .portal-card:hover {
            border-color: rgba(255, 255, 255, 0.2);
            transform: translateY(-6px);
            background-color: rgba(14, 14, 16, 0.6);
            box-shadow: 
                0 25px 50px rgba(0, 0, 0, 0.6), 
                0 0 30px rgba(255, 255, 255, 0.05);
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
            transform: scale(1.15) rotate(8deg);
        }

        .portal-tag {
            font-family: var(--font-mono);
            font-size: 10px;
            color: var(--text-secondary);
            border: 1px solid var(--border-color);
            padding: 4px 10px;
            border-radius: 100px;
            background: rgba(0, 0, 0, 0.3);
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }

        .portal-card:hover .portal-tag {
            border-color: rgba(255, 255, 255, 0.15);
            color: #ffffff;
            background: rgba(255, 255, 255, 0.05);
        }

        .portal-title {
            font-family: var(--font-heading);
            font-size: 20px;
            font-weight: 700;
            color: #ffffff;
            letter-spacing: -0.01em;
        }

        .portal-desc {
            font-size: 13.5px;
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
            transition: all 0.3s;
            display: inline-flex;
            align-items: center;
            gap: 6px;
        }

        .portal-card:hover .portal-action {
            color: #ffffff;
        }

        .portal-action svg {
            transition: transform 0.2s;
        }
        
        .portal-card:hover .portal-action svg {
            transform: translateX(3px);
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
            font-size: 32px;
            font-weight: 800;
            color: #ffffff;
            margin-bottom: 12px;
            text-align: center;
            letter-spacing: -0.02em;
            background: linear-gradient(135deg, #ffffff 60%, #a1a1aa 100%);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }

        .matrix-subtitle {
            font-size: 15px;
            color: var(--text-secondary);
            text-align: center;
            margin-bottom: 56px;
            max-width: 600px;
            margin-left: auto;
            margin-right: auto;
            font-weight: 300;
            line-height: 1.6;
        }

        .matrix-grid {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 32px;
            width: 100%;
        }

        .matrix-card {
            display: flex;
            flex-direction: column;
            gap: 16px;
            border: 1px solid var(--border-color);
            background: rgba(10, 10, 12, 0.3);
            padding: 32px;
            border-radius: 12px;
            transition: all 0.3s ease;
            position: relative;
            overflow: hidden;
            backdrop-filter: blur(10px);
        }

        .matrix-card::before {
            content: "";
            position: absolute;
            inset: 0;
            border-radius: 12px;
            padding: 1px;
            background: linear-gradient(135deg, rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0));
            -webkit-mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
            -webkit-mask-composite: xor;
            mask-composite: exclude;
            pointer-events: none;
        }

        .matrix-card:hover {
            border-color: rgba(255, 255, 255, 0.25);
            background: rgba(10, 10, 12, 0.55);
            transform: translateY(-4px);
            box-shadow: 
                0 20px 40px rgba(0, 0, 0, 0.5),
                0 0 30px rgba(255, 255, 255, 0.02);
        }

        .matrix-card-title {
            font-family: var(--font-heading);
            font-size: 18px;
            font-weight: 700;
            color: #ffffff;
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .matrix-card-text {
            font-size: 13.5px;
            color: var(--text-secondary);
            line-height: 1.6;
            font-weight: 300;
        }

        /* Comparison Table Styles (xAI / Grok inspired) */
        .comparison-section {
            padding: 80px 40px;
            max-width: 1200px;
            margin: 0 auto;
            position: relative;
        }

        .comparison-title {
            font-family: var(--font-heading);
            font-size: 32px;
            font-weight: 800;
            color: #ffffff;
            letter-spacing: -0.03em;
            text-align: center;
            margin-bottom: 12px;
            text-transform: lowercase;
        }

        .comparison-subtitle {
            font-size: 16px;
            color: var(--text-secondary);
            text-align: center;
            margin-bottom: 48px;
            max-width: 600px;
            margin-left: auto;
            margin-right: auto;
            line-height: 1.5;
            font-weight: 300;
        }

        .comparison-table-container {
            width: 100%;
            overflow-x: auto;
            border-radius: 12px;
            border: 1px solid var(--border-color);
            background: rgba(15, 15, 18, 0.6);
            backdrop-filter: blur(20px);
            box-shadow: 0 30px 60px rgba(0, 0, 0, 0.4);
        }

        .comparison-table {
            width: 100%;
            border-collapse: collapse;
            text-align: left;
            font-size: 14px;
        }

        .comparison-table th, .comparison-table td {
            padding: 18px 24px;
            border-bottom: 1px solid var(--border-color);
        }

        .comparison-table th {
            font-family: var(--font-heading);
            font-size: 14px;
            font-weight: 700;
            color: #a1a1aa;
            text-transform: lowercase;
            letter-spacing: 0.05em;
            background: rgba(20, 20, 23, 0.8);
        }

        .comparison-table tr:last-child td {
            border-bottom: none;
        }

        .comparison-table .feature-col {
            font-weight: 500;
            color: #ffffff;
            width: 25%;
            font-family: var(--font-sans);
        }

        .comparison-table .korg-col {
            background: rgba(255, 255, 255, 0.02);
            border-left: 1px solid rgba(255, 255, 255, 0.08);
            border-right: 1px solid rgba(255, 255, 255, 0.08);
            width: 35%;
        }

        .comparison-table th.korg-col {
            background: rgba(255, 255, 255, 0.04);
            color: #ffffff;
            font-weight: 800;
        }

        .comparison-table .other-col {
            color: var(--text-secondary);
            width: 20%;
        }

        .comparison-badge {
            display: inline-flex;
            align-items: center;
            gap: 6px;
            font-family: var(--font-mono);
            font-size: 11px;
            padding: 4px 8px;
            border-radius: 4px;
            font-weight: 500;
        }

        .comparison-badge.yes {
            background: rgba(255, 255, 255, 0.08);
            color: #ffffff;
            border: 1px solid rgba(255, 255, 255, 0.15);
        }

        .comparison-badge.no {
            background: rgba(239, 68, 68, 0.1);
            color: #ef4444;
            border: 1px solid rgba(239, 68, 68, 0.2);
        }

        .comparison-badge.partial {
            background: rgba(245, 158, 11, 0.1);
            color: #f59e0b;
            border: 1px solid rgba(245, 158, 11, 0.2);
        }

        .table-text {
            line-height: 1.5;
            margin-top: 6px;
            font-size: 13px;
            color: var(--text-secondary);
        }

        .korg-col .table-text {
            color: #e2e8f0;
        }

        /* Footer */
        footer {
            border-top: 1px solid var(--border-color);
            padding: 40px;
            text-align: center;
            font-family: var(--font-mono);
            font-size: 11px;
            color: var(--text-muted);
            background-color: #030303;
            letter-spacing: 0.05em;
        }

        /* Inline Drawers style - 100% Zero-Overlap DOM */
        .modal-overlay {
            max-height: 0;
            overflow: hidden;
            transition: max-height 0.45s cubic-bezier(0.16, 1, 0.3, 1), padding 0.45s ease, margin-top 0.45s ease, opacity 0.45s ease, margin-bottom 0.45s ease;
            width: 100%;
            background-color: var(--bg-surface);
            border: 1px solid transparent;
            border-radius: 16px;
            padding: 0 40px;
            margin-top: 0;
            margin-bottom: 0;
            opacity: 0;
            display: flex;
            flex-direction: column;
            box-sizing: border-box;
        }

        .modal-overlay.active {
            max-height: 1000px;
            padding: 32px 40px;
            margin-top: 24px;
            margin-bottom: 24px;
            opacity: 1;
            border: 1px solid var(--border-color);
            box-shadow: 0 20px 40px rgba(0, 0, 0, 0.6), 0 0 30px rgba(255, 255, 255, 0.02);
        }

        .modal-card {
            background-color: transparent;
            border: none;
            border-radius: 0;
            width: 100%;
            max-width: 100%;
            padding: 0;
            display: flex;
            flex-direction: column;
            gap: 24px;
            box-shadow: none;
            position: relative;
        }

        .modal-title {
            font-family: var(--font-heading);
            font-size: 24px;
            font-weight: 700;
            color: #ffffff;
            letter-spacing: -0.02em;
            display: flex;
            align-items: center;
            gap: 12px;
        }

        .modal-desc {
            font-size: 14px;
            color: var(--text-secondary);
            line-height: 1.6;
            font-weight: 300;
        }

        .terminal-box {
            display: flex;
            align-items: center;
            justify-content: space-between;
            background-color: #070709;
            border: 1px solid var(--border-color);
            border-radius: 8px;
            padding: 16px 20px;
            font-family: var(--font-mono);
            font-size: 12px;
            color: #ffffff;
            box-shadow: inset 0 2px 8px rgba(0, 0, 0, 0.8);
        }

        .terminal-prompt {
            color: var(--accent-emerald);
            user-select: none;
            margin-right: 12px;
            font-weight: bold;
        }

        .terminal-command {
            flex-grow: 1;
            color: #ffffff;
        }

        .copy-btn {
            background: rgba(255, 255, 255, 0.04);
            border: 1px solid rgba(255, 255, 255, 0.1);
            color: #d4d4d8;
            font-family: var(--font-mono);
            font-size: 11px;
            font-weight: bold;
            padding: 6px 12px;
            border-radius: 6px;
            cursor: pointer;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            transition: all 0.2s;
        }

        .copy-btn:hover {
            border-color: #ffffff;
            color: #ffffff;
            background-color: rgba(255, 255, 255, 0.1);
            box-shadow: 0 0 10px rgba(255, 255, 255, 0.05);
        }

        .cli-details {
            display: flex;
            flex-direction: column;
            gap: 12px;
            font-family: var(--font-mono);
            font-size: 11px;
            border-top: 1px solid var(--border-color);
            padding-top: 18px;
        }

        .cli-detail-row {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 4px 0;
        }

        .cli-detail-key {
            color: var(--accent-cyan);
            font-weight: bold;
        }

        .cli-detail-val {
            color: var(--text-secondary);
        }

        .btn-modal-close {
            font-family: var(--font-sans);
            font-size: 12px;
            font-weight: 600;
            padding: 12px 24px;
            background: linear-gradient(135deg, #ffffff 0%, #e4e4e7 100%);
            color: #030206;
            border: 1px solid #ffffff;
            border-radius: 8px;
            cursor: pointer;
            transition: all 0.2s;
        }

        .btn-modal-close:hover {
            background-color: #ffffff;
            transform: translateY(-1px);
        }

        /* Provenance Explorer Modal Layout */
        .modal-dag-layout {
            display: flex;
            flex-direction: column;
            gap: 20px;
        }

        .modal-dag-visual {
            border: 1px solid var(--border-color);
            background-color: #070709;
            border-radius: 8px;
            padding: 24px 16px;
            display: flex;
            justify-content: center;
            align-items: center;
            box-shadow: inset 0 2px 10px rgba(0,0,0,0.8);
        }

        .mini-edge {
            stroke: rgba(255, 255, 255, 0.08);
            stroke-width: 2.5;
            stroke-dasharray: 4 4;
            transition: all 0.3s;
        }

        .mini-node {
            cursor: pointer;
        }

        .mini-node .node-base {
            fill: #09090b;
            stroke: rgba(255, 255, 255, 0.15);
            stroke-width: 2.5;
            transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
        }

        .mini-node .node-glow {
            fill: transparent;
            stroke: transparent;
            stroke-width: 4;
            transition: all 0.3s ease;
        }

        .mini-node:hover .node-base {
            stroke: #ffffff;
            fill: rgba(255, 255, 255, 0.1);
        }

        .mini-node.active .node-base {
            fill: var(--accent-emerald);
            stroke: #ffffff;
        }

        .mini-node.active .node-glow {
            stroke: var(--accent-emerald-glow);
            filter: drop-shadow(0 0 8px var(--accent-emerald));
            animation: pulseGlow 2s infinite alternate;
        }

        @keyframes pulseGlow {
            from { r: 18; opacity: 0.5; }
            to { r: 24; opacity: 0.9; }
        }

        .mini-node text {
            font-family: var(--font-mono);
            font-size: 10px;
            fill: var(--text-secondary);
            text-anchor: middle;
            user-select: none;
            font-weight: 700;
        }

        .mini-node.active text {
            fill: #ffffff;
        }

        .modal-dag-properties {
            border: 1px solid var(--border-color);
            background-color: rgba(7, 7, 9, 0.4);
            border-radius: 8px;
            padding: 24px;
            font-family: var(--font-mono);
            backdrop-filter: blur(10px);
        }

        .properties-header {
            font-size: 11px;
            color: var(--text-muted);
            text-transform: uppercase;
            letter-spacing: 0.1em;
            margin-bottom: 16px;
            border-bottom: 1px solid var(--border-color);
            padding-bottom: 10px;
            font-weight: bold;
        }

        .prop-table {
            display: flex;
            flex-direction: column;
            gap: 12px;
            font-size: 12px;
        }

        .prop-row {
            display: flex;
            align-items: flex-start;
            padding: 2px 0;
        }

        .prop-key {
            width: 150px;
            color: var(--text-muted);
            font-weight: 500;
        }

        .prop-val {
            flex-grow: 1;
            color: var(--text-primary);
            word-break: break-all;
        }

        /* Animations */
        @keyframes fadeIn {
            from { opacity: 0; transform: translateY(-12px); }
            to { opacity: 1; transform: translateY(0); }
        }

        @keyframes fadeInUp {
            from { opacity: 0; transform: translateY(24px); }
            to { opacity: 1; transform: translateY(0); }
        }

        /* Responsive scaling */
        @media (max-width: 900px) {
            .logo-sub {
                display: none;
            }
            .hero-title {
                font-size: 44px;
            }
            .portals-grid {
                grid-template-columns: 1fr;
            }
            .matrix-grid {
                grid-template-columns: 1fr;
            }
            .sim-workspace {
                flex-direction: column;
                height: auto;
            }
            .sim-sidebar {
                width: 100%;
                border-right: none;
                border-bottom: 1px solid var(--border-color);
                height: 180px;
            }
            .simulator-window {
                height: 620px;
            }
            header {
                padding: 16px 20px;
            }
        }
        /* ── CRT Scanline Overlay ────────────────────────────────────────────────────
           Full-page atmospheric scanlines. pointer-events: none so it never
           intercepts clicks. z-index: 9999 sits above all content visually. */
        body::before {
            content: "";
            position: fixed;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            z-index: 9999;
            pointer-events: none;
            background: repeating-linear-gradient(
                0deg,
                rgba(0, 0, 0, 0.10) 0px,
                rgba(0, 0, 0, 0.10) 1px,
                transparent 1px,
                transparent 3px
            );
            animation: crt-drift 10s linear infinite;
        }

        @keyframes crt-drift {
            0%   { background-position: 0 0; }
            100% { background-position: 0 120px; }
        }
    </style>
</head>
<body>
    <div class="ambient-glow"></div>
    <header>
        <div class="logo-container">
            <div class="logo-circle">k</div>
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
                <span>korg v0.2.0 is now live</span>
            </div>
            <h1 class="hero-title">the autonomous software engineering runtime.</h1>
            <p class="hero-subtitle">
                Meet the first self-contained AI developer team that lives in your workspace. Korg doesn't just suggest code snippets—it runs a complete, secure environment where specialized AI agents (Architects, Coders, and Testers) collaborate, write code, run builds, and automatically heal broken tests.
            </p>
            <div class="hero-ctas">
                <a href="/dashboard" class="btn-primary">
                    <span>launch engineering Hub</span>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </a>
                <a href="#matrix" class="btn-secondary" onclick="document.getElementById('matrix').scrollIntoView({behavior: 'smooth'}); return false;">
                    <span>view architecture spec</span>
                </a>
            </div>
        </div>

        <!-- Interactive Simulator -->
        <div class="simulator-section">
            <div class="simulator-window">
                <div class="sim-header">
                    <div class="sim-controls">
                        <span class="sim-dot dot-red"></span>
                        <span class="sim-dot dot-yellow"></span>
                        <span class="sim-dot dot-green"></span>
                    </div>
                    <div class="sim-tabs">
                        <button class="sim-tab active" onclick="switchSimTab('architect')">architect.rs</button>
                        <button class="sim-tab" onclick="switchSimTab('coder')">coder.rs</button>
                        <button class="sim-tab" onclick="switchSimTab('tester')">tester.rs</button>
                        <button class="sim-tab" onclick="switchSimTab('ledger')">ledger.json</button>
                    </div>
                    <span class="sim-title-right">korg://swarm-control</span>
                </div>
                
                <div class="sim-workspace">
                    <div class="sim-sidebar">
                        <div class="sidebar-header">WORKFLOWS</div>
                        <div class="sidebar-item active" onclick="startCampaignSim()">
                            <span class="item-icon"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon></svg></span> run swarm campaign
                        </div>
                        <div class="sidebar-item" onclick="startPolicySim()">
                            <span class="item-icon"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"></path></svg></span> verify vision policy
                        </div>
                        <div class="sidebar-item" onclick="startDagSim()">
                            <span class="item-icon"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"></path><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"></path></svg></span> audit merkle replay
                        </div>
                        <div class="sidebar-header" style="margin-top: 20px;">ACTIVE PERSONAS</div>
                        <div class="sidebar-persona">
                            <span class="persona-indicator pulse-emerald"></span>
                            <span class="persona-name">architect_primary</span>
                        </div>
                        <div class="sidebar-persona">
                            <span class="persona-indicator pulse-blue"></span>
                            <span class="persona-name">coder_synthesis</span>
                        </div>
                        <div class="sidebar-persona">
                            <span class="persona-indicator pulse-green"></span>
                            <span class="persona-name">tester_verification</span>
                        </div>
                    </div>
                    <div class="sim-editor">
                        <div class="editor-header" id="editor-file-name">architect.rs</div>
                        <div class="editor-body">
                            <pre class="code-output" id="term-output">
                                <!-- typing simulation content goes here -->
                            </pre>
                        </div>
                    </div>
                </div>
                
                <div class="sim-footer-controls">
                    <button class="sim-action-btn active" id="btn-sim-run" onclick="startCampaignSim()">Execute Swarm Sandbox</button>
                    <button class="sim-action-btn" id="btn-sim-policy" onclick="startPolicySim()">Verify OCR Intercepts</button>
                    <button class="sim-action-btn" id="btn-sim-dag" onclick="startDagSim()">Audit Cryptographic Ledger</button>
                </div>
            </div>
        </div>

        <div class="portals-grid">
            <a href="/dashboard" class="portal-card">
                <div class="portal-header">
                    <span class="portal-icon"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"></polygon></svg></span>
                    <span class="portal-tag">live dashboard</span>
                </div>
                <h3 class="portal-title">enter engineering Hub</h3>
                <p class="portal-desc">Observe live multi-persona agent execution streams, check real-time OCR visual intercepts, and authorize manual plan overrides.</p>
                <span class="portal-action">
                    <span>launch engineering Hub</span>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </span>
            </a>
            
            <div class="portal-card" onclick="openCliModal()">
                <div class="portal-header">
                    <span class="portal-icon"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect><line x1="8" y1="21" x2="16" y2="21"></line><line x1="12" y1="17" x2="12" y2="21"></line></svg></span>
                    <span class="portal-tag">cli engine</span>
                </div>
                <h3 class="portal-title">run campaign via cli</h3>
                <p class="portal-desc">Initiate highly isolated autonomous campaigns directly from your terminal. Full local workspace isolation and rollback support.</p>
                <span class="portal-action">
                    <span>reveal schema guidelines</span>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </span>
            </div>
            
            <div class="portal-card" onclick="openDagModal()">
                <div class="portal-header">
                    <span class="portal-icon"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"></path><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"></path></svg></span>
                    <span class="portal-tag">provenance ledger</span>
                </div>
                <h3 class="portal-title">verify provenance trace</h3>
                <p class="portal-desc">Audit the cryptographic attestation chain. Verify content-addressed Merkle hashes and ed25519 system signature paths.</p>
                <span class="portal-action">
                    <span>execute ledger verification</span>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="5" y1="12" x2="19" y2="12"></line><polyline points="12 5 19 12 12 19"></polyline></svg>
                </span>
            </div>
        </div>

        <!-- CLI Guide Drawer (Zero-Overlap Inline) -->
        <div class="modal-overlay" id="cli-modal" onclick="if(event.target === this) closeCliModal()">
            <div class="modal-card">
                <div class="modal-title">
                    <span style="display: flex; align-items: center; gap: 8px;"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect><line x1="8" y1="21" x2="16" y2="21"></line><line x1="12" y1="17" x2="12" y2="21"></line></svg> run campaign via cli</span>
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
                        <span class="cli-detail-key">--tui</span>
                        <span class="cli-detail-val">Launches Ratatui-based interactive terminal dashboard</span>
                    </div>
                    <div class="cli-detail-row">
                        <span class="cli-detail-key">--goal</span>
                        <span class="cli-detail-val">Bypasses plan/arena consensus prompts for autonomous running</span>
                    </div>
                </div>
                <div class="modal-actions">
                    <button class="btn-modal-close" onclick="closeCliModal()">close</button>
                </div>
            </div>
        </div>

        <!-- Provenance Drawer (Zero-Overlap Inline) -->
        <div class="modal-overlay" id="dag-modal" onclick="if(event.target === this) closeDagModal()">
            <div class="modal-card">
                <div class="modal-title">
                    <span style="display: flex; align-items: center; gap: 8px;"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"></path><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"></path></svg> provenance trace audit verifier</span>
                </div>
                <p class="modal-desc">Cryptographically verify the content-addressed chain of custody from genesis state through compilation release.</p>
                
                <div class="modal-dag-layout">
                    <div class="modal-dag-visual">
                        <svg width="100%" height="120" viewBox="0 0 540 120" id="mini-dag-svg">
                            <defs>
                                <marker id="arrow" viewBox="0 0 10 10" refX="24" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                                    <path d="M 0 1 L 10 5 L 0 9 z" fill="rgba(255, 255, 255, 0.15)"/>
                                </marker>
                                <linearGradient id="neon-grad" x1="0%" y1="0%" x2="100%" y2="0%">
                                    <stop offset="0%" stop-color="var(--accent-emerald)" />
                                    <stop offset="100%" stop-color="var(--accent-cyan)" />
                                </linearGradient>
                            </defs>
                            
                            <line x1="60" y1="60" x2="160" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                            <line x1="170" y1="60" x2="270" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                            <line x1="280" y1="60" x2="380" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                            <line x1="390" y1="60" x2="490" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                            
                            <g class="mini-node active" id="mn-0" onclick="selectMiniNode(0)">
                                <circle cx="60" cy="60" r="18" class="node-glow"></circle>
                                <circle cx="60" cy="60" r="14" class="node-base"></circle>
                                <text x="60" y="64">tx_0</text>
                            </g>
                            <g class="mini-node" id="mn-1" onclick="selectMiniNode(1)">
                                <circle cx="170" cy="60" r="18" class="node-glow"></circle>
                                <circle cx="170" cy="60" r="14" class="node-base"></circle>
                                <text x="170" y="64">tx_1</text>
                            </g>
                            <g class="mini-node" id="mn-2" onclick="selectMiniNode(2)">
                                <circle cx="280" cy="60" r="18" class="node-glow"></circle>
                                <circle cx="280" cy="60" r="14" class="node-base"></circle>
                                <text x="280" y="64">tx_2</text>
                            </g>
                            <g class="mini-node" id="mn-3" onclick="selectMiniNode(3)">
                                <circle cx="390" cy="60" r="18" class="node-glow"></circle>
                                <circle cx="390" cy="60" r="14" class="node-base"></circle>
                                <text x="390" y="64">tx_3</text>
                            </g>
                            <g class="mini-node" id="mn-4" onclick="selectMiniNode(4)">
                                <circle cx="500" cy="60" r="18" class="node-glow"></circle>
                                <circle cx="500" cy="60" r="14" class="node-base"></circle>
                                <text x="500" y="64">tx_4</text>
                            </g>
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

        <div class="matrix-section" id="matrix">
            <h2 class="matrix-title">runtime specification matrix</h2>
            <p class="matrix-subtitle">Every building block of Korg is engineered for deterministic, high-assurance software synthesis.</p>
            <div class="matrix-grid">
                <div class="matrix-card">
                    <div class="matrix-card-title">
                        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="var(--accent-emerald)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v8"/><path d="m17 7-5 3-5-3"/><rect x="3" y="14" width="6" height="6" rx="1"/><rect x="15" y="14" width="6" height="6" rx="1"/></svg>
                        <span>tamper-proof history (merkle ledger)</span>
                    </div>
                    <p class="matrix-card-text">Every action Korg takes is recorded into a secure history chain. This makes it impossible for the AI to hide its steps, allowing you to replay and audit everything.</p>
                </div>
                <div class="matrix-card">
                    <div class="matrix-card-title">
                        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="var(--accent-cyan)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/></svg>
                        <span>visual safety &amp; screen guardrails</span>
                    </div>
                    <p class="matrix-card-text">Korg continuously takes screenshots and scans its own work. If it accidentally exposes a password, API key, or private data, it automatically blurs it out to protect your security.</p>
                </div>
                <div class="matrix-card">
                    <div class="matrix-card-title">
                        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="var(--accent-emerald)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/></svg>
                        <span>autonomous team testing (swarm)</span>
                    </div>
                    <p class="matrix-card-text">Multiple specialized AI bots review and test the code against five different quality checks before saving. They check each other's work so you don't have to.</p>
                </div>
                <div class="matrix-card">
                    <div class="matrix-card-title">
                        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="var(--accent-cyan)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><line x1="9" y1="3" x2="9" y2="21"/><line x1="15" y1="3" x2="15" y2="21"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="3" y1="15" x2="21" y2="15"/></svg>
                        <span>safe isolated sandbox (git worktrees)</span>
                    </div>
                    <p class="matrix-card-text">Korg works in temporary, isolated safe spaces. If a test fails or something breaks, Korg simply discards the sandbox and rolls back, keeping your main project completely untouched and safe.</p>
                </div>
            </div>
        </div>

        <!-- Comparative Matrix Section (xAI/Grok inspired) -->
        <div class="comparison-section" id="comparison">
            <h2 class="comparison-title">korg vs. traditional tools</h2>
            <p class="comparison-subtitle">See how Korg's autonomous engineering runtime compares to standard AI code editors and command-line scripts.</p>
            
            <div class="comparison-table-container">
                <table class="comparison-table">
                    <thead>
                        <tr>
                            <th>Capability</th>
                            <th class="korg-col">Korg Swarm Runtime</th>
                            <th>Traditional AI IDEs (e.g. Cursor)</th>
                            <th>Standard CLI Bots</th>
                        </tr>
                    </thead>
                    <tbody>
                        <tr>
                            <td class="feature-col">Who writes &amp; runs the code?</td>
                            <td class="korg-col">
                                <span class="comparison-badge yes">Yes (Autonomous Swarm)</span>
                                <div class="table-text">A collaborative team of AI agents (Architect, Coder, and Tester) writes code, runs builds, and fixes bugs autonomously in the background.</div>
                            </td>
                            <td>
                                <span class="comparison-badge partial">Partial (Autocompletes)</span>
                                <div class="table-text">Suggests edits line-by-line. You still have to manually run the build, test for errors, and prompt for fixes.</div>
                            </td>
                            <td>
                                <span class="comparison-badge partial">Partial (Single script)</span>
                                <div class="table-text">Applies static edits sequentially, but lacks multi-persona teamwork and self-contained sandbox execution.</div>
                            </td>
                        </tr>
                        <tr>
                            <td class="feature-col">Built-in Safe Testing?</td>
                            <td class="korg-col">
                                <span class="comparison-badge yes">Yes (Adversarial Sandbox)</span>
                                <div class="table-text">Every change is built, checked, and tested inside isolated safe sandboxes before being committed to your main branch.</div>
                            </td>
                            <td>
                                <span class="comparison-badge no">No (Runs on Host)</span>
                                <div class="table-text">Writes code directly into your active project workspace. If the code breaks or contains malware, your local environment suffers.</div>
                            </td>
                            <td>
                                <span class="comparison-badge no">No</span>
                                <div class="table-text">Edits are applied directly onto your physical files with zero isolated compiler testing.</div>
                            </td>
                        </tr>
                        <tr>
                            <td class="feature-col">Security Guardrails?</td>
                            <td class="korg-col">
                                <span class="comparison-badge yes">Yes (Screen &amp; OCR Policy)</span>
                                <div class="table-text">Active screen checks and text recognition look at terminal outputs and screenshots to automatically blur and block API keys or secret leaks.</div>
                            </td>
                            <td>
                                <span class="comparison-badge no">No</span>
                                <div class="table-text">No active UI or screenshot safety firewalls. Relies purely on text-based prompt rules which are easily bypassed.</div>
                            </td>
                            <td>
                                <span class="comparison-badge no">No</span>
                                <div class="table-text">No visual safety filters or automated secret detection models.</div>
                            </td>
                        </tr>
                        <tr>
                            <td class="feature-col">Verifiable History Ledger?</td>
                            <td class="korg-col">
                                <span class="comparison-badge yes">Yes (Provenance Ledger)</span>
                                <div class="table-text">Logs all actions to a cryptographically secure, tamper-proof audit trail (Merkle Ledger) so you can verify exactly what the AI did.</div>
                            </td>
                            <td>
                                <span class="comparison-badge no">No</span>
                                <div class="table-text">Scattered logs that are difficult to trace. No verifiable cryptographic signatures or step-by-step history audits.</div>
                            </td>
                            <td>
                                <span class="comparison-badge no">No</span>
                                <div class="table-text">Standard git commits only, which don't prove the security or origin of the AI's internal process.</div>
                            </td>
                        </tr>
                    </tbody>
                </table>
            </div>
        </div>
    </main>

    <footer>
        korg v0.2.0 — autonomous software engineering runtime — cryptographically secure
    </footer>

    <!-- CLI Guide Modal -->
    <div class="modal-overlay" id="cli-modal" onclick="if(event.target === this) closeCliModal()">
        <div class="modal-card">
            <div class="modal-title">
                <span style="display: flex; align-items: center; gap: 8px;"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><rect x="2" y="3" width="20" height="14" rx="2" ry="2"></rect><line x1="8" y1="21" x2="16" y2="21"></line><line x1="12" y1="17" x2="12" y2="21"></line></svg> run campaign via cli</span>
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
                    <span class="cli-detail-key">--tui</span>
                    <span class="cli-detail-val">Launches Ratatui-based interactive terminal dashboard</span>
                </div>
                <div class="cli-detail-row">
                    <span class="cli-detail-key">--goal</span>
                    <span class="cli-detail-val">Bypasses plan/arena consensus prompts for autonomous running</span>
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
                <span style="display: flex; align-items: center; gap: 8px;"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" style="display: inline-block; vertical-align: middle;"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"></path><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"></path></svg> provenance trace audit verifier</span>
            </div>
            <p class="modal-desc">Cryptographically verify the content-addressed chain of custody from genesis state through compilation release.</p>
            
            <div class="modal-dag-layout">
                <div class="modal-dag-visual">
                    <svg width="100%" height="120" viewBox="0 0 540 120" id="mini-dag-svg">
                        <defs>
                            <marker id="arrow" viewBox="0 0 10 10" refX="24" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                                <path d="M 0 1 L 10 5 L 0 9 z" fill="rgba(255, 255, 255, 0.15)"/>
                            </marker>
                            <linearGradient id="neon-grad" x1="0%" y1="0%" x2="100%" y2="0%">
                                <stop offset="0%" stop-color="var(--accent-emerald)" />
                                <stop offset="100%" stop-color="var(--accent-cyan)" />
                            </linearGradient>
                        </defs>
                        
                        <line x1="60" y1="60" x2="160" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                        <line x1="170" y1="60" x2="270" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                        <line x1="280" y1="60" x2="380" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                        <line x1="390" y1="60" x2="490" y2="60" class="mini-edge" marker-end="url(#arrow)"></line>
                        
                        <g class="mini-node active" id="mn-0" onclick="selectMiniNode(0)">
                            <circle cx="60" cy="60" r="18" class="node-glow"></circle>
                            <circle cx="60" cy="60" r="14" class="node-base"></circle>
                            <text x="60" y="64">tx_0</text>
                        </g>
                        <g class="mini-node" id="mn-1" onclick="selectMiniNode(1)">
                            <circle cx="170" cy="60" r="18" class="node-glow"></circle>
                            <circle cx="170" cy="60" r="14" class="node-base"></circle>
                            <text x="170" y="64">tx_1</text>
                        </g>
                        <g class="mini-node" id="mn-2" onclick="selectMiniNode(2)">
                            <circle cx="280" cy="60" r="18" class="node-glow"></circle>
                            <circle cx="280" cy="60" r="14" class="node-base"></circle>
                            <text x="280" y="64">tx_2</text>
                        </g>
                        <g class="mini-node" id="mn-3" onclick="selectMiniNode(3)">
                            <circle cx="390" cy="60" r="18" class="node-glow"></circle>
                            <circle cx="390" cy="60" r="14" class="node-base"></circle>
                            <text x="390" y="64">tx_3</text>
                        </g>
                        <g class="mini-node" id="mn-4" onclick="selectMiniNode(4)">
                            <circle cx="500" cy="60" r="18" class="node-glow"></circle>
                            <circle cx="500" cy="60" r="14" class="node-base"></circle>
                            <text x="500" y="64">tx_4</text>
                        </g>
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
                btn.innerText = "copied! ✓";
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
                        <div class="prop-val" style="color: var(--accent-emerald); font-weight: bold;">${data.tx}</div>
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
                        <div class="prop-val" style="color: var(--accent-green);">${data.signature}</div>
                    </div>
                    <div class="prop-row">
                        <div class="prop-key">State Root</div>
                        <div class="prop-val" style="color: var(--accent-cyan); font-family: var(--font-mono);">${data.state_root}</div>
                    </div>
                    <div class="prop-row">
                        <div class="prop-key">Status</div>
                        <div class="prop-val" style="color: ${data.tx.includes('tx_03') ? 'var(--accent-gold)' : '#ffffff'}; font-weight: 500;">${data.status}</div>
                    </div>
                </div>
            `;
        }

        // Terminal Interactive Simulator Logic
        let simInterval = null;
        const termElement = document.getElementById("term-output");

        const simulatorScripts = {
            run: [
                { type: "input", text: "korg campaign --web --prompt \"Refactor database pool size allocation\"" },
                { type: "output", text: "[korg] Initializing campaign environment...", color: "#94a3b8" },
                { type: "output", text: "[korg] Creating transient isolation sandbox (git worktree)...", color: "#94a3b8" },
                { type: "output", text: "[korg] Sandbox created at: /tmp/korg-worktree-a8f3", color: "#64748b" },
                { type: "output", text: "[korg] Spawning autonomous swarm (3 personas active):", color: "#ffffff" },
                { type: "output", text: "   ▸ [architect] Designing execution layout...", color: "#6ee7b7" },
                { type: "output", text: "   ▸ [coder] Generating patch for src/db.rs...", color: "#38bdf8" },
                { type: "output", text: "   ▸ [tester] Synthesizing adversarial verification suite...", color: "#34d399" },
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
                { type: "output", text: "[policy-engine] Booting zero-trust visual intercept interceptor...", color: "#94a3b8" },
                { type: "output", text: "[policy-engine] Monitoring active workspace GUI state...", color: "#94a3b8" },
                { type: "output", text: "[policy-engine] Screenshot triggered by tester persona.", color: "#ffffff" },
                { type: "output", text: "[policy-engine] Processing screenshot_382.png through vision firewall...", color: "#ffffff" },
                { type: "output", text: "   ▸ Scanning metadata and OCR layers...", color: "#64748b" },
                { type: "output", text: "   ⚠ VIOLATION DETECTED: Found string pattern 'DATABASE_PASSWORD=********' in visual OCR buffer!", color: "#ef4444" },
                { type: "output", text: "   ⚠ FAIL-SECURE POLICY ACTIVATED: Triggering zero-trust filter.", color: "#ef4444" },
                { type: "output", text: "[policy-engine] Redacting raw screenshot in memory...", color: "#ffffff" },
                { type: "output", text: "   ▸ Method: Grayscale Overlay + Total Blur redaction", color: "#94a3b8" },
                { type: "output", text: "   ✔ Screenshot redacted. Safe base64 broadcast emitted.", color: "#10b981" },
                { type: "output", text: "[policy-engine] Attestation tx_03 recorded: OCR_VIOLATION_AUTO_REDACTED", color: "#10b981" },
                { type: "output", text: "[policy-engine] No raw sensitive credentials escaped the sandbox.", color: "#ffffff" }
            ],
            dag: [
                { type: "input", text: "korg dag log --tx tx_04" },
                { type: "output", text: "[korg-dag] Content-Addressed Merkle ledger audit trace:", color: "#ffffff" },
                { type: "output", text: "--------------------------------------------------------", color: "#64748b" },
                { type: "output", text: "Transaction: tx_04", color: "#ffffff" },
                { type: "output", text: "Parent Hash: e3b0c44298fc1c149afbf4c8996fb92427ae41e4...", color: "#94a3b8" },
                { type: "output", text: "State Root:  9f3c2b8a7d5e4f3c2b1a0d9e8f7a6b5c4d3e2f1a...", color: "#94a3b8" },
                { type: "output", text: "Signature:   ed25519::verified [attester: leader_primary]", color: "#10b981" },
                { type: "output", text: "Payload Type: RELEASE_COMMIT", color: "#ffffff" },
                { type: "output", text: "Diff Attestation:", color: "#94a3b8" },
                { type: "output", text: "   + modified: src/web.rs (monochrome layout upgrade)", color: "#10b981" },
                { type: "output", text: "   + verified: adversarial-arena compiler passes", color: "#10b981" },
                { type: "output", text: "Cryptographic Attestation Chain: VALID", color: "#10b981" },
                { type: "output", text: "   Genesis (tx_00) ➔ Plan (tx_01) ➔ Synthesis (tx_02) ➔ Intercept (tx_03) ➔ Release (tx_04)", color: "#ffffff" }
            ]
        };

        const fileMockContents = {
            architect: `// korg - Persona: Architect Suite
// Speculating system changes under ACP-V2 protocol.
pub struct ArchitectPersona {
    blackboard: Arc<RwLock<Blackboard>>,
    cognitive_depth: u32,
}

impl Persona for ArchitectPersona {
    fn design_plan(&self, goal: &Goal) -> Result<Plan> {
        log::info!("Decomposing execution space...");
        let steps = vec![
            Step::new("Analyze Database connections"),
            Step::new("Patch src/db.rs with ConnectionPool"),
            Step::new("Run Speculative arena check")
        ];
        Ok(Plan::formulate(steps))
    }
}`,
            coder: `// korg - Persona: Coder Speculative Engine
// Translating structural plan steps into Rust code commits.
use tokio::fs;

pub async fn apply_synthesis(patch: &Patch) -> Result<Attestation> {
    log::info!("Synthesizing modifications in isolated workspace...");
    let workspace = Worktree::create_temp().await?;
    
    // Apply patch
    fs::write(workspace.join("src/db.rs"), patch.code()).await?;
    
    log::info!("Verifying compiler flags...");
    workspace.cargo_check().await?;
    
    Ok(Attestation::from_workspace(&workspace))
}`,
            tester: `// korg - Persona: Tester Arena Suite
// Subjecting synthesize patches to adversarial validation criteria.

#[tokio::test]
async fn test_adversarial_security_leaks() {
    let sandbox = Sandbox::boot_isolated().await;
    let scanner = SecretScanner::new(Policy::zero_trust());
    
    // Intercept code structures for keys/tokens
    let violations = scanner.scan_files(sandbox.files()).await;
    assert!(violations.is_empty(), "Violations found: {:?}", violations);
}`,
            ledger: `{
  "attestation_chain": {
    "tx_00": { "event": "SYSTEM_GENESIS", "status": "finalized" },
    "tx_01": { "event": "PLAN_FORMULATION", "status": "approved" },
    "tx_02": { "event": "WORKSPACE_SYNTHESIS", "status": "green" },
    "tx_03": { "event": "POLICY_REDISTRIBUTION", "status": "redacted" },
    "tx_04": { "event": "RELEASE_COMMIT", "status": "finalized" }
  }
}`
        };

        function switchSimTab(tabId) {
            document.querySelectorAll(".sim-tab").forEach(tab => tab.classList.remove("active"));
            const tabEl = document.querySelector(`[onclick="switchSimTab('${tabId}')"]`);
            if(tabEl) tabEl.classList.add("active");
            
            document.getElementById("editor-file-name").textContent = tabId === 'ledger' ? 'ledger.json' : `${tabId}.rs`;
            
            // Set editor text content
            const codeOutput = document.getElementById("term-output");
            codeOutput.style.color = tabId === 'ledger' ? 'var(--accent-cyan)' : 'rgba(255, 255, 255, 0.7)';
            codeOutput.textContent = fileMockContents[tabId];
            
            // Pause any running log simulations
            if (simInterval) {
                clearInterval(simInterval);
                simInterval = null;
            }
            
            // Set action active tab
            document.querySelectorAll(".sim-action-btn").forEach(btn => btn.classList.remove("active"));
            document.querySelectorAll(".sidebar-item").forEach(item => item.classList.remove("active"));
        }

        function setSimButtonActive(simId) {
            document.querySelectorAll(".sim-action-btn").forEach(btn => btn.classList.remove("active"));
            document.querySelectorAll(".sidebar-item").forEach(item => item.classList.remove("active"));
            
            if (simId === "run") {
                document.getElementById("btn-sim-run").classList.add("active");
                document.querySelector(`[onclick="startCampaignSim()"]`).classList.add("active");
            }
            if (simId === "policy") {
                document.getElementById("btn-sim-policy").classList.add("active");
                document.querySelector(`[onclick="startPolicySim()"]`).classList.add("active");
            }
            if (simId === "dag") {
                document.getElementById("btn-sim-dag").classList.add("active");
                document.querySelector(`[onclick="startDagSim()"]`).classList.add("active");
            }
        }

        function runSimScript(script) {
            if (simInterval) clearInterval(simInterval);
            termElement.innerHTML = "";
            let lineIndex = 0;
            
            function printNextLine() {
                if (lineIndex >= script.length) return;
                
                const line = script[lineIndex];
                const div = document.createElement("div");
                div.style.marginBottom = "4px";
                
                if (line.type === "input") {
                    div.innerHTML = `<span style="color: var(--accent-emerald); user-select: none; font-weight: bold;">$</span> <span style="color: #ffffff; font-weight: 500;"></span>`;
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
                            setTimeout(printNextLine, 400);
                        }
                    }, 15);
                } else {
                    div.style.color = line.color || "var(--text-secondary)";
                    div.textContent = line.text;
                    termElement.appendChild(div);
                    termElement.scrollTop = termElement.scrollHeight;
                    lineIndex++;
                    setTimeout(printNextLine, 350);
                }
            }
            
            printNextLine();
        }

        function startCampaignSim() {
            setSimButtonActive("run");
            document.getElementById("editor-file-name").textContent = "terminal://campaign-log";
            document.querySelectorAll(".sim-tab").forEach(tab => tab.classList.remove("active"));
            termElement.style.color = "var(--text-secondary)";
            runSimScript(simulatorScripts.run);
        }

        function startPolicySim() {
            setSimButtonActive("policy");
            document.getElementById("editor-file-name").textContent = "terminal://policy-firewall";
            document.querySelectorAll(".sim-tab").forEach(tab => tab.classList.remove("active"));
            termElement.style.color = "var(--text-secondary)";
            runSimScript(simulatorScripts.policy);
        }

        function startDagSim() {
            setSimButtonActive("dag");
            document.getElementById("editor-file-name").textContent = "terminal://merkle-audit";
            document.querySelectorAll(".sim-tab").forEach(tab => tab.classList.remove("active"));
            termElement.style.color = "var(--text-secondary)";
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
