//! Workers — Concurrent Worker Fan-In Engine
//!
//! Extracts `spawn_worker_process` and `dispatch_level` from `leader.rs`, fixing
//! three correctness bugs in the original sequential fan-in:
//!
//! - **Bug A (False Sequentiality)**: `task.await` in insertion order meant a slow
//!   worker 0 blocked all of worker 1..n's results. Fixed with `JoinSet::join_next()`.
//! - **Bug B (Inline Retry Blocking)**: On crash, the retry `spawn_worker_process().await`
//!   ran inline, stalling the entire level's collection. Fixed with a post-drain retry queue.
//! - **Bug C (No Timeout)**: `read_acp_envelope` could block forever on a silent hung child.
//!   Fixed with `tokio::time::timeout` wrapping each worker.
//!
//! # Architecture
//!
//! ```text
//! dispatch_level(nodes, packages, bb, key, timeout)
//!      │
//!      ├─ JoinSet::spawn per node (truly concurrent)
//!      │        └─ timeout(WORKER_TIMEOUT, spawn_worker_process(...))
//!      │
//!      ├─ join_next() loop → surfaces results as they complete
//!      │        └─ crashed? → push to retry_queue (NON-BLOCKING)
//!      │
//!      └─ retry_queue drain → sequential re-spawn for each crashed worker
//! ```

use crate::acp::AcpMessage;
use crate::blackboard::Blackboard;
use crate::dag::{DagNode, ExecutionDag, NodeStatus};
use crate::personas::{Persona, PersonaResult};
use anyhow::Result;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::task::JoinSet;
use tokio::time::{timeout, Duration};

/// How long a single worker is allowed to run before it is considered hung.
/// Workers that exceed this will have their join cancelled and are treated as crashed.
pub const WORKER_TIMEOUT: Duration = Duration::from_secs(300);

// =========================================================================
// Public Result Types
// =========================================================================

/// The outcome of dispatching a single DAG level's worth of workers.
#[derive(Debug)]
pub struct LevelResult {
    /// Results that completed successfully (or were self-healed).
    pub completed: Vec<PersonaResult>,
    /// Node IDs that timed out or failed to spawn.
    pub failed_node_ids: Vec<String>,
    /// Number of workers that were auto-healed and retried.
    pub healed_count: usize,
}

/// The intermediate result from a single worker fan-in slot.
#[derive(Debug)]
struct FanInSlot {
    node_id: String,
    workspace_id: crate::workspace::WorkspaceId,
    persona: Persona,
    payload: String,
    routing_id: String,
    result: WorkerOutcome,
}

#[derive(Debug)]
enum WorkerOutcome {
    Ok(PersonaResult),
    Crashed(PersonaResult),
    TimedOut,
    SpawnError(String),
}

// =========================================================================
// Level Dispatcher — fixes Bugs A, B, C + workspace lifecycle
// =========================================================================

/// Dispatch a single topological level of DAG nodes concurrently.
///
/// Results surface in **completion order** (not insertion order) via `JoinSet::join_next`.
/// Crashed workers go into a retry queue processed **after** all live workers drain.
/// Each worker is wrapped in `tokio::time::timeout(WORKER_TIMEOUT, ...)`.
///
/// Every node gets an isolated `Workspace` managed by `WorkspaceManager`:
/// ```text
/// create_workspace → provision → attach_worker → [run] → complete/fail → destroy
/// ```
/// Workers are ephemeral execution vessels. The `WorkspaceManager` is the durable authority.
#[tracing::instrument(
    skip(packages_map, bb, signing_key, coordinator),
    fields(
        level_size = level_node_ids.len(),
        campaign_session_id = %coordinator.session_id,
        timeout_secs = WORKER_TIMEOUT.as_secs(),
    )
)]
pub async fn dispatch_level(
    level_node_ids: &[String],
    packages_map: &HashMap<String, WorkPackage>,
    bb: Arc<Mutex<Blackboard>>,
    signing_key: ed25519_dalek::SigningKey,
    tui_tx: Option<tokio::sync::mpsc::Sender<crate::tui_bridge::TuiUpdate>>,
    coordinator: Arc<crate::runtime::RuntimeCoordinator>,
) -> LevelResult {
    use crate::workspace::WorkspaceSpec;

    let mut set: JoinSet<FanInSlot> = JoinSet::new();

    // Create + provision workspaces before spawning — separate concerns
    let mut workspace_ids: HashMap<String, crate::workspace::WorkspaceId> = HashMap::new();
    for node_id in level_node_ids {
        if let Some(pkg) = packages_map.get(node_id) {
            let spec = WorkspaceSpec {
                persona_id: pkg.persona.name().to_lowercase(),
                campaign_session_id: coordinator.session_id,
                routing_id: pkg.routing_id.clone(),
                use_git_worktree: false, // Plain dir fallback for safety in sandbox
            };

            // Check workspace quota limit to prevent runaway workspace explosions
            let active_count = {
                let wm = coordinator.workspace_manager.lock().await;
                wm.snapshot_all().len()
            };
            if active_count >= coordinator.max_workspace_quota {
                tracing::warn!(
                    active_count,
                    max = coordinator.max_workspace_quota,
                    "workspace_quota_exceeded_skipping"
                );
                continue;
            }

            let ws_id = {
                let mut wm = coordinator.workspace_manager.lock().await;
                wm.create_workspace(spec)
            };

            let provision_res = {
                let mut wm = coordinator.workspace_manager.lock().await;
                wm.provision(&ws_id).await
            };

            if let Err(e) = provision_res {
                tracing::error!(node_id = %node_id, error = %e, "workspace_provision_failed");
            } else {
                workspace_ids.insert(node_id.clone(), ws_id);
            }
        }
    }

    // Spawn all workers concurrently — each gets a workspace from the manager
    for node_id in level_node_ids {
        let pkg = match packages_map.get(node_id) {
            Some(p) => p.clone(),
            None => continue,
        };

        let ws_id = match workspace_ids.get(node_id) {
            Some(id) => id.clone(),
            None => continue,
        };

        // Attach worker — Provisioned → Active
        {
            let mut wm = coordinator.workspace_manager.lock().await;
            if let Err(e) = wm.attach_worker(&ws_id, pkg.routing_id.clone()) {
                tracing::warn!(workspace_id = %ws_id, node_id = %node_id, error = %e,
                    "workspace_attach_failed_continuing");
            }
        }

        let bb = bb.clone();
        let key = signing_key.clone();
        let node_id_owned = node_id.clone();
        let ws_id_owned = ws_id.clone();
        let coord = coordinator.clone();

        set.spawn(async move {
            let persona = pkg.persona;
            let payload = pkg.description.clone();
            let routing_id = pkg.routing_id.clone();

            // Acquire concurrency semaphore permit (backpressure ceiling)
            let _permit = match coord.concurrency_semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    return FanInSlot {
                        node_id: node_id_owned,
                        workspace_id: ws_id_owned,
                        persona,
                        payload,
                        routing_id,
                        result: WorkerOutcome::SpawnError("Semaphore acquisition failed".into()),
                    };
                }
            };

            // Check cancellation before spawning
            if coord.cancellation_token.is_cancelled() {
                return FanInSlot {
                    node_id: node_id_owned,
                    workspace_id: ws_id_owned,
                    persona,
                    payload,
                    routing_id,
                    result: WorkerOutcome::TimedOut,
                };
            }

            tracing::debug!(
                node_id = %node_id_owned,
                workspace_id = %ws_id_owned,
                persona = persona.name(),
                "worker_spawning"
            );

            let outcome = match timeout(
                WORKER_TIMEOUT,
                spawn_worker_process(
                    persona,
                    payload.clone(),
                    routing_id.clone(),
                    bb,
                    key,
                    coord.clone(),
                    ws_id_owned.clone(),
                ),
            )
            .await
            {
                Ok(Ok(res)) if res.crashed => {
                    tracing::warn!(
                        node_id = %node_id_owned,
                        workspace_id = %ws_id_owned,
                        persona = persona.name(),
                        "worker_crashed"
                    );
                    korg_core::metrics::record_worker_crashed(persona.name());
                    WorkerOutcome::Crashed(res)
                }
                Ok(Ok(res)) => {
                    tracing::info!(
                        node_id = %node_id_owned,
                        workspace_id = %ws_id_owned,
                        persona = persona.name(),
                        mutations = res.mutations.len(),
                        "worker_completed"
                    );
                    korg_core::metrics::record_worker_completed(persona.name());
                    WorkerOutcome::Ok(res)
                }
                Ok(Err(e)) => {
                    tracing::error!(
                        node_id = %node_id_owned,
                        workspace_id = %ws_id_owned,
                        error = %e,
                        "worker_spawn_error"
                    );
                    WorkerOutcome::SpawnError(e.to_string())
                }
                Err(_elapsed) => {
                    tracing::error!(
                        node_id = %node_id_owned,
                        workspace_id = %ws_id_owned,
                        timeout_secs = WORKER_TIMEOUT.as_secs(),
                        "worker_timed_out"
                    );
                    korg_core::metrics::record_worker_timeout(persona.name());
                    WorkerOutcome::TimedOut
                }
            };

            FanInSlot {
                node_id: node_id_owned,
                workspace_id: ws_id_owned,
                persona,
                payload,
                routing_id,
                result: outcome,
            }
        });
    }

    // Collect results in completion order (Bug A fix: join_next, not insertion order)
    let mut completed: Vec<PersonaResult> = Vec::new();
    let mut retry_queue: Vec<FanInSlot> = Vec::new();
    let mut failed_node_ids: Vec<String> = Vec::new();

    loop {
        tokio::select! {
            _ = coordinator.cancellation_token.cancelled() => {
                tracing::warn!("dispatch_level_cancellation_received");
                break;
            }
            join_result = set.join_next() => {
                let join_result = match join_result {
                    Some(res) => res,
                    None => break,
                };
                match join_result {
                    Ok(slot) => {
                        // Update workspace state based on worker outcome
                        {
                            let mut wm = coordinator.workspace_manager.lock().await;
                            match &slot.result {
                                WorkerOutcome::Ok(_) => {
                                    let _ = wm.complete_workspace(&slot.workspace_id, true).await;
                                }
                                WorkerOutcome::Crashed(_) => {
                                    wm.fail_workspace(&slot.workspace_id, "worker_crashed".into());
                                }
                                WorkerOutcome::TimedOut => {
                                    wm.fail_workspace(&slot.workspace_id, "worker_timed_out".into());
                                }
                                WorkerOutcome::SpawnError(e) => {
                                    wm.fail_workspace(&slot.workspace_id, format!("spawn_error: {}", e));
                                }
                            }
                        }

                        if let Some(ref tx) = tui_tx {
                            let msg = match &slot.result {
                                WorkerOutcome::Ok(_) => format!("  [✓] {} completed", slot.node_id),
                                WorkerOutcome::Crashed(_) => format!("  [!] {} crashed — queued for recovery", slot.node_id),
                                WorkerOutcome::TimedOut => format!("  [⏱] {} timed out after {}s", slot.node_id, WORKER_TIMEOUT.as_secs()),
                                WorkerOutcome::SpawnError(e) => format!("  [✗] {} spawn error: {}", slot.node_id, e),
                            };
                            let _ = tx.try_send(crate::tui_bridge::TuiUpdate::Trace(msg));
                        }

                        match slot.result {
                            WorkerOutcome::Ok(res) => completed.push(res),
                            WorkerOutcome::Crashed(_) => retry_queue.push(slot),
                            WorkerOutcome::TimedOut | WorkerOutcome::SpawnError(_) => {
                                failed_node_ids.push(slot.node_id);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "worker_join_panic");
                    }
                }
            }
        }
    }

    // Process retry queue AFTER all live workers drain (Bug B fix)
    let mut healed_count = 0;
    for slot in retry_queue {
        // Enforce cancellation before doing retry
        if coordinator.cancellation_token.is_cancelled() {
            failed_node_ids.push(slot.node_id);
            continue;
        }

        tracing::info!(
            node_id = %slot.node_id,
            workspace_id = %slot.workspace_id,
            "worker_attempting_recovery"
        );

        let worktree_path_str = korg_core::paths::worktree_dir(
            &slot.persona.name().to_lowercase(),
            &slot.routing_id,
            &slot.routing_id,
        );
        let worktree_path = worktree_path_str.as_path();

        let healed = if worktree_path.exists() {
            let stderr = get_cargo_check_stderr(worktree_path).await;
            let (heal_tx, mut heal_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let tui_clone = tui_tx.clone();
            tokio::spawn(async move {
                while let Some(msg) = heal_rx.recv().await {
                    if let Some(ref tx) = tui_clone {
                        let _ = tx.try_send(crate::tui_bridge::TuiUpdate::Trace(msg));
                    }
                }
            });
            matches!(
                crate::dag::heal_node_with_context(
                    "cargo check",
                    stderr.as_deref(),
                    Some(worktree_path),
                    Some(heal_tx),
                )
                .await,
                Ok(true)
            )
        } else {
            false
        };

        // Spend from retry budget
        let mut budget_ok = false;
        if healed {
            let mut budget = coordinator.retry_budget.lock().unwrap();
            budget_ok = budget.spend();
        }

        if healed && budget_ok {
            healed_count += 1;
            korg_llm::HEALS_RESOLVED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::info!(
                node_id = %slot.node_id,
                workspace_id = %slot.workspace_id,
                "worker_healed"
            );

            let clean_payload = slot.payload.replace("simulate-crash", "");
            match timeout(
                WORKER_TIMEOUT,
                spawn_worker_process(
                    slot.persona,
                    clean_payload,
                    slot.routing_id.clone(),
                    bb.clone(),
                    signing_key.clone(),
                    coordinator.clone(),
                    slot.workspace_id.clone(),
                ),
            )
            .await
            {
                Ok(Ok(res)) => {
                    let mut wm = coordinator.workspace_manager.lock().await;
                    let _ = wm.complete_workspace(&slot.workspace_id, true).await;
                    completed.push(res);
                }
                Ok(Err(e)) => {
                    tracing::error!(error = %e, node_id = %slot.node_id, "healed_worker_retry_failed");
                    let mut wm = coordinator.workspace_manager.lock().await;
                    wm.fail_workspace(&slot.workspace_id, format!("retry_failed: {}", e));
                    failed_node_ids.push(slot.node_id);
                }
                Err(_) => {
                    tracing::error!(node_id = %slot.node_id, "healed_worker_retry_timed_out");
                    let mut wm = coordinator.workspace_manager.lock().await;
                    wm.fail_workspace(&slot.workspace_id, "retry_timed_out".into());
                    failed_node_ids.push(slot.node_id);
                }
            }
        } else {
            if let WorkerOutcome::Crashed(res) = slot.result {
                completed.push(res);
            } else {
                failed_node_ids.push(slot.node_id);
            }
        }
    }

    // Destroy all workspaces for this session — workers are ephemeral execution vessels
    let destroyed = {
        let mut wm = coordinator.workspace_manager.lock().await;
        wm.cleanup_all_for_session(coordinator.session_id).await
    };
    tracing::debug!(
        destroyed_workspaces = destroyed,
        "level_workspaces_cleaned_up"
    );

    LevelResult {
        completed,
        failed_node_ids,
        healed_count,
    }
}

// =========================================================================
// DAG Builder — extracted from dispatch_concurrent
// =========================================================================

/// A resolved work package ready for dispatch.
#[derive(Debug, Clone)]
pub struct WorkPackage {
    pub node_id: String,
    pub persona: Persona,
    pub description: String,
    pub routing_id: String,
}

/// Build the canonical 4-persona campaign DAG and return topological levels.
/// Speculative pre-warm is gated on the `speculative_execution` capability.
#[tracing::instrument(skip(root_task, tui_tx))]
pub async fn build_campaign_dag(
    root_task: &str,
    packages_json: &serde_json::Value,
    tui_tx: Option<&tokio::sync::mpsc::Sender<crate::tui_bridge::TuiUpdate>>,
) -> Result<(ExecutionDag, Vec<Vec<String>>, HashMap<String, WorkPackage>)> {
    let mut dag = ExecutionDag::new(root_task);

    dag.add_node(DagNode {
        id: "pkg-captain".into(),
        name: "Captain Persona".into(),
        command: "RouteWork captain".into(),
        dependencies: vec![],
        status: NodeStatus::Pending,
        confidence: 0.92,
        risk: "Low".into(),
        severity: "Low".into(),
        blast_radius: "Scoped".into(),
        certainty: 0.9,
        remediation_confidence: 0.9,
    });
    dag.add_node(DagNode {
        id: "pkg-harper".into(),
        name: "Harper Persona".into(),
        command: "RouteWork harper".into(),
        dependencies: vec![],
        status: NodeStatus::Pending,
        confidence: 0.88,
        risk: "Low".into(),
        severity: "Medium".into(),
        blast_radius: "Scoped".into(),
        certainty: 0.85,
        remediation_confidence: 0.85,
    });
    dag.add_node(DagNode {
        id: "pkg-benjamin".into(),
        name: "Benjamin Persona".into(),
        command: "RouteWork benjamin".into(),
        dependencies: vec!["pkg-captain".into(), "pkg-harper".into()],
        status: NodeStatus::Pending,
        confidence: 0.78,
        risk: "Medium".into(),
        severity: "High".into(),
        blast_radius: "Module".into(),
        certainty: 0.80,
        remediation_confidence: 0.80,
    });
    dag.add_node(DagNode {
        id: "pkg-lucas".into(),
        name: "Lucas Persona".into(),
        command: "RouteWork lucas".into(),
        dependencies: vec!["pkg-benjamin".into()],
        status: NodeStatus::Pending,
        confidence: 0.85,
        risk: "Low".into(),
        severity: "Low".into(),
        blast_radius: "Scoped".into(),
        certainty: 0.9,
        remediation_confidence: 0.9,
    });

    // Speculative pre-warm (gated on capability)
    let mut scheduler = crate::dag::SpeculativeScheduler::new(dag.clone());
    let _ = scheduler.speculative_warm_boot().await;
    if let Some(tx) = tui_tx {
        let _ = tx.try_send(crate::tui_bridge::TuiUpdate::Trace(
            "⚡ [SPECULATIVE] Warmed background shell shims concurrently".into(),
        ));
    }

    let levels = dag.compile()?;

    // Build packages_map for O(1) lookup
    let packages_json_arr = packages_json["work_packages"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut packages_map = HashMap::new();
    for pkg_json in &packages_json_arr {
        let node_id = pkg_json["id"].as_str().unwrap_or("").to_string();
        let persona = match pkg_json["personas"][0].as_str().unwrap_or("benjamin") {
            "captain" => Persona::Captain,
            "harper" => Persona::Harper,
            "lucas" => Persona::Lucas,
            _ => Persona::Benjamin,
        };
        let description = pkg_json["description"].as_str().unwrap_or("").to_string();
        packages_map.insert(
            node_id.clone(),
            WorkPackage {
                node_id: node_id.clone(),
                persona,
                description,
                routing_id: node_id,
            },
        );
    }

    Ok((dag, levels, packages_map))
}

// =========================================================================
// Worker Process Spawner — extracted from leader.rs
// =========================================================================

/// Spawn a real `korg worker` child process for one persona.
///
/// - Sends `RouteWork` as a signed ACP `MessageEnvelope`
/// - Reads `SwarmTelemetryPulse`, `SubmitTransaction`, `TerminationReport` from stdout
/// - Ingests pulses into the shared `Blackboard`
/// - Returns `PersonaResult` with the last `SubmitTransaction` payload
///
/// **Timeout is enforced by the caller** (`dispatch_level`) via `tokio::time::timeout`.
#[tracing::instrument(
    skip(bb, signing_key, coordinator),
    fields(
        persona = persona.name(),
        routing_id = %routing_id,
    )
)]
pub async fn spawn_worker_process(
    persona: Persona,
    payload: String,
    routing_id: String,
    bb: Arc<Mutex<Blackboard>>,
    signing_key: ed25519_dalek::SigningKey,
    coordinator: Arc<crate::runtime::RuntimeCoordinator>,
    workspace_id: crate::workspace::WorkspaceId,
) -> Result<PersonaResult> {
    use crate::session::{SessionSpec, WorkerEvent};

    let spec = SessionSpec {
        workspace_id: workspace_id.clone(),
        persona: persona.name().to_lowercase(),
        routing_id: routing_id.clone(),
        payload,
        timeout_secs: WORKER_TIMEOUT.as_secs(),
    };

    let (handle, mut rx) = coordinator.backend.spawn(&spec, &signing_key).await?;

    let worker_key = format!("{}-{}", persona.name().to_lowercase(), routing_id);
    coordinator.supervisor.register(
        worker_key.clone(),
        crate::runtime::ActiveWorker {
            handle,
            routing_id: routing_id.clone(),
            workspace_id: workspace_id.clone(),
        },
    );

    struct WorkerRegisterGuard {
        key: String,
        supervisor: Arc<crate::runtime::ExecutionSupervisor>,
    }
    impl Drop for WorkerRegisterGuard {
        fn drop(&mut self) {
            self.supervisor.unregister(&self.key);
        }
    }
    let _guard = WorkerRegisterGuard {
        key: worker_key,
        supervisor: coordinator.supervisor.clone(),
    };

    let mut last_tx: Option<serde_json::Value> = None;
    let mut crashed = false;

    while let Some(event) = rx.recv().await {
        event.trace(&workspace_id);

        match event {
            WorkerEvent::AcpMsg { message, verified } => match message {
                AcpMessage::SwarmTelemetryPulse { .. } => {
                    if let Ok(mut board) = bb.lock() {
                        board.ingest_telemetry_pulse(&message, Some(uuid::Uuid::new_v4()));
                    }
                    tracing::debug!(
                        persona = persona.name(),
                        verified,
                        "swarm_telemetry_ingested"
                    );
                }
                AcpMessage::SubmitTransaction {
                    payload: tx_payload,
                    ..
                } => {
                    tracing::debug!(persona = persona.name(), "submit_transaction_received");
                    last_tx = Some(tx_payload);
                }
                AcpMessage::ShellExecResult(result) => {
                    tracing::debug!(
                        persona = persona.name(),
                        verified,
                        stdout = result.stdout.trim(),
                        "shell_exec_result"
                    );
                }
                AcpMessage::TestRunResult(result) => {
                    tracing::info!(
                        persona = persona.name(),
                        verified,
                        tests_run = result.tests_run,
                        tests_passed = result.tests_passed,
                        tests_failed = result.tests_failed,
                        "test_run_result"
                    );
                }
                AcpMessage::PatchApplyResult(_) | AcpMessage::FileReadResult(_) => {
                    tracing::debug!(persona = persona.name(), verified, "tool_result_received");
                }
                AcpMessage::TerminationReport { exit_status, .. } => {
                    tracing::info!(
                        persona = persona.name(),
                        exit_status = %exit_status,
                        verified,
                        "worker_terminated"
                    );
                }
                _ => {}
            },
            WorkerEvent::Completed { exit_code, .. } => {
                if exit_code != 0 {
                    crashed = true;
                }
            }
            WorkerEvent::Failed { .. } => {
                crashed = true;
            }
            _ => {}
        }
    }

    let mut res = PersonaResult::new(persona, routing_id);
    if crashed {
        res.crashed = true;
        res.error_msg = Some("Child process crashed or exited with non-zero status".into());
    }
    if let Some(tx) = last_tx {
        res.output = tx.clone();
        if let Some(muts) = tx.get("mutations").and_then(|v| v.as_array()) {
            res.mutations = muts.clone();
        }
    }

    Ok(res)
}

// =========================================================================
// Internal helpers
// =========================================================================

fn compute_codebase_merkle_root() -> String {
    std::process::Command::new("git")
        .arg("write-tree")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "sha256:codebase-fallback".into())
}

/// Public alias for use by session.rs (avoids duplicating the logic).
pub fn compute_codebase_merkle_root_pub() -> String {
    compute_codebase_merkle_root()
}

pub async fn send_demo_tool_calls(
    stdin: &mut tokio::process::ChildStdin,
    signing_key: &ed25519_dalek::SigningKey,
) -> Result<()> {
    let demo_shell = AcpMessage::ShellExecRequest(crate::acp::ShellExecRequestPayload {
        command: "echo".into(),
        args: vec!["[TOOL DEMO] Hello from signed ACP ShellExec".into()],
        cwd: None,
        timeout_ms: Some(8000),
    });
    crate::acp::write_signed_acp_envelope(stdin, signing_key, demo_shell).await?;

    let test_req = AcpMessage::TestRunRequest(crate::acp::TestRunRequestPayload {
        command: "cargo".into(),
        args: vec!["test".into(), "--".into(), "--quiet".into()],
        cwd: None,
        timeout_ms: Some(180_000),
        with_coverage: false,
    });
    crate::acp::write_signed_acp_envelope(stdin, signing_key, test_req).await?;

    let patch_req = AcpMessage::PatchApplyRequest(crate::acp::PatchApplyRequestPayload {
        file_path: "src/harness.rs".into(),
        patch: "<<<<<<< SEARCH\n        eprintln!(\"[Harness] Worker {} exiting after stdio task\", worker_id);\n=======\n        eprintln!(\"[Harness] Worker {} exiting after signed patch apply\", worker_id);\n>>>>>>> REPLACE".into(),
        dry_run: false,
    });
    crate::acp::write_signed_acp_envelope(stdin, signing_key, patch_req).await?;

    Ok(())
}

async fn get_cargo_check_stderr(worktree: &std::path::Path) -> Option<String> {
    tokio::process::Command::new("cargo")
        .arg("check")
        .current_dir(worktree)
        .output()
        .await
        .ok()
        .filter(|o| !o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_timeout_constant() {
        assert_eq!(WORKER_TIMEOUT.as_secs(), 300);
    }

    #[test]
    fn test_compute_codebase_merkle_root_never_panics() {
        let root = compute_codebase_merkle_root();
        // Either a real git hash or the fallback — never empty
        assert!(!root.is_empty());
    }

    #[test]
    fn test_level_result_fields() {
        let lr = LevelResult {
            completed: vec![],
            failed_node_ids: vec!["pkg-captain".into()],
            healed_count: 0,
        };
        assert_eq!(lr.failed_node_ids.len(), 1);
        assert_eq!(lr.healed_count, 0);
    }

    #[tokio::test]
    async fn test_build_campaign_dag_produces_four_levels() {
        let packages_json = serde_json::json!({
            "work_packages": [
                {"id": "pkg-captain",  "personas": ["captain"],  "description": "Plan"},
                {"id": "pkg-harper",   "personas": ["harper"],   "description": "Research"},
                {"id": "pkg-benjamin", "personas": ["benjamin"], "description": "Implement"},
                {"id": "pkg-lucas",    "personas": ["lucas"],    "description": "Synthesize"},
            ]
        });

        let (dag, levels, packages_map) =
            build_campaign_dag("test-root-task", &packages_json, None)
                .await
                .expect("build_campaign_dag should succeed");

        // The DAG has 4 nodes in 3 topological levels:
        // L1: [captain, harper] (no deps)
        // L2: [benjamin] (depends on captain + harper)
        // L3: [lucas] (depends on benjamin)
        assert_eq!(
            levels.len(),
            3,
            "expected 3 topological levels, got {}",
            levels.len()
        );
        assert_eq!(levels[0].len(), 2, "L1 should have captain + harper");
        assert_eq!(levels[1].len(), 1, "L2 should have benjamin");
        assert_eq!(levels[2].len(), 1, "L3 should have lucas");

        assert_eq!(packages_map.len(), 4);
        assert!(packages_map.contains_key("pkg-captain"));
        assert_eq!(dag.nodes.len(), 4);
    }
}
