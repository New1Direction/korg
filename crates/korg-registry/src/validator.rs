use crate::plan::{SafetyCheck, TransitionPlan};
use crate::types::{CapabilityNode, CapabilityState};
use std::collections::{HashMap, HashSet};

pub(crate) struct CapabilityValidator;

impl CapabilityValidator {
    /// Verify static DAG safety checks for cycle detection
    pub(crate) fn compile_and_verify(
        nodes: &HashMap<String, CapabilityNode>,
    ) -> Result<(), String> {
        Self::verify_no_cycles(nodes)?;
        Self::verify_dangling_references(nodes)?;
        Ok(())
    }

    fn verify_no_cycles(nodes: &HashMap<String, CapabilityNode>) -> Result<(), String> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for node_id in nodes.keys() {
            if Self::dfs_detect_cycle(node_id, nodes, &mut visited, &mut rec_stack) {
                return Err(format!(
                    "Circular capability dependency detected involving '{}'",
                    node_id
                ));
            }
        }
        Ok(())
    }

    fn dfs_detect_cycle(
        node_id: &str,
        nodes: &HashMap<String, CapabilityNode>,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        if !visited.contains(node_id) {
            visited.insert(node_id.to_string());
            rec_stack.insert(node_id.to_string());

            if let Some(node) = nodes.get(node_id) {
                for dep in &node.dependencies {
                    if !visited.contains(dep)
                        && Self::dfs_detect_cycle(dep, nodes, visited, rec_stack)
                    {
                        return true;
                    } else if rec_stack.contains(dep) {
                        return true;
                    }
                }
            }
        }
        rec_stack.remove(node_id);
        false
    }

    fn verify_dangling_references(nodes: &HashMap<String, CapabilityNode>) -> Result<(), String> {
        for (node_id, node) in nodes {
            for dep in &node.dependencies {
                if !nodes.contains_key(dep) {
                    return Err(format!(
                        "Dangling dependency reference: '{}' depends on nonexistent capability '{}'", 
                        node_id, dep
                    ));
                }
            }
            for conflict in &node.conflicts {
                if !nodes.contains_key(conflict) {
                    return Err(format!(
                        "Dangling conflict reference: '{}' conflicts with nonexistent capability '{}'", 
                        node_id, conflict
                    ));
                }
            }
        }
        Ok(())
    }

    /// Dynamically assert all safety checks in a transition plan against active states
    pub(crate) fn validate_transition(
        plan: &TransitionPlan,
        nodes: &HashMap<String, CapabilityNode>,
        active_states: &HashMap<String, CapabilityState>,
    ) -> Result<(), String> {
        for check in &plan.safety_checks {
            match check {
                SafetyCheck::VerifyDag => {
                    Self::compile_and_verify(nodes)?;
                }
                SafetyCheck::CheckDependency(dep) => {
                    let active = active_states.get(dep).unwrap_or(&CapabilityState::Disabled);
                    if active == &CapabilityState::Disabled {
                        return Err(format!(
                            "Dynamic validation failed: dependency '{}' is disabled",
                            dep
                        ));
                    }
                }
                SafetyCheck::CheckConflict(conflict) => {
                    let active = active_states
                        .get(conflict)
                        .unwrap_or(&CapabilityState::Disabled);
                    if active != &CapabilityState::Disabled {
                        return Err(format!(
                            "Dynamic validation failed: active capability conflicts with '{}'",
                            conflict
                        ));
                    }
                }
                SafetyCheck::RequireCapability(req) => {
                    let active = active_states.get(req).unwrap_or(&CapabilityState::Disabled);
                    if active == &CapabilityState::Disabled {
                        return Err(format!(
                            "Dynamic validation failed: required capability '{}' is disabled",
                            req
                        ));
                    }
                }
            }
        }

        // Validate plan steps constraints dynamically
        for step in &plan.steps {
            let node = nodes.get(&step.target_id).ok_or_else(|| {
                format!("Capability '{}' does not exist in registry", step.target_id)
            })?;

            // If enabling this capability, make sure all its dependencies will be active
            if step.target_state != CapabilityState::Disabled {
                for dep in &node.dependencies {
                    let is_dep_being_enabled = plan.steps.iter().any(|s| {
                        s.target_id == *dep && s.target_state != CapabilityState::Disabled
                    });
                    let is_dep_currently_active =
                        active_states.get(dep).unwrap_or(&CapabilityState::Disabled)
                            != &CapabilityState::Disabled;
                    if !is_dep_being_enabled && !is_dep_currently_active {
                        return Err(format!(
                            "Dynamic validation failed: active capability '{}' has disabled dependency '{}'",
                            step.target_id, dep
                        ));
                    }
                }

                // Check conflict constraints
                for conflict in &node.conflicts {
                    let is_conflict_being_disabled = plan.steps.iter().any(|s| {
                        s.target_id == *conflict && s.target_state == CapabilityState::Disabled
                    });
                    let is_conflict_currently_active = active_states
                        .get(conflict)
                        .unwrap_or(&CapabilityState::Disabled)
                        != &CapabilityState::Disabled;
                    if !is_conflict_being_disabled && is_conflict_currently_active {
                        return Err(format!(
                            "Dynamic validation failed: active capability '{}' conflicts with active '{}'",
                            step.target_id, conflict
                        ));
                    }
                }
            } else {
                // If disabling this capability, make sure no active or remaining active capability depends on it
                for (other_id, other_node) in nodes {
                    if other_node.dependencies.contains(&step.target_id) {
                        let is_other_active = active_states
                            .get(other_id)
                            .unwrap_or(&CapabilityState::Disabled)
                            != &CapabilityState::Disabled;
                        let is_other_being_disabled = plan.steps.iter().any(|s| {
                            s.target_id == *other_id && s.target_state == CapabilityState::Disabled
                        });
                        if is_other_active && !is_other_being_disabled {
                            return Err(format!(
                                "Dynamic validation failed: cannot disable '{}' because active '{}' depends on it",
                                step.target_id, other_id
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
