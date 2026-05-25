use crate::plan::{MutationStep, SafetyCheck, TransitionPlan};
use crate::types::{CapabilityEffect, CapabilityNode, CapabilityState, EffectNode};
use std::collections::HashMap;

pub(crate) struct CapabilityPlanner;

impl CapabilityPlanner {
    pub(crate) fn plan_transition(
        nodes: &HashMap<String, CapabilityNode>,
        active_states: &HashMap<String, CapabilityState>,
        target_id: &str,
        target_state: CapabilityState,
    ) -> Result<TransitionPlan, String> {
        let node = nodes
            .get(target_id)
            .ok_or_else(|| format!("Unknown capability: {}", target_id))?;

        let previous_state = active_states
            .get(target_id)
            .unwrap_or(&node.default_state)
            .clone();

        if previous_state == target_state {
            return Ok(TransitionPlan::new()); // No-op plan
        }

        let mut plan = TransitionPlan::new();

        // Populate forward steps with corresponding typed effects
        let forward_effects = Self::derive_effects(target_id, &target_state);
        plan.steps.push(MutationStep {
            target_id: target_id.to_string(),
            previous_state: previous_state.clone(),
            target_state: target_state.clone(),
            effect_nodes: forward_effects,
        });

        // Populate rollback steps
        let rollback_effects = Self::derive_effects(target_id, &previous_state);
        plan.rollback_steps.push(MutationStep {
            target_id: target_id.to_string(),
            previous_state: target_state,
            target_state: previous_state,
            effect_nodes: rollback_effects,
        });

        // Generate safety checks
        plan.safety_checks.push(SafetyCheck::VerifyDag);
        for dep in &node.dependencies {
            plan.safety_checks
                .push(SafetyCheck::CheckDependency(dep.clone()));
        }
        for conflict in &node.conflicts {
            plan.safety_checks
                .push(SafetyCheck::CheckConflict(conflict.clone()));
        }

        Ok(plan)
    }

    fn derive_effects(id: &str, state: &CapabilityState) -> Vec<EffectNode> {
        let mut effect_nodes = vec![];
        match id {
            "docker_sandbox" => {
                if state == &CapabilityState::Enabled {
                    effect_nodes.push(EffectNode {
                        id: 0,
                        effect: CapabilityEffect::InitializeSandbox {
                            container_name: "korg-zero-trust-worker".to_string(),
                            memory_limit_mb: 512,
                        },
                        depends_on: vec![],
                    });
                } else if state == &CapabilityState::Disabled {
                    effect_nodes.push(EffectNode {
                        id: 0,
                        effect: CapabilityEffect::TeardownSandbox {
                            container_name: "korg-zero-trust-worker".to_string(),
                        },
                        depends_on: vec![],
                    });
                }
            }
            "semantic_llm_cache" => {
                if state == &CapabilityState::Enabled {
                    effect_nodes.push(EffectNode {
                        id: 0,
                        effect: CapabilityEffect::ExecuteTool {
                            tool: "init_semantic_cache_dir".to_string(),
                        },
                        depends_on: vec![],
                    });
                }
            }
            "cognition_mode" => {
                if let CapabilityState::Mode(m) = state {
                    effect_nodes.push(EffectNode {
                        id: 0,
                        effect: CapabilityEffect::ModifyGraph {
                            node: format!("cognition_mode:{}", m),
                        },
                        depends_on: vec![],
                    });
                }
            }
            _ => {}
        }
        effect_nodes
    }
}
