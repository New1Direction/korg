//! Execution DAG — vendored replacement for `thumper_cli::bun::dag` and
//! `thumper_cli::bun::recovery`.
//!
//! Provides the exact types and functions that `leader.rs` depends on:
//!   - `ExecutionDag`, `DagNode`, `NodeStatus` (DAG scheduling)
//!   - `SpeculativeScheduler` (pre-warm shim)
//!   - `heal_node_with_context` (self-healing recovery)

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

// =========================================================================
// Core DAG Types
// =========================================================================

/// Status of an individual DAG node during campaign execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeStatus {
    Pending,
    Running,
    Success,
    Failed,
    Healed,
}

/// A single node in the execution DAG.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DagNode {
    pub id: String,
    pub name: String,
    pub command: String,
    pub dependencies: Vec<String>,
    pub status: NodeStatus,
    pub confidence: f64,
    pub risk: String,
    pub severity: String,
    pub blast_radius: String,
    pub certainty: f64,
    pub remediation_confidence: f64,
}

/// A topologically-sortable execution DAG.
///
/// Nodes are stored by ID. `compile()` returns a level-ordered schedule
/// where each level contains node IDs that can execute in parallel.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionDag {
    pub root_task: String,
    pub nodes: HashMap<String, DagNode>,
    insertion_order: Vec<String>,
}

impl ExecutionDag {
    pub fn new(root_task: &str) -> Self {
        Self {
            root_task: root_task.to_string(),
            nodes: HashMap::new(),
            insertion_order: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: DagNode) {
        let id = node.id.clone();
        self.nodes.insert(id.clone(), node);
        if !self.insertion_order.contains(&id) {
            self.insertion_order.push(id);
        }
    }

    /// Topological level-order compilation.
    ///
    /// Returns `Vec<Vec<String>>` where each inner vec is a "level" of
    /// independent nodes that can execute concurrently.
    pub fn compile(&self) -> Result<Vec<Vec<String>>> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for id in self.nodes.keys() {
            in_degree.entry(id.as_str()).or_insert(0);
        }
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                let _ = in_degree.entry(dep.as_str()).or_insert(0); // ensure dep exists
                if let Some(deg) = in_degree.get_mut(node.id.as_str()) {
                    // Only count deps that actually exist in the graph
                    if self.nodes.contains_key(dep) {
                        *deg += 1;
                    }
                }
            }
        }
        // Re-count properly
        let mut in_deg: HashMap<String, usize> = HashMap::new();
        for id in self.nodes.keys() {
            in_deg.insert(id.clone(), 0);
        }
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                if self.nodes.contains_key(dep) {
                    *in_deg.get_mut(&node.id).unwrap() += 1;
                }
            }
        }

        let mut levels = Vec::new();
        let mut remaining = in_deg;

        loop {
            let ready: Vec<String> = remaining
                .iter()
                .filter(|(_, deg)| **deg == 0)
                .map(|(id, _)| id.clone())
                .collect();

            if ready.is_empty() {
                break;
            }

            // Sort for determinism
            let mut sorted_ready = ready;
            sorted_ready.sort();

            for id in &sorted_ready {
                remaining.remove(id);
            }

            // Decrease in-degree for dependents
            for id in &sorted_ready {
                for (nid, node) in &self.nodes {
                    if node.dependencies.contains(id) {
                        if let Some(deg) = remaining.get_mut(nid) {
                            *deg = deg.saturating_sub(1);
                        }
                    }
                }
            }

            levels.push(sorted_ready);
        }

        if !remaining.is_empty() {
            anyhow::bail!(
                "Cycle detected in execution DAG. Remaining nodes: {:?}",
                remaining.keys().collect::<Vec<_>>()
            );
        }

        Ok(levels)
    }

    /// Computes a SHA-256 Merkle root over the DAG's node identifiers and statuses.
    ///
    /// This provides a deterministic fingerprint of the DAG execution state
    /// for cryptographic provenance attestations.
    pub fn compute_merkle_root(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        // Sort node IDs for deterministic ordering
        let mut ids: Vec<&String> = self.nodes.keys().collect();
        ids.sort();
        for id in ids {
            if let Some(node) = self.nodes.get(id) {
                hasher.update(id.as_bytes());
                hasher.update(format!("{:?}", node.status).as_bytes());
                hasher.update(node.command.as_bytes());
            }
        }
        hex::encode(hasher.finalize())
    }
}

// =========================================================================
// Speculative Scheduler
// =========================================================================

/// Lightweight speculative scheduler that pre-warms execution resources.
///
/// In the vendored version this is a no-op shim — the original thumper
/// implementation pre-warmed Bun/shell subprocesses. The DAG compilation
/// and level-order scheduling is handled directly by `ExecutionDag::compile()`.
pub struct SpeculativeScheduler {
    _dag: ExecutionDag,
}

impl SpeculativeScheduler {
    pub fn new(dag: ExecutionDag) -> Self {
        Self { _dag: dag }
    }

    /// Pre-warm execution resources. Currently a no-op in the vendored version.
    pub async fn speculative_warm_boot(&mut self) -> Result<()> {
        // In the original thumper-cli this pre-spawned shell processes.
        // For the vendored version we just return Ok — the real work
        // happens in dispatch_concurrent.
        Ok(())
    }
}

// =========================================================================
// Self-Healing Recovery
// =========================================================================

/// Attempts to heal a failed DAG node by re-running compilation checks.
///
/// This is the vendored replacement for `thumper_cli::bun::recovery::heal_node_with_context`.
///
/// # Arguments
/// * `check_command` — The command that failed (e.g. "cargo check")
/// * `stderr` — Optional captured stderr from the failed run
/// * `worktree_path` — Optional path to the worktree to check
/// * `progress_tx` — Optional channel to stream progress messages
///
/// # Returns
/// `Ok(true)` if healing succeeded (the check now passes), `Ok(false)` otherwise.
pub async fn heal_node_with_context(
    check_command: &str,
    stderr: Option<&str>,
    worktree_path: Option<&Path>,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> Result<bool> {
    let send = |msg: String| {
        if let Some(ref tx) = progress_tx {
            let _ = tx.send(msg);
        }
    };

    send(format!(
        "🔧 [HEAL] Attempting self-healing for: {}",
        check_command
    ));

    if let Some(err) = stderr {
        let line_count = err.lines().count();
        send(format!(
            "🔧 [HEAL] Analyzing {} lines of compiler output...",
            line_count
        ));
    }

    // If we have a worktree, try running the check command again
    // (the caller is expected to have applied fixes before calling this)
    if let Some(path) = worktree_path {
        if path.exists() {
            send(format!(
                "🔧 [HEAL] Re-running `{}` in {}...",
                check_command,
                path.display()
            ));

            let parts: Vec<&str> = check_command.split_whitespace().collect();
            if parts.is_empty() {
                return Ok(false);
            }

            let output = tokio::process::Command::new(parts[0])
                .args(&parts[1..])
                .current_dir(path)
                .output()
                .await?;

            if output.status.success() {
                send("🔧 [HEAL] ✓ Check passed after healing!".to_string());
                return Ok(true);
            } else {
                let stderr_out = String::from_utf8_lossy(&output.stderr);
                send(format!(
                    "🔧 [HEAL] ✗ Check still failing: {}",
                    stderr_out.lines().take(3).collect::<Vec<_>>().join(" | ")
                ));
                return Ok(false);
            }
        }
    }

    // No worktree available — can't verify healing
    send("🔧 [HEAL] No worktree available for verification.".to_string());
    Ok(false)
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dag_compile_linear() {
        let mut dag = ExecutionDag::new("test");
        dag.add_node(DagNode {
            id: "a".into(),
            name: "A".into(),
            command: "echo a".into(),
            dependencies: vec![],
            status: NodeStatus::Pending,
            confidence: 0.9,
            risk: "Low".into(),
            severity: "Low".into(),
            blast_radius: "Scoped".into(),
            certainty: 0.9,
            remediation_confidence: 0.9,
        });
        dag.add_node(DagNode {
            id: "b".into(),
            name: "B".into(),
            command: "echo b".into(),
            dependencies: vec!["a".into()],
            status: NodeStatus::Pending,
            confidence: 0.9,
            risk: "Low".into(),
            severity: "Low".into(),
            blast_radius: "Scoped".into(),
            certainty: 0.9,
            remediation_confidence: 0.9,
        });

        let levels = dag.compile().unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0], vec!["a"]);
        assert_eq!(levels[1], vec!["b"]);
    }

    #[test]
    fn test_dag_compile_parallel() {
        let mut dag = ExecutionDag::new("test");
        dag.add_node(DagNode {
            id: "a".into(),
            name: "A".into(),
            command: "echo a".into(),
            dependencies: vec![],
            status: NodeStatus::Pending,
            confidence: 0.9,
            risk: "Low".into(),
            severity: "Low".into(),
            blast_radius: "Scoped".into(),
            certainty: 0.9,
            remediation_confidence: 0.9,
        });
        dag.add_node(DagNode {
            id: "b".into(),
            name: "B".into(),
            command: "echo b".into(),
            dependencies: vec![],
            status: NodeStatus::Pending,
            confidence: 0.9,
            risk: "Low".into(),
            severity: "Low".into(),
            blast_radius: "Scoped".into(),
            certainty: 0.9,
            remediation_confidence: 0.9,
        });
        dag.add_node(DagNode {
            id: "c".into(),
            name: "C".into(),
            command: "echo c".into(),
            dependencies: vec!["a".into(), "b".into()],
            status: NodeStatus::Pending,
            confidence: 0.9,
            risk: "Low".into(),
            severity: "Low".into(),
            blast_radius: "Scoped".into(),
            certainty: 0.9,
            remediation_confidence: 0.9,
        });

        let levels = dag.compile().unwrap();
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0], vec!["a", "b"]);
        assert_eq!(levels[1], vec!["c"]);
    }

    #[test]
    fn test_node_status_transitions() {
        let mut dag = ExecutionDag::new("test");
        dag.add_node(DagNode {
            id: "x".into(),
            name: "X".into(),
            command: "echo x".into(),
            dependencies: vec![],
            status: NodeStatus::Pending,
            confidence: 0.9,
            risk: "Low".into(),
            severity: "Low".into(),
            blast_radius: "Scoped".into(),
            certainty: 0.9,
            remediation_confidence: 0.9,
        });

        assert_eq!(dag.nodes["x"].status, NodeStatus::Pending);
        dag.nodes.get_mut("x").unwrap().status = NodeStatus::Running;
        assert_eq!(dag.nodes["x"].status, NodeStatus::Running);
        dag.nodes.get_mut("x").unwrap().status = NodeStatus::Success;
        assert_eq!(dag.nodes["x"].status, NodeStatus::Success);
    }
}
