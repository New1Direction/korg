//! Workspace Lifecycle Management
//!
//! A Workspace is the **first-class isolated execution context** for a single agent run.
//! It owns:
//! - An isolated filesystem (git worktree on a dedicated branch)
//! - Lifecycle state (Created → Provisioned → Active → Completed | Failed → Destroyed)
//! - A snapshot handle (Merkle root of the worktree at completion)
//! - Provenance metadata (persona, campaign session, routing ID)
//!
//! # Architecture position
//!
//! ```text
//! Capability Kernel
//!     ↓
//! Campaign State Machine
//!     ↓
//! Arena / Evaluator
//!     ↓
//! WorkspaceManager          ← this module
//!     ↓
//! Session Backend (session.rs)
//!     ↓
//! Worker Processes
//! ```
//!
//! The `WorkspaceManager` is the single authority over worktree creation and destruction.
//! Nothing else may call `git worktree add/remove` directly.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

// =========================================================================
// WorkspaceId newtype
// =========================================================================

/// Strongly-typed workspace identifier. Prevents accidental routing_id / workspace_id confusion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(Uuid);

impl WorkspaceId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for WorkspaceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ws-{}", self.0)
    }
}

// =========================================================================
// WorkspaceState machine
// =========================================================================

/// The lifecycle state of a single workspace.
///
/// Valid transitions:
/// ```text
/// Created → Provisioned → Active → Completed
///                     ↘         ↗
///                       Failed
///                     ↘
///                       Destroyed (from any terminal state)
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkspaceState {
    /// Path allocated, git worktree not yet created.
    Created,
    /// Git worktree exists on an isolated branch; no worker attached yet.
    Provisioned { branch: String },
    /// A worker process is attached and running.
    Active {
        /// The ACP routing_id of the worker currently using this workspace.
        session_routing_id: String,
    },
    /// Worker finished. Artifacts persisted. Ready for harvest or cleanup.
    Completed {
        exit_ok: bool,
        /// git write-tree hash of the worktree at completion.
        snapshot: String,
    },
    /// Unrecoverable error. Workspace contents may be partial.
    Failed { reason: String },
    /// Cleaned up. Path removed. Object is a tombstone only.
    Destroyed,
}

impl WorkspaceState {
    /// Returns true if the workspace is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            WorkspaceState::Completed { .. }
                | WorkspaceState::Failed { .. }
                | WorkspaceState::Destroyed
        )
    }

    /// Returns true if a worker can attach to this workspace.
    pub fn is_attachable(&self) -> bool {
        matches!(self, WorkspaceState::Provisioned { .. })
    }
}

// =========================================================================
// Workspace
// =========================================================================

/// Specification for creating a new workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceSpec {
    /// Human-readable persona (e.g. "captain", "harper").
    pub persona_id: String,
    /// The campaign session this workspace belongs to.
    pub campaign_session_id: Uuid,
    /// The ACP routing_id for the work package assigned to this workspace.
    pub routing_id: String,
    /// If true, create an isolated git worktree on a dedicated branch.
    /// If false, workspace is a plain temp directory (no git).
    pub use_git_worktree: bool,
}

/// A single isolated execution context for one agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    /// Persona name (captain / harper / benjamin / lucas).
    pub persona_id: String,
    /// The campaign session this workspace belongs to.
    pub campaign_session_id: Uuid,
    /// ACP routing ID of the work package.
    pub routing_id: String,
    /// Absolute path to the isolated worktree (or temp dir).
    pub worktree_path: PathBuf,
    /// Git branch name (empty string if non-git workspace).
    pub branch: String,
    /// Current lifecycle state.
    pub state: WorkspaceState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Workspace {
    fn new(id: WorkspaceId, spec: &WorkspaceSpec, worktree_path: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id,
            persona_id: spec.persona_id.clone(),
            campaign_session_id: spec.campaign_session_id,
            routing_id: spec.routing_id.clone(),
            worktree_path,
            branch: String::new(),
            state: WorkspaceState::Created,
            created_at: now,
            updated_at: now,
        }
    }

    fn transition(&mut self, next: WorkspaceState) {
        tracing::debug!(
            workspace_id = %self.id,
            persona = %self.persona_id,
            from = ?self.state,
            to = ?next,
            "workspace_state_transition"
        );
        self.state = next;
        self.updated_at = Utc::now();
    }
}

// =========================================================================
// WorkspaceManager
// =========================================================================

/// The single authority over workspace lifecycle.
///
/// All worktree creation, attachment, snapshotting, and destruction flows through here.
/// Nothing else may directly call `git worktree add/remove`.
#[derive(Debug, Default)]
pub struct WorkspaceManager {
    /// All workspaces managed by this instance (alive + terminal/tombstone).
    workspaces: HashMap<WorkspaceId, Workspace>,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        Self::default()
    }

    // -----------------------------------------------------------------------
    // Create
    // -----------------------------------------------------------------------

    /// Allocate a workspace. Does not touch the filesystem yet.
    pub fn create_workspace(&mut self, spec: WorkspaceSpec) -> WorkspaceId {
        let id = WorkspaceId::new();
        let path = workspace_path(&id, &spec);
        let ws = Workspace::new(id.clone(), &spec, path);

        tracing::info!(
            workspace_id = %id,
            persona = %ws.persona_id,
            routing_id = %ws.routing_id,
            "workspace_created"
        );
        korg_core::metrics::record_workspace_created(&ws.persona_id);

        self.workspaces.insert(id.clone(), ws);
        id
    }

    // -----------------------------------------------------------------------
    // Provision — creates the actual filesystem context
    // -----------------------------------------------------------------------

    /// Provision a workspace: create its filesystem context (git worktree or plain dir).
    ///
    /// Transitions: `Created → Provisioned`.
    pub async fn provision(&mut self, id: &WorkspaceId) -> Result<()> {
        let ws = self.workspaces.get(id).context("workspace not found")?;

        if !matches!(ws.state, WorkspaceState::Created) {
            anyhow::bail!("cannot provision workspace in state {:?}", ws.state);
        }

        let path = ws.worktree_path.clone();
        let persona_id = ws.persona_id.clone();
        let session_id = ws.campaign_session_id;
        let routing_id = ws.routing_id.clone();

        // Create the directory
        tokio::fs::create_dir_all(&path)
            .await
            .with_context(|| format!("failed to create workspace dir: {}", path.display()))?;

        // Attempt git worktree add (best-effort — falls back to plain dir)
        let branch = format!("ws/{}/{}/{}", persona_id, session_id, routing_id);
        let git_ok = try_git_worktree_add(&path, &branch).await;

        let actual_branch = if git_ok { branch } else { String::new() };

        tracing::info!(
            workspace_id = %id,
            path = %path.display(),
            branch = %actual_branch,
            git_ok,
            "workspace_provisioned"
        );

        let ws = self.workspaces.get_mut(id).unwrap();
        ws.branch = actual_branch.clone();
        ws.transition(WorkspaceState::Provisioned {
            branch: actual_branch,
        });

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Attach
    // -----------------------------------------------------------------------

    /// Attach a worker (routing_id) to a provisioned workspace.
    ///
    /// Transitions: `Provisioned → Active`.
    pub fn attach_worker(
        &mut self,
        id: &WorkspaceId,
        session_routing_id: String,
    ) -> Result<&Workspace> {
        let ws = self.workspaces.get_mut(id).context("workspace not found")?;

        if !ws.state.is_attachable() {
            anyhow::bail!(
                "workspace {} is not in Provisioned state (current: {:?})",
                id,
                ws.state
            );
        }

        tracing::info!(
            workspace_id = %id,
            session_routing_id = %session_routing_id,
            "workspace_worker_attached"
        );

        ws.transition(WorkspaceState::Active { session_routing_id });
        Ok(&*ws)
    }

    // -----------------------------------------------------------------------
    // Snapshot
    // -----------------------------------------------------------------------

    /// Snapshot the workspace at its current state (git write-tree).
    ///
    /// Returns the tree hash. Does not change workspace state.
    pub async fn snapshot_workspace(&self, id: &WorkspaceId) -> Result<String> {
        let ws = self.workspaces.get(id).context("workspace not found")?;

        let hash = tokio::process::Command::new("git")
            .arg("write-tree")
            .current_dir(&ws.worktree_path)
            .output()
            .await
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| format!("sha256:snapshot-fallback-{}", id));

        tracing::debug!(workspace_id = %id, snapshot = %hash, "workspace_snapshotted");
        Ok(hash)
    }

    /// Revert the workspace's files to a target snapshot tree hash (Git read-tree -u --reset <hash>).
    pub async fn restore_workspace(&self, id: &WorkspaceId, tree_hash: &str) -> Result<()> {
        let ws = self.workspaces.get(id).context("workspace not found")?;

        if !ws.branch.is_empty() {
            let output = tokio::process::Command::new("git")
                .args(["read-tree", "-u", "--reset"])
                .arg(tree_hash)
                .current_dir(&ws.worktree_path)
                .output()
                .await?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "Failed to restore workspace to tree {}: {}",
                    tree_hash,
                    err
                ));
            }
        } else {
            tracing::warn!(workspace_id = %id, "Cannot restore non-git plain workspace; skipping file revert");
        }

        tracing::info!(workspace_id = %id, snapshot = %tree_hash, "workspace_restored_to_snapshot");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Complete / Fail
    // -----------------------------------------------------------------------

    /// Mark a workspace as completed after its worker exits successfully.
    pub async fn complete_workspace(&mut self, id: &WorkspaceId, exit_ok: bool) -> Result<String> {
        let snapshot = self.snapshot_workspace(id).await?;
        let ws = self.workspaces.get_mut(id).context("workspace not found")?;
        ws.transition(WorkspaceState::Completed {
            exit_ok,
            snapshot: snapshot.clone(),
        });
        korg_core::metrics::record_workspace_completed(&ws.persona_id, exit_ok);
        Ok(snapshot)
    }

    /// Mark a workspace as failed with a reason.
    pub fn fail_workspace(&mut self, id: &WorkspaceId, reason: String) {
        if let Some(ws) = self.workspaces.get_mut(id) {
            tracing::warn!(workspace_id = %id, reason = %reason, "workspace_failed");
            ws.transition(WorkspaceState::Failed { reason });
        }
    }

    // -----------------------------------------------------------------------
    // Destroy
    // -----------------------------------------------------------------------

    /// Destroy a workspace: remove the git worktree and clean up the filesystem.
    ///
    /// Safe to call on any terminal state. Idempotent.
    pub async fn destroy_workspace(&mut self, id: &WorkspaceId) -> Result<()> {
        let ws = self.workspaces.get(id).context("workspace not found")?;

        if matches!(ws.state, WorkspaceState::Destroyed) {
            return Ok(()); // idempotent
        }

        let path = ws.worktree_path.clone();
        let branch = ws.branch.clone();

        // Remove git worktree registration (ignore failure — path may not be a worktree)
        if !branch.is_empty() {
            let _ = tokio::process::Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .output()
                .await;
        }

        // Remove the directory
        if path.exists() {
            tokio::fs::remove_dir_all(&path)
                .await
                .with_context(|| format!("failed to remove workspace dir: {}", path.display()))?;
        }

        tracing::info!(workspace_id = %id, path = %path.display(), "workspace_destroyed");
        korg_core::metrics::record_workspace_destroyed(&ws.persona_id);

        let ws = self.workspaces.get_mut(id).unwrap();
        ws.transition(WorkspaceState::Destroyed);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Query
    // -----------------------------------------------------------------------

    /// Get a workspace by ID.
    pub fn get(&self, id: &WorkspaceId) -> Option<&Workspace> {
        self.workspaces.get(id)
    }

    /// Iterate over all workspaces that are currently active (worker attached).
    pub fn active_workspaces(&self) -> impl Iterator<Item = &Workspace> {
        self.workspaces
            .values()
            .filter(|ws| matches!(ws.state, WorkspaceState::Active { .. }))
    }

    /// All workspaces for a given campaign session.
    pub fn workspaces_for_session(&self, session_id: Uuid) -> impl Iterator<Item = &Workspace> {
        self.workspaces
            .values()
            .filter(move |ws| ws.campaign_session_id == session_id)
    }

    /// Destroy all non-destroyed workspaces belonging to a session. Returns count destroyed.
    pub async fn cleanup_all_for_session(&mut self, session_id: Uuid) -> usize {
        let ids: Vec<WorkspaceId> = self
            .workspaces
            .values()
            .filter(|ws| {
                ws.campaign_session_id == session_id
                    && !matches!(ws.state, WorkspaceState::Destroyed)
            })
            .map(|ws| ws.id.clone())
            .collect();

        let count = ids.len();
        for id in ids {
            if let Err(e) = self.destroy_workspace(&id).await {
                tracing::warn!(workspace_id = %id, error = %e, "cleanup_destroy_failed");
            }
        }

        tracing::info!(session_id = %session_id, destroyed = count, "session_workspaces_cleaned_up");
        count
    }

    /// A JSON-serializable snapshot of all workspaces (for /api/workspaces).
    pub fn snapshot_all(&self) -> Vec<&Workspace> {
        self.workspaces.values().collect()
    }

    /// Total workspace count.
    pub fn len(&self) -> usize {
        self.workspaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workspaces.is_empty()
    }
}

// =========================================================================
// Internal helpers
// =========================================================================

/// Compute the workspace directory path from its ID and spec.
fn workspace_path(id: &WorkspaceId, spec: &WorkspaceSpec) -> PathBuf {
    korg_core::paths::cache_dir().join("workspaces").join(format!(
        "{}-{}-{}",
        spec.persona_id,
        spec.routing_id,
        id.as_uuid()
    ))
}

/// Attempt to register a git worktree at `path` on `branch`.
/// Returns true on success, false if git is unavailable or the directory isn't a repo.
async fn try_git_worktree_add(path: &PathBuf, branch: &str) -> bool {
    let root = korg_core::paths::project_root();
    // Create a new orphan branch in the main repo, then register the worktree
    let status = tokio::process::Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(path)
        .current_dir(&root)
        .status()
        .await;

    match status {
        Ok(s) if s.success() => {
            // Create and checkout the named branch inside the worktree
            let _ = tokio::process::Command::new("git")
                .args(["checkout", "-b", branch])
                .current_dir(path)
                .status()
                .await;
            true
        }
        _ => false,
    }
}

// =========================================================================
// paths.rs extension
// =========================================================================

/// Canonical path for a workspace directory (used by WorkspaceManager internally).
pub fn workspace_cache_root() -> PathBuf {
    korg_core::paths::cache_dir().join("workspaces")
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_spec(persona: &str) -> WorkspaceSpec {
        WorkspaceSpec {
            persona_id: persona.into(),
            campaign_session_id: Uuid::now_v7(),
            routing_id: "test-routing-001".into(),
            use_git_worktree: false, // plain dir for tests
        }
    }

    #[test]
    fn workspace_id_is_unique() {
        let a = WorkspaceId::new();
        let b = WorkspaceId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn workspace_id_display_has_ws_prefix() {
        let id = WorkspaceId::new();
        assert!(id.to_string().starts_with("ws-"));
    }

    #[test]
    fn workspace_state_terminal_checks() {
        assert!(WorkspaceState::Destroyed.is_terminal());
        assert!(WorkspaceState::Failed {
            reason: "oops".into()
        }
        .is_terminal());
        assert!(WorkspaceState::Completed {
            exit_ok: true,
            snapshot: "abc".into()
        }
        .is_terminal());
        assert!(!WorkspaceState::Created.is_terminal());
        assert!(!WorkspaceState::Active {
            session_routing_id: "r".into()
        }
        .is_terminal());
    }

    #[test]
    fn workspace_state_attachable_only_when_provisioned() {
        assert!(WorkspaceState::Provisioned {
            branch: "ws/x".into()
        }
        .is_attachable());
        assert!(!WorkspaceState::Created.is_attachable());
        assert!(!WorkspaceState::Active {
            session_routing_id: "r".into()
        }
        .is_attachable());
    }

    #[test]
    fn manager_create_returns_unique_ids() {
        let mut mgr = WorkspaceManager::new();
        let id1 = mgr.create_workspace(make_spec("captain"));
        let id2 = mgr.create_workspace(make_spec("harper"));
        assert_ne!(id1, id2);
        assert_eq!(mgr.len(), 2);
    }

    #[test]
    fn manager_get_returns_correct_workspace() {
        let mut mgr = WorkspaceManager::new();
        let id = mgr.create_workspace(make_spec("lucas"));
        let ws = mgr.get(&id).expect("workspace should exist");
        assert_eq!(ws.persona_id, "lucas");
        assert!(matches!(ws.state, WorkspaceState::Created));
    }

    #[test]
    fn manager_fail_workspace_transitions() {
        let mut mgr = WorkspaceManager::new();
        let id = mgr.create_workspace(make_spec("benjamin"));
        mgr.fail_workspace(&id, "test failure".into());
        let ws = mgr.get(&id).unwrap();
        assert!(matches!(ws.state, WorkspaceState::Failed { .. }));
        assert!(ws.state.is_terminal());
    }

    #[test]
    fn manager_active_workspaces_filters_correctly() {
        let mut mgr = WorkspaceManager::new();
        let id = mgr.create_workspace(make_spec("captain"));

        // Manually set state to Active (bypassing provision for unit test)
        let ws = mgr.workspaces.get_mut(&id).unwrap();
        ws.state = WorkspaceState::Active {
            session_routing_id: "r1".into(),
        };

        assert_eq!(mgr.active_workspaces().count(), 1);
    }

    #[test]
    fn manager_workspaces_for_session_filters() {
        let mut mgr = WorkspaceManager::new();
        let s1 = Uuid::now_v7();
        let s2 = Uuid::now_v7();

        let spec1 = WorkspaceSpec {
            persona_id: "captain".into(),
            campaign_session_id: s1,
            routing_id: "r1".into(),
            use_git_worktree: false,
        };
        let spec2 = WorkspaceSpec {
            persona_id: "harper".into(),
            campaign_session_id: s2,
            routing_id: "r2".into(),
            use_git_worktree: false,
        };

        mgr.create_workspace(spec1);
        mgr.create_workspace(spec2);

        assert_eq!(mgr.workspaces_for_session(s1).count(), 1);
        assert_eq!(mgr.workspaces_for_session(s2).count(), 1);
    }

    #[tokio::test]
    async fn provision_creates_directory() {
        let mut mgr = WorkspaceManager::new();
        let session_id = Uuid::now_v7();
        let spec = WorkspaceSpec {
            persona_id: "test-persona".into(),
            campaign_session_id: session_id,
            routing_id: "test-routing-provision".into(),
            use_git_worktree: false,
        };
        let id = mgr.create_workspace(spec);
        let result = mgr.provision(&id).await;
        assert!(result.is_ok(), "provision failed: {:?}", result.err());

        let ws = mgr.get(&id).unwrap();
        assert!(ws.worktree_path.exists());
        assert!(matches!(ws.state, WorkspaceState::Provisioned { .. }));

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&ws.worktree_path).await;
    }

    #[tokio::test]
    async fn attach_worker_transitions_to_active() {
        let mut mgr = WorkspaceManager::new();
        let session_id = Uuid::now_v7();
        let spec = WorkspaceSpec {
            persona_id: "captain".into(),
            campaign_session_id: session_id,
            routing_id: "r-attach".into(),
            use_git_worktree: false,
        };
        let id = mgr.create_workspace(spec);
        mgr.provision(&id).await.unwrap();
        mgr.attach_worker(&id, "routing-001".into()).unwrap();

        let ws = mgr.get(&id).unwrap();
        assert!(matches!(ws.state, WorkspaceState::Active { .. }));

        let _ = tokio::fs::remove_dir_all(&ws.worktree_path).await;
    }

    #[tokio::test]
    async fn destroy_workspace_removes_directory() {
        let mut mgr = WorkspaceManager::new();
        let session_id = Uuid::now_v7();
        let spec = WorkspaceSpec {
            persona_id: "lucas".into(),
            campaign_session_id: session_id,
            routing_id: "r-destroy".into(),
            use_git_worktree: false,
        };
        let id = mgr.create_workspace(spec);
        mgr.provision(&id).await.unwrap();

        let path = mgr.get(&id).unwrap().worktree_path.clone();
        assert!(path.exists());

        mgr.destroy_workspace(&id).await.unwrap();

        assert!(!path.exists());
        assert!(matches!(
            mgr.get(&id).unwrap().state,
            WorkspaceState::Destroyed
        ));
    }

    #[tokio::test]
    async fn cleanup_all_for_session_destroys_all() {
        let mut mgr = WorkspaceManager::new();
        let session_id = Uuid::now_v7();

        for persona in &["captain", "harper"] {
            let spec = WorkspaceSpec {
                persona_id: (*persona).into(),
                campaign_session_id: session_id,
                routing_id: format!("r-{}", persona),
                use_git_worktree: false,
            };
            let id = mgr.create_workspace(spec);
            mgr.provision(&id).await.unwrap();
        }

        let count = mgr.cleanup_all_for_session(session_id).await;
        assert_eq!(count, 2);

        // All should now be Destroyed
        assert_eq!(
            mgr.workspaces_for_session(session_id)
                .filter(|ws| matches!(ws.state, WorkspaceState::Destroyed))
                .count(),
            2
        );
    }
}
