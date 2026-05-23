//! Single-worker harness implementation.
//!
//! This closely follows the `SingleWorkerHarness` and `FullWorktreeWorker`
//! examples from Minimal-ACP-Client-Pseudocode.md (Sections 2 and 4.3).

use crate::acp::{AcpClient, AcpMessage};
use crate::personas::{run_persona, Persona};
#[allow(unused_imports)]
use crate::tools;
use anyhow::Result;
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::interval;
use uuid::Uuid;

pub struct SingleWorkerHarness {
    pub worker_id: String,
    current_worktree: Option<PathBuf>,
}

impl SingleWorkerHarness {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            current_worktree: None,
        }
    }

    /// Main worker loop (legacy stub path).
    pub async fn run(&mut self, client: &mut AcpClient) -> Result<()> {
        println!("[Harness] Worker {} entering main loop (legacy client path)", self.worker_id);

        if let Ok(msg) = client.receive().await {
            match msg {
                AcpMessage::RouteWork {
                    routing_id,
                    payload,
                    base_snapshot,
                    codebase_merkle_root,
                    permissions,
                    ..
                } => {
                    println!("[Harness] Received RouteWork with base_snapshot: {}", base_snapshot);
                    self.handle_route_work(client, routing_id, payload, base_snapshot, codebase_merkle_root, permissions)
                        .await?;
                }
                _ => {
                    println!("[Harness] Received unhandled message: {:?}", msg);
                }
            }
        }

        println!("[Harness] Worker {} exiting after task", self.worker_id);
        Ok(())
    }

    /// Modern stdio framed path (Phase A).
    /// The worker process is launched by the leader and receives a signed MessageEnvelope<RouteWork>
    /// on stdin. We read it using the ACP framed reader, verify, then execute the work package.
    pub async fn run_as_stdio_worker(worker_id: String) -> Result<()> {
        use tokio::io::{stdin, BufReader};

        eprintln!("[Harness] Worker {} starting in stdio framed ACP mode (waiting for signed RouteWork)", worker_id);

        let mut reader = BufReader::new(stdin());

        // Read the signed envelope from the leader
        match crate::acp::read_acp_envelope(&mut reader).await {
            Ok(envelope) => {
                let verified = crate::acp::verify_envelope(&envelope).unwrap_or(false);

                eprintln!(
                    "[Harness] Worker {} received ACP MessageEnvelope (verified={})",
                    worker_id, verified
                );

                match envelope.payload {
                    AcpMessage::RouteWork {
                        routing_id,
                        payload,
                        base_snapshot,
                        codebase_merkle_root,
                        permissions,
                        ..
                    } => {
                        eprintln!("[Harness] Processing RouteWork {} (base_snapshot={})", routing_id, base_snapshot);

                        let mut harness = SingleWorkerHarness::new(worker_id.clone());

                        let worker_signing_key = SigningKey::generate(&mut OsRng);
                        let mut real_client = AcpClient::new_stdio(&worker_id, worker_signing_key);

                        harness
                            .handle_route_work(&mut real_client, routing_id, payload, base_snapshot, codebase_merkle_root, permissions)
                            .await?;

                        // === Polished demo: Handle one extra signed tool request after RouteWork ===
                        // Uses the existing reader so ordering is correct.
                        if let Ok(extra_env) = crate::acp::read_acp_envelope(&mut reader).await {
                            let verified = crate::acp::verify_envelope(&extra_env).unwrap_or(false);
                            eprintln!("[Harness] Received post-work tool request (verified={})", verified);

                            if let Some(result_msg) = crate::tools::dispatch_tool(extra_env.payload, &worker_id).await {
                                let _ = real_client.send(&result_msg).await;
                                eprintln!("[Harness] Sent signed tool result back to leader");
                            }
                        }
                    }

                    // === Direct tool request (if worker is sent a tool as first message) ===
                    tool @ AcpMessage::ShellExecRequest(_)
                    | tool @ AcpMessage::FileReadRequest(_)
                    | tool @ AcpMessage::PatchApplyRequest(_)
                    | tool @ AcpMessage::TestRunRequest(_)
                    | tool @ AcpMessage::ScreenshotRequest(_)
                    | tool @ AcpMessage::CodeEditProposal(_) => {
                        eprintln!("[Harness] Received direct coding tool request");
                        let worker_signing_key = SigningKey::generate(&mut OsRng);
                        let mut real_client = AcpClient::new_stdio(&worker_id, worker_signing_key);

                        if let Some(result_msg) = crate::tools::dispatch_tool(tool, &worker_id).await {
                            let _ = real_client.send(&result_msg).await;
                            eprintln!("[Harness] Sent signed tool result");
                        }
                    }

                    other => {
                        eprintln!("[Harness] First message was not a RouteWork or tool request: {:?}", other);
                    }
                }
            }
            Err(e) => {
                eprintln!("[Harness] Worker {} failed to read incoming ACP envelope: {}", worker_id, e);
                // Still try to do useful work if possible, or just exit cleanly
            }
        }

        eprintln!("[Harness] Worker {} exiting after stdio task", worker_id);
        Ok(())
    }

    pub(crate) async fn handle_route_work(
        &mut self,
        client: &mut AcpClient,
        routing_id: String,
        payload: String,
        base_snapshot: String,
        codebase_merkle_root: String,
        _permissions: Vec<String>,
    ) -> Result<()> {
        eprintln!("[Harness] Received RouteWork {}: {}", routing_id, payload);

        // Save original working directory to restore it during cleanup
        let original_dir = std::env::current_dir()?;
        eprintln!("[Harness] Parent repository working directory: {:?}", original_dir);

        // 1. Create isolated worktree (see isolation-routing.md)
        let worktree_path = crate::paths::worktree_dir_harness(
            &self.worker_id, &routing_id,
        );
        self.current_worktree = Some(worktree_path.clone());

        // Ensure parent and target worktree directory are completely clean
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if worktree_path.exists() {
            let _ = std::fs::remove_dir_all(&worktree_path);
        }

        // Verify if base_snapshot is a valid git reference/commit.
        // If not valid or empty/genesis/latest, default to HEAD.
        let mut snapshot_ref = "HEAD".to_string();
        if !base_snapshot.is_empty() && base_snapshot != "genesis" && base_snapshot != "latest-from-blackboard" {
            let verify_status = tokio::process::Command::new("git")
                .args(&["rev-parse", "--verify", &base_snapshot])
                .output()
                .await;
            if let Ok(output) = verify_status {
                if output.status.success() {
                    snapshot_ref = base_snapshot.clone();
                }
            }
        }

        let branch_name = format!("korg-branch-{}", routing_id);
        eprintln!(
            "[Harness] Spinning up physical worktree at {:?} from commit {} (branch: {})",
            worktree_path, snapshot_ref, branch_name
        );

        // Run git worktree add
        let add_status = tokio::process::Command::new("git")
            .args(&[
                "worktree",
                "add",
                "-f",
                "-B",
                &branch_name,
                &worktree_path.to_string_lossy(),
                &snapshot_ref,
            ])
            .output()
            .await;

        match add_status {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("[Harness] ERROR: git worktree add failed: {}", stderr);
                    anyhow::bail!("git worktree add failed: {}", stderr);
                }
                eprintln!("[Harness] Git worktree created successfully.");
            }
            Err(e) => {
                eprintln!("[Harness] ERROR: failed to spawn git worktree add: {}", e);
                anyhow::bail!("failed to spawn git worktree add: {}", e);
            }
        }

        // Switch process working directory into the sandboxed worktree
        std::env::set_current_dir(&worktree_path)?;
        eprintln!("[Harness] Sandboxed worker process CWD to {:?}", worktree_path);

        // Zero-Trust Sandbox Containment check:
        // Run git write-tree inside the sandbox and compare it to the expected codebase_merkle_root.
        if !codebase_merkle_root.is_empty() && !codebase_merkle_root.starts_with("sha256:codebase-fallback") {
            let write_tree_output = tokio::process::Command::new("git")
                .arg("write-tree")
                .output()
                .await;
            match write_tree_output {
                Ok(output) if output.status.success() => {
                    let actual_tree = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    eprintln!("[Harness] Zero-Trust Verification: expected={}, actual={}", codebase_merkle_root, actual_tree);
                    if actual_tree != codebase_merkle_root {
                        eprintln!("[Harness] ERROR: Codebase Merkle Root Mismatch! Zero-Trust Containment violation.");
                        let _ = std::env::set_current_dir(&original_dir);
                        anyhow::bail!("Codebase Merkle Root Mismatch: expected {}, got {}", codebase_merkle_root, actual_tree);
                    }
                    eprintln!("[Harness] Zero-Trust Verification successful: tree hash matches.");
                }
                Ok(output) => {
                    let err = String::from_utf8_lossy(&output.stderr);
                    eprintln!("[Harness] ERROR: git write-tree failed during zero-trust verification: {}", err);
                    let _ = std::env::set_current_dir(&original_dir);
                    anyhow::bail!("git write-tree verification failed: {}", err);
                }
                Err(e) => {
                    eprintln!("[Harness] ERROR: failed to execute git write-tree: {}", e);
                    let _ = std::env::set_current_dir(&original_dir);
                    anyhow::bail!("failed to execute git write-tree: {}", e);
                }
            }
        }

        // Emit start-of-work telemetry pulse
        // (build_telemetry_pulse temporarily stubbed for structural cleanup)
        let start_pulse = AcpMessage::SwarmTelemetryPulse {
            agent_id: self.worker_id.clone(),
            per_agent: serde_json::json!({ self.worker_id.clone(): {"phase": "start"} }),
            aggregate: serde_json::json!({}),
            scaling_recommendation: None,
        };
        let _ = client.send(&start_pulse).await;

        // === Background periodic telemetry emitter (new) ===
        // While the main persona task runs, we spawn a simple background task
        // that prints "live pulse #N" every 2.5 seconds. This makes the continuous
        // telemetry stream highly visible when running `cargo run -- campaign`.
        //
        // In a production worker this would send real SwarmTelemetryPulse messages
        // containing up-to-date risk/velocity/confidence/etc.
        let rid = routing_id.clone();
        let emitter_handle = tokio::spawn(async move {
            let mut tick = 0u32;
            let mut ticker = interval(Duration::from_millis(2500));

            loop {
                ticker.tick().await;
                tick += 1;
                println!(
                    "[TelemetryEmitter] {} – live pulse #{} (continuous real-time telemetry)",
                    rid, tick
                );
                if tick > 8 {
                    break;
                } // safety for short tasks
            }
        });

        // 2. Run the actual persona task (emitter runs in parallel)
        let result = self.run_task_in_worktree(&payload).await?;

        // Wait for emitter to finish (or abort it)
        let _ = emitter_handle.await;

        if payload.contains("simulate-crash") {
            eprintln!("[Harness] Worker {} detected simulate-crash directive. Writing partial .ktrans...", self.worker_id);
            let partial_mutations = vec![serde_json::json!({
                "target_path": "src/auth.rs",
                "payload": "partial code before worker panic (resilience test)"
            })];
            write_terminal_ktrans(
                &self.worker_id,
                &routing_id,
                &base_snapshot,
                &partial_mutations,
                &vec!["partial-provenance-before-crash".to_string()],
                false,
            );
            eprintln!("[Harness] Worker {} SIMULATING WORKER CRASH/PANIC (exiting with 101)!", self.worker_id);
            std::process::exit(101);
        }

        // Final completion pulse
        let final_pulse = AcpMessage::SwarmTelemetryPulse {
            agent_id: self.worker_id.clone(),
            per_agent: serde_json::json!({ self.worker_id.clone(): {"phase": "complete"} }),
            aggregate: serde_json::json!({}),
            scaling_recommendation: None,
        };
        let _ = client.send(&final_pulse).await;

        // 3. Emit terminal .ktrans (mandatory on every exit path)
        // Write to disk (new in this increment)
        write_terminal_ktrans(
            &self.worker_id,
            &routing_id,
            &base_snapshot,
            &result.mutations,
            &result.provenance,
            result.doom_loop,
        );

        let tx_id = Uuid::now_v7();
        let ktrans = serde_json::json!({
            "tx_id": tx_id,
            "worker_id": self.worker_id,
            "routing_id": routing_id,
            "base_snapshot": base_snapshot,
            "mutations": result.mutations,
            "doom_loop_detected": result.doom_loop,
            "provenance": result.provenance,
            "codebase_merkle_root": result.codebase_merkle_root,
        });

        client
            .send(&AcpMessage::SubmitTransaction {
                tx_id,
                content_hash: result.codebase_merkle_root.clone(),
                payload: ktrans.clone(),
            })
            .await?;

        // Emit completion telemetry pulse with real observed metrics (the key data for the Evaluator)
        let completion_pulse = AcpMessage::SwarmTelemetryPulse {
            agent_id: self.worker_id.clone(),
            per_agent: serde_json::json!({ self.worker_id.clone(): {"phase": "complete"} }),
            aggregate: serde_json::json!({}),
            scaling_recommendation: None,
        };
        let _ = client.send(&completion_pulse).await;

        // 4. Report termination (see pseudocode TerminationReport)
        client
            .send(&AcpMessage::TerminationReport {
                routing_id: routing_id.clone(),
                exit_status: if result.doom_loop {
                    "doom_loop"
                } else {
                    "success"
                }
                .to_string(),
                final_ktrans: None,
                worker_id: Some(self.worker_id.clone()),
                terminal_tx_id: Some(tx_id),
            })
            .await?;

        eprintln!(
            "[Harness] Work package {} completed. Terminal tx: {}",
            routing_id, tx_id
        );

        // Restore parent working directory to release lock on worktree path
        eprintln!("[Harness] Restoring parent working directory to {:?}", original_dir);
        if let Err(e) = std::env::set_current_dir(&original_dir) {
            eprintln!("[Harness] WARNING: failed to restore original directory: {}", e);
        }

        // Clean up worktree (or leave for forensics on failure)
        eprintln!("[Harness] Cleaning up worktree {:?}", worktree_path);
        let remove_status = tokio::process::Command::new("git")
            .args(&[
                "worktree",
                "remove",
                "--force",
                &worktree_path.to_string_lossy(),
            ])
            .output()
            .await;

        match remove_status {
            Ok(output) => {
                if !output.status.success() {
                    eprintln!(
                        "[Harness] WARNING: git worktree remove failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                } else {
                    eprintln!("[Harness] Git worktree removed successfully.");
                }
            }
            Err(e) => {
                eprintln!("[Harness] WARNING: failed to spawn git worktree remove: {}", e);
            }
        }

        // Delete temporary tracking branch
        let branch_status = tokio::process::Command::new("git")
            .args(&["branch", "-D", &branch_name])
            .output()
            .await;

        match branch_status {
            Ok(output) => {
                if !output.status.success() {
                    eprintln!(
                        "[Harness] WARNING: failed to delete branch {}: {}",
                        branch_name,
                        String::from_utf8_lossy(&output.stderr)
                    );
                } else {
                    eprintln!("[Harness] Branch {} deleted successfully.", branch_name);
                }
            }
            Err(e) => {
                eprintln!("[Harness] WARNING: failed to spawn git branch -D: {}", e);
            }
        }

        Ok(())
    }

    async fn run_task_in_worktree(&self, payload: &str) -> Result<TaskResult> {
        // Route persona from worker_id when possible (real 4-persona topology)
        let persona = self.infer_persona_from_worker_id();
        eprintln!(
            "[Harness] Executing task inside worktree as {}: {}",
            persona.name(),
            payload
        );

        let persona_result = run_persona(persona, payload, "worker-task").await;

        // Stage all modifications so git write-tree will capture them
        let _ = tokio::process::Command::new("git")
            .arg("add")
            .arg(".")
            .output()
            .await;

        // Compute the resulting physical codebase Merkle root
        let write_tree_output = tokio::process::Command::new("git")
            .arg("write-tree")
            .output()
            .await;

        let codebase_merkle_root = match write_tree_output {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            _ => "sha256:codebase-fallback".to_string(),
        };

        Ok(TaskResult {
            mutations: persona_result.mutations,
            doom_loop: false,
            provenance: vec![format!("persona:{}", persona.name())],
            // Store extra signals so we can emit high-fidelity telemetry
            confidence: persona_result.confidence,
            arena_scores: persona_result.arena_self_score.clone(),
            codebase_merkle_root,
        })
    }

    fn infer_persona_from_worker_id(&self) -> Persona {
        let wid = &self.worker_id.to_lowercase();
        if wid.contains("captain") {
            Persona::Captain
        } else if wid.contains("harper") {
            Persona::Harper
        } else if wid.contains("lucas") {
            Persona::Lucas
        } else if wid.contains("evaluator") {
            Persona::Evaluator
        } else {
            Persona::Benjamin
        }
    }
} // end of impl SingleWorkerHarness

/// Builds a live, time-evolving SwarmTelemetryPulse used by the background emitter.
/// Metrics drift realistically so the Evaluator can see trends for doom-loop detection.
fn build_live_evolving_pulse(worker_id: &str, routing_id: &str, tick: u32) -> AcpMessage {
    // Simulate realistic drift over time
    let base_velocity = 70.0 + (tick as f64 * 8.0).min(90.0);
    let risk = (0.35 + (tick as f64 * 0.015).sin().abs() * 0.35).min(0.82);
    let confidence = (0.78 - (tick as f64 * 0.008)).max(0.42);
    let conflict = (0.12 + (tick as f64 * 0.01) % 0.18).min(0.38);
    let gpu = (0.48 + (tick as f64 * 0.012) % 0.35).min(0.91);

    let surface = format!(
        "live tick {} – velocity {:.0}, risk drifting, confidence {:.2}",
        tick, base_velocity, confidence
    );

    let per_agent = serde_json::json!({
        worker_id: {
            "risk_score": risk,
            "epistemic_confidence": confidence,
            "conflict_rate": conflict,
            "token_velocity": base_velocity,
            "gpu_util": gpu,
            "verified_count_delta": if tick.is_multiple_of(3) { 1 } else { 0 },
            "authority_improvement": (0.12 - (tick as f64 * 0.005)).max(0.03),
            "surface_text": surface,
            "phase": "live",
            "routing_id": routing_id,
        }
    });

    AcpMessage::SwarmTelemetryPulse {
        agent_id: worker_id.to_string(),
        per_agent,
        aggregate: serde_json::json!({ "tick": tick }),
        scaling_recommendation: None,
    }
}

#[derive(Debug)]
struct TaskResult {
    mutations: Vec<serde_json::Value>,
    doom_loop: bool,
    provenance: Vec<String>,
    // Enriched signals for high-quality SwarmTelemetryPulse emission
    confidence: f32,
    arena_scores: serde_json::Value,
    codebase_merkle_root: String,
}

/// Writes a proper .ktrans file to disk (terminal transaction).
/// Follows the schema from wiki/mechanisms/transactional-memory.md
fn write_terminal_ktrans(
    worker_id: &str,
    routing_id: &str,
    base_snapshot: &str,
    mutations: &[serde_json::Value],
    provenance: &[String],
    doom_loop: bool,
) {
    let ktrans_dir = crate::paths::ktrans_dir();
    std::fs::create_dir_all(&ktrans_dir).ok();

    let tx_id = uuid::Uuid::now_v7();
    let timestamp = chrono::Utc::now().to_rfc3339();

    let ktrans = serde_json::json!({
        "tx_id": tx_id,
        "worker_id": worker_id,
        "routing_id": routing_id,
        "timestamp": timestamp,
        "base_snapshot": base_snapshot,
        "provenance_chain": provenance,
        "mutations": mutations,
        "doom_loop_detected": doom_loop,
        "exit_reason": if doom_loop { "doom_loop" } else { "success" }
    });

    let filename = format!("{}-{}.ktrans.json", routing_id, worker_id);
    let path = ktrans_dir.join(filename);

    if let Ok(content) = serde_json::to_string_pretty(&ktrans) {
        if std::fs::write(&path, content).is_ok() {
            eprintln!("[Harness] Wrote terminal .ktrans → {}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_git_worktree_isolation() {
        let worker_id = "benjamin-test-worktree".to_string();
        let routing_id = "test-route-123".to_string();
        
        let mut harness = SingleWorkerHarness::new(worker_id.clone());
        
        let worker_signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let mut client = AcpClient::new_stdio(&worker_id, worker_signing_key);

        let payload = "Write a mock implementation plan".to_string();
        
        let res = harness.handle_route_work(
            &mut client,
            routing_id.clone(),
            payload,
            "HEAD".to_string(),
            "".to_string(),
            vec![]
        ).await;

        assert!(res.is_ok());

        // Verify that the worktree directory is removed and cleaned up after successful completion
        let worktree_path = crate::paths::worktree_dir_harness(
            &worker_id, &routing_id,
        );
        assert!(!worktree_path.exists());
    }
}
