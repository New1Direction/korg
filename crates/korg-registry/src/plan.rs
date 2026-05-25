use crate::types::{CapabilityState, EffectNode};
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
pub(crate) struct MutationStep {
    pub(crate) target_id: String,
    pub(crate) previous_state: CapabilityState,
    pub(crate) target_state: CapabilityState,
    pub(crate) effect_nodes: Vec<EffectNode>,
}

/// Strongly typed safety checks to support static analysis and verification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "check_type", content = "payload")]
pub(crate) enum SafetyCheck {
    VerifyDag,
    CheckDependency(String),
    CheckConflict(String),
    RequireCapability(String),
}

/// Immutable plan capturing the declarative transition intent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct TransitionPlan {
    pub(crate) plan_id: Uuid,
    pub(crate) steps: Vec<MutationStep>,
    pub(crate) rollback_steps: Vec<MutationStep>,
    pub(crate) safety_checks: Vec<SafetyCheck>,
}

/// Short-lived runtime container representing the live transition phase
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TransitionExecution {
    pub(crate) plan: TransitionPlan,
    pub(crate) state: TransitionState,
}

impl TransitionPlan {
    pub(crate) fn new() -> Self {
        Self {
            plan_id: Uuid::new_v4(),
            steps: vec![],
            rollback_steps: vec![],
            safety_checks: vec![],
        }
    }
}
