use super::types::{CapabilityState, EffectNode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransitionState {
    Planned,
    Validated,
    Committed,
    Applying,
    Applied,
    RolledBack,
    Failed,
}

/// Individual step in a mutation transaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MutationStep {
    pub target_id: String,
    pub previous_state: CapabilityState,
    pub target_state: CapabilityState,
    pub effect_nodes: Vec<EffectNode>,
}

/// Strongly typed safety checks to support static analysis and verification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "check_type", content = "payload")]
pub enum SafetyCheck {
    VerifyDag,
    CheckDependency(String),
    CheckConflict(String),
    RequireCapability(String),
}

/// Immutable plan capturing the declarative transition intent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransitionPlan {
    pub plan_id: Uuid,
    pub steps: Vec<MutationStep>,
    pub rollback_steps: Vec<MutationStep>,
    pub safety_checks: Vec<SafetyCheck>,
}

/// Short-lived runtime container representing the live transition phase
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionExecution {
    pub plan: TransitionPlan,
    pub state: TransitionState,
}

impl TransitionPlan {
    pub fn new() -> Self {
        Self {
            plan_id: Uuid::new_v4(),
            steps: vec![],
            rollback_steps: vec![],
            safety_checks: vec![],
        }
    }
}
