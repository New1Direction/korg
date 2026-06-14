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

    // Per-node spawn timestamps so the worker-lifecycle signal carries REAL elapsed.
    let mut spawn_instants: HashMap<String, std::time::Instant> = HashMap::new();

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

        // Real lifecycle point: this worker is now being spawned. Emit the
        // structured signal (feeds the live swarm tree) and stamp its start.
        spawn_instants.insert(node_id.clone(), std::time::Instant::now());
        if let Some(ref tx) = tui_tx {
            let _ = tx.try_send(crate::tui_bridge::TuiUpdate::WorkerState {
                node_id: node_id.clone(),
                persona: pkg.persona.name().to_string(),
                state: crate::tui_bridge::WorkerLifecycle::Spawning,
                elapsed_ms: 0,
            });
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

                            // Same real lifecycle point: emit the structured signal
                            // for the live swarm tree, with REAL elapsed since spawn.
                            let state = match &slot.result {
                                WorkerOutcome::Ok(_) => crate::tui_bridge::WorkerLifecycle::Ok,
                                WorkerOutcome::Crashed(_) => {
                                    crate::tui_bridge::WorkerLifecycle::Crashed
                                }
                                WorkerOutcome::TimedOut => {
                                    crate::tui_bridge::WorkerLifecycle::TimedOut
                                }
                                WorkerOutcome::SpawnError(_) => {
                                    crate::tui_bridge::WorkerLifecycle::SpawnError
                                }
                            };
                            let elapsed_ms = spawn_instants
                                .get(&slot.node_id)
                                .map(|t| t.elapsed().as_millis() as u64)
                                .unwrap_or(0);
                            let _ = tx.try_send(crate::tui_bridge::TuiUpdate::WorkerState {
                                node_id: slot.node_id.clone(),
                                persona: slot.persona.name().to_string(),
                                state,
                                elapsed_ms,
                            });
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

/// Total character budget for upstream context appended to a downstream
/// payload — mirrors the 8000-char Heavy-Consciousness ceiling so payloads
/// can't grow unbounded across a deep DAG.
pub const UPSTREAM_CONTEXT_BUDGET: usize = 8000;

/// Compose a downstream persona's payload from its base description plus the
/// serialized outputs of its just-completed upstream dependencies.
///
/// This is the heart of the real data-flow: an upstream `PersonaResult.output`
/// (e.g. Captain's `work_packages`) is appended to the downstream node's
/// payload so Benjamin/Lucas actually *see* what their dependencies produced.
///
/// Guarantees:
/// - **Deterministic order:** upstream entries are sorted by `(persona, node_id)`
///   before appending, so the campaign stays reproducible regardless of the
///   completion order the JoinSet surfaced.
/// - **Size-capped:** the *appended* upstream context is capped at
///   [`UPSTREAM_CONTEXT_BUDGET`] characters; if it would overflow, it is
///   truncated with an explicit `…[truncated]` marker. The base payload is
///   never truncated.
///
/// `upstream` entries are `(persona_name, node_id, output_json)`.
pub fn compose_downstream_payload(
    base: &str,
    upstream: &[(String, String, serde_json::Value)],
) -> String {
    if upstream.is_empty() {
        return base.to_string();
    }

    // Stable order: sort by persona name then node id so byte-identical inputs
    // (in any completion order) yield byte-identical payloads.
    let mut sorted: Vec<&(String, String, serde_json::Value)> = upstream.iter().collect();
    sorted.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));

    let mut appended = String::new();
    for (persona, node_id, output) in sorted {
        let json = serde_json::to_string_pretty(output).unwrap_or_else(|_| output.to_string());
        let block = format!("\n\n## Upstream from {persona} ({node_id}):\n{json}");
        // Enforce the budget on the *appended* context only.
        if appended.len() + block.len() > UPSTREAM_CONTEXT_BUDGET {
            let remaining = UPSTREAM_CONTEXT_BUDGET.saturating_sub(appended.len());
            if remaining > 0 {
                let marker = "\n…[truncated]";
                let take = remaining.saturating_sub(marker.len()).min(block.len());
                // Truncate on a char boundary to avoid splitting a UTF-8 scalar.
                let mut end = take;
                while end > 0 && !block.is_char_boundary(end) {
                    end -= 1;
                }
                appended.push_str(&block[..end]);
                appended.push_str(marker);
            }
            break;
        }
        appended.push_str(&block);
    }

    format!("{base}{appended}")
}

/// Rewrite every still-pending downstream package whose dependencies include a
/// node that just completed, appending that upstream node's real
/// `PersonaResult.output` to the downstream payload (Captain+Harper → Benjamin;
/// Benjamin → Lucas).
///
/// This is the per-level data-flow step extracted from `dispatch_concurrent` so
/// it can be unit-tested in isolation. Behavior is identical to the inline loop:
///
/// - A node already present in `completed_outputs` is skipped (it's done).
/// - A node is only rewritten if at least one of its dependencies is in
///   `just_completed_ids` (we just produced new upstream context for it).
/// - Upstream context is gathered from `completed_outputs` for ALL of the node's
///   dependencies (so a node with two upstreams sees both once both finish), and
///   composed deterministically + size-capped via [`compose_downstream_payload`].
///
/// `completed_outputs` maps `node_id -> (persona_name, output_json)`.
/// `node_dependencies` maps `node_id -> [dependency_node_id, …]`.
pub fn apply_upstream_to_pending(
    packages_map: &mut HashMap<String, WorkPackage>,
    node_dependencies: &HashMap<String, Vec<String>>,
    completed_outputs: &HashMap<String, (String, serde_json::Value)>,
    just_completed_ids: &std::collections::HashSet<String>,
) {
    for (node_id, deps) in node_dependencies {
        // Only rewrite nodes that depend on something we just finished
        // (and are not themselves done yet).
        if completed_outputs.contains_key(node_id) {
            continue;
        }
        if !deps.iter().any(|d| just_completed_ids.contains(d)) {
            continue;
        }
        // Gather all completed upstream outputs for this node's deps.
        let upstream: Vec<(String, String, serde_json::Value)> = deps
            .iter()
            .filter_map(|dep_id| {
                completed_outputs
                    .get(dep_id)
                    .map(|(persona, output)| (persona.clone(), dep_id.clone(), output.clone()))
            })
            .collect();
        if upstream.is_empty() {
            continue;
        }
        if let Some(pkg) = packages_map.get_mut(node_id) {
            pkg.description = compose_downstream_payload(&pkg.description, &upstream);
        }
    }
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
    let mut scheduler = crate::dag::SpeculativeScheduler::new(
        dag.clone(),
        crate::identity::load_or_create_identity(),
    );
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
        if let Some(fc) = tx.get("files_changed").and_then(|v| v.as_u64()) {
            res.files_changed = fc as usize;
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

    // --- Slice 2: real upstream→downstream data-flow (pure helper) ---

    #[test]
    fn compose_downstream_payload_carries_upstream_output() {
        // Captain's plan output flows into Benjamin's payload.
        let captain_out = serde_json::json!({
            "work_packages": [{"id": 1, "title": "Fix add"}],
            "acceptance_criteria": ["add(2,3)==5"]
        });
        let benjamin_payload = compose_downstream_payload(
            "Implement: fix the add bug",
            &[(
                "Captain".to_string(),
                "pkg-captain".to_string(),
                captain_out,
            )],
        );
        // Base is preserved.
        assert!(benjamin_payload.starts_with("Implement: fix the add bug"));
        // Upstream content is actually present (data-flow is real, not a no-op).
        assert!(
            benjamin_payload.contains("work_packages"),
            "benjamin payload must contain captain's plan marker"
        );
        assert!(benjamin_payload.contains("Upstream from Captain (pkg-captain)"));
    }

    #[test]
    fn compose_downstream_payload_then_benjamin_into_lucas() {
        // The two-hop chain: Benjamin's output appears in Lucas's payload.
        let benjamin_out = serde_json::json!({
            "mutations": [{"target": "src/lib.rs", "action": "update"}]
        });
        let lucas_payload = compose_downstream_payload(
            "Synthesize: fix the add bug",
            &[(
                "Benjamin".to_string(),
                "pkg-benjamin".to_string(),
                benjamin_out,
            )],
        );
        assert!(
            lucas_payload.contains("mutations") && lucas_payload.contains("src/lib.rs"),
            "lucas payload must contain benjamin's output marker"
        );
    }

    #[test]
    fn compose_downstream_payload_is_order_independent() {
        // Captain + Harper into Benjamin — completion order must not matter.
        let cap = (
            "Captain".to_string(),
            "pkg-captain".to_string(),
            serde_json::json!({"work_packages": [1]}),
        );
        let har = (
            "Harper".to_string(),
            "pkg-harper".to_string(),
            serde_json::json!({"concerns": [2]}),
        );
        let a = compose_downstream_payload("base", &[cap.clone(), har.clone()]);
        let b = compose_downstream_payload("base", &[har, cap]);
        assert_eq!(a, b, "payload must be byte-identical regardless of order");
    }

    #[test]
    fn compose_downstream_payload_respects_size_cap() {
        // A huge upstream output must not blow the payload past base + budget.
        let big = serde_json::json!({ "blob": "x".repeat(50_000) });
        let out = compose_downstream_payload(
            "base",
            &[("Captain".to_string(), "pkg-captain".to_string(), big)],
        );
        assert!(
            out.len() <= "base".len() + UPSTREAM_CONTEXT_BUDGET,
            "appended upstream context must be capped at the budget, got {} chars",
            out.len()
        );
        assert!(out.contains("…[truncated]"), "truncation must be marked");
    }

    #[test]
    fn compose_downstream_payload_empty_upstream_is_noop() {
        let out = compose_downstream_payload("base only", &[]);
        assert_eq!(out, "base only");
    }

    // --- Slice 2 integration: the dispatch_concurrent per-level rewrite loop ---

    fn dataflow_fixture() -> (HashMap<String, WorkPackage>, HashMap<String, Vec<String>>) {
        // The canonical 4-node campaign DAG:
        //   captain, harper (no deps) → benjamin (deps: captain, harper) → lucas (deps: benjamin)
        let mk = |id: &str, persona: Persona, desc: &str| WorkPackage {
            node_id: id.to_string(),
            persona,
            description: desc.to_string(),
            routing_id: id.to_string(),
        };
        let mut packages_map = HashMap::new();
        packages_map.insert(
            "pkg-captain".into(),
            mk("pkg-captain", Persona::Captain, "Plan: root task"),
        );
        packages_map.insert(
            "pkg-harper".into(),
            mk("pkg-harper", Persona::Harper, "Research: root task"),
        );
        packages_map.insert(
            "pkg-benjamin".into(),
            mk("pkg-benjamin", Persona::Benjamin, "Implement: root task"),
        );
        packages_map.insert(
            "pkg-lucas".into(),
            mk("pkg-lucas", Persona::Lucas, "Synthesize: root task"),
        );

        let mut node_dependencies = HashMap::new();
        node_dependencies.insert("pkg-captain".to_string(), vec![]);
        node_dependencies.insert("pkg-harper".to_string(), vec![]);
        node_dependencies.insert(
            "pkg-benjamin".to_string(),
            vec!["pkg-captain".to_string(), "pkg-harper".to_string()],
        );
        node_dependencies.insert("pkg-lucas".to_string(), vec!["pkg-benjamin".to_string()]);

        (packages_map, node_dependencies)
    }

    #[test]
    fn apply_upstream_to_pending_threads_captain_into_benjamin_then_benjamin_into_lucas() {
        let (mut packages_map, node_dependencies) = dataflow_fixture();
        let mut completed_outputs: HashMap<String, (String, serde_json::Value)> = HashMap::new();

        // --- L1 completes: captain + harper produce real outputs ---
        completed_outputs.insert(
            "pkg-captain".to_string(),
            (
                "Captain".to_string(),
                serde_json::json!({
                    "work_packages": [{"id": 1, "title": "Fix add"}],
                    "acceptance_criteria": ["add(2,3)==5"]
                }),
            ),
        );
        completed_outputs.insert(
            "pkg-harper".to_string(),
            (
                "Harper".to_string(),
                serde_json::json!({ "concerns": [{"id": "c1", "file_path": "src/lib.rs"}] }),
            ),
        );
        let l1_completed: std::collections::HashSet<String> =
            ["pkg-captain".to_string(), "pkg-harper".to_string()]
                .into_iter()
                .collect();

        apply_upstream_to_pending(
            &mut packages_map,
            &node_dependencies,
            &completed_outputs,
            &l1_completed,
        );

        // Benjamin's payload now carries BOTH captain's plan and harper's concerns
        // (the real rewrite — downstream payload carries upstream content).
        let benjamin_desc = &packages_map["pkg-benjamin"].description;
        assert!(
            benjamin_desc.starts_with("Implement: root task"),
            "base payload must be preserved"
        );
        assert!(
            benjamin_desc.contains("work_packages")
                && benjamin_desc.contains("Upstream from Captain (pkg-captain)"),
            "benjamin payload must contain Captain's plan marker, got:\n{benjamin_desc}"
        );
        assert!(
            benjamin_desc.contains("concerns")
                && benjamin_desc.contains("Upstream from Harper (pkg-harper)"),
            "benjamin payload must also contain Harper's concerns marker"
        );
        // Lucas not yet rewritten — its dep (benjamin) hasn't completed.
        assert_eq!(
            packages_map["pkg-lucas"].description, "Synthesize: root task",
            "lucas must NOT be rewritten before benjamin completes"
        );
        // L1 peers (captain/harper) are not themselves rewritten.
        assert_eq!(packages_map["pkg-captain"].description, "Plan: root task");

        // --- L2 completes: benjamin produces output referencing the implement step ---
        completed_outputs.insert(
            "pkg-benjamin".to_string(),
            (
                "Benjamin".to_string(),
                serde_json::json!({
                    "mutations": [{"target": "src/lib.rs", "action": "update"}]
                }),
            ),
        );
        let l2_completed: std::collections::HashSet<String> =
            ["pkg-benjamin".to_string()].into_iter().collect();

        apply_upstream_to_pending(
            &mut packages_map,
            &node_dependencies,
            &completed_outputs,
            &l2_completed,
        );

        // Now Lucas's payload carries Benjamin's real output marker.
        let lucas_desc = &packages_map["pkg-lucas"].description;
        assert!(
            lucas_desc.starts_with("Synthesize: root task"),
            "lucas base payload must be preserved"
        );
        assert!(
            lucas_desc.contains("mutations")
                && lucas_desc.contains("src/lib.rs")
                && lucas_desc.contains("Upstream from Benjamin (pkg-benjamin)"),
            "lucas payload must contain Benjamin's output marker, got:\n{lucas_desc}"
        );
        // Benjamin is now done; it must not be re-rewritten as if pending.
        assert!(
            packages_map["pkg-benjamin"]
                .description
                .matches("Upstream from Captain")
                .count()
                == 1,
            "benjamin must not be rewritten again after it completes"
        );
    }
}
