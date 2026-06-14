//! Session Backend Abstraction
//!
//! Decouples orchestration logic from the mechanism used to launch and communicate
//! with worker processes. The `SessionBackend` trait has two implementations:
//! - `SubprocessBackend` — spawns `korg worker` as a local subprocess (default)
//! - `DockerBackend` — wraps each worker in a docker container
//!
//! Workers emit `WorkerEvent`s rather than raw stdout bytes. This feeds:
//! - The capability journal
//! - The tracing system
//! - The live cockpit / TUI
//! - The `WorkspaceManager` state transitions
//!
//! # Architecture
//!
//! ```text
//! WorkspaceManager
//!     ↓ workspace_id
//! SessionBackend::spawn(SessionSpec)
//!     ↓ SessionHandle
//! EventStream → WorkerEvent  (typed, structured)
//!     ↓
//! workers::dispatch_level fan-in
//! ```

use crate::workspace::WorkspaceId;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc;

// =========================================================================
// WorkerEvent — typed event stream
// =========================================================================

/// An artifact kind produced by a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    SourcePatch,
    TestResult,
    KtransEntry,
    SemanticMerge,
    ProvenanceCertificate,
    Unknown,
}

/// A tool invocation emitted by a worker (structured, not raw stdout).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub routing_id: String,
    pub args: serde_json::Value,
    pub invoked_at: DateTime<Utc>,
}

/// Typed events emitted by a worker process over its lifetime.
///
/// Replace raw stdout scraping. These events are the primitive that feeds
/// tracing, the journal, and the live cockpit.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    /// Worker process started.
    Started {
        workspace_id: WorkspaceId,
        persona: String,
        routing_id: String,
        ts: DateTime<Utc>,
    },
    /// Raw line from the worker's stdout.
    Stdout { line: String, ts: DateTime<Utc> },
    /// Raw line from the worker's stderr.
    Stderr { line: String, ts: DateTime<Utc> },
    /// Worker is making a tool call.
    ToolCall(ToolInvocation),
    /// Worker produced a file artifact.
    ArtifactProduced {
        path: PathBuf,
        kind: ArtifactKind,
        ts: DateTime<Utc>,
    },
    /// Periodic heartbeat from a long-running worker.
    Heartbeat {
        ts: DateTime<Utc>,
        elapsed_secs: u64,
    },
    /// Non-fatal warning from the worker.
    Warning { message: String, ts: DateTime<Utc> },
    /// Worker failed unrecoverably.
    Failed { reason: String, ts: DateTime<Utc> },
    /// Worker completed successfully.
    Completed { exit_code: i32, ts: DateTime<Utc> },
    /// Opaque/structured verified ACP message received from the worker
    AcpMsg {
        message: crate::acp::AcpMessage,
        verified: bool,
    },
}

impl WorkerEvent {
    pub fn started(workspace_id: WorkspaceId, persona: &str, routing_id: &str) -> Self {
        Self::Started {
            workspace_id,
            persona: persona.into(),
            routing_id: routing_id.into(),
            ts: Utc::now(),
        }
    }

    pub fn completed(exit_code: i32) -> Self {
        Self::Completed {
            exit_code,
            ts: Utc::now(),
        }
    }

    pub fn failed(reason: impl Into<String>) -> Self {
        Self::Failed {
            reason: reason.into(),
            ts: Utc::now(),
        }
    }

    pub fn heartbeat(elapsed_secs: u64) -> Self {
        Self::Heartbeat {
            ts: Utc::now(),
            elapsed_secs,
        }
    }

    /// Returns true if this event signals the end of the worker's lifecycle.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Completed { .. })
    }

    /// Log this event to the tracing system with appropriate level.
    pub fn trace(&self, workspace_id: &WorkspaceId) {
        match self {
            Self::Started {
                persona,
                routing_id,
                ..
            } => {
                tracing::info!(
                    %workspace_id, persona = %persona,
                    routing_id = %routing_id, "worker_event_started"
                );
            }
            Self::Stdout { line, .. } => {
                tracing::debug!(%workspace_id, stdout = %line, "worker_event_stdout");
            }
            Self::Stderr { line, .. } => {
                tracing::debug!(%workspace_id, stderr = %line, "worker_event_stderr");
            }
            Self::ToolCall(inv) => {
                tracing::debug!(
                    %workspace_id, tool = %inv.tool_name,
                    "worker_event_tool_call"
                );
                korg_core::metrics::record_agent_tool_invocation(&inv.tool_name);
            }
            Self::ArtifactProduced { path, kind, .. } => {
                tracing::info!(
                    %workspace_id, path = %path.display(),
                    kind = ?kind, "worker_event_artifact"
                );
            }
            Self::Heartbeat { elapsed_secs, .. } => {
                tracing::debug!(%workspace_id, elapsed_secs, "worker_event_heartbeat");
            }
            Self::Warning { message, .. } => {
                tracing::warn!(%workspace_id, message = %message, "worker_event_warning");
            }
            Self::Failed { reason, .. } => {
                tracing::error!(%workspace_id, reason = %reason, "worker_event_failed");
            }
            Self::Completed { exit_code, .. } => {
                tracing::info!(%workspace_id, exit_code, "worker_event_completed");
            }
            Self::AcpMsg { message, verified } => {
                tracing::debug!(%workspace_id, verified = *verified, ?message, "worker_event_acp_msg");
            }
        }
    }
}

/// Channel receiver for worker events. The consumer side of the event stream.
pub type EventStream = mpsc::Receiver<WorkerEvent>;

/// Channel sender for worker events. The producer side — held by the session backend.
pub type EventSink = mpsc::Sender<WorkerEvent>;

// =========================================================================
// SessionSpec and SessionHandle
// =========================================================================

/// Specification for spawning a worker session.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    /// The workspace this session executes within.
    pub workspace_id: WorkspaceId,
    /// Persona name (used to build the worker --id flag).
    pub persona: String,
    /// ACP routing ID for this work package.
    pub routing_id: String,
    /// The ACP payload to send as the first RouteWork message.
    pub payload: String,
    /// Timeout in seconds before the session is forcibly terminated.
    pub timeout_secs: u64,
    /// Campaign session id. Used (with `speculative`) to derive the shared warm
    /// `CARGO_TARGET_DIR` so the worker's `cargo check` reuses the warmed cache.
    pub session_id: String,
    /// When true, the worker child is spawned with `CARGO_TARGET_DIR` pointing at
    /// `warm_target_dir(session_id)` — the anti-theater link to the warm boot.
    pub speculative: bool,
}

/// An opaque handle to a running session. Returned by `SessionBackend::spawn`.
#[derive(Debug, Clone)]
pub struct SessionHandle {
    /// Internal handle identifier.
    pub id: String,
    /// The workspace this session is bound to.
    pub workspace_id: WorkspaceId,
    /// When the session was spawned.
    pub spawned_at: DateTime<Utc>,
}

impl SessionHandle {
    pub fn new(workspace_id: WorkspaceId, routing_id: &str) -> Self {
        Self {
            id: format!("sess-{}-{}", routing_id, uuid::Uuid::now_v7()),
            workspace_id,
            spawned_at: Utc::now(),
        }
    }
}

// =========================================================================
// SessionBackend trait
// =========================================================================

/// Abstraction over worker execution backends.
///
/// Implementations:
/// - `SubprocessBackend` — local `korg worker` subprocess
/// - `DockerBackend` — isolated docker container
///
/// The trait is object-safe (via `async_trait`) and `Send + Sync`, making
/// it suitable for `Arc<dyn SessionBackend>`.
#[async_trait]
pub trait SessionBackend: Send + Sync + std::fmt::Debug {
    /// Spawn a worker session and return a handle + event stream.
    ///
    /// The event stream carries all lifecycle events from the worker.
    /// The caller drives the fan-in loop via `stream.recv()`.
    async fn spawn(
        &self,
        spec: &SessionSpec,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Result<(SessionHandle, EventStream)>;

    /// Terminate a running session (SIGTERM → SIGKILL).
    async fn terminate(&self, handle: &SessionHandle) -> Result<()>;

    /// Human-readable backend identifier for logging and metrics.
    fn backend_kind(&self) -> &'static str;
}

// =========================================================================
// Speculative warm-cache env decision (pure, testable)
// =========================================================================

/// Decide the extra env a worker child needs to reuse the warm shared cargo
/// cache. Pure so the anti-theater link can be unit-tested without inspecting a
/// spawned `Command`.
///
/// When `speculative` is on, returns a single `("CARGO_TARGET_DIR", <path>)` pair
/// equal to [`crate::execution::warm_target_dir`] for this `session_id` — exactly
/// what the warm boot populated — so the worker's `cargo check` reuses it. When
/// off (the default), returns an empty vec and the worker uses its own target dir
/// (unchanged behavior).
pub fn worker_cargo_env(session_id: &str, speculative: bool) -> Vec<(String, String)> {
    if !speculative {
        return Vec::new();
    }
    let target = crate::execution::warm_target_dir(session_id);
    vec![(
        "CARGO_TARGET_DIR".to_string(),
        target.to_string_lossy().into_owned(),
    )]
}

// =========================================================================
// SubprocessBackend
// =========================================================================

/// Spawns `korg worker` as a local subprocess.
///
/// Replaces the ad-hoc process spawning in `workers.rs` with a clean,
/// instrumented, event-driven implementation.
#[derive(Debug, Clone)]
pub struct SubprocessBackend {
    pub active_pids: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, u32>>>,
}

impl Default for SubprocessBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SubprocessBackend {
    pub fn new() -> Self {
        Self {
            active_pids: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        }
    }
}

#[async_trait]
impl SessionBackend for SubprocessBackend {
    fn backend_kind(&self) -> &'static str {
        "subprocess"
    }

    async fn spawn(
        &self,
        spec: &SessionSpec,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Result<(SessionHandle, EventStream)> {
        use crate::acp::AcpMessage;
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};

        let handle = SessionHandle::new(spec.workspace_id.clone(), &spec.routing_id);
        let (tx, rx) = mpsc::channel::<WorkerEvent>(256);

        // Build the command (mirrors workers.rs build_local_cmd)
        let exe = std::env::current_exe()?;
        let mut cmd = tokio::process::Command::new(exe);
        cmd.arg("worker")
            .arg("--id")
            .arg(format!("{}-{}", spec.persona, spec.routing_id))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Anti-theater link: when speculative is on, point this worker's cargo at
        // the warm shared cache the warm boot populated, so `observation::cargo_check`
        // (which honors CARGO_TARGET_DIR automatically) reuses it instead of cold.
        for (k, v) in worker_cargo_env(&spec.session_id, spec.speculative) {
            cmd.env(k, v);
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn()?;

        let pid = child.id().unwrap_or(0);
        if pid > 0 {
            let mut active = self.active_pids.lock().unwrap();
            active.insert(handle.id.clone(), pid);
        }

        let mut stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut stderr = BufReader::new(child.stderr.take().unwrap());

        // Send RouteWork
        let codebase_root = crate::workers::compute_codebase_merkle_root_pub();
        let route_work = AcpMessage::RouteWork {
            routing_id: spec.routing_id.clone(),
            capabilities: vec![spec.persona.clone()],
            payload: spec.payload.clone(),
            base_snapshot: "latest-from-blackboard".into(),
            codebase_merkle_root: codebase_root,
            // Per-persona capability list (SP2 Slice 3): implementers (benjamin/
            // lucas) get fs:write:worktree; read-only personas (harper/captain/
            // evaluator) get fs:read and analyze-only — they never mutate.
            permissions: crate::permissions::permissions_for(&spec.persona),
        };
        crate::acp::write_signed_acp_envelope(&mut stdin, signing_key, route_work).await?;

        // Send demo tool calls (ShellExec, TestRun, PatchApply)
        crate::workers::send_demo_tool_calls(&mut stdin, signing_key).await?;

        drop(stdin);

        // Emit Started event
        let _ = tx
            .send(WorkerEvent::started(
                spec.workspace_id.clone(),
                &spec.persona,
                &spec.routing_id,
            ))
            .await;

        // Stderr → WorkerEvent::Stderr in background
        let tx_err = tx.clone();
        tokio::spawn(async move {
            let mut line = String::new();
            while stderr.read_line(&mut line).await.unwrap_or(0) > 0 {
                if !line.trim().is_empty() {
                    let _ = tx_err
                        .send(WorkerEvent::Stderr {
                            line: line.trim().to_string(),
                            ts: Utc::now(),
                        })
                        .await;
                }
                line.clear();
            }
        });

        // Stdout (ACP envelopes) → WorkerEvent stream
        let workspace_id = spec.workspace_id.clone();
        let tx_out = tx.clone();
        let signing_key_bytes = signing_key.to_bytes();
        let active_pids_clone = self.active_pids.clone();
        let handle_id_clone = handle.id.clone();

        tokio::spawn(async move {
            let key = ed25519_dalek::SigningKey::from_bytes(&signing_key_bytes);
            let mut reader = stdout;

            loop {
                match crate::acp::read_acp_envelope(&mut reader).await {
                    Ok(envelope) => {
                        let env_clone = envelope.clone();
                        let verified = tokio::task::spawn_blocking(move || {
                            crate::acp::verify_envelope(&env_clone).unwrap_or(false)
                        })
                        .await
                        .unwrap_or(false);

                        // Yield verified structured AcpMsg event
                        let _ = tx_out
                            .send(WorkerEvent::AcpMsg {
                                message: envelope.payload.clone(),
                                verified,
                            })
                            .await;

                        match &envelope.payload {
                            AcpMessage::TerminationReport { exit_status, .. } => {
                                let code = if exit_status == "success" { 0 } else { 1 };
                                let _ = tx_out.send(WorkerEvent::completed(code)).await;
                                break;
                            }
                            AcpMessage::ShellExecRequest(_)
                            | AcpMessage::TestRunRequest(_)
                            | AcpMessage::PatchApplyRequest(_) => {
                                let event = WorkerEvent::ToolCall(ToolInvocation {
                                    tool_name: format!("{:?}", envelope.payload)
                                        .split('(')
                                        .next()
                                        .unwrap_or("tool")
                                        .to_string(),
                                    routing_id: envelope.message_id.to_string(),
                                    args: serde_json::Value::Null,
                                    invoked_at: Utc::now(),
                                });
                                let _ = tx_out.send(event).await;
                            }
                            _ => {
                                // Other ACP messages forwarded as opaque stdout
                                if let Ok(line) = serde_json::to_string(&envelope.payload) {
                                    let _ = tx_out
                                        .send(WorkerEvent::Stdout {
                                            line,
                                            ts: Utc::now(),
                                        })
                                        .await;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // EOF — worker exited without sending TerminationReport
                        let _ = tx_out.send(WorkerEvent::completed(-1)).await;
                        break;
                    }
                }
            }

            // Reap child
            let _ = child.wait().await;
            {
                let mut active = active_pids_clone.lock().unwrap();
                active.remove(&handle_id_clone);
            }
            drop(key); // suppress unused warning
        });

        Ok((handle, rx))
    }

    async fn terminate(&self, handle: &SessionHandle) -> Result<()> {
        let pid = {
            let active = self.active_pids.lock().unwrap();
            active.get(&handle.id).cloned()
        };

        if let Some(p) = pid {
            tracing::warn!(
                handle_id = %handle.id,
                pid = p,
                "subprocess_backend_terminating_process_group"
            );

            #[cfg(unix)]
            {
                let pgid = -(p as libc::pid_t);
                unsafe {
                    let res = libc::kill(pgid, libc::SIGKILL);
                    if res == -1 {
                        let err = std::io::Error::last_os_error();
                        tracing::debug!(
                            handle_id = %handle.id,
                            error = %err,
                            "libc_kill_failed_already_terminated"
                        );
                    } else {
                        tracing::info!(
                            handle_id = %handle.id,
                            pgid,
                            "libc_kill_process_group_succeeded"
                        );
                    }
                }
            }

            #[cfg(not(unix))]
            {
                tracing::warn!("non_unix_fallback_terminate_called");
            }
        } else {
            tracing::warn!(handle_id = %handle.id, "session_terminate_called_no_active_pid");
        }

        Ok(())
    }
}

// =========================================================================
// DockerBackend
// =========================================================================

/// Spawns `korg worker` inside an isolated Docker container.
#[derive(Debug)]
pub struct DockerBackend {
    pub image: String,
    pub inner: SubprocessBackend,
}

impl DockerBackend {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            inner: SubprocessBackend::new(),
        }
    }
}

#[async_trait]
impl SessionBackend for DockerBackend {
    fn backend_kind(&self) -> &'static str {
        "docker"
    }

    async fn spawn(
        &self,
        spec: &SessionSpec,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Result<(SessionHandle, EventStream)> {
        // TODO: Full docker backend — for now delegates to SubprocessBackend
        // since the container image must be pre-built.
        tracing::warn!(
            image = %self.image,
            "docker_backend_falling_back_to_subprocess"
        );
        self.inner.spawn(spec, signing_key).await
    }

    async fn terminate(&self, handle: &SessionHandle) -> Result<()> {
        self.inner.terminate(handle).await
    }
}

// =========================================================================
// Backend factory
// =========================================================================

/// Build the appropriate session backend based on the Korg config.
pub fn build_backend() -> std::sync::Arc<dyn SessionBackend> {
    let config = korg_llm::KorgConfig::load();
    if config.sandbox_mode == "docker" {
        let image = std::env::var("KORG_DOCKER_IMAGE").unwrap_or_else(|_| "korg:latest".into());
        std::sync::Arc::new(DockerBackend::new(image))
    } else {
        std::sync::Arc::new(SubprocessBackend::new())
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceId;

    #[test]
    fn worker_event_started_is_not_terminal() {
        let ev = WorkerEvent::started(WorkspaceId::new(), "captain", "r-001");
        assert!(!ev.is_terminal());
    }

    #[test]
    fn worker_event_completed_is_terminal() {
        let ev = WorkerEvent::completed(0);
        assert!(ev.is_terminal());
    }

    #[test]
    fn worker_event_failed_is_terminal() {
        let ev = WorkerEvent::failed("some reason");
        assert!(ev.is_terminal());
    }

    #[test]
    fn worker_event_heartbeat_is_not_terminal() {
        let ev = WorkerEvent::heartbeat(60);
        assert!(!ev.is_terminal());
    }

    #[test]
    fn session_handle_id_is_unique() {
        let ws = WorkspaceId::new();
        let h1 = SessionHandle::new(ws.clone(), "r1");
        let h2 = SessionHandle::new(ws, "r1");
        assert_ne!(h1.id, h2.id);
    }

    #[test]
    fn worker_cargo_env_is_empty_when_not_speculative() {
        // Default (non-speculative) path: no CARGO_TARGET_DIR override, so workers
        // use their own target dir — unchanged behavior.
        assert!(worker_cargo_env("session-1", false).is_empty());
    }

    #[test]
    fn worker_cargo_env_points_at_warm_target_dir_when_speculative() {
        // The anti-theater link: speculative workers must set CARGO_TARGET_DIR to
        // exactly the path the warm boot populated, so cargo_check reuses the cache.
        let session = "session-xyz";
        let env = worker_cargo_env(session, true);
        assert_eq!(env.len(), 1, "exactly the CARGO_TARGET_DIR pair");
        let (k, v) = &env[0];
        assert_eq!(k, "CARGO_TARGET_DIR");
        assert_eq!(
            v,
            &crate::execution::warm_target_dir(session)
                .to_string_lossy()
                .into_owned(),
            "worker must reuse the SAME shared cache the warm boot derived"
        );
    }

    #[test]
    fn subprocess_backend_kind() {
        assert_eq!(SubprocessBackend::new().backend_kind(), "subprocess");
    }

    #[test]
    fn docker_backend_kind() {
        let b = DockerBackend::new("korg:latest");
        assert_eq!(b.backend_kind(), "docker");
    }

    #[test]
    fn worker_event_serializes_to_json() {
        let ev = WorkerEvent::completed(0);
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("completed"));
        assert!(json.contains("exit_code"));
    }
}
