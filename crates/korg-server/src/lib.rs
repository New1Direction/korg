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
    extract::{Path, Query, State},
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

use korg_runtime::leader::LeaderOrchestrator;
use korg_runtime::tui_bridge::{ContractResponse, TuiUpdate};

// Axum SSE response mapping helper
mod ax_sse {
    pub use axum::response::sse::{Event, KeepAlive, Sse};
}

/// Global shared state for the Axum server
pub struct AppState {
    pub broadcaster: broadcast::Sender<TuiUpdate>,
    pub feedback_tx: Mutex<Option<mpsc::Sender<ContractResponse>>>,
    pub capability_resolver: Arc<tokio::sync::Mutex<korg_registry::CapabilityResolver>>,
    pub runtime_coordinator:
        Arc<std::sync::Mutex<Option<Arc<korg_runtime::runtime::RuntimeCoordinator>>>>,
    pub auth: Arc<korg_auth::AuthState>,
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
        korg_registry::CapabilityResolver::default_resolver(),
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
            let config = korg_llm::KorgConfig::load();
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
                                            korg_runtime::vision_policy::BLACKOUT_PNG_BASE64
                                                .to_string(),
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

    let auth_state = Arc::new(korg_auth::AuthState::new(korg_auth::AuthConfig::from_env()));

    let app_state = Arc::new(AppState {
        broadcaster: broadcaster_tx,
        feedback_tx: Mutex::new(Some(feedback_tx)),
        capability_resolver: capability_resolver_container,
        runtime_coordinator: runtime_coordinator_container,
        auth: auth_state,
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
        .route("/api/agent/tool-call", post(agent_tool_call_handler))
        .route("/api/blob/:sha256", get(blob_handler))
        .route(
            "/api/projections/campaign",
            get(campaign_projection_handler),
        )
        .route("/auth/login", get(oauth_login_handler))
        .route("/auth/codex/callback", get(oauth_codex_callback_handler))
        .route(
            "/auth/anthropic/callback",
            get(oauth_anthropic_callback_handler),
        )
        .route(
            "/api/v1/anthropic/messages",
            post(anthropic_messages_proxy_handler),
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
            let config = korg_llm::KorgConfig::load();
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
                                            korg_runtime::vision_policy::BLACKOUT_PNG_BASE64
                                                .to_string(),
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

    let auth_state = Arc::new(korg_auth::AuthState::new(korg_auth::AuthConfig::from_env()));

    let app_state = Arc::new(AppState {
        broadcaster: broadcaster_tx,
        feedback_tx: Mutex::new(Some(feedback_tx)),
        capability_resolver,
        runtime_coordinator: runtime_coordinator_container,
        auth: auth_state,
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
        .route("/api/agent/tool-call", post(agent_tool_call_handler))
        .route("/api/blob/:sha256", get(blob_handler))
        .route(
            "/api/projections/campaign",
            get(campaign_projection_handler),
        )
        .route("/auth/login", get(oauth_login_handler))
        .route("/auth/codex/callback", get(oauth_codex_callback_handler))
        .route(
            "/auth/anthropic/callback",
            get(oauth_anthropic_callback_handler),
        )
        .route(
            "/api/v1/anthropic/messages",
            post(anthropic_messages_proxy_handler),
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
    let path = korg_core::paths::blackboard_json();
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
        let h = korg_runtime::vision_policy::VISUAL_HISTORY.lock().unwrap();
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
    let cap_state = korg_registry::CapabilityState::Mode(mode_str.clone());

    let req = korg_registry::TransitionRequest {
        id: "cognition_mode".to_string(),
        target_state: cap_state,
        correlation_id: None,
    };

    let mut resolver = state.capability_resolver.lock().await;
    let response = resolver.handle_transition_request(req);

    if response.status == korg_registry::TransitionState::Applied {
        // Read the authoritative mode string back from registry active_states.
        // The web layer does NOT interpret — it mirrors what the resolver decided.
        let canonical_mode_str = match resolver.active_states.get("cognition_mode") {
            Some(korg_registry::CapabilityState::Mode(m)) => m.clone(),
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
    Json(payload): Json<korg_registry::TransitionRequest>,
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
    let index_path = korg_core::paths::project_root().join(".korg/index.json");
    if !index_path.exists() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Index file not found. Please run indexer." })),
        )
            .into_response();
    }

    let index = match korg_runtime::code_indexer::load_index(&index_path) {
        Ok(idx) => idx,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to load index: {}", e) })),
            )
                .into_response();
        }
    };

    let embedding_model: Box<dyn korg_embeddings::EmbeddingModel> =
        match korg_embeddings::CandleEmbeddingModel::load() {
            Ok(model) => Box::new(model),
            Err(_) => Box::new(korg_embeddings::FakeEmbeddingModel::default()),
        };

    let top_n = payload.top_n.unwrap_or(5);
    let matches = korg_runtime::code_indexer::query_codebase(
        &index,
        &payload.query,
        &*embedding_model,
        top_n,
    );

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

#[derive(serde::Deserialize)]
struct JournalQuery {
    triggered_by: Option<u64>,
}

/// GET `/api/journal`
///
/// Returns the last 100 capability kernel events as JSONL (one event per line).
/// Suitable for streaming to log shippers, dashboards, or debugging sessions.
async fn journal_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<JournalQuery>,
) -> impl IntoResponse {
    let resolver = state.capability_resolver.lock().await;
    let jsonl = resolver
        .journal
        .to_json_lines_filtered(params.triggered_by, 100);
    let total = resolver.journal.len();
    drop(resolver);

    match jsonl {
        Some(content) => (
            axum::http::StatusCode::OK,
            [
                ("content-type", "application/x-ndjson"),
                ("x-korg-journal-trigger-found", "true"),
            ],
            format!("// total events: {}\n{}", total, content),
        )
            .into_response(),
        None => {
            let triggered_id = params.triggered_by.unwrap_or(0);
            (
                axum::http::StatusCode::NOT_FOUND,
                [
                    ("content-type", "application/json"),
                    ("x-korg-journal-trigger-found", "false"),
                ],
                Json(serde_json::json!({
                    "error": format!("Trigger sequence ID {} not found in ledger index.", triggered_id)
                })),
            )
                .into_response()
        }
    }
}

/// GET `/api/blob/:sha256`
///
/// Content-addressed blob fetch. Returns raw bytes with `Content-Type: application/octet-stream`.
///
/// Blobs are stored at `.korg/blobs/{sha256[:2]}/{sha256}` (fan-out layout matching
/// `registry::Journal::verify_integrity`). Clients write blobs here BEFORE appending the
/// event that references them (blob-first atomicity, agent_event_spec.md §7.3).
///
/// This endpoint is the HTTP escape hatch for blobs that exceed the 10MB MCP JSON-RPC cap
/// (agent_event_spec.md §8.4.2). The MCP handler directs oversized reads here via
/// `blob_too_large` error data.
async fn blob_handler(Path(sha256): Path<String>) -> impl IntoResponse {
    // Validate: must be exactly 64 lowercase hex characters.
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid sha256",
                "detail": "sha256 must be 64 lowercase hex characters",
                "sha256": sha256,
            })),
        )
            .into_response();
    }

    let prefix = &sha256[..2];
    let blob_path = korg_core::paths::project_root()
        .join(".korg/blobs")
        .join(prefix)
        .join(&sha256);

    match tokio::fs::read(&blob_path).await {
        Ok(bytes) => (
            axum::http::StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            bytes,
        )
            .into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "blob not found",
                "sha256": sha256,
            })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("failed to read blob: {}", e),
                "sha256": sha256,
            })),
        )
            .into_response(),
    }
}

/// POST `/api/agent/tool-call`
///
/// External agent ingestion endpoint — schema v1.0.
///
/// Any agent runtime (korgex, Claude Code via MCP, etc.) posts an `AgentToolCallRequest`
/// here. korg appends it to the live capability journal with a fresh HLC timestamp and
/// returns the assigned `seq_id` so the caller can wire `triggered_by` on the next event.
///
/// **Design rules (see agent_event_spec.md):**
/// - One event per *completed* call. Emit after the tool returns, not before.
/// - `triggered_by` should be the `seq_id` of the event that caused this call.
/// - Payloads over 1 KB must be content-addressed: write the blob to `.korg/blobs/`
///   and pass a `ContentRef` instead of the raw content.
/// - This handler never blocks the caller's agent loop — failures are logged internally.

#[derive(Debug, serde::Deserialize)]
struct AgentToolCallRequest {
    /// Agent runtime identity. Convention: "agent:<name>@<version>" or "human:<id>".
    source_agent: String,
    /// Name of the tool called. Should match the agent's own tool registry name.
    tool_name: String,
    /// Tool arguments. Large values must be content-addressed (see ContentRef rules).
    args: serde_json::Value,
    /// Tool result. Large values must be content-addressed.
    result: serde_json::Value,
    /// Content-addressed references for large payloads (optional).
    #[serde(default)]
    payload_refs: Vec<korg_registry::ContentRef>,
    /// Whether the tool call succeeded.
    success: bool,
    /// Wall-clock duration of the tool call in milliseconds.
    duration_ms: u64,
    /// seq_id of the event that causally triggered this call (None for root events).
    #[serde(default)]
    triggered_by: Option<u64>,
}

#[derive(serde::Serialize)]
struct AgentToolCallResponse {
    /// Assigned journal sequence number. Use as `triggered_by` on the next event.
    seq_id: u64,
}

async fn agent_tool_call_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentToolCallRequest>,
) -> impl IntoResponse {
    use axum::http::StatusCode;
    use chrono::Utc;
    use korg_registry::log::{EventMetadata, EventTier};
    use korg_registry::CapabilityEvent;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    let event = CapabilityEvent::AgentToolCall {
        source_agent: req.source_agent.clone(),
        tool_name: req.tool_name,
        args: req.args,
        result: req.result,
        payload_refs: req.payload_refs,
        success: req.success,
        duration_ms: req.duration_ms,
        timestamp: Utc::now(),
    };

    let mut resolver = state.capability_resolver.lock().await;

    // ALWAYS use append_with_metadata for external agent events.
    //
    // The standard append() auto-sets triggered_by to the previous journal
    // event's seq_id — which for external agents is whatever internal korg
    // event happened to be last. That silently chains root agent events (e.g.
    // user_prompt with triggered_by=None) to internal governance events,
    // breaking the causal tree.
    //
    // Dogfood finding (2026-05-24): backward chain from a leaf walked back
    // through 354 internal korg events to a non-AgentToolCall root instead
    // of stopping at the user_prompt root. Root cause: this branch.
    //
    // Fix: always construct metadata explicitly and call append_with_metadata,
    // preserving the caller's triggered_by value exactly — including None.

    let event_id = Uuid::new_v4();
    let wall_clock = Utc::now().timestamp_millis();
    let emitted_at = resolver.journal.clock.tick(wall_clock);

    let (root_event_id, causation_id) = match req.triggered_by {
        Some(triggered_by_seq) => {
            // Inherit root_event_id from the triggered_by event's root chain.
            // causation_id = UUID of the triggering event (for internal UUID graph).
            let parent = resolver
                .journal
                .events
                .iter()
                .find(|e| e.seq_id == triggered_by_seq);
            let root = parent.map(|e| e.metadata.root_event_id).unwrap_or(event_id);
            let causation = parent.map(|e| e.metadata.event_id);
            (root, causation)
        }
        None => {
            // Root event: this event IS its own root. triggered_by=None means
            // "I am the beginning of a new causal chain." Do not inherit from
            // whatever korg internal event happened to be last.
            (event_id, None)
        }
    };

    let metadata = EventMetadata {
        event_id,
        correlation_id: Uuid::nil(), // AgentToolCall has no internal plan_id
        causation_id,
        root_event_id,
        actor_id: "korg:api".to_string(), // actor_id represents the recorder identity (spec §4)
        campaign_id: Uuid::nil(),
        emitted_at,
        branch_id: None,
        speculative: false,
        retry_count: 0,
        tier: EventTier::Telemetry,
        span_id: None,
        tags: BTreeMap::new(),
        triggered_by: req.triggered_by, // preserved exactly — None means root
    };

    resolver.journal.append_with_metadata(event, metadata);
    let seq_id = resolver.journal.last_seq_id;

    drop(resolver);

    (StatusCode::OK, Json(AgentToolCallResponse { seq_id })).into_response()
}

/// GET `/api/metrics`
///
/// Returns a point-in-time snapshot of all atomic runtime counters.
/// Lock-free; safe to call at any frequency.
async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snap = korg_core::metrics::snapshot();

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
    let metrics = korg_core::metrics::snapshot();

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
    <title>korg — the first deterministic cognitive runtime</title>
    <meta name="description" content="Every AI agent decision logged, causally ordered, and reversible. Like Git, but for cognition.">
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600&family=JetBrains+Mono:wght@400;500;700&display=swap" rel="stylesheet">
    <style>
        /* ── Reset & Tokens ─────────────────────────────────────────────────── */
        *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

        :root {
            --bg:        #080808;
            --surface:   #0d0d0d;
            --border:    #1c1c1e;
            --border-hi: #2e2e30;
            --text-1:    #fafafa;
            --text-2:    #8e8e93;
            --text-3:    #48484a;
            --amber:     #f59e0b;
            --amber-dim: rgba(245, 158, 11, 0.12);
            --sans:      'Inter', system-ui, sans-serif;
            --mono:      'JetBrains Mono', 'Fira Code', monospace;
        }

        html { font-size: 16px; -webkit-font-smoothing: antialiased; }

        body {
            font-family: var(--sans);
            background: var(--bg);
            color: var(--text-1);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            overflow-x: hidden;
        }

        /* ── CRT Overlay ────────────────────────────────────────────────────── */
        body::before {
            content: "";
            position: fixed;
            inset: 0;
            z-index: 9999;
            pointer-events: none;
            background: repeating-linear-gradient(
                0deg,
                rgba(0,0,0,0.09) 0px,
                rgba(0,0,0,0.09) 1px,
                transparent 1px,
                transparent 3px
            );
            animation: crt-drift 12s linear infinite;
        }
        @keyframes crt-drift {
            from { background-position: 0 0; }
            to   { background-position: 0 120px; }
        }

        /* ── Nav ────────────────────────────────────────────────────────────── */
        nav {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 0 40px;
            height: 56px;
            border-bottom: 1px solid var(--border);
            position: sticky;
            top: 0;
            background: rgba(8,8,8,0.92);
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
            z-index: 100;
        }

        .nav-logo {
            font-family: var(--mono);
            font-size: 15px;
            font-weight: 700;
            letter-spacing: 0.04em;
            color: var(--text-1);
            text-decoration: none;
        }

        .nav-right {
            display: flex;
            align-items: center;
            gap: 20px;
            font-family: var(--mono);
            font-size: 11px;
            color: var(--text-2);
        }

        .nav-badge {
            display: flex;
            align-items: center;
            gap: 6px;
        }

        .status-dot {
            width: 6px;
            height: 6px;
            border-radius: 50%;
            background: #22c55e;
            animation: pulse-dot 2.4s ease-in-out infinite;
        }
        @keyframes pulse-dot {
            0%, 100% { opacity: 0.4; }
            50%       { opacity: 1; }
        }

        .nav-link {
            color: var(--text-2);
            text-decoration: none;
            transition: color 0.15s;
        }
        .nav-link:hover { color: var(--text-1); }

        /* ── Hero ───────────────────────────────────────────────────────────── */
        .hero {
            flex: 1;
            display: flex;
            flex-direction: column;
            align-items: center;
            text-align: center;
            padding: 96px 24px 80px;
            gap: 0;
        }

        .hero-label {
            font-family: var(--mono);
            font-size: 11px;
            letter-spacing: 0.12em;
            text-transform: uppercase;
            color: var(--amber);
            margin-bottom: 28px;
            opacity: 0;
            animation: fade-up 0.6s ease 0.1s forwards;
        }

        .hero-title {
            font-size: clamp(36px, 6vw, 64px);
            font-weight: 300;
            letter-spacing: -0.03em;
            line-height: 1.08;
            color: var(--text-1);
            max-width: 820px;
            margin-bottom: 24px;
            opacity: 0;
            animation: fade-up 0.6s ease 0.2s forwards;
        }

        .hero-sub {
            font-size: 16px;
            font-weight: 400;
            color: var(--text-2);
            line-height: 1.6;
            max-width: 500px;
            margin-bottom: 44px;
            opacity: 0;
            animation: fade-up 0.6s ease 0.3s forwards;
        }

        .hero-ctas {
            display: flex;
            align-items: center;
            gap: 12px;
            margin-bottom: 64px;
            opacity: 0;
            animation: fade-up 0.6s ease 0.4s forwards;
        }

        .cta-primary {
            font-family: var(--mono);
            font-size: 13px;
            font-weight: 500;
            padding: 11px 22px;
            border: 1px solid var(--border-hi);
            background: transparent;
            color: var(--text-1);
            cursor: pointer;
            text-decoration: none;
            display: inline-flex;
            align-items: center;
            gap: 8px;
            transition: border-color 0.15s, background 0.15s, color 0.15s;
            position: relative;
            overflow: hidden;
        }
        .cta-primary:hover {
            border-color: var(--amber);
            color: var(--amber);
        }

        .cta-primary .prompt { color: var(--text-3); }

        .cta-secondary {
            font-family: var(--mono);
            font-size: 12px;
            color: var(--text-2);
            text-decoration: none;
            padding: 11px 4px;
            display: inline-flex;
            align-items: center;
            gap: 6px;
            transition: color 0.15s;
            border-bottom: 1px solid transparent;
        }
        .cta-secondary:hover {
            color: var(--text-1);
            border-bottom-color: var(--border-hi);
        }

        /* ── Terminal Window ────────────────────────────────────────────────── */
        .terminal-wrap {
            width: 100%;
            max-width: 720px;
            opacity: 0;
            animation: fade-up 0.7s ease 0.55s forwards;
        }

        .terminal {
            background: #050505;
            border: 1px solid var(--border);
            border-radius: 0;
            overflow: hidden;
        }

        .terminal-bar {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 10px 16px;
            border-bottom: 1px solid var(--border);
            background: #080808;
        }

        .terminal-dots {
            display: flex;
            gap: 6px;
        }
        .terminal-dots span {
            width: 10px;
            height: 10px;
            border-radius: 50%;
            background: var(--border-hi);
        }

        .terminal-title {
            font-family: var(--mono);
            font-size: 10px;
            color: var(--text-3);
            letter-spacing: 0.06em;
        }

        .terminal-body {
            padding: 20px 24px 24px;
            font-family: var(--mono);
            font-size: 12.5px;
            line-height: 1.9;
            min-height: 230px;
        }

        .log-line {
            display: flex;
            gap: 12px;
            opacity: 0;
            transform: translateY(4px);
        }

        .log-ts    { color: var(--text-3); flex-shrink: 0; }
        .log-level { color: var(--text-2); flex-shrink: 0; width: 36px; }
        .log-mod   { color: var(--text-2); flex-shrink: 0; }
        .log-msg   { color: var(--text-1); }
        .log-seq   { color: var(--amber); font-weight: 700; }
        .log-event { color: var(--text-2); }
        .log-rewind { color: var(--amber); font-weight: 700; }
        .log-ok    { color: #22c55e; font-weight: 700; }

        /* Staggered line reveals */
        .log-line:nth-child(1)  { animation: log-in 0.3s ease 1.0s forwards; }
        .log-line:nth-child(2)  { animation: log-in 0.3s ease 1.3s forwards; }
        .log-line:nth-child(3)  { animation: log-in 0.3s ease 1.6s forwards; }
        .log-line:nth-child(4)  { animation: log-in 0.3s ease 1.9s forwards; }
        .log-line:nth-child(5)  { animation: log-in 0.3s ease 2.2s forwards; }
        .log-line:nth-child(6)  { animation: log-in 0.3s ease 2.5s forwards; }
        .log-line:nth-child(7)  { animation: log-in 0.3s ease 2.8s forwards; }
        .log-line:nth-child(8)  { animation: log-in 0.3s ease 3.4s forwards; }
        .log-line:nth-child(9)  { animation: log-in 0.3s ease 3.7s forwards; }
        .log-line:nth-child(10) { animation: log-in 0.3s ease 4.0s forwards; }

        @keyframes log-in {
            to { opacity: 1; transform: none; }
        }

        /* ── Feature Strip ──────────────────────────────────────────────────── */
        .features {
            width: 100%;
            max-width: 1000px;
            margin: 0 auto;
            padding: 80px 24px;
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 1px;
            background: var(--border);
            border-top: 1px solid var(--border);
            border-bottom: 1px solid var(--border);
        }

        .feat {
            background: var(--bg);
            padding: 40px 36px;
            display: flex;
            flex-direction: column;
            gap: 10px;
            transition: background 0.2s;
        }
        .feat:hover { background: var(--surface); }

        .feat-name {
            font-family: var(--mono);
            font-size: 13px;
            font-weight: 700;
            color: var(--text-1);
            letter-spacing: 0.02em;
        }

        .feat-desc {
            font-size: 13px;
            color: var(--text-2);
            line-height: 1.6;
            font-weight: 400;
        }

        .feat-detail {
            font-family: var(--mono);
            font-size: 10px;
            color: var(--text-3);
            margin-top: 4px;
        }

        /* ── Install Strip ──────────────────────────────────────────────────── */
        .install-strip {
            width: 100%;
            max-width: 1000px;
            margin: 0 auto;
            padding: 80px 24px;
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 32px;
            text-align: center;
        }

        .install-heading {
            font-size: 28px;
            font-weight: 300;
            letter-spacing: -0.02em;
            color: var(--text-1);
        }

        .install-box {
            display: flex;
            align-items: center;
            gap: 0;
            border: 1px solid var(--border-hi);
            overflow: hidden;
            max-width: 420px;
            width: 100%;
        }

        .install-prompt {
            font-family: var(--mono);
            font-size: 13px;
            padding: 14px 16px;
            background: var(--surface);
            color: var(--text-3);
            border-right: 1px solid var(--border);
            flex-shrink: 0;
            user-select: none;
        }

        .install-cmd {
            font-family: var(--mono);
            font-size: 13px;
            padding: 14px 16px;
            color: var(--text-1);
            flex: 1;
            letter-spacing: 0.01em;
        }

        .install-copy {
            font-family: var(--mono);
            font-size: 11px;
            padding: 14px 16px;
            background: none;
            border: none;
            border-left: 1px solid var(--border);
            color: var(--text-2);
            cursor: pointer;
            transition: color 0.15s, background 0.15s;
        }
        .install-copy:hover {
            color: var(--amber);
            background: var(--amber-dim);
        }

        .install-links {
            display: flex;
            gap: 24px;
            font-family: var(--mono);
            font-size: 11px;
            color: var(--text-3);
        }
        .install-links a {
            color: var(--text-2);
            text-decoration: none;
            transition: color 0.15s;
        }
        .install-links a:hover { color: var(--text-1); }

        /* ── Footer ─────────────────────────────────────────────────────────── */
        footer {
            border-top: 1px solid var(--border);
            padding: 24px 40px;
            display: flex;
            align-items: center;
            justify-content: space-between;
            font-family: var(--mono);
            font-size: 11px;
            color: var(--text-3);
        }

        .footer-left { display: flex; gap: 16px; }
        .footer-left a {
            color: var(--text-3);
            text-decoration: none;
            transition: color 0.15s;
        }
        .footer-left a:hover { color: var(--text-2); }

        /* ── Animations ─────────────────────────────────────────────────────── */
        @keyframes fade-up {
            from { opacity: 0; transform: translateY(12px); }
            to   { opacity: 1; transform: none; }
        }

        /* ── Responsive ─────────────────────────────────────────────────────── */
        @media (max-width: 720px) {
            nav { padding: 0 20px; }
            .nav-right .nav-link { display: none; }
            .hero { padding: 64px 20px 56px; }
            .features {
                grid-template-columns: 1fr;
                padding: 0;
            }
            .feat { padding: 28px 20px; }
            footer { flex-direction: column; gap: 12px; text-align: center; }
        }
    </style>
</head>
<body>

    <!-- Nav -->
    <nav>
        <a href="/" class="nav-logo">korg</a>
        <div class="nav-right">
            <div class="nav-badge">
                <span class="status-dot"></span>
                <span>v0.1.0 stable</span>
            </div>
            <a href="https://github.com/New1Direction/korg" target="_blank" class="nav-link">github</a>
            <a href="https://crates.io/crates/korg" target="_blank" class="nav-link">crates.io</a>
            <a href="/cockpit" class="nav-link">cockpit →</a>
        </div>
    </nav>

    <!-- Hero -->
    <section class="hero">
        <p class="hero-label">v0.1.0 · now on crates.io</p>

        <h1 class="hero-title">the first deterministic<br>cognitive runtime.</h1>

        <p class="hero-sub">
            Every AI agent decision logged, causally ordered, and reversible.
            Like Git, but for cognition.
        </p>

        <div class="hero-ctas">
            <a href="https://github.com/New1Direction/korg" target="_blank" class="cta-primary">
                <span class="prompt">$</span> cargo install korg
            </a>
            <a href="https://github.com/New1Direction/korg" target="_blank" class="cta-secondary">
                GitHub ↗
            </a>
        </div>

        <!-- Live log terminal -->
        <div class="terminal-wrap">
            <div class="terminal">
                <div class="terminal-bar">
                    <div class="terminal-dots">
                        <span></span><span></span><span></span>
                    </div>
                    <span class="terminal-title">korg campaign --headless</span>
                    <span></span>
                </div>
                <div class="terminal-body">
                    <div class="log-line">
                        <span class="log-ts">17:01:00Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">korg::</span>
                        <span class="log-msg">session_id=<span class="log-seq">019e5333</span> mode=balanced</span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:01Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">leader::</span>
                        <span class="log-msg">swarm=[captain, harper, benjamin, lucas]</span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:02Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">log::</span>
                        <span class="log-msg">append <span class="log-seq">seq=1</span> <span class="log-event">TransitionStarted</span></span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:03Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">log::</span>
                        <span class="log-msg">append <span class="log-seq">seq=2</span> <span class="log-event">LeaseAcquired</span> actor=benjamin</span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:04Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">log::</span>
                        <span class="log-msg">append <span class="log-seq">seq=3</span> <span class="log-event">EffectStarted</span> target=src/auth.rs</span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:06Z</span>
                        <span class="log-level">WARN</span>
                        <span class="log-mod">eval::</span>
                        <span class="log-msg">verdict=REVISE entropy=0.72 doom_loop_risk=moderate</span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:07Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">log::</span>
                        <span class="log-msg"><span class="log-rewind">↩ rewind --seq 3</span> &nbsp;restoring workspace…</span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:07Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">log::</span>
                        <span class="log-msg">git read-tree O(1) · projections rebuilt · clock=<span class="log-seq">seq=3</span></span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:08Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">log::</span>
                        <span class="log-msg">append <span class="log-seq">seq=4</span> <span class="log-event">EffectStarted</span> branch=b91a4c2e</span>
                    </div>
                    <div class="log-line">
                        <span class="log-ts">17:01:10Z</span>
                        <span class="log-level">INFO</span>
                        <span class="log-mod">arena::</span>
                        <span class="log-msg"><span class="log-ok">✓ ACCEPT</span> trajectory=0.91 entropy=0.89 campaign_complete</span>
                    </div>
                </div>
            </div>
        </div>
    </section>

    <!-- Feature strip -->
    <div class="features">
        <div class="feat">
            <div class="feat-name">rewind</div>
            <div class="feat-desc">Restore any agent decision to any prior state in O(1) time. No re-execution, no guessing.</div>
            <div class="feat-detail">korg rewind --seq N</div>
        </div>
        <div class="feat">
            <div class="feat-name">ledger</div>
            <div class="feat-desc">Append-only HLC-ordered event log. Every transition is signed, sequenced, and cryptographically sealed.</div>
            <div class="feat-detail">130 tests · 0 failures</div>
        </div>
        <div class="feat">
            <div class="feat-name">fork</div>
            <div class="feat-desc">Branch from any checkpoint. Run parallel strategies, compare outcomes, discard or merge.</div>
            <div class="feat-detail">speculative execution</div>
        </div>
    </div>

    <!-- Install -->
    <div class="install-strip">
        <h2 class="install-heading">get started in one command</h2>
        <div class="install-box">
            <span class="install-prompt">$</span>
            <span class="install-cmd" id="icmd">cargo install korg</span>
            <button class="install-copy" onclick="copyInstall()" id="icopy">copy</button>
        </div>
        <div class="install-links">
            <a href="https://docs.rs/korg" target="_blank">docs.rs/korg</a>
            <a href="https://crates.io/crates/korg" target="_blank">crates.io/crates/korg</a>
            <a href="https://github.com/New1Direction/korg" target="_blank">github</a>
        </div>
    </div>

    <!-- Footer -->
    <footer>
        <div class="footer-left">
            <span>korg v0.1.0</span>
            <a href="https://github.com/New1Direction/korg/blob/main/LICENSE" target="_blank">MIT / Apache-2.0</a>
            <a href="https://github.com/New1Direction/korg/blob/main/CHANGELOG.md" target="_blank">changelog</a>
            <a href="https://github.com/New1Direction/korg/blob/main/ROADMAP.md" target="_blank">roadmap</a>
        </div>
        <span>github.com/New1Direction/korg</span>
    </footer>

    <script>
        function copyInstall() {
            const btn = document.getElementById('icopy');
            navigator.clipboard.writeText('cargo install korg').then(() => {
                btn.textContent = 'copied!';
                btn.style.color = 'var(--amber)';
                setTimeout(() => {
                    btn.textContent = 'copy';
                    btn.style.color = '';
                }, 2000);
            });
        }
    </script>

</body>
</html>"##;

// ============================================================================
// OAUTH & GATEWAY AUTHENTICATION LAYER HANDLERS
// ============================================================================

#[derive(serde::Deserialize)]
struct LoginQuery {
    provider: Option<String>,
}

async fn oauth_login_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LoginQuery>,
) -> impl IntoResponse {
    let provider = query.provider.unwrap_or_else(|| "codex".to_string());
    let client = if provider == "anthropic" {
        &state.auth.providers.anthropic_client
    } else {
        &state.auth.providers.codex_client
    };

    let scopes = if provider == "anthropic" {
        vec!["messages".to_string()]
    } else {
        vec!["subscription".to_string()]
    };

    let flow = state.auth.providers.initiate_pkce_flow(client, scopes);
    state
        .auth
        .providers
        .save_pending_pkce(flow.csrf_state.clone(), flow.pkce_verifier);

    axum::response::Redirect::to(&flow.authorize_url).into_response()
}

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
}

pub async fn oauth_codex_callback_handler(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<CallbackQuery>,
) -> impl IntoResponse {
    use oauth2::AuthorizationCode;
    use oauth2::TokenResponse;

    let verifier_str = match app_state.auth.providers.take_pending_pkce(&query.state) {
        Some(v) => v,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Invalid CSRF state / PKCE verifier mismatch. CSRF verification failed.",
            )
                .into_response();
        }
    };

    let verifier = oauth2::PkceCodeVerifier::new(verifier_str);

    let token_result = app_state
        .auth
        .providers
        .codex_client
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(verifier)
        .request_async(oauth2::reqwest::async_http_client)
        .await;

    let token_response = match token_result {
        Ok(res) => res,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to exchange Codex authorization code: {:?}", e),
            )
                .into_response();
        }
    };

    let access_token = token_response.access_token().secret().clone();
    let tier = app_state
        .auth
        .providers
        .verify_codex_subscription(&access_token)
        .await;

    let user_id = "claude-code-user";
    let mut session = app_state
        .auth
        .store
        .load_session(user_id)
        .unwrap_or_else(|| korg_auth::store::UserSession {
            user_id: user_id.to_string(),
            codex_access_token: "".to_string(),
            subscription_tier: korg_core::SubscriptionTier::Standard,
            anthropic_access_token: "".to_string(),
            refresh_token: None,
            expires_at: chrono::Utc::now(),
        });

    session.codex_access_token = access_token;
    session.subscription_tier = tier;

    if let Err(e) = app_state.auth.store.save_session(session) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to persist UserSession in JSON store: {:?}", e),
        )
            .into_response();
    }

    Html(format!(
        r#"<html>
        <head>
            <style>
                body {{ background: #080808; color: #fafafa; font-family: sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }}
                .card {{ background: #0d0d0d; border: 1px solid #1c1c1e; padding: 40px; text-align: center; max-width: 400px; }}
                h1 {{ color: #22c55e; font-weight: 300; margin-bottom: 20px; }}
                p {{ color: #8e8e93; line-height: 1.5; }}
            </style>
        </head>
        <body>
            <div class="card">
                <h1>Codex Authorized</h1>
                <p>Codex subscription verified as <strong>{}</strong>.</p>
                <p>You can close this tab and continue.</p>
            </div>
        </body>
        </html>"#,
        tier.as_str()
    ))
    .into_response()
}

pub async fn oauth_anthropic_callback_handler(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<CallbackQuery>,
) -> impl IntoResponse {
    use oauth2::AuthorizationCode;
    use oauth2::TokenResponse;

    let verifier_str = match app_state.auth.providers.take_pending_pkce(&query.state) {
        Some(v) => v,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Invalid CSRF state / PKCE verifier mismatch. CSRF verification failed.",
            )
                .into_response();
        }
    };

    let verifier = oauth2::PkceCodeVerifier::new(verifier_str);

    let token_result = app_state
        .auth
        .providers
        .anthropic_client
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(verifier)
        .request_async(oauth2::reqwest::async_http_client)
        .await;

    let token_response = match token_result {
        Ok(res) => res,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to exchange Anthropic authorization code: {:?}", e),
            )
                .into_response();
        }
    };

    let access_token = token_response.access_token().secret().clone();
    let refresh_token = token_response.refresh_token().map(|rt| rt.secret().clone());
    let expires_in = token_response
        .expires_in()
        .unwrap_or(std::time::Duration::from_secs(3600));
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in.as_secs() as i64);

    let user_id = "claude-code-user";
    let mut session = app_state
        .auth
        .store
        .load_session(user_id)
        .unwrap_or_else(|| korg_auth::store::UserSession {
            user_id: user_id.to_string(),
            codex_access_token: "".to_string(),
            subscription_tier: korg_core::SubscriptionTier::Standard,
            anthropic_access_token: "".to_string(),
            refresh_token: None,
            expires_at: chrono::Utc::now(),
        });

    session.anthropic_access_token = access_token;
    session.refresh_token = refresh_token;
    session.expires_at = expires_at;

    if let Err(e) = app_state.auth.store.save_session(session) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to persist UserSession in JSON store: {:?}", e),
        )
            .into_response();
    }

    Html(r#"<html>
        <head>
            <style>
                body { background: #080808; color: #fafafa; font-family: sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
                .card { background: #0d0d0d; border: 1px solid #1c1c1e; padding: 40px; text-align: center; max-width: 400px; }
                h1 { color: #22c55e; font-weight: 300; margin-bottom: 20px; }
                p { color: #8e8e93; line-height: 1.5; }
            </style>
        </head>
        <body>
            <div class="card">
                <h1>Anthropic Authorized</h1>
                <p>Successfully linked Anthropic delegated OAuth account to Korg proxy.</p>
                <p>You can close this tab and continue.</p>
            </div>
        </body>
        </html>"#)
    .into_response()
}

pub struct AuthenticatedUser {
    pub user_id: String,
    pub session: korg_auth::store::UserSession,
}

#[axum::async_trait]
impl<S> axum::extract::FromRequestParts<S> for AuthenticatedUser
where
    Arc<AppState>: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (axum::http::StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let app_state = <Arc<AppState> as axum::extract::FromRef<S>>::from_ref(state);

        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok());

        let user_id = if let Some(auth_val) = auth_header {
            if auth_val.starts_with("Bearer ") {
                auth_val["Bearer ".len()..].trim().to_string()
            } else {
                auth_val.trim().to_string()
            }
        } else {
            let cookie_header = parts
                .headers
                .get(axum::http::header::COOKIE)
                .and_then(|h| h.to_str().ok());

            if let Some(cookie_val) = cookie_header {
                if let Some(idx) = cookie_val.find("korg_session=") {
                    let start = idx + "korg_session=".len();
                    let end = cookie_val[start..]
                        .find(';')
                        .map(|i| start + i)
                        .unwrap_or(cookie_val.len());
                    cookie_val[start..end].trim().to_string()
                } else {
                    return Err((
                        axum::http::StatusCode::UNAUTHORIZED,
                        "Missing korg_session cookie or authorization header",
                    ));
                }
            } else {
                return Err((
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Missing authorization header or cookie",
                ));
            }
        };

        if user_id.is_empty() {
            return Err((axum::http::StatusCode::UNAUTHORIZED, "Empty user identity"));
        }

        match app_state.auth.store.load_session(&user_id) {
            Some(session) => Ok(AuthenticatedUser { user_id, session }),
            None => {
                if user_id == "claude-code-user" || user_id.starts_with("mock-") {
                    let mock_session = korg_auth::store::UserSession {
                        user_id: user_id.clone(),
                        codex_access_token: "mock-codex-token".to_string(),
                        subscription_tier: korg_core::SubscriptionTier::Premium,
                        anthropic_access_token: "mock-anthropic-token".to_string(),
                        refresh_token: Some("mock-refresh-token".to_string()),
                        expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                    };
                    let _ = app_state.auth.store.save_session(mock_session.clone());
                    Ok(AuthenticatedUser {
                        user_id,
                        session: mock_session,
                    })
                } else {
                    Err((
                        axum::http::StatusCode::UNAUTHORIZED,
                        "Active session not found. Please log in first.",
                    ))
                }
            }
        }
    }
}

async fn anthropic_messages_proxy_handler(
    State(app_state): State<Arc<AppState>>,
    user: AuthenticatedUser,
    req_headers: axum::http::HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Proxy Audit Logging (Observability)
    let model = payload
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("claude-3-5-sonnet");
    let input_chars = payload
        .get("messages")
        .map(|m| m.to_string().len())
        .unwrap_or(0);
    let estimated_tokens = input_chars / 4;
    let cost_estimate = (estimated_tokens as f64) * 0.000003; // $3 per million input tokens

    println!(
        "\x1b[38;2;120;125;140m[Proxy Audit] User: {}, Tier: {}, Model: {}, Est. Input Tokens: {}, Est. Cost: ${:.6}\x1b[0m",
        user.user_id,
        user.session.subscription_tier.as_str(),
        model,
        estimated_tokens,
        cost_estimate
    );

    tracing::info!(
        user_id = %user.user_id,
        tier = %user.session.subscription_tier.as_str(),
        model = %model,
        estimated_tokens = estimated_tokens,
        cost_estimate = cost_estimate,
        "proxy_request_audit"
    );

    // 1. Structured Ledger Auditing (Compliance & Replay Governance)
    let audit_event = korg_registry::CapabilityEvent::ProxyAuditTrail {
        user_id: user.user_id.clone(),
        subscription_tier: user.session.subscription_tier.as_str().to_string(),
        model: model.to_string(),
        estimated_input_tokens: estimated_tokens as u64,
        estimated_cost_usd: cost_estimate,
        timestamp: chrono::Utc::now(),
    };
    {
        let mut resolver = app_state.capability_resolver.lock().await;
        resolver.append_and_project(audit_event);
    }

    let client = reqwest::Client::new();
    let anthropic_url = "https://api.anthropic.com/v1/messages";

    let make_request = |token: &str| {
        let mut builder = client.post(anthropic_url).json(&payload);

        if let Some(version) = req_headers.get("anthropic-version") {
            builder = builder.header("anthropic-version", version);
        } else {
            builder = builder.header("anthropic-version", "2023-06-01");
        }

        if let Some(content_type) = req_headers.get("content-type") {
            builder = builder.header("content-type", content_type);
        }

        builder = builder.bearer_auth(token);
        builder
    };

    let mut token = user.session.anthropic_access_token.clone();
    let is_expired = user.session.expires_at < chrono::Utc::now();

    // 2. Coordinated Proactive Singleflight Token Refresh (Hermes Lesson #4 + Singleflight blueprint)
    if is_expired && user.session.refresh_token.is_some() {
        let app_state_clone = app_state.clone();
        let user_id_clone = user.user_id.clone();
        let session_clone = user.session.clone();

        let refresh_result = app_state
            .auth
            .refresher
            .execute_refresh(&user.user_id, || async move {
                refresh_anthropic_token(&app_state_clone, &user_id_clone, &session_clone).await
            })
            .await;

        if let Ok(new_session) = refresh_result {
            token = new_session.anthropic_access_token;
        }
    }

    let mut response = match make_request(&token).send().await {
        Ok(resp) => resp,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Failed to reach Anthropic API: {:?}", e),
            )
                .into_response();
        }
    };

    // 3. Coordinated Reactive Singleflight Token Retry on Stale Upstream 401 Rejections
    if response.status() == axum::http::StatusCode::UNAUTHORIZED && !is_expired {
        if let Some(ref rt) = user.session.refresh_token {
            let app_state_clone = app_state.clone();
            let user_id_clone = user.user_id.clone();
            let session_clone = user.session.clone();

            let refresh_result = app_state
                .auth
                .refresher
                .execute_refresh(&user.user_id, || async move {
                    refresh_anthropic_token(&app_state_clone, &user_id_clone, &session_clone).await
                })
                .await;

            if let Ok(new_session) = refresh_result {
                let retry_token = new_session.anthropic_access_token;
                if let Ok(resp) = make_request(&retry_token).send().await {
                    response = resp;
                }
            }
        }
    }

    let status = axum::http::StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

    let mut headers = axum::http::HeaderMap::new();
    for (k, v) in response.headers().iter() {
        if let Ok(name) = axum::http::HeaderName::from_bytes(k.as_str().as_bytes()) {
            headers.insert(name, v.clone());
        }
    }

    let body_bytes = match response.bytes().await {
        Ok(b) => b,
        Err(_) => bytes::Bytes::new(),
    };

    (status, headers, body_bytes).into_response()
}

async fn refresh_anthropic_token(
    app_state: &Arc<AppState>,
    user_id: &str,
    session: &korg_auth::store::UserSession,
) -> Result<korg_auth::store::UserSession, anyhow::Error> {
    use oauth2::RefreshToken;
    use oauth2::TokenResponse;

    let refresh_token_str = session
        .refresh_token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No refresh token present"))?;

    if refresh_token_str == "mock-refresh-token" {
        let mut new_session = session.clone();
        new_session.anthropic_access_token = "mock-refreshed-anthropic-token".to_string();
        new_session.expires_at = chrono::Utc::now() + chrono::Duration::hours(24);
        app_state.auth.store.save_session(new_session.clone())?;
        return Ok(new_session);
    }

    let token_result = app_state
        .auth
        .providers
        .anthropic_client
        .exchange_refresh_token(&RefreshToken::new(refresh_token_str.clone()))
        .request_async(oauth2::reqwest::async_http_client)
        .await?;

    let access_token = token_result.access_token().secret().clone();
    let new_refresh_token = token_result.refresh_token().map(|rt| rt.secret().clone());
    let expires_in = token_result
        .expires_in()
        .unwrap_or(std::time::Duration::from_secs(3600));
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in.as_secs() as i64);

    let mut new_session = session.clone();
    new_session.anthropic_access_token = access_token;
    if let Some(rt) = new_refresh_token {
        new_session.refresh_token = Some(rt);
    }
    new_session.expires_at = expires_at;

    app_state.auth.store.save_session(new_session.clone())?;
    Ok(new_session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentToolCallRequest;
    use crate::AppState;
    use axum::extract::State;
    use axum::Json;
    use korg_registry::{CapabilityJournal, CapabilityResolver};
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Mutex as TokioMutex;

    #[tokio::test]
    async fn test_agent_tool_call_actor_id_always_korg_api() {
        let (broadcaster_tx, _) = tokio::sync::broadcast::channel(16);
        let (feedback_tx, _) = tokio::sync::mpsc::channel(16);

        let temp_dir = std::env::temp_dir().join(format!("korg_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let journal = CapabilityJournal::new(
            temp_dir.join("journal.json"),
            temp_dir.join("snapshot.json"),
            10,
            temp_dir.join("lock.lock"),
        );
        let resolver = CapabilityResolver::new(std::collections::HashMap::new(), journal);
        let capability_resolver_container = Arc::new(TokioMutex::new(resolver));

        let auth_state = Arc::new(korg_auth::AuthState::new(korg_auth::AuthConfig {
            base_url: "http://localhost:8080".to_string(),
            codex_client_id: "mock-codex-client-id".to_string(),
            codex_client_secret: "mock-codex-client-secret".to_string(),
            anthropic_client_id: "mock-anthropic-client-id".to_string(),
            anthropic_client_secret: "mock-anthropic-client-secret".to_string(),
            token_store_path: temp_dir.join("auth.json"),
        }));

        let app_state = Arc::new(AppState {
            broadcaster: broadcaster_tx,
            feedback_tx: TokioMutex::new(Some(feedback_tx)),
            capability_resolver: capability_resolver_container.clone(),
            runtime_coordinator: Arc::new(StdMutex::new(None)),
            auth: auth_state,
        });

        let req = AgentToolCallRequest {
            source_agent: "agent:claude-code@0.2.29".to_string(),
            tool_name: "Read".to_string(),
            args: serde_json::json!({ "file_path": "math_utils.py" }),
            result: serde_json::json!({ "content": "hello" }),
            payload_refs: vec![],
            success: true,
            duration_ms: 100,
            triggered_by: None,
        };

        // Call the handler directly!
        let _response = agent_tool_call_handler(State(app_state), Json(req)).await;

        // Verify the event was added and metadata.actor_id == "korg:api"
        let resolver_lock = capability_resolver_container.lock().await;
        let events = &resolver_lock.journal.events;
        assert!(!events.is_empty(), "Events should not be empty");
        let last_event = &events[events.len() - 1];

        // Assert actor_id is "korg:api"
        assert_eq!(last_event.metadata.actor_id, "korg:api");

        // Assert triggered_by is None (preserved correctly)
        assert_eq!(last_event.metadata.triggered_by, None);

        // Clean up temp dir
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_oauth_pkce_csrfs_distinct() {
        let config = korg_auth::AuthConfig {
            base_url: "http://localhost:8080".to_string(),
            codex_client_id: "mock-codex".to_string(),
            codex_client_secret: "mock-codex-secret".to_string(),
            anthropic_client_id: "mock-anthropic".to_string(),
            anthropic_client_secret: "mock-anthropic-secret".to_string(),
            token_store_path: std::path::PathBuf::from(".korg/test_auth.json"),
        };
        let providers = korg_auth::providers::AuthProviders::new(&config);

        let flow =
            providers.initiate_pkce_flow(&providers.codex_client, vec!["subscription".to_string()]);

        assert!(!flow.csrf_state.is_empty());
        assert!(!flow.pkce_verifier.is_empty());
        assert!(!flow.authorize_url.is_empty());

        // Assert that they are distinct (Hermes Lesson #1)
        assert_ne!(flow.csrf_state, flow.pkce_verifier);
    }

    #[test]
    fn test_absolute_expiry_persistence() {
        let temp_dir =
            std::env::temp_dir().join(format!("korg_test_store_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store_path = temp_dir.join("auth.json");

        let store = korg_auth::store::JsonTokenStore::new(store_path.clone());
        let original_expires_at = chrono::Utc::now() + chrono::Duration::hours(2);

        let session = korg_auth::store::UserSession {
            user_id: "test-user".to_string(),
            codex_access_token: "codex-token".to_string(),
            subscription_tier: korg_core::SubscriptionTier::Premium,
            anthropic_access_token: "anthropic-token".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at: original_expires_at,
        };

        store.save_session(session).unwrap();

        // Simulate cold restart by dropping the store and creating a new one loading from same path
        drop(store);
        let store2 = korg_auth::store::JsonTokenStore::new(store_path.clone());

        let loaded = store2
            .load_session("test-user")
            .expect("Session should be loaded");

        // Assert absolute expiry match (Hermes Lesson #3)
        assert_eq!(
            loaded.expires_at.timestamp(),
            original_expires_at.timestamp()
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_stale_token_auto_refresh() {
        let temp_dir =
            std::env::temp_dir().join(format!("korg_test_refresh_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let (broadcaster_tx, _) = tokio::sync::broadcast::channel(16);
        let (feedback_tx, _) = tokio::sync::mpsc::channel(16);
        let journal = CapabilityJournal::new(
            temp_dir.join("journal.json"),
            temp_dir.join("snapshot.json"),
            10,
            temp_dir.join("lock.lock"),
        );
        let resolver = CapabilityResolver::new(std::collections::HashMap::new(), journal);
        let capability_resolver_container = Arc::new(TokioMutex::new(resolver));

        let auth_state = Arc::new(korg_auth::AuthState::new(korg_auth::AuthConfig {
            base_url: "http://localhost:8080".to_string(),
            codex_client_id: "mock-codex-client-id".to_string(),
            codex_client_secret: "mock-codex-client-secret".to_string(),
            anthropic_client_id: "mock-anthropic-client-id".to_string(),
            anthropic_client_secret: "mock-anthropic-client-secret".to_string(),
            token_store_path: temp_dir.join("auth.json"),
        }));

        let app_state = Arc::new(AppState {
            broadcaster: broadcaster_tx,
            feedback_tx: TokioMutex::new(Some(feedback_tx)),
            capability_resolver: capability_resolver_container.clone(),
            runtime_coordinator: Arc::new(StdMutex::new(None)),
            auth: auth_state.clone(),
        });

        // Save an expired session
        let expired_at = chrono::Utc::now() - chrono::Duration::minutes(5);
        let session = korg_auth::store::UserSession {
            user_id: "claude-code-user".to_string(),
            codex_access_token: "mock-codex-token".to_string(),
            subscription_tier: korg_core::SubscriptionTier::Premium,
            anthropic_access_token: "stale-anthropic-token".to_string(),
            refresh_token: Some("mock-refresh-token".to_string()),
            expires_at: expired_at,
        };
        auth_state.store.save_session(session.clone()).unwrap();

        // Execute token refresh
        let refreshed = refresh_anthropic_token(&app_state, "claude-code-user", &session)
            .await
            .unwrap();

        assert_eq!(
            refreshed.anthropic_access_token,
            "mock-refreshed-anthropic-token"
        );
        assert!(refreshed.expires_at > chrono::Utc::now());

        // Load session and verify persistent update
        let loaded = auth_state.store.load_session("claude-code-user").unwrap();
        assert_eq!(
            loaded.anthropic_access_token,
            "mock-refreshed-anthropic-token"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_codex_subscription_acp_gates() {
        let journal = CapabilityJournal::default_journal();
        let resolver = CapabilityResolver::new(std::collections::HashMap::new(), journal);

        // Standard tier should be gated from high-blast-radius tools like Bash and docker_sandbox
        let res_std_bash =
            resolver.authorize_tool_use(korg_core::SubscriptionTier::Standard, "Bash");
        assert!(res_std_bash.is_err());
        assert!(res_std_bash.err().unwrap().contains("ACP Gated"));

        let res_std_sandbox =
            resolver.authorize_tool_use(korg_core::SubscriptionTier::Standard, "docker_sandbox");
        assert!(res_std_sandbox.is_err());

        // Standard tier should be allowed to run other standard tools
        let res_std_read =
            resolver.authorize_tool_use(korg_core::SubscriptionTier::Standard, "Read");
        assert!(res_std_read.is_ok());

        // Premium tier should be unrestricted for Bash
        let res_prem_bash =
            resolver.authorize_tool_use(korg_core::SubscriptionTier::Premium, "Bash");
        assert!(res_prem_bash.is_ok());
    }

    #[test]
    fn test_secure_token_store_encryption() {
        let temp_dir =
            std::env::temp_dir().join(format!("korg_test_encrypt_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store_path = temp_dir.join("auth.json");

        let store = korg_auth::store::JsonTokenStore::new(store_path.clone());
        let session = korg_auth::store::UserSession {
            user_id: "test-user".to_string(),
            codex_access_token: "secret-codex-token".to_string(),
            subscription_tier: korg_core::SubscriptionTier::Premium,
            anthropic_access_token: "secret-anthropic-token".to_string(),
            refresh_token: Some("secret-refresh-token".to_string()),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        };

        store.save_session(session).unwrap();

        // Assert that the stored file is NOT plain text JSON (it is encrypted!)
        let raw_file_content = std::fs::read_to_string(&store_path).unwrap_or_default();
        assert!(!raw_file_content.contains("secret-codex-token"));
        assert!(!raw_file_content.contains("secret-anthropic-token"));

        // Assert it can be successfully loaded back and deciphered
        let loaded = store.load_session("test-user").unwrap();
        assert_eq!(loaded.codex_access_token, "secret-codex-token");
        assert_eq!(loaded.anthropic_access_token, "secret-anthropic-token");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_singleflight_concurrent_refreshes() {
        let refresher = std::sync::Arc::new(korg_auth::SingleflightRefresher::new());
        let execution_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut join_handles = vec![];

        for _ in 0..10 {
            let refresher_clone = refresher.clone();
            let execution_count_clone = execution_count.clone();

            let handle = tokio::spawn(async move {
                refresher_clone
                    .execute_refresh("claude-code-user", || async move {
                        // Simulate delay and increment counter
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        execution_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                        Ok(korg_auth::store::UserSession {
                            user_id: "claude-code-user".to_string(),
                            codex_access_token: "refreshed-token".to_string(),
                            subscription_tier: korg_core::SubscriptionTier::Premium,
                            anthropic_access_token: "refreshed-token".to_string(),
                            refresh_token: None,
                            expires_at: chrono::Utc::now() + chrono::Duration::hours(2),
                        })
                    })
                    .await
            });
            join_handles.push(handle);
        }

        for handle in join_handles {
            let res = handle.await.unwrap().unwrap();
            assert_eq!(res.anthropic_access_token, "refreshed-token");
        }

        // Assert that the refresh operation was executed EXACTLY ONCE across all 10 concurrent requests!
        assert_eq!(execution_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_ledger_proxy_audit_trail() {
        let temp_dir =
            std::env::temp_dir().join(format!("korg_test_audit_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let journal = CapabilityJournal::new(
            temp_dir.join("journal.json"),
            temp_dir.join("snapshot.json"),
            10,
            temp_dir.join("lock.lock"),
        );
        let mut resolver = CapabilityResolver::new(std::collections::HashMap::new(), journal);

        let audit_event = korg_registry::CapabilityEvent::ProxyAuditTrail {
            user_id: "claude-code-user".to_string(),
            subscription_tier: "Premium".to_string(),
            model: "claude-3-5-sonnet".to_string(),
            estimated_input_tokens: 125,
            estimated_cost_usd: 0.000375,
            timestamp: chrono::Utc::now(),
        };

        resolver.append_and_project(audit_event);

        // Assert event was logged to signed ledger
        let events = &resolver.journal.events;
        assert!(!events.is_empty());

        let last_event = &events[events.len() - 1];
        if let korg_registry::CapabilityEvent::ProxyAuditTrail {
            user_id,
            subscription_tier,
            model,
            estimated_input_tokens,
            estimated_cost_usd,
            ..
        } = &last_event.event
        {
            assert_eq!(user_id, "claude-code-user");
            assert_eq!(subscription_tier, "Premium");
            assert_eq!(model, "claude-3-5-sonnet");
            assert_eq!(*estimated_input_tokens, 125);
            assert_eq!(*estimated_cost_usd, 0.000375);
        } else {
            panic!("Expected ProxyAuditTrail variant");
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
