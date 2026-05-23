use serde::{Deserialize, Serialize};

/// Governs the active intelligence tier of the cognitive swarm.
/// Defined here (in the registry kernel) as the single authoritative source of this state.
/// All layers (leader, web, CLI) must read and mutate this exclusively through the CapabilityResolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CognitionMode {
    Instant,
    Balanced,
    Heavy,
    Research,
    Recovery,
    Autonomous,
    HeavyConsciousness,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Category {
    Runtime,
    Security,
    Observability,
    Execution,
    /// Governs cognitive and knowledge-building capabilities:
    /// embeddings, code indexer, reconcile/synthesize modes.
    Intelligence,
}

/// Strictly typed capability value representations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum CapabilityState {
    Disabled,
    Enabled,
    Scaled(f32),
    Mode(String),
    Structured(serde_json::Value),
}

/// Authoritative time-bound capability lease lock
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityLease {
    pub owner_id: uuid::Uuid,
    pub acquired_at: chrono::DateTime<chrono::Utc>,
    pub duration_secs: u64,
}

/// Dynamic projections to external surfaces
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectionMap {
    pub cli_flag: Option<String>,
    pub lsp_command: Option<String>,
    pub sdk_config_path: String,
    pub ui_toggle_id: Option<String>,
    pub ui_group: String,
}

/// The core node of the Capability DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityNode {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    pub default_state: CapabilityState,
    pub dependencies: Vec<String>,
    pub conflicts: Vec<String>,
    pub projections: ProjectionMap,
}

/// Strongly typed execution effects to avoid side-effect ambiguity
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "effect_type", content = "payload")]
pub enum CapabilityEffect {
    SpawnAgent {
        agent_type: String,
    },
    KillAgent {
        id: String,
    },
    ModifyGraph {
        node: String,
    },
    StartSwarm {
        swarm_id: String,
    },
    StopSwarm {
        swarm_id: String,
    },
    ExecuteTool {
        tool: String,
    },
    InitializeSandbox {
        container_name: String,
        memory_limit_mb: usize,
    },
    TeardownSandbox {
        container_name: String,
    },
}

/// The individual node in the transactional Effect DAG
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectNode {
    pub id: usize,
    pub effect: CapabilityEffect,
    /// Indexes of other EffectNodes within this step that must complete before this runs
    pub depends_on: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionRequest {
    pub id: String,
    pub target_state: CapabilityState,
    pub correlation_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionResponse {
    pub plan_id: uuid::Uuid,
    pub status: super::plan::TransitionState,
    pub errors: Vec<String>,
}

impl CapabilityState {
    /// Convert an active `CapabilityState` into a strongly-typed `CognitionMode`.
    /// Returns `CognitionMode::Balanced` as the safe default for unrecognised mode strings
    /// or non-Mode capability variants.
    pub fn as_cognition_mode(&self) -> CognitionMode {
        match self {
            CapabilityState::Mode(s) => match s.to_lowercase().as_str() {
                "instant" => CognitionMode::Instant,
                "heavy" => CognitionMode::Heavy,
                "research" => CognitionMode::Research,
                "recovery" => CognitionMode::Recovery,
                "autonomous" => CognitionMode::Autonomous,
                "heavy-consciousness" | "consciousness" => CognitionMode::HeavyConsciousness,
                _ => CognitionMode::Balanced,
            },
            _ => CognitionMode::Balanced,
        }
    }
}
