//! Korg Dashboard — Axum-based web server and live minimalist monochrome browser dashboard.
//!
//! Provides the production web dashboard for Phase 2:
//!   - GET `/api/events` (SSE stream broadcasting TuiUpdate JSONs)
//!   - POST `/api/override` (forwards ContractResponse user overrides back to the leader)
//!   - GET `/api/state` (exposes active blackboard.json snapshot)
//!   - Serves a static landing page (LANDING_HTML); no SPA or WASM frontend is bundled
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

/// The address the dashboard server binds to. Defaults to loopback
/// (`127.0.0.1:8080`) so the (mostly unauthenticated) control/telemetry routes
/// aren't exposed to the network; set `KORG_SERVER_ADDR` to bind elsewhere on purpose.
fn server_bind_addr() -> String {
    resolve_bind_addr(std::env::var("KORG_SERVER_ADDR").ok())
}

/// Pure resolution of the bind address from an optional override — loopback
/// unless an explicit, non-empty override is given.
fn resolve_bind_addr(override_addr: Option<String>) -> String {
    override_addr
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:8080".to_string())
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
        .route("/assets/hero-loop.mp4", get(hero_video_handler))
        .route("/assets/hero-mesh.glb", get(hero_mesh_handler))
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

    let listener = tokio::net::TcpListener::bind(server_bind_addr()).await?;
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
        .route("/assets/hero-loop.mp4", get(hero_video_handler))
        .route("/assets/hero-mesh.glb", get(hero_mesh_handler))
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

    let listener = tokio::net::TcpListener::bind(server_bind_addr()).await?;
    println!("\n\x1b[1m[korg] Axum server listening on http://localhost:8080\x1b[0m");

    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        open_browser("http://localhost:8080");
    });

    axum::serve(listener, router).await?;
    Ok(())
}

/// Serves the static landing page (LANDING_HTML).
async fn index_handler() -> impl IntoResponse {
    Html(LANDING_HTML)
}

async fn wasm_js_handler() -> impl IntoResponse {
    // No WASM frontend is bundled in this build — 404 honestly rather than
    // serving an empty 200 that looks like a real (but empty) asset.
    (
        axum::http::StatusCode::NOT_FOUND,
        "korg WASM frontend is not bundled in this build",
    )
}

async fn wasm_bytes_handler() -> impl IntoResponse {
    (
        axum::http::StatusCode::NOT_FOUND,
        "korg WASM frontend is not bundled in this build",
    )
}

/// Serves the premium monochrome landing page
async fn landing_handler() -> impl IntoResponse {
    Html(LANDING_HTML)
}

async fn hero_video_handler() -> impl IntoResponse {
    const BYTES: &[u8] = include_bytes!("../assets/hero-loop.mp4");
    (
        [
            ("content-type", "video/mp4"),
            ("cache-control", "public, max-age=31536000, immutable"),
        ],
        BYTES,
    )
}

async fn hero_mesh_handler() -> impl IntoResponse {
    const BYTES: &[u8] = include_bytes!("../assets/hero-mesh.glb");
    (
        [
            ("content-type", "model/gltf-binary"),
            ("cache-control", "public, max-age=31536000, immutable"),
        ],
        BYTES,
    )
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
const LANDING_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>KORG — CROSS-VENDOR MEMORY FOR AI AGENTS</title>
<meta name="description" content="Capture every AI session into one open ledger. Recall across all of them, semantically, from inside Claude Code. Local, auditable, cross-vendor.">

<meta property="og:title" content="KORG — CROSS-VENDOR MEMORY FOR AI AGENTS">
<meta property="og:description" content="ChatGPT Memory but local and cross-vendor. Captures Claude Code, Codex, Grok into one ledger. Recall semantically from inside Claude Code.">
<meta property="og:type" content="website">
<meta property="og:url" content="https://yvaehkorg.lol/">
<meta name="twitter:card" content="summary_large_image">

<!-- D-DIN family (industrial DIN cut). Fallback chain prioritizes width
     compression over ornament: Arial Narrow → Arial → Verdana. -->
<link rel="preconnect" href="https://fonts.cdnfonts.com">
<link rel="stylesheet" href="https://fonts.cdnfonts.com/css/d-din">
<!-- JetBrains Mono retained ONLY for code blocks (the brand officially
     has no mono — but a dev-tooling site needs authentic terminal text). -->
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">

<!-- Importmap so Three.js's example modules resolve internal 'three' bare specifiers.
     Required for GLTFLoader / DRACOLoader. Must come before any module scripts. -->
<script type="importmap">
{
  "imports": {
    "three": "https://unpkg.com/three@0.160.0/build/three.module.js",
    "three/addons/": "https://unpkg.com/three@0.160.0/examples/jsm/"
  }
}
</script>

<style>
  /* ── Design tokens — SpaceX-inspired (see design-context) ────────── */
  :root {
    --canvas-night:      #000000;
    --canvas-night-soft: #0a0a0a;
    --canvas-light:      #ffffff;
    --canvas-cool:       #f0f0fa;

    --on-primary:        #ffffff;
    --on-primary-mute:   #f0f0fa;

    --ink:               #000000;
    --ink-mute:          #5a5a5f;

    --hairline-on-dark:  #3a3a3f;
    --hairline-on-light: #e0e0e8;

    --font-display: 'D-DIN', 'Arial Narrow', Arial, Verdana, sans-serif;
    --font-body:    'D-DIN', Arial, Verdana, sans-serif;
    --font-mono:    'JetBrains Mono', 'SF Mono', Menlo, Consolas, monospace;

    /* Type tokens — directly from the design system */
    --type-display-xxl-size:    80px;
    --type-display-xxl-lh:      0.95;
    --type-display-xxl-track:   1.6px;

    --type-display-xl-size:     60px;
    --type-display-xl-lh:       1.2;
    --type-display-xl-track:    1.2px;

    --type-display-lg-size:     48px;
    --type-display-lg-lh:       1.25;
    --type-display-lg-track:    0.96px;

    --type-body-lg-size:        19px;     /* was 16 — hero + section ledes */
    --type-body-lg-lh:          1.6;      /* was 1.7 — tightened for the bigger size */
    --type-body-lg-track:       0.16px;   /* less tracking at larger sizes */

    --type-body-md-size:        17px;     /* was 16 — general body */
    --type-body-md-lh:          1.55;     /* was 1.5 */
    --type-body-md-track:       0.16px;

    --type-button-cap-size:     13.5px;   /* was 13.008 */
    --type-button-cap-lh:       0.94;
    --type-button-cap-track:    1.2px;

    --type-micro-cap-size:      12.5px;   /* was 12 */
    --type-micro-cap-lh:        2.0;
    --type-micro-cap-track:     1.0px;

    --type-caption-size:        14px;     /* was 13.008 */
    --type-caption-lh:          1.55;
    --type-caption-track:       0px;

    /* Spacing */
    --s-xxs: 4px;
    --s-xs:  8px;
    --s-sm:  12px;
    --s-md:  16px;
    --s-lg:  18px;
    --s-xl:  24px;
    --s-xxl: 32px;
    --s-huge: 48px;

    /* Rounding */
    --r-xs:   4px;
    --r-sm:   8px;
    --r-md:   16px;
    --r-pill: 32px;
    --r-full: 9999px;

    --max-w:  1200px;
    --gutter: var(--s-xxl);
  }

  * { box-sizing: border-box; }
  html {
    background: var(--canvas-night);
    color: var(--on-primary);
  }
  body {
    font-family: var(--font-body);
    font-size: var(--type-body-md-size);
    line-height: var(--type-body-md-lh);
    letter-spacing: var(--type-body-md-track);
    margin: 0;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
    overflow-x: hidden;
  }
  ::selection { background: var(--on-primary); color: var(--canvas-night); }
  a { color: inherit; text-decoration: none; transition: opacity 140ms; }
  a.inline { text-decoration: underline; text-underline-offset: 3px; }
  code, pre { font-family: var(--font-mono); }

  .container { max-width: var(--max-w); margin: 0 auto; padding: 0 var(--gutter); }

  /* ── Display tiers ──────────────────────────────────────────────── */
  .display-xxl {
    font-family: var(--font-display);
    font-weight: 700;
    text-transform: uppercase;
    font-size: var(--type-display-xxl-size);
    line-height: var(--type-display-xxl-lh);
    letter-spacing: var(--type-display-xxl-track);
    color: var(--on-primary);
    margin: 0;
  }
  .display-xl {
    font-family: var(--font-display);
    font-weight: 700;
    text-transform: uppercase;
    font-size: var(--type-display-xl-size);
    line-height: var(--type-display-xl-lh);
    letter-spacing: var(--type-display-xl-track);
    color: var(--on-primary);
    margin: 0;
  }
  .display-lg {
    font-family: var(--font-display);
    font-weight: 700;
    text-transform: uppercase;
    font-size: var(--type-display-lg-size);
    line-height: var(--type-display-lg-lh);
    letter-spacing: var(--type-display-lg-track);
    color: var(--on-primary);
    margin: 0;
  }
  .micro-cap {
    font-family: var(--font-body);
    font-weight: 400;
    text-transform: uppercase;
    font-size: var(--type-micro-cap-size);
    line-height: var(--type-micro-cap-lh);
    letter-spacing: var(--type-micro-cap-track);
    color: var(--on-primary-mute);
  }
  .button-cap {
    font-family: var(--font-body);
    font-weight: 700;
    text-transform: uppercase;
    font-size: var(--type-button-cap-size);
    line-height: var(--type-button-cap-lh);
    letter-spacing: var(--type-button-cap-track);
  }
  .body-lg {
    font-family: var(--font-body);
    font-size: var(--type-body-lg-size);
    line-height: var(--type-body-lg-lh);
    letter-spacing: var(--type-body-lg-track);
    color: var(--on-primary-mute);
  }
  .caption {
    font-family: var(--font-body);
    font-size: var(--type-caption-size);
    line-height: var(--type-caption-lh);
    letter-spacing: var(--type-caption-track);
    color: var(--on-primary-mute);
  }

  /* ── Ghost pill CTA (the brand's signature button) ──────────────── */
  .ghost-pill {
    display: inline-block;
    background: var(--canvas-night);
    color: var(--on-primary);
    border: 1px solid var(--on-primary);
    border-radius: var(--r-pill);
    padding: var(--s-lg) var(--s-xl);
    font-family: var(--font-body);
    font-weight: 700;
    text-transform: uppercase;
    font-size: var(--type-button-cap-size);
    line-height: var(--type-button-cap-lh);
    letter-spacing: var(--type-button-cap-track);
    cursor: pointer;
    transition: background 160ms, color 160ms;
  }
  .ghost-pill:hover {
    background: var(--on-primary);
    color: var(--canvas-night);
  }

  /* ── Nav (overlay on dark) ──────────────────────────────────────── */
  nav.top {
    position: fixed; top: 0; left: 0; right: 0;
    z-index: 50;
    padding: var(--s-xl) var(--s-xxl);
    background: transparent;
  }
  nav.top .row {
    display: flex; align-items: center; justify-content: space-between;
    max-width: 100%;
  }
  nav.top .links {
    display: flex; gap: var(--s-xxl);
    align-items: center;
  }
  nav.top a {
    color: var(--on-primary);
    font-family: var(--font-body);
    font-weight: 700;
    text-transform: uppercase;
    font-size: var(--type-button-cap-size);
    letter-spacing: var(--type-button-cap-track);
    line-height: var(--type-button-cap-lh);
  }
  nav.top a:hover { opacity: 0.72; }

  /* Logo wordmark */
  .logo {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    font-family: var(--font-display);
    font-weight: 700;
    font-size: 18px;
    letter-spacing: 1.6px;
    text-transform: uppercase;
    color: var(--on-primary);
  }
  .logo .mark {
    display: inline-block;
    width: 6px; height: 6px;
    background: var(--on-primary);
    border-radius: 50%;
    margin-bottom: 1px;
  }

  /* ── Bands ───────────────────────────────────────────────────────── */
  section.band {
    position: relative;
    padding: var(--s-huge) 0;
  }
  section.band.full {
    min-height: 100vh;
    display: flex;
    align-items: center;
    padding: 0;
  }
  section.band.dark-soft { background: var(--canvas-night-soft); }
  section.band + section.band { border-top: 1px solid var(--hairline-on-dark); }

  /* ── Hero ───────────────────────────────────────────────────────── */
  section.hero {
    position: relative;
    min-height: 100vh;
    display: flex; align-items: center;
    overflow: hidden;
    padding: 120px 0 96px;
  }
  /* Constellation = our 'full-bleed photograph' */
  #hero-video {
    position: absolute; inset: 0;
    width: 100%; height: 100%;
    object-fit: cover;
    z-index: 0;
    pointer-events: none;
    background: var(--canvas-night);
    /* Dial 0.0 (invisible) → 1.0 (full strength).
       0.4 = very ambient · 0.55 = balanced · 0.75 = present but muted */
    opacity: 0.55;
  }
  /* Subtle vignette to anchor type — graded canvas, not a scrim.
     Two-stack: outer ring darkens edges (premium framing), inner
     bell darkens the center where the headline sits (legibility on
     bright video patches). Tunable rgba values 0.35 / 0.25 — lower
     values feel more cinematic, higher values feel safer. */
  .hero::before {
    content: ''; position: absolute; inset: 0; z-index: 1;
    background:
      radial-gradient(ellipse 50% 40% at 50% 50%, rgba(0,0,0,0.35) 0%, transparent 75%),
      radial-gradient(ellipse 70% 60% at 50% 50%, transparent 40%, rgba(0,0,0,0.55) 100%);
    pointer-events: none;
  }
  .hero .content {
    position: relative; z-index: 2;
    text-align: center;
    width: 100%;
    max-width: 920px;
    margin: 0 auto;
    padding: 0 var(--gutter);
  }
  .hero .eyebrow {
    display: block;
    margin-bottom: var(--s-xxl);
  }
  .hero h1 {
    margin: 0 0 var(--s-xl);
    /* Soft dark halo lifts the type off bright video patches.
       Stays invisible on dark patches. */
    text-shadow: 0 2px 14px rgba(0,0,0,0.55);
  }
  .hero .lede {
    margin: 0 auto var(--s-huge);
    max-width: 640px;
    color: var(--on-primary);
    text-shadow: 0 1px 8px rgba(0,0,0,0.65);
  }
  .hero .eyebrow,
  .hero .install-hint {
    text-shadow: 0 1px 4px rgba(0,0,0,0.7);
  }
  .hero .ctas { display: flex; justify-content: center; }

  /* Install hint: ONE unified pill containing label + divider + code.
     Visually balanced, single click target, premium "system caption" vibe. */
  .hero .install-hint {
    margin-top: var(--s-xl);
    display: inline-flex;
    align-items: stretch;
    border: 1px solid var(--on-primary);
    border-radius: var(--r-pill);
    background: rgba(0,0,0,0.45);
    overflow: hidden;
    cursor: pointer;
    transition: background 160ms, color 160ms, border-color 160ms;
  }
  .hero .install-hint .label,
  .hero .install-hint code {
    padding: 10px 18px;
    line-height: 1;
    font-size: 13.5px;
    display: inline-flex;
    align-items: center;
  }
  .hero .install-hint .label {
    font-family: var(--font-body);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 1.4px;
    color: var(--on-primary-mute);
    border-right: 1px solid rgba(255,255,255,0.22);
    padding-right: 16px;
  }
  .hero .install-hint code {
    font-family: var(--font-mono);
    font-weight: 500;
    letter-spacing: 0;
    color: var(--on-primary);
    padding-left: 16px;
  }
  .hero .install-hint:hover {
    background: var(--on-primary);
  }
  .hero .install-hint:hover .label,
  .hero .install-hint:hover code {
    color: var(--canvas-night);
  }
  .hero .install-hint:hover .label {
    border-right-color: rgba(0,0,0,0.2);
  }

  /* ── Stats band (hairline-separated cells) ──────────────────────── */
  .stats {
    display: grid; grid-template-columns: repeat(4, 1fr);
  }
  .stat-cell {
    padding: var(--s-huge) var(--s-xl);
    text-align: center;
    border-right: 1px solid var(--hairline-on-dark);
  }
  .stat-cell:last-child { border-right: none; }
  .stat-num {
    font-family: var(--font-display);
    font-size: 56px;
    font-weight: 700;
    line-height: 1;
    letter-spacing: 1px;
    color: var(--on-primary);
    margin-bottom: var(--s-md);
    font-variant-numeric: tabular-nums;
  }
  .stat-label { /* uses micro-cap */ }

  /* ── Section heading block ──────────────────────────────────────── */
  .section-head {
    text-align: center;
    margin-bottom: var(--s-huge);
  }
  .section-head .eyebrow { display: block; margin-bottom: var(--s-md); }
  .section-head h2 { margin: 0 auto var(--s-md); max-width: 880px; }
  .section-head .lede { max-width: 640px; margin: var(--s-md) auto 0; }

  /* ── Demo card (the engineering display) ────────────────────────── */
  .demo-card {
    background: var(--canvas-night-soft);
    border: 1px solid var(--hairline-on-dark);
    overflow: hidden;
    max-width: 960px;
    margin: 0 auto;
    border-radius: var(--r-xs);
  }
  .demo-card .titlebar {
    display: flex; align-items: center;
    gap: var(--s-sm);
    padding: var(--s-sm) var(--s-lg);
    border-bottom: 1px solid var(--hairline-on-dark);
  }
  .demo-card .titlebar .dots { display: flex; gap: 6px; }
  .demo-card .titlebar .dots span {
    width: 9px; height: 9px; border-radius: 50%;
    background: var(--hairline-on-dark);
  }
  .demo-card .titlebar .label {
    margin-left: var(--s-sm);
    font-family: var(--font-body);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.96px;
    color: var(--on-primary-mute);
  }
  .demo-card .titlebar .label .live {
    margin-left: var(--s-sm);
    padding: 1px 8px;
    border: 1px solid var(--on-primary);
    border-radius: var(--r-xs);
    font-size: 10px;
    color: var(--on-primary);
    letter-spacing: 1.17px;
  }
  .demo-body {
    padding: var(--s-huge) var(--s-huge) var(--s-huge);
    font-family: var(--font-mono);
    font-size: 13.5px;
    line-height: 1.75;
    color: var(--on-primary-mute);
  }
  .demo-body .meta { color: var(--ink-mute); font-style: italic; font-size: 12.5px; }
  .demo-body .you {
    color: var(--on-primary);
    font-weight: 500;
  }
  .demo-body .you::before { content: '> '; color: var(--ink-mute); font-style: normal; font-weight: 400; }
  .demo-body .claude { color: var(--on-primary); }
  .demo-body .claude::before { content: '* '; color: var(--on-primary-mute); font-weight: 700; }
  .demo-body .tool {
    margin: var(--s-md) 0;
    padding: var(--s-sm) var(--s-md);
    background: var(--canvas-night);
    border-left: 2px solid var(--on-primary);
    font-size: 12.5px;
    color: var(--on-primary-mute);
  }
  .demo-body .tool .name { color: var(--on-primary); font-weight: 500; }
  .demo-body .result {
    color: var(--on-primary-mute);
    font-size: 12.5px;
    padding-left: var(--s-md);
    border-left: 1px solid var(--hairline-on-dark);
    margin: var(--s-xxs) 0 var(--s-xs);
  }
  .demo-body .divider {
    border: none;
    border-top: 1px solid var(--hairline-on-dark);
    margin: var(--s-xl) 0;
  }
  .demo-callout {
    margin-top: var(--s-xl);
    padding: var(--s-md) var(--s-lg);
    border-left: 2px solid var(--on-primary);
    font-family: var(--font-body);
    font-size: 14.5px;
    line-height: 1.5;
    letter-spacing: 0.32px;
    color: var(--on-primary);
    text-transform: none;
    background: transparent;
  }
  .demo-callout strong { font-weight: 700; }

  /* ── Feature grid (engineering spec sheet) ──────────────────────── */
  .feature-grid {
    display: grid; grid-template-columns: repeat(3, 1fr);
    border-top: 1px solid var(--hairline-on-dark);
    border-bottom: 1px solid var(--hairline-on-dark);
  }
  .feature {
    padding: var(--s-huge) var(--s-xl);
    border-right: 1px solid var(--hairline-on-dark);
  }
  .feature:last-child { border-right: none; }
  .feature .num {
    /* micro-cap */
    font-family: var(--font-body);
    font-size: var(--type-micro-cap-size);
    text-transform: uppercase;
    letter-spacing: var(--type-micro-cap-track);
    color: var(--on-primary-mute);
    margin-bottom: var(--s-xl);
    display: block;
  }
  .feature h3 {
    font-family: var(--font-display);
    font-weight: 700;
    text-transform: uppercase;
    font-size: 26px;            /* was 22 */
    line-height: 1.12;
    letter-spacing: 0.7px;
    margin: 0 0 var(--s-md);
    color: var(--on-primary);
  }
  .feature p {
    margin: 0;
    font-size: var(--type-body-md-size);
    line-height: var(--type-body-md-lh);
    letter-spacing: var(--type-body-md-track);
    color: var(--on-primary-mute);
  }
  .feature p code {
    background: var(--canvas-night-soft);
    border: 1px solid var(--hairline-on-dark);
    padding: 1px 6px;
    color: var(--on-primary);
    font-size: 12.5px;
  }

  /* ── Comparison table (white-on-black inversion for "us") ──────── */
  .compare-wrap {
    border: 1px solid var(--hairline-on-dark);
    border-radius: var(--r-xs);
    overflow: hidden;
    max-width: 960px;
    margin: 0 auto;
  }
  table.compare { width: 100%; border-collapse: collapse; font-size: 14.5px; }
  table.compare th, table.compare td {
    padding: var(--s-lg) var(--s-xl);
    text-align: left;
    border-bottom: 1px solid var(--hairline-on-dark);
  }
  table.compare th {
    font-family: var(--font-body);
    font-size: var(--type-micro-cap-size);
    text-transform: uppercase;
    letter-spacing: var(--type-micro-cap-track);
    color: var(--on-primary-mute);
    font-weight: 700;
  }
  table.compare tr:last-child td { border-bottom: none; }
  table.compare td { color: var(--on-primary-mute); }
  table.compare td.product { font-weight: 700; color: var(--on-primary); text-transform: uppercase; letter-spacing: 0.96px; font-size: 13px; }
  table.compare tr.us td { background: var(--on-primary); color: var(--canvas-night); }
  table.compare tr.us td.product { color: var(--canvas-night); }
  table.compare tr.us td.product::before { content: '◆ '; }

  /* ── Install steps ──────────────────────────────────────────────── */
  .steps {
    display: grid; grid-template-columns: repeat(3, 1fr);
    border-top: 1px solid var(--hairline-on-dark);
    border-bottom: 1px solid var(--hairline-on-dark);
  }
  .step {
    padding: var(--s-huge) var(--s-xl);
    border-right: 1px solid var(--hairline-on-dark);
    display: flex; flex-direction: column;
  }
  .step:last-child { border-right: none; }
  .step .step-num {
    font-family: var(--font-body);
    font-size: var(--type-micro-cap-size);
    text-transform: uppercase;
    letter-spacing: var(--type-micro-cap-track);
    color: var(--on-primary-mute);
    margin-bottom: var(--s-md);
  }
  .step h4 {
    font-family: var(--font-display);
    font-weight: 700;
    text-transform: uppercase;
    font-size: 20px;            /* was 18 */
    letter-spacing: 0.7px;
    line-height: 1.2;
    margin: 0 0 var(--s-md);
    color: var(--on-primary);
  }
  .step p {
    margin: 0 0 var(--s-md);
    font-size: var(--type-body-md-size);
    line-height: var(--type-body-md-lh);
    letter-spacing: var(--type-body-md-track);
    color: var(--on-primary-mute);
    flex: 1;
  }
  .step pre {
    background: var(--canvas-night);
    border: 1px solid var(--hairline-on-dark);
    border-radius: var(--r-xs);
    padding: var(--s-md);
    overflow-x: auto; margin: 0;
    color: var(--on-primary-mute);
    font-size: 12.5px;
    line-height: 1.6;
    font-family: var(--font-mono);
  }
  .step pre code { background: none; padding: 0; }
  .step pre .cmt { color: var(--ink-mute); }
  .step pre .key { color: var(--on-primary); font-weight: 600; }
  .step pre .str { color: var(--on-primary-mute); }

  /* ── Closing band ───────────────────────────────────────────────── */
  section.closing {
    text-align: center;
    padding: 160px 0 120px;
    position: relative;
    overflow: hidden;
  }
  section.closing .content { position: relative; z-index: 2; }
  section.closing h2 { margin-bottom: var(--s-xl); }
  section.closing .lede {
    margin: 0 auto var(--s-huge);
    max-width: 560px;
  }
  /* faint background depth — the constellation as a tiny reprise */
  section.closing::before {
    content: ''; position: absolute; inset: 0; z-index: 0;
    background-image:
      radial-gradient(ellipse 60% 50% at 50% 60%, rgba(255,255,255,0.04), transparent 70%);
    pointer-events: none;
  }

  /* ── Footer ──────────────────────────────────────────────────────── */
  footer {
    background: var(--canvas-night);
    border-top: 1px solid var(--hairline-on-dark);
    padding: var(--s-xxl) var(--s-xl);
  }
  footer .row {
    display: flex; justify-content: space-between;
    align-items: flex-start; gap: var(--s-huge);
    flex-wrap: wrap;
    max-width: var(--max-w);
    margin: 0 auto;
    padding: var(--s-xl) var(--gutter) var(--s-huge);
  }
  footer .col { flex: 1; min-width: 200px; }
  footer h5 {
    font-family: var(--font-body);
    font-size: var(--type-micro-cap-size);
    text-transform: uppercase;
    letter-spacing: var(--type-micro-cap-track);
    color: var(--on-primary-mute);
    margin: 0 0 var(--s-md);
    font-weight: 700;
  }
  footer ul { list-style: none; padding: 0; margin: 0; }
  footer li {
    margin-bottom: var(--s-xs);
    font-family: var(--font-body);
    font-size: var(--type-caption-size);
    line-height: var(--type-caption-lh);
    color: var(--on-primary-mute);
  }
  footer li a { color: var(--on-primary-mute); }
  footer li a:hover { color: var(--on-primary); }
  footer .signature {
    border-top: 1px solid var(--hairline-on-dark);
    padding: var(--s-xl) var(--gutter);
    max-width: var(--max-w);
    margin: 0 auto;
    display: flex; justify-content: space-between;
    align-items: center; flex-wrap: wrap; gap: var(--s-md);
    font-family: var(--font-body);
    font-size: var(--type-caption-size);
    text-transform: uppercase;
    letter-spacing: 0.96px;
    color: var(--on-primary-mute);
  }
  footer .signature .badges { display: flex; gap: var(--s-xs); }
  footer .signature .badge {
    padding: 3px 10px;
    border: 1px solid var(--hairline-on-dark);
    border-radius: var(--r-xs);
    color: var(--on-primary-mute);
    font-size: 10px;
    letter-spacing: 0.96px;
  }
  footer .signature a { color: var(--on-primary); }

  /* ── Ledger feature band (single-screen 3D feature) ─────────────── */
  section.ledger-deepdive {
    min-height: 100vh;
    background: var(--canvas-night);
    position: relative;
    overflow: hidden;
    display: flex;
    align-items: center;
  }
  .ledger-sticky {
    position: relative;
    width: 100%;
    min-height: 80vh;
    overflow: hidden;
    display: flex;
    align-items: center;
  }
  #ledger-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    z-index: 0;
    pointer-events: none;
  }
  /* Soft center vignette for type legibility — same recipe as hero */
  .ledger-sticky::before {
    content: ''; position: absolute; inset: 0; z-index: 1;
    background:
      radial-gradient(ellipse 40% 35% at 50% 50%, rgba(0,0,0,0.45) 0%, transparent 75%),
      radial-gradient(ellipse 70% 60% at 50% 50%, transparent 40%, rgba(0,0,0,0.6) 100%);
    pointer-events: none;
  }
  .ledger-overlay {
    position: relative;
    z-index: 2;
    width: 100%;
    max-width: var(--max-w);
    margin: 0 auto;
    padding: 0 var(--gutter);
  }
  .ledger-frame {
    text-align: left;
    max-width: 640px;
  }
  .ledger-frame .eyebrow {
    display: block;
    margin-bottom: var(--s-lg);
    color: var(--on-primary);
  }
  .ledger-frame h2 {
    font-family: var(--font-display);
    font-weight: 700;
    text-transform: uppercase;
    font-size: clamp(40px, 5vw, 64px);
    line-height: 1.0;
    letter-spacing: 1.2px;
    color: var(--on-primary);
    margin: 0 0 var(--s-xl);
    text-shadow: 0 2px 16px rgba(0,0,0,0.6);
  }
  .ledger-frame p {
    font-size: 18px;
    line-height: 1.55;
    color: var(--on-primary);
    margin: 0;
    max-width: 520px;
    text-shadow: 0 1px 8px rgba(0,0,0,0.7);
  }

  /* ── Responsive ─────────────────────────────────────────────────── */
  @media (max-width: 1279px) {
    :root { --type-display-xxl-size: 64px; }
  }
  @media (max-width: 960px) {
    :root {
      --type-display-xxl-size: 48px;
      --type-display-xxl-track: 1.2px;
      --gutter: var(--s-xl);
    }
    .feature-grid, .stats, .steps { grid-template-columns: 1fr; }
    .feature, .stat-cell, .step { border-right: none; border-bottom: 1px solid var(--hairline-on-dark); }
    .feature:last-child, .stat-cell:last-child, .step:last-child { border-bottom: none; }
    nav.top .links a:not(.cta) { display: none; }
    nav.top { padding: var(--s-md) var(--s-xl); }
    section.hero { padding: 100px 0 60px; min-height: auto; }
    section.band { padding: var(--s-xxl) 0; }
    .ledger-frame h2 { font-size: 38px; }
  }
  @media (max-width: 600px) {
    :root { --type-display-xxl-size: 40px; }
    .stat-num { font-size: 40px; }
    .demo-body { padding: var(--s-xl) var(--s-md); font-size: 12px; }
  }
  @media (prefers-reduced-motion: reduce) {
    #hero-video { display: none; }
    .hero::before { opacity: 0.5; }
  }
</style>
</head>

<body>

<!-- ── Nav (overlay) ─────────────────────────────────────────────────── -->
<nav class="top">
  <div class="row">
    <a href="/" class="logo">KORG<span class="mark"></span></a>
    <div class="links">
      <a href="#how">How it works</a>
      <a href="#compare">Comparison</a>
      <a href="#install">Install</a>
      <a href="https://github.com/New1Direction/korg">GitHub</a>
    </div>
  </div>
</nav>

<!-- ── Hero ─────────────────────────────────────────────────────────── -->
<section class="hero">
  <video
    id="hero-video"
    src="assets/hero-loop.mp4"
    autoplay
    muted
    playsinline
    preload="auto"
    aria-hidden="true"
  ></video>
  <script>
    // Spin once on arrival; freeze on last frame.
    // If the natural end-frame is mid-motion (i.e. doesn't compose well
    // as a static hero), un-comment FREEZE_AT below and the video will
    // stop at that specific timestamp instead.
    (function () {
      const v = document.getElementById('hero-video');
      if (!v) return;
      // const FREEZE_AT = 0.62; // seconds — pick whichever frame composes best
      // if (typeof FREEZE_AT === 'number') {
      //   v.addEventListener('timeupdate', () => {
      //     if (v.currentTime >= FREEZE_AT) { v.pause(); v.currentTime = FREEZE_AT; }
      //   });
      // }
      v.addEventListener('ended', () => {
        // Ensure last frame stays visible (default browser behavior, but
        // explicit so we can hook future transitions here).
        v.classList.add('done');
      });
    })();
  </script>
  <div class="content">
    <span class="micro-cap eyebrow">COGNITIVE INFRASTRUCTURE · OPEN SOURCE · CROSS-VENDOR</span>
    <h1 class="display-xxl">YOUR AI SESSIONS,<br>REMEMBERED.</h1>
    <p class="body-lg lede">
      Capture every Claude Code, Codex, or Grok session into one open
      ledger on your machine. Recall semantically across all of them
      from inside Claude Code. Cross-vendor memory you actually own.
    </p>
    <div class="ctas">
      <a class="ghost-pill" href="#install">GET STARTED</a>
    </div>
    <div class="install-hint" role="button" tabindex="0"
         onclick="(function(el){var c=el.querySelector('code');var t=c.textContent;navigator.clipboard.writeText(t);c.textContent='COPIED';setTimeout(function(){c.textContent=t;},1200);})(this)">
      <span class="label">INSTALL</span>
      <code>npx -y @korgg/recall-mcp</code>
    </div>
  </div>
</section>

<!-- ── Stats band ────────────────────────────────────────────────────── -->
<section class="band dark-soft" style="padding: 0;">
  <div class="stats">
    <div class="stat-cell">
      <div class="stat-num">04</div>
      <div class="micro-cap stat-label">CAPTURE ADAPTERS</div>
    </div>
    <div class="stat-cell">
      <div class="stat-num">715+</div>
      <div class="micro-cap stat-label">TESTS PASSING</div>
    </div>
    <div class="stat-cell">
      <div class="stat-num">30+</div>
      <div class="micro-cap stat-label">MCP TOOLS EXPOSED</div>
    </div>
    <div class="stat-cell">
      <div class="stat-num">100%</div>
      <div class="micro-cap stat-label">LOCAL · OPEN SOURCE</div>
    </div>
  </div>
</section>

<!-- ── Demo band ────────────────────────────────────────────────────── -->
<section class="band">
  <div class="container">
    <div class="section-head">
      <span class="micro-cap eyebrow">THE KILLER DEMO</span>
      <h2 class="display-lg">YOU SOLVED THIS BEFORE.<br>NOW YOUR AI KNOWS IT TOO.</h2>
      <p class="body-lg lede">
        Three weeks ago in a different session, you fixed an OAuth refresh
        bug. Today you've forgotten exactly how. Without memory, Claude
        gives a generic OAuth explanation. With Korg installed:
      </p>
    </div>

    <div class="demo-card">
      <div class="titlebar">
        <div class="dots"><span></span><span></span><span></span></div>
        <div class="label">CLAUDE-CODE / KORG-RECALL <span class="live">LIVE</span></div>
      </div>
      <div class="demo-body">
        <div class="you">how did i fix the oauth token refresh failing on 401 last time?</div>
        <hr class="divider">
        <div class="meta">// claude calls the recall MCP tool</div>
        <div class="tool">
          <span class="name">recall</span>(query="oauth token refresh 401", mode="semantic", top_n=3)
        </div>
        <div class="result">[recall · semantic] 2 match(es):</div>
        <div class="result">&nbsp;&nbsp;· seq=4127 score=0.84 agent=claude-code#a3b · the 401 happens because the access_token expires during the request lifecycle…</div>
        <div class="result">&nbsp;&nbsp;· seq=4131 score=0.79 agent=claude-code#a3b · added a 5-second refresh window + retry-once-on-401 wrapper…</div>
        <hr class="divider">
        <div class="claude">You solved this on May 7 — the 401 was from racing the token refresh. You added a 5-second refresh window and a retry-once-on-401 wrapper around fetch. Want me to apply the same pattern here?</div>
        <div class="demo-callout">
          <strong>THAT'S IT.</strong> No prompt engineering. No doc dive. No
          re-searching Stack Overflow. The model recalled the exact session
          where you solved it — across days, projects, machines if you sync.
        </div>
      </div>
    </div>
  </div>
</section>

<!-- ── How it works ─────────────────────────────────────────────────── -->
<section class="band dark-soft" id="how">
  <div class="container">
    <div class="section-head">
      <span class="micro-cap eyebrow">HOW IT WORKS</span>
      <h2 class="display-lg">THREE PIECES.<br>ALL RUNNING LOCALLY.</h2>
      <p class="body-lg lede">
        No cloud, no API keys, no vendor account. Your ledger is one
        JSONL file on your disk. You can grep it, version it, replay it.
      </p>
    </div>

    <div class="feature-grid">
      <div class="feature">
        <span class="num">01 — CAPTURE</span>
        <h3>EVERY SESSION,<br>AUTOMATICALLY.</h3>
        <p>
          A background daemon tails <code>~/.claude/projects/**/*.jsonl</code>
          as Claude Code writes them. Every prompt, tool call, and reply
          appended to one open ledger. Adapters for OpenAI Codex (WebSocket)
          and Grok Heavy (NDJSON) ship in the box.
        </p>
      </div>
      <div class="feature">
        <span class="num">02 — RECALL</span>
        <h3>SEARCH BY MEANING,<br>NOT KEYWORDS.</h3>
        <p>
          A single MCP tool — <code>recall</code> — embeds your query with
          BGE-small (local, no API), cosine-ranks against your entire ledger,
          returns top matches. Works in Claude Code or any MCP client via
          <code>npx @korgg/recall-mcp</code>.
        </p>
      </div>
      <div class="feature">
        <span class="num">03 — INVOKE</span>
        <h3>RE-EXECUTE,<br>DON'T RE-DERIVE.</h3>
        <p>
          Recall returns events tagged with stable <code>tool_name</code>s.
          A bridge (<code>@korgg/introspect-mcp</code>) exposes every ecosystem
          binary's callables under the same identifiers. Recall a past
          command. Invoke it on the current branch. Loop closes.
        </p>
      </div>
    </div>
  </div>
</section>

<!-- ── Ledger feature band (single-screen, with 3D mesh) ────────────── -->
<section class="ledger-deepdive" id="ledger">
  <div class="ledger-sticky">
    <canvas id="ledger-canvas"></canvas>
    <div class="ledger-overlay">
      <div class="ledger-frame">
        <span class="micro-cap eyebrow">THE SHAPE OF MEMORY</span>
        <h2>CAUSAL.<br>SEARCHABLE.<br>STRUCTURED.</h2>
        <p>
          Every event carries a stable <code>tool_name</code>, a
          <code>triggered_by</code> parent, and a flattened embedding
          text. Recall walks it semantically; the bridge re-executes
          from the same identifiers. The loop closes deterministically.
        </p>
      </div>
    </div>
  </div>
</section>

<!-- ── Comparison ───────────────────────────────────────────────────── -->
<section class="band" id="compare">
  <div class="container">
    <div class="section-head">
      <span class="micro-cap eyebrow">WHY THIS DIDN'T EXIST BEFORE</span>
      <h2 class="display-lg">THE ONLY MEMORY LAYER<br>THAT SPANS VENDOR BOUNDARIES.</h2>
      <p class="body-lg lede">
        Every other vendor has the same incentive: lock you into their
        memory. Korg has the opposite — the ledger format is open JSONL
        on your own disk.
      </p>
    </div>

    <div class="compare-wrap">
      <table class="compare">
        <thead>
          <tr>
            <th>PRODUCT</th>
            <th>MEMORY SCOPE</th>
            <th>TOOLS COVERED</th>
            <th>FORMAT</th>
          </tr>
        </thead>
        <tbody>
          <tr><td class="product">CHATGPT MEMORY</td><td>per-account</td><td>OpenAI only</td><td>proprietary</td></tr>
          <tr><td class="product">ANTHROPIC MEMORY</td><td>—</td><td>—</td><td>ships nothing today</td></tr>
          <tr><td class="product">CURSOR MEMORIES</td><td>per-project</td><td>Cursor only</td><td>proprietary</td></tr>
          <tr class="us"><td class="product">KORG</td><td>per-machine</td><td>any tool with a capture adapter</td><td>open JSONL · greppable · yours</td></tr>
        </tbody>
      </table>
    </div>
  </div>
</section>

<!-- ── Install ──────────────────────────────────────────────────────── -->
<section class="band dark-soft" id="install">
  <div class="container">
    <div class="section-head">
      <span class="micro-cap eyebrow">INSTALL · SIXTY SECONDS</span>
      <h2 class="display-lg">THREE STEPS.<br>ONE JSON EDIT.<br>RESTART.</h2>
      <p class="body-lg lede">
        No Python install needed for the recall server (npx). The capture
        daemon is a one-time pipx install — runs in the background forever
        after.
      </p>
    </div>

    <div class="steps">
      <div class="step">
        <div class="step-num">STEP 01</div>
        <h4>REGISTER THE MCP SERVER</h4>
        <p>Add to <code>~/.claude.json</code> — the only config edit.</p>
<pre><code><span class="cmt">// ~/.claude.json</span>
{
  <span class="key">"mcpServers"</span>: {
    <span class="key">"korg-recall"</span>: {
      <span class="key">"command"</span>: <span class="str">"npx"</span>,
      <span class="key">"args"</span>: [<span class="str">"-y"</span>, <span class="str">"@korgg/recall-mcp"</span>]
    }
  }
}</code></pre>
      </div>
      <div class="step">
        <div class="step-num">STEP 02</div>
        <h4>START THE CAPTURE DAEMON</h4>
        <p>One-time install. <code>--tail</code> watches your sessions forever.</p>
<pre><code><span class="cmt"># Python — one time</span>
pipx install korg-claude-code-adapter
korg-ingest-claude <span class="key">--tail</span> &amp;</code></pre>
      </div>
      <div class="step">
        <div class="step-num">STEP 03</div>
        <h4>RESTART CLAUDE CODE</h4>
        <p>Done. <code>recall</code> is in the toolset. Every session compounds.</p>
<pre><code><span class="cmt"># try it:</span>
<span class="str">"how did i solve X last week?"</span></code></pre>
      </div>
    </div>
  </div>
</section>

<!-- ── Closing ──────────────────────────────────────────────────────── -->
<section class="band closing">
  <div class="content container">
    <span class="micro-cap eyebrow" style="margin-bottom: var(--s-xl); display: block;">BUILT IN PUBLIC · MIT</span>
    <h2 class="display-xl">FREE FOREVER LOCALLY.<br>STAR IT, FORK IT, AUDIT IT.</h2>
    <p class="body-lg lede" style="margin: var(--s-xl) auto 0;">
      ~715 tests across 5 languages of code. Four GitHub repos. MIT.
      No cloud account required.
    </p>
    <div class="ctas" style="margin-top: var(--s-huge);">
      <a class="ghost-pill" href="https://github.com/New1Direction/korg">VIEW ON GITHUB</a>
    </div>
  </div>
</section>

<!-- ── Footer ───────────────────────────────────────────────────────── -->
<footer>
  <div class="row">
    <div class="col">
      <a href="/" class="logo" style="margin-bottom: var(--s-md);">KORG<span class="mark"></span></a>
      <p class="caption" style="margin: var(--s-sm) 0 0; max-width: 280px; color: var(--on-primary-mute);">
        Cognitive infrastructure for AI agents. Capture every session.
        Recall across all of them. Invoke from inside Claude Code.
      </p>
    </div>
    <div class="col">
      <h5>ECOSYSTEM</h5>
      <ul>
        <li><a href="https://github.com/New1Direction/korg">korg</a></li>
        <li><a href="https://github.com/New1Direction/korgex">korgex</a></li>
        <li><a href="https://github.com/New1Direction/korgchat">korgchat</a></li>
        <li><a href="https://github.com/New1Direction/thumper">thumper</a></li>
      </ul>
    </div>
    <div class="col">
      <h5>NPM PACKAGES</h5>
      <ul>
        <li><a href="https://www.npmjs.com/package/@korgg/recall-mcp">@korgg/recall-mcp</a></li>
        <li><a href="https://www.npmjs.com/package/@korgg/introspect-mcp">@korgg/introspect-mcp</a></li>
      </ul>
    </div>
    <div class="col">
      <h5>CONNECT</h5>
      <ul>
        <li><a href="https://github.com/New1Direction">github.com/New1Direction</a></li>
        <li><a href="https://github.com/New1Direction/korg/issues">Issues &amp; feedback</a></li>
      </ul>
    </div>
  </div>
  <div class="signature">
    <div>BUILT BY <a href="https://github.com/New1Direction">ARES</a> · 2026</div>
    <div class="badges">
      <span class="badge">MIT</span>
      <span class="badge">OPEN SOURCE</span>
      <span class="badge">NO TELEMETRY</span>
    </div>
  </div>
</footer>

<!-- ── Ledger deep-dive 3D — scroll-driven .glb rotation + frame swap ── -->
<script type="module">
  // Skip the whole thing on reduced-motion preference.
  if (!(window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches)) {
    const canvas = document.getElementById('ledger-canvas');
    const section = document.getElementById('ledger');
    if (canvas && section) {
      // Use bare specifiers so GLTFLoader / DRACOLoader's internal
      // `import 'three'` statements resolve via the importmap in <head>.
      const THREE = await import('three');
      const { GLTFLoader } = await import('three/addons/loaders/GLTFLoader.js');
      const { DRACOLoader } = await import('three/addons/loaders/DRACOLoader.js');

      // ── Scene setup ──
      const scene = new THREE.Scene();
      const camera = new THREE.PerspectiveCamera(35, 1, 0.1, 100);
      camera.position.set(0, 0, 3.2);

      const renderer = new THREE.WebGLRenderer({
        canvas, antialias: true, alpha: true,
        powerPreference: 'low-power',
      });
      renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
      renderer.toneMapping = THREE.ACESFilmicToneMapping;
      renderer.toneMappingExposure = 1.1;
      renderer.outputColorSpace = THREE.SRGBColorSpace;

      // ── Lighting (premium key + soft fill, no HDR for self-containment) ──
      const key = new THREE.DirectionalLight(0xffffff, 2.4);
      key.position.set(2.5, 3, 2);
      scene.add(key);
      const fill = new THREE.DirectionalLight(0xffffff, 0.55);
      fill.position.set(-3, -1, 1.5);
      scene.add(fill);
      const rim = new THREE.DirectionalLight(0xffffff, 0.8);
      rim.position.set(0, -2, -3);
      scene.add(rim);
      scene.add(new THREE.AmbientLight(0xffffff, 0.18));

      // ── The .glb mesh (with wireframe-icosphere fallback while loading) ──
      const modelGroup = new THREE.Group();
      scene.add(modelGroup);

      const fallbackGeom = new THREE.IcosahedronGeometry(0.85, 1);
      const fallbackMat = new THREE.MeshStandardMaterial({
        color: 0xffffff, metalness: 0.9, roughness: 0.22,
        wireframe: true, transparent: true, opacity: 0.55,
      });
      const fallback = new THREE.Mesh(fallbackGeom, fallbackMat);
      modelGroup.add(fallback);

      const dracoLoader = new DRACOLoader();
      dracoLoader.setDecoderPath('https://www.gstatic.com/draco/versioned/decoders/1.5.6/');
      dracoLoader.setDecoderConfig({ type: 'js' });
      const gltfLoader = new GLTFLoader();
      gltfLoader.setDRACOLoader(dracoLoader);

      gltfLoader.load(
        'assets/hero-mesh.glb',
        (gltf) => {
          modelGroup.remove(fallback);
          const loaded = gltf.scene;
          const box = new THREE.Box3().setFromObject(loaded);
          const size = box.getSize(new THREE.Vector3());
          const maxDim = Math.max(size.x, size.y, size.z);
          loaded.scale.setScalar(1.6 / maxDim);
          box.setFromObject(loaded);
          const center = box.getCenter(new THREE.Vector3());
          loaded.position.sub(center);
          modelGroup.add(loaded);
        },
        undefined,
        (err) => {
          console.warn('[ledger-3d] glb load failed, using wireframe fallback:', err);
        }
      );

      // ── Sizing ──
      function resize() {
        const w = canvas.clientWidth;
        const h = canvas.clientHeight;
        renderer.setSize(w, h, false);
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
      }
      const ro = new ResizeObserver(resize);
      ro.observe(canvas);
      resize();

      // ── Render only while section is on-screen ──
      let isVisible = false;
      const io = new IntersectionObserver(([entry]) => { isVisible = entry.isIntersecting; });
      io.observe(section);

      // ── Animation loop — gentle continuous rotation, no scroll-driven scrub ──
      function loop() {
        requestAnimationFrame(loop);
        if (!isVisible) return;
        const now = performance.now();
        modelGroup.rotation.y = now * 0.00018;
        modelGroup.rotation.x = Math.sin(now * 0.00025) * 0.08;
        modelGroup.rotation.z = Math.sin(now * 0.0004) * 0.03;
        renderer.render(scene, camera);
      }
      loop();
    }
  }
</script>

</body>
</html>
"##;

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
            // Prefer strip_prefix over starts_with + slice — same effect, but
            // expresses the intent more directly and is impossible to
            // accidentally turn into a panic if someone refactors the constant.
            if let Some(token) = auth_val.strip_prefix("Bearer ") {
                token.trim().to_string()
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
                // Mock-auth fallback for local dev/CI. Two gates so neither alone is sufficient:
                //  1. `cfg(debug_assertions)` — code does not compile into release builds at all.
                //  2. `KORG_ALLOW_MOCK_AUTH` env var — even debug builds must opt in explicitly.
                // Also: the mock session is NOT persisted to the auth store — it's request-scoped only.
                #[cfg(debug_assertions)]
                {
                    let mock_allowed = std::env::var("KORG_ALLOW_MOCK_AUTH").is_ok()
                        && (user_id == "claude-code-user" || user_id.starts_with("mock-"));
                    if mock_allowed {
                        let mock_session = korg_auth::store::UserSession {
                            user_id: user_id.clone(),
                            codex_access_token: "mock-codex-token".to_string(),
                            subscription_tier: korg_core::SubscriptionTier::Premium,
                            anthropic_access_token: "mock-anthropic-token".to_string(),
                            refresh_token: Some("mock-refresh-token".to_string()),
                            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
                        };
                        return Ok(AuthenticatedUser {
                            user_id,
                            session: mock_session,
                        });
                    }
                }
                Err((
                    axum::http::StatusCode::UNAUTHORIZED,
                    "Active session not found. Please log in first.",
                ))
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

    // Only forward a vetted allowlist of upstream headers. The previous code
    // blindly copied every header, which meant a compromised upstream (or
    // header-smuggling MITM) could set Set-Cookie / Location in our origin.
    const FORWARDED_HEADERS: &[&str] = &[
        "content-type",
        "content-length",
        "content-encoding",
        "cache-control",
        "etag",
        "last-modified",
        "request-id",
        "x-request-id",
        "anthropic-request-id",
        "retry-after",
    ];
    let mut headers = axum::http::HeaderMap::new();
    for (k, v) in response.headers().iter() {
        let k_lower = k.as_str().to_ascii_lowercase();
        if !FORWARDED_HEADERS.contains(&k_lower.as_str()) {
            continue;
        }
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

    #[test]
    fn default_bind_addr_is_loopback_not_all_interfaces() {
        // Security: with no override the server must bind loopback only — never
        // 0.0.0.0, which exposed the (mostly unauthenticated) control + telemetry
        // routes to the whole local network.
        let addr = resolve_bind_addr(None);
        assert!(addr.starts_with("127.0.0.1"), "default must be loopback, got {addr}");
        assert!(!addr.starts_with("0.0.0.0"), "default must not bind all interfaces");
    }

    #[test]
    fn bind_addr_honors_explicit_override() {
        // Intentional network exposure stays possible, but only by explicit opt-in.
        assert_eq!(resolve_bind_addr(Some("0.0.0.0:9000".into())), "0.0.0.0:9000");
    }

    #[tokio::test]
    async fn wasm_routes_404_when_no_frontend_is_bundled() {
        // No WASM frontend ships in this build — the routes must 404 honestly,
        // not serve an empty 200 that masquerades as a real (empty) asset.
        use axum::response::IntoResponse;
        let js = wasm_js_handler().await.into_response();
        assert_eq!(js.status(), axum::http::StatusCode::NOT_FOUND);
        let wasm = wasm_bytes_handler().await.into_response();
        assert_eq!(wasm.status(), axum::http::StatusCode::NOT_FOUND);
    }

    /// Set KORG_MASTER_KEY once for the whole test binary so the auth store's
    /// production-mode `expect()` doesn't panic in tests. Anything that touches
    /// JsonTokenStore must call this first.
    fn ensure_test_master_key() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            if std::env::var("KORG_MASTER_KEY").is_err() {
                std::env::set_var(
                    "KORG_MASTER_KEY",
                    "test-master-key-for-unit-tests-only-not-secret",
                );
            }
        });
    }

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
        ensure_test_master_key();
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
        ensure_test_master_key();
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
        ensure_test_master_key();
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
