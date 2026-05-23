use super::log::HlcTimestamp;
use super::types::{CapabilityLease, CapabilityState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckpointMetadata {
    pub checkpoint_id: Uuid,
    pub parent_checkpoint_id: Option<Uuid>,
    pub branch_id: Option<Uuid>,
    pub created_at: HlcTimestamp,
    pub evaluated_entropy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionCheckpoint {
    pub checkpoint_version: u32,
    pub metadata: CheckpointMetadata,
    pub ledger_offset: u64,
    pub workspace_snapshot: String,
    pub projection_snapshot: serde_json::Value,
    pub lease_map: HashMap<String, CapabilityLease>,
    pub active_states: HashMap<String, CapabilityState>,
    pub cryptographic_attestation: Option<String>,
}
