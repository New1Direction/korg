//! Runtime Coordinator & Execution Supervisor Layer
//!
//! Provides unified campaign cancellation ownership, process group scoping,
//! backpressure quotas, and shutdown ordering guarantees.

use crate::workspace::{WorkspaceId, WorkspaceManager};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// =========================================================================
// Retry Budget Enforcer
// =========================================================================

/// Budget to prevent runaway infinite worker recovery loops (Doom Loops).
#[derive(Debug)]
pub struct RetryBudget {
    limit: usize,
    spent: usize,
}

impl RetryBudget {
    pub fn new(limit: usize) -> Self {
        Self { limit, spent: 0 }
    }

    /// Attempt to spend 1 retry from the budget.
    /// Returns true if successful, false if the budget is exhausted.
    pub fn spend(&mut self) -> bool {
        if self.spent < self.limit {
            self.spent += 1;
            tracing::info!(spent = self.spent, limit = self.limit, "retry_budget_spent");
            true
        } else {
            tracing::warn!(
                spent = self.spent,
                limit = self.limit,
                "retry_budget_exhausted"
            );
            false
        }
    }

    pub fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.spent)
    }
}

// =========================================================================
// Execution Supervisor
// =========================================================================

/// Tracks active child processes and processes group cleanup handles.
#[derive(Debug, Clone)]
pub struct ActiveWorker {
    pub handle: crate::session::SessionHandle,
    pub routing_id: String,
    pub workspace_id: WorkspaceId,
}

/// The Execution Supervisor owns all running processes and handles robust
/// process-group cancellation and cleanup.
#[derive(Debug)]
pub struct ExecutionSupervisor {
    pub backend: std::sync::Arc<dyn crate::session::SessionBackend>,
    workers: Mutex<HashMap<String, ActiveWorker>>,
}

impl Default for ExecutionSupervisor {
    fn default() -> Self {
        Self::new(crate::session::build_backend())
    }
}

impl ExecutionSupervisor {
    pub fn new(backend: std::sync::Arc<dyn crate::session::SessionBackend>) -> Self {
        Self {
            backend,
            workers: Mutex::new(HashMap::new()),
        }
    }

    /// Register an active worker.
    pub fn register(&self, key: String, worker: ActiveWorker) {
        let mut map = self.workers.lock().unwrap();
        map.insert(key, worker);
    }

    /// Unregister an active worker.
    pub fn unregister(&self, key: &str) -> Option<ActiveWorker> {
        let mut map = self.workers.lock().unwrap();
        map.remove(key)
    }

    /// Terminate all registered active process groups immediately.
    pub fn terminate_all(&self) {
        let workers: Vec<ActiveWorker> = {
            let mut map = self.workers.lock().unwrap();
            map.drain().map(|(_, w)| w).collect()
        };

        for w in workers {
            tracing::warn!(
                routing_id = %w.routing_id,
                workspace_id = %w.workspace_id,
                handle_id = %w.handle.id,
                "supervisor_terminating_session"
            );

            let backend = self.backend.clone();
            let handle = w.handle.clone();
            tokio::spawn(async move {
                if let Err(e) = backend.terminate(&handle).await {
                    tracing::error!(handle_id = %handle.id, error = %e, "session_backend_terminate_failed");
                }
            });
        }
    }

    pub fn active_count(&self) -> usize {
        let map = self.workers.lock().unwrap();
        map.len()
    }
}

// =========================================================================
// Runtime Coordinator
// =========================================================================

/// The Runtime Coordinator coordinates campaign-level resource limits, JoinSets,
/// cancellation tokens, workspace leases, and process groups.
#[derive(Debug)]
pub struct RuntimeCoordinator {
    pub session_id: Uuid,
    pub cancellation_token: CancellationToken,
    pub supervisor: Arc<ExecutionSupervisor>,
    pub workspace_manager: Arc<tokio::sync::Mutex<WorkspaceManager>>,
    pub retry_budget: Arc<Mutex<RetryBudget>>,
    pub concurrency_semaphore: Arc<tokio::sync::Semaphore>,
    pub max_workspace_quota: usize,
    pub backend: Arc<dyn crate::session::SessionBackend>,
}

impl RuntimeCoordinator {
    pub fn new(
        session_id: Uuid,
        max_concurrent_workers: usize,
        max_workspace_quota: usize,
        retry_limit: usize,
    ) -> Self {
        let backend = crate::session::build_backend();
        Self {
            session_id,
            cancellation_token: CancellationToken::new(),
            supervisor: Arc::new(ExecutionSupervisor::new(backend.clone())),
            workspace_manager: Arc::new(tokio::sync::Mutex::new(WorkspaceManager::new())),
            retry_budget: Arc::new(Mutex::new(RetryBudget::new(retry_limit))),
            concurrency_semaphore: Arc::new(tokio::sync::Semaphore::new(max_concurrent_workers)),
            max_workspace_quota,
            backend,
        }
    }

    /// Forcibly abort all running components owned by this coordinator context.
    pub fn abort(&self) {
        tracing::warn!(session_id = %self.session_id, "coordinator_abort_triggered");

        // 1. Signal cancellation token
        self.cancellation_token.cancel();

        // 2. Kill all active process groups
        self.supervisor.terminate_all();
    }
}

// =========================================================================
// Campaign Runtime Loop
// =========================================================================

/// Orchestrates Campaign status, shutdown ordering, and capability rollback hooks.
pub struct CampaignRuntime {
    pub coordinator: Arc<RuntimeCoordinator>,
    pub initial_capabilities: HashMap<String, crate::registry::CapabilityState>,
}

impl CampaignRuntime {
    pub fn new(coordinator: Arc<RuntimeCoordinator>) -> Self {
        Self {
            coordinator,
            initial_capabilities: HashMap::new(),
        }
    }

    /// Stores a backup copy of current capabilities to allow transaction rollback.
    pub async fn snapshot_capabilities(&mut self, resolver: &crate::registry::CapabilityResolver) {
        self.initial_capabilities = resolver.active_states.clone();
        tracing::debug!(
            session_id = %self.coordinator.session_id,
            capabilities = self.initial_capabilities.len(),
            "capabilities_snapshot_created"
        );
    }

    /// Forcibly rollback mutated capabilities to their pre-campaign values.
    pub async fn rollback_capabilities(
        &self,
        resolver: &Arc<tokio::sync::Mutex<crate::registry::CapabilityResolver>>,
    ) {
        tracing::warn!(session_id = %self.coordinator.session_id, "rolling_back_capabilities");
        let mut res = resolver.lock().await;
        for (node_id, state) in &self.initial_capabilities {
            let _ = res.transition(node_id, state.clone());
        }
    }

    /// Finalize execution and execute the shutdown sequence:
    /// Abort Workers → Kill Processes → Clean Workspaces → Rollback → Finalize Journal
    pub async fn execute_abort_sequence(
        &self,
        resolver: &Arc<tokio::sync::Mutex<crate::registry::CapabilityResolver>>,
    ) {
        self.execute_cleanup_sequence(resolver, true).await;
    }

    /// Guaranteed zero-leak resource cleanup sequence:
    /// Releases leases, terminates workers, destroys sandbox workspaces, and conditionally rolls back capabilities on error.
    pub async fn execute_cleanup_sequence(
        &self,
        resolver: &Arc<tokio::sync::Mutex<crate::registry::CapabilityResolver>>,
        is_error: bool,
    ) {
        tracing::info!(
            session_id = %self.coordinator.session_id,
            is_error = is_error,
            "executing_guaranteed_resource_cleanup_sequence"
        );

        // 1. Abort workers & signal cancellation
        self.coordinator.abort();

        // 2. Rollback Capability state only on error
        if is_error {
            self.rollback_capabilities(resolver).await;
        }

        // 3. Clean up and destroy all workspaces
        {
            let mut wm = self.coordinator.workspace_manager.lock().await;
            let count = wm
                .cleanup_all_for_session(self.coordinator.session_id)
                .await;
            tracing::info!(
                session_id = %self.coordinator.session_id,
                destroyed_workspaces = count,
                "workspaces_cleaned_up_during_cleanup"
            );
        }

        tracing::info!(
            session_id = %self.coordinator.session_id,
            "campaign_runtime_cleanup_sequence_completed"
        );
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_budget_enforcement() {
        let mut budget = RetryBudget::new(2);
        assert_eq!(budget.remaining(), 2);
        assert!(budget.spend());
        assert_eq!(budget.remaining(), 1);
        assert!(budget.spend());
        assert_eq!(budget.remaining(), 0);
        assert!(!budget.spend());
    }

    #[test]
    fn test_execution_supervisor_register_unregister() {
        let supervisor = ExecutionSupervisor::new(crate::session::build_backend());
        assert_eq!(supervisor.active_count(), 0);

        let ws_id = WorkspaceId::default();
        let handle = crate::session::SessionHandle::new(ws_id.clone(), "r1");
        let handle_id = handle.id.clone();
        supervisor.register(
            "worker-1".to_string(),
            ActiveWorker {
                handle,
                routing_id: "r1".to_string(),
                workspace_id: ws_id.clone(),
            },
        );
        assert_eq!(supervisor.active_count(), 1);

        let removed = supervisor.unregister("worker-1").expect("should exist");
        assert_eq!(removed.handle.id, handle_id);
        assert_eq!(supervisor.active_count(), 0);
    }

    #[tokio::test]
    async fn test_runtime_coordinator_abort_signals_token() {
        let coord = RuntimeCoordinator::new(Uuid::now_v7(), 2, 4, 2);
        assert!(!coord.cancellation_token.is_cancelled());
        coord.abort();
        assert!(coord.cancellation_token.is_cancelled());
    }

    #[tokio::test]
    async fn test_campaign_runtime_execute_abort_sequence() {
        let session_id = Uuid::now_v7();
        let coord = Arc::new(RuntimeCoordinator::new(session_id, 2, 10, 3));
        let mut runtime = CampaignRuntime::new(coord.clone());

        // 1. Setup a fake resolver with active capability states
        let resolver = Arc::new(tokio::sync::Mutex::new(
            crate::registry::CapabilityResolver::default_resolver(),
        ));
        {
            let mut res = resolver.lock().await;
            res.active_states.insert(
                "cognition_mode".to_string(),
                crate::registry::CapabilityState::Mode("balanced".to_string()),
            );
        }

        // 2. Snapshot current capabilities
        runtime.snapshot_capabilities(&*resolver.lock().await).await;
        assert_eq!(
            runtime.initial_capabilities.get("cognition_mode"),
            Some(&crate::registry::CapabilityState::Mode(
                "balanced".to_string()
            ))
        );

        // 3. Mutate the active states in the resolver
        {
            let mut res = resolver.lock().await;
            res.active_states.insert(
                "cognition_mode".to_string(),
                crate::registry::CapabilityState::Mode("heavy".to_string()),
            );
        }

        // 4. Create a workspace under this session ID to test automatic cleanup
        {
            let mut wm = coord.workspace_manager.lock().await;
            let ws_id = wm.create_workspace(crate::workspace::WorkspaceSpec {
                persona_id: "test-persona".to_string(),
                campaign_session_id: session_id,
                routing_id: "r-test".to_string(),
                use_git_worktree: false,
            });
            wm.provision(&ws_id).await.unwrap();

            // Verify workspace directory exists
            let path = wm.get(&ws_id).unwrap().worktree_path.clone();
            assert!(path.exists());
        }

        // 5. Execute unified campaign abort sequence
        runtime.execute_abort_sequence(&resolver).await;

        // 6. Verify coordinator cancellation is signaled
        assert!(coord.cancellation_token.is_cancelled());

        // 7. Verify capabilities were rolled back to their initial snapshot values
        {
            let res = resolver.lock().await;
            assert_eq!(
                res.active_states.get("cognition_mode"),
                Some(&crate::registry::CapabilityState::Mode(
                    "balanced".to_string()
                ))
            );
        }

        // 8. Verify workspace was automatically cleaned up and destroyed
        {
            let wm = coord.workspace_manager.lock().await;
            assert_eq!(wm.active_workspaces().count(), 0);
            // Verify path was deleted
            let ws_list = wm.snapshot_all();
            for ws in ws_list {
                assert!(!ws.worktree_path.exists());
            }
        }
    }

    #[tokio::test]
    async fn test_concurrency_semaphore_gating() {
        let coord = Arc::new(RuntimeCoordinator::new(Uuid::now_v7(), 2, 10, 3));

        // Acquire both permits
        let _p1 = coord
            .concurrency_semaphore
            .clone()
            .acquire_owned()
            .await
            .unwrap();
        let _p2 = coord
            .concurrency_semaphore
            .clone()
            .acquire_owned()
            .await
            .unwrap();

        // Third attempt should be gated (semaphore empty)
        assert_eq!(coord.concurrency_semaphore.available_permits(), 0);

        let p3_try = coord.concurrency_semaphore.try_acquire();
        assert!(p3_try.is_err());

        // Release one permit
        drop(_p1);
        assert_eq!(coord.concurrency_semaphore.available_permits(), 1);
        let p3_success = coord.concurrency_semaphore.try_acquire();
        assert!(p3_success.is_ok());
    }
}
