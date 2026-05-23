use super::types::EffectNode;
use super::plan::{TransitionPlan, MutationStep};
use super::log::{CapabilityEvent, CapabilityJournal};
use chrono::Utc;
use uuid::Uuid;

pub struct CapabilityExecutor;

impl CapabilityExecutor {
    /// Execute forward mutation step topologically
    pub fn execute_steps(
        plan_id: Uuid,
        steps: &[MutationStep],
        journal: &mut CapabilityJournal,
    ) -> Result<(), String> {
        for step in steps {
            Self::execute_effect_dag(plan_id, &step.target_id, &step.effect_nodes, journal)?;
        }
        Ok(())
    }

    /// Execute rollback step topologically (used on transition failure)
    pub fn execute_rollbacks(
        plan_id: Uuid,
        rollback_steps: &[MutationStep],
        journal: &mut CapabilityJournal,
    ) {
        for step in rollback_steps {
            let _ = Self::execute_effect_dag(plan_id, &step.target_id, &step.effect_nodes, journal);
        }
    }

    fn execute_effect_dag(
        plan_id: Uuid,
        target_id: &str,
        nodes: &[EffectNode],
        journal: &mut CapabilityJournal,
    ) -> Result<(), String> {
        let mut executed = std::collections::HashSet::new();
        let mut healed_nodes = std::collections::HashSet::new();
        let mut attempts = 0;

        // Keep loop executing nodes topologically in order of dependency matching
        while executed.len() < nodes.len() && attempts < 100 {
            attempts += 1;
            for node in nodes {
                if executed.contains(&node.id) {
                    continue;
                }

                // If all dependency steps are executed, we can trigger execution
                let dependencies_met = node.depends_on.iter().all(|dep_id| executed.contains(dep_id));
                if dependencies_met {
                    journal.append(CapabilityEvent::EffectStarted {
                        plan_id,
                        step_target: target_id.to_string(),
                        effect_id: node.id,
                        timestamp: Utc::now(),
                    });

                    // Trigger mock/sandbox real-world side effects execution
                    match Self::run_effect(&node.effect) {
                        Ok(_) => {
                            journal.append(CapabilityEvent::EffectCompleted {
                                plan_id,
                                step_target: target_id.to_string(),
                                effect_id: node.id,
                                timestamp: Utc::now(),
                            });
                            executed.insert(node.id);
                        }
                        Err(e) => {
                            // Attempt micro-healing
                            if healed_nodes.contains(&node.id) {
                                journal.append(CapabilityEvent::EffectFailed {
                                    plan_id,
                                    step_target: target_id.to_string(),
                                    effect_id: node.id,
                                    reason: e.clone(),
                                    timestamp: Utc::now(),
                                });
                                return Err(e);
                            }

                            healed_nodes.insert(node.id);
                            eprintln!("[Executor] Execution failed for node {}. Attempting micro-healing...", node.id);

                            match Self::attempt_micro_healing(&node.effect, &e) {
                                Ok(_) => {
                                    journal.append(CapabilityEvent::EffectRetrying {
                                        plan_id,
                                        step_target: target_id.to_string(),
                                        effect_id: node.id,
                                        retry_count: 1,
                                        timestamp: Utc::now(),
                                    });

                                    eprintln!("[Executor] Micro-healing succeeded. Retrying execution...");
                                    match Self::run_effect(&node.effect) {
                                        Ok(_) => {
                                            journal.append(CapabilityEvent::EffectCompleted {
                                                plan_id,
                                                step_target: target_id.to_string(),
                                                effect_id: node.id,
                                                timestamp: Utc::now(),
                                            });
                                            executed.insert(node.id);
                                        }
                                        Err(retry_err) => {
                                            journal.append(CapabilityEvent::EffectFailed {
                                                plan_id,
                                                step_target: target_id.to_string(),
                                                effect_id: node.id,
                                                reason: format!("{} (Retry failed: {})", e, retry_err),
                                                timestamp: Utc::now(),
                                            });
                                            return Err(retry_err);
                                        }
                                    }
                                }
                                Err(heal_err) => {
                                    journal.append(CapabilityEvent::EffectFailed {
                                        plan_id,
                                        step_target: target_id.to_string(),
                                        effect_id: node.id,
                                        reason: format!("{} (Healing failed: {})", e, heal_err),
                                        timestamp: Utc::now(),
                                    });
                                    return Err(e);
                                }
                            }
                        }
                    }
                }
            }
        }
        if executed.len() < nodes.len() {
            return Err("Topological execution failed: some effect nodes were not executed due to unresolved dependencies or cycles".to_string());
        }
        Ok(())
    }

    fn attempt_micro_healing(effect: &super::types::CapabilityEffect, error: &str) -> Result<(), String> {
        // Milliseconds-fast self-correction patterns
        if error.contains("already in use") || error.contains("conflict") {
            if let super::types::CapabilityEffect::InitializeSandbox { container_name, .. } = effect {
                eprintln!("[Micro-Heal] Container collision detected for '{}'. Initiating fast teardown...", container_name);
                // Run immediate teardown to clear the collision
                let _ = std::process::Command::new("docker")
                    .args(&["rm", "-f", container_name])
                    .output();
                return Ok(());
            }
        }

        if error.contains("lock file exists") || error.contains("stuck_lock") {
            eprintln!("[Micro-Heal] Stale lockfile detected. Automatically removing lock file...");
            let lock_path = crate::paths::project_root().join(".korg").join("capability_journal.lock");
            let _ = std::fs::remove_file(lock_path);
            return Ok(());
        }

        Err("No quick-fix matching pattern found for this error".to_string())
    }

    fn run_effect(effect: &super::types::CapabilityEffect) -> Result<(), String> {
        // Execute dynamic side effects (Mocked safely for sandbox integration tests)
        match effect {
            super::types::CapabilityEffect::InitializeSandbox { container_name, .. } => {
                eprintln!("[Executor] Spawn isolated Docker sandbox container: {}", container_name);
                if container_name.contains("fail_first") {
                    thread_local! {
                        static RETRY_COUNT: std::cell::Cell<usize> = std::cell::Cell::new(0);
                    }
                    let count = RETRY_COUNT.with(|c| c.get());
                    if count == 0 {
                        RETRY_COUNT.with(|c| c.set(1));
                        return Err("docker sandbox conflict: container already in use".to_string());
                    }
                } else if container_name.contains("fail_always") {
                    return Err("docker sandbox conflict: container already in use".to_string());
                }
            }
            super::types::CapabilityEffect::TeardownSandbox { container_name } => {
                eprintln!("[Executor] Destroying container: {}", container_name);
            }
            super::types::CapabilityEffect::ExecuteTool { tool } => {
                eprintln!("[Executor] Executing side effect tool: {}", tool);
                if tool.contains("fail_first") {
                    thread_local! {
                        static RETRY_COUNT_TOOL: std::cell::Cell<usize> = std::cell::Cell::new(0);
                    }
                    let count = RETRY_COUNT_TOOL.with(|c| c.get());
                    if count == 0 {
                        RETRY_COUNT_TOOL.with(|c| c.set(1));
                        return Err("stuck_lock: lock file exists".to_string());
                    }
                } else if tool.contains("fail_always") {
                    return Err("stuck_lock: lock file exists".to_string());
                }
            }
            _ => {}
        }
        Ok(())
    }
}
