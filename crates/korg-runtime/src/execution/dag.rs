//! Speculative DAG scheduling engine.
//!
//! Resolves dependencies, compiles graphs, and runs independent nodes
//! concurrently via tokio::spawn. Brings in the full thumper execution substrate
//! while preserving korg-runtime's existing ExecutionDag / DagNode / NodeStatus
//! call sites.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

// =========================================================================
// Core Types
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
    Healed,
}

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

/// Topologically-sortable execution DAG.
///
/// The `root_task` field holds the intent description — same as `intent` in
/// the upstream thumper version but renamed to match korg-runtime call sites.
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

    /// Level-order topological compilation.
    ///
    /// Returns `Vec<Vec<String>>` where each inner vec contains node IDs that
    /// can execute in parallel. Errors on cycles.
    pub fn compile(&self) -> Result<Vec<Vec<String>>> {
        let mut in_deg: HashMap<String, usize> = HashMap::new();
        for id in self.nodes.keys() {
            in_deg.insert(id.clone(), 0);
        }
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                if self.nodes.contains_key(dep) {
                    // in_deg was seeded with every node id in the loop above,
                    // so this lookup can't fail today. The if-let is
                    // belt-and-braces — a refactor that moves the seeding
                    // can't accidentally turn this into a panic.
                    if let Some(d) = in_deg.get_mut(&node.id) {
                        *d += 1;
                    }
                }
            }
        }

        let mut levels = Vec::new();
        let mut remaining = in_deg;

        loop {
            let mut ready: Vec<String> = remaining
                .iter()
                .filter(|(_, deg)| **deg == 0)
                .map(|(id, _)| id.clone())
                .collect();
            if ready.is_empty() {
                break;
            }
            ready.sort();
            for id in &ready {
                remaining.remove(id);
            }
            for id in &ready {
                for (nid, node) in &self.nodes {
                    if node.dependencies.contains(id) {
                        if let Some(deg) = remaining.get_mut(nid) {
                            *deg = deg.saturating_sub(1);
                        }
                    }
                }
            }
            levels.push(ready);
        }

        if !remaining.is_empty() {
            return Err(anyhow!(
                "Dependency graph contains cycles (not a valid DAG)"
            ));
        }
        Ok(levels)
    }

    /// SHA-256 Merkle root over node identifiers, commands, and statuses.
    pub fn compute_merkle_root(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut node_hashes: HashMap<String, String> = HashMap::new();
        if let Ok(levels) = self.compile() {
            for level in levels {
                for node_id in level {
                    if let Some(node) = self.nodes.get(&node_id) {
                        let mut hasher = Sha256::new();
                        hasher.update(node.id.as_bytes());
                        hasher.update(node.name.as_bytes());
                        hasher.update(node.command.as_bytes());
                        hasher.update(format!("{:?}", node.status).as_bytes());
                        hasher.update(node.confidence.to_bits().to_be_bytes());
                        hasher.update(node.certainty.to_bits().to_be_bytes());
                        hasher.update(node.remediation_confidence.to_bits().to_be_bytes());
                        hasher.update(node.risk.as_bytes());
                        hasher.update(node.severity.as_bytes());
                        hasher.update(node.blast_radius.as_bytes());
                        for dep_id in &node.dependencies {
                            if let Some(parent_hash) = node_hashes.get(dep_id) {
                                hasher.update(parent_hash.as_bytes());
                            }
                        }
                        node_hashes.insert(node_id, hex::encode(hasher.finalize()));
                    }
                }
            }
        }
        if node_hashes.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(self.root_task.as_bytes());
            return hex::encode(hasher.finalize());
        }
        let mut sorted: Vec<(&String, &String)> = node_hashes.iter().collect();
        sorted.sort_by_key(|&(id, _)| id);
        let mut root_hasher = Sha256::new();
        for (_, hash) in sorted {
            root_hasher.update(hash.as_bytes());
        }
        hex::encode(root_hasher.finalize())
    }
}

// =========================================================================
// Execution summary (SQLite-free replacement for SqliteExecution)
// =========================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionSummary {
    pub session_id: String,
    pub root_task: String,
    pub status: String,
    pub overall_success: bool,
    pub start_time: String,
    pub end_time: String,
    pub merkle_root: String,
    pub node_pubkey: Option<String>,
    pub signature: Option<String>,
    pub dag_json: String,
}

// =========================================================================
// Speculative Scheduler
// =========================================================================

pub struct SpeculativeScheduler {
    pub dag: Arc<Mutex<ExecutionDag>>,
    warm_boot_started: bool,
    /// Stable agent identity that signs this execution's Merkle root — injected,
    /// not minted per run (see `crate::identity`).
    signing_key: ed25519_dalek::SigningKey,
}

impl SpeculativeScheduler {
    pub fn new(dag: ExecutionDag, signing_key: ed25519_dalek::SigningKey) -> Self {
        Self {
            dag: Arc::new(Mutex::new(dag)),
            warm_boot_started: false,
            signing_key,
        }
    }

    /// Pre-warm execution resources. No-op in the embedded form (no bun discovery needed).
    pub async fn speculative_warm_boot(&mut self) -> Result<()> {
        if self.warm_boot_started {
            return Ok(());
        }
        self.warm_boot_started = true;
        Ok(())
    }

    /// Execute the compiled DAG, running independent levels in parallel.
    ///
    /// Each node in a level is spawned as a concurrent tokio task. Failed nodes
    /// attempt self-healing via `execution::recovery::heal_node_with_context`.
    pub async fn run(
        &mut self,
        logs_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> Result<ExecutionSummary> {
        self.speculative_warm_boot().await.ok();

        let start_time = chrono::Utc::now().to_rfc3339();
        let start_instant = Instant::now();
        let session_id = Uuid::new_v4().to_string();

        let levels = {
            let guard = self.dag.lock().unwrap();
            guard.compile()?
        };

        if let Some(ref tx) = logs_tx {
            let root = self.dag.lock().unwrap().root_task.clone();
            let _ = tx.send(format!(
                "[THUMPER] Launching Speculative DAG Execution Engine — session {} — {}",
                &session_id[..8],
                root
            ));
        }

        let mut overall_success = true;

        for (idx, level) in levels.into_iter().enumerate() {
            if let Some(ref tx) = logs_tx {
                let _ = tx.send(format!(
                    "[LEVEL {}] Scheduling {} nodes in parallel: {:?}",
                    idx + 1,
                    level.len(),
                    level
                ));
            }

            let mut tasks = Vec::new();
            for node_id in level {
                let dag_clone = self.dag.clone();
                let logs_tx_clone = logs_tx.clone();

                let task = tokio::spawn(async move {
                    let mut node = {
                        let guard = dag_clone.lock().unwrap();
                        let Some(n) = guard.nodes.get(&node_id).cloned() else {
                            drop(guard);
                            if let Some(ref tx) = logs_tx_clone {
                                let _ = tx.send(format!(
                                    "  [!] Skipping {}: node missing from DAG (concurrent mutation)",
                                    node_id
                                ));
                            }
                            return false;
                        };
                        n
                    };

                    node.status = NodeStatus::Running;
                    {
                        let mut guard = dag_clone.lock().unwrap();
                        guard.nodes.insert(node_id.clone(), node.clone());
                    }

                    if let Some(ref tx) = logs_tx_clone {
                        let _ = tx.send(format!(
                            "  [→] Starting: {} ({}) [Risk: {}]",
                            node.name, node.command, node.risk
                        ));
                    }

                    let node_start = Instant::now();
                    let success = run_command(&node.command).await;

                    if success {
                        node.status = NodeStatus::Success;
                        node.certainty = 100.0;
                        if let Some(ref tx) = logs_tx_clone {
                            let _ = tx.send(format!(
                                "  [✓] Done: {} in {:.2?}",
                                node.name,
                                node_start.elapsed()
                            ));
                        }
                    } else {
                        node.status = NodeStatus::Failed;
                        if let Some(ref tx) = logs_tx_clone {
                            let _ = tx.send(format!(
                                "  [!] Failed: {} — attempting self-heal",
                                node.name
                            ));
                        }
                        let healed =
                            super::recovery::heal_node(&node.command, logs_tx_clone.clone())
                                .await
                                .unwrap_or(false);
                        if healed {
                            node.status = NodeStatus::Healed;
                            node.remediation_confidence = 1.0;
                        }
                    }

                    {
                        let mut guard = dag_clone.lock().unwrap();
                        guard.nodes.insert(node_id.clone(), node.clone());
                    }

                    node.status != NodeStatus::Failed
                });

                tasks.push(task);
            }

            for t in tasks {
                match t.await {
                    Ok(success) if !success => overall_success = false,
                    Err(_) => overall_success = false,
                    _ => {}
                }
            }
        }

        let end_time = chrono::Utc::now().to_rfc3339();
        let dag_guard = self.dag.lock().unwrap();
        let merkle_root = dag_guard.compute_merkle_root();
        let dag_json = serde_json::to_string(&*dag_guard).unwrap_or_default();

        // Sign this execution's Merkle root with the agent's persistent identity.
        let (node_pubkey, signature) = sign_merkle_root(merkle_root.as_bytes(), &self.signing_key);

        if let Some(ref tx) = logs_tx {
            let _ = tx.send(format!(
                "[PROOF] Merkle root sha256_{}...",
                &merkle_root[..16]
            ));
            if let Some(ref pk) = node_pubkey {
                let _ = tx.send(format!("[PROOF] Pubkey ed25519_{}", &pk[..16]));
            }
        }

        Ok(ExecutionSummary {
            session_id,
            root_task: dag_guard.root_task.clone(),
            status: if overall_success { "done" } else { "error" }.to_string(),
            overall_success,
            start_time,
            end_time,
            merkle_root,
            node_pubkey,
            signature,
            dag_json,
        })
    }
}

fn sign_merkle_root(
    data: &[u8],
    key: &ed25519_dalek::SigningKey,
) -> (Option<String>, Option<String>) {
    use ed25519_dalek::Signer;
    let sig = key.sign(data);
    let pubkey_hex = hex::encode(key.verifying_key().to_bytes());
    let sig_hex = hex::encode(sig.to_bytes());
    (Some(pubkey_hex), Some(sig_hex))
}

/// Run a shell command for real and report whether it exited successfully.
/// Replaces the previous `!command.contains("fail")` simulation — the DAG node's
/// status now reflects the command's actual exit code.
async fn run_command(command: &str) -> bool {
    match tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, deps: Vec<&str>) -> DagNode {
        DagNode {
            id: id.into(),
            name: id.into(),
            command: format!("echo {}", id),
            dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
            status: NodeStatus::Pending,
            confidence: 1.0,
            risk: "Low".into(),
            severity: "Low".into(),
            blast_radius: "None".into(),
            certainty: 100.0,
            remediation_confidence: 1.0,
        }
    }

    #[test]
    fn test_dag_compilation_and_levels() {
        let mut dag = ExecutionDag::new("Test Intent");
        dag.add_node(make_node("A", vec![]));
        dag.add_node(make_node("B", vec!["A"]));
        dag.add_node(make_node("C", vec!["A"]));
        dag.add_node(make_node("D", vec!["B", "C"]));

        let levels = dag.compile().unwrap();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], vec!["A"]);
        let mut l1 = levels[1].clone();
        l1.sort();
        assert_eq!(l1, vec!["B", "C"]);
        assert_eq!(levels[2], vec!["D"]);
    }

    #[test]
    fn test_dag_cycle_detection() {
        let mut dag = ExecutionDag::new("Cycle");
        dag.add_node(make_node("A", vec!["B"]));
        dag.add_node(make_node("B", vec!["A"]));
        assert!(dag.compile().unwrap_err().to_string().contains("cycles"));
    }

    #[test]
    fn test_merkle_dag_root_determinism() {
        let mut dag1 = ExecutionDag::new("Deploy");
        dag1.add_node(DagNode {
            id: "A".into(),
            name: "Audit".into(),
            command: "bun audit".into(),
            dependencies: vec![],
            status: NodeStatus::Success,
            confidence: 0.99,
            risk: "Low".into(),
            severity: "High".into(),
            blast_radius: "None".into(),
            certainty: 100.0,
            remediation_confidence: 1.0,
        });
        let root1 = dag1.compute_merkle_root();

        let mut dag2 = ExecutionDag::new("Deploy");
        dag2.add_node(DagNode {
            id: "A".into(),
            name: "Audit".into(),
            command: "bun audit".into(),
            dependencies: vec![],
            status: NodeStatus::Success,
            confidence: 0.99,
            risk: "Low".into(),
            severity: "High".into(),
            blast_radius: "None".into(),
            certainty: 100.0,
            remediation_confidence: 1.0,
        });
        assert_eq!(root1, dag2.compute_merkle_root());

        dag2.nodes.get_mut("A").unwrap().status = NodeStatus::Failed;
        assert_ne!(root1, dag2.compute_merkle_root());
    }

    #[tokio::test]
    async fn run_command_reflects_real_exit_status() {
        assert!(run_command("true").await, "`true` exits 0 → success");
        assert!(!run_command("false").await, "`false` exits non-zero → failure");
    }

    #[tokio::test]
    async fn run_succeeds_when_commands_really_succeed() {
        let mut dag = ExecutionDag::new("smoke");
        dag.add_node(make_node("a", vec![]));
        dag.add_node(make_node("b", vec!["a"]));
        let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let mut scheduler = SpeculativeScheduler::new(dag, key);
        let summary = scheduler.run(None).await.unwrap();
        assert!(summary.overall_success);
        assert_eq!(summary.status, "done");
        assert!(!summary.merkle_root.is_empty());
    }

    #[tokio::test]
    async fn run_fails_on_a_real_nonzero_exit_not_a_string_match() {
        // The old sim used `success = !command.contains("fail")`, so `false`
        // (no "fail" substring, real exit 1) wrongly passed. Real execution must
        // fail it — and with no heal context it must stay failed, not fake-heal.
        let mut dag = ExecutionDag::new("smoke");
        dag.add_node(make_node("ok", vec![]));
        let mut bad = make_node("bad", vec![]);
        bad.command = "false".into();
        dag.add_node(bad);
        let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let mut scheduler = SpeculativeScheduler::new(dag, key);
        let summary = scheduler.run(None).await.unwrap();
        assert!(!summary.overall_success, "a real non-zero exit must fail the DAG");
    }

    #[test]
    fn sign_merkle_root_uses_the_given_key_deterministically() {
        use ed25519_dalek::Verifier;
        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let (pk, sig) = sign_merkle_root(b"root-abc", &key);
        // The pubkey is the given key's stable identity (not a throwaway), and
        // Ed25519 is deterministic so re-signing the same data matches.
        assert_eq!(pk, Some(hex::encode(key.verifying_key().to_bytes())));
        let (_, sig2) = sign_merkle_root(b"root-abc", &key);
        assert_eq!(sig, sig2);
        let raw = hex::decode(sig.unwrap()).unwrap();
        let signature = ed25519_dalek::Signature::from_slice(&raw).unwrap();
        assert!(key.verifying_key().verify(b"root-abc", &signature).is_ok());
    }
}
