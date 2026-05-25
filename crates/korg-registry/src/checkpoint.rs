use crate::log::HlcTimestamp;
use crate::types::{CapabilityLease, CapabilityState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct CheckpointMetadata {
    pub(crate) checkpoint_id: Uuid,
    pub(crate) parent_checkpoint_id: Option<Uuid>,
    pub(crate) branch_id: Option<Uuid>,
    pub(crate) created_at: HlcTimestamp,
    pub(crate) evaluated_entropy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ExecutionCheckpoint {
    pub(crate) checkpoint_version: u32,
    pub(crate) metadata: CheckpointMetadata,
    pub(crate) ledger_offset: u64,
    pub(crate) workspace_snapshot: String,
    pub(crate) projection_snapshot: serde_json::Value,
    pub(crate) lease_map: HashMap<String, CapabilityLease>,
    pub(crate) active_states: HashMap<String, CapabilityState>,
    pub(crate) cryptographic_attestation: Option<String>,
}
