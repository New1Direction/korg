// Wire types shared between the orchestration engine and the operator TUI.
// Lives in korg-runtime so that leader/workers/agent can reference them
// without depending on the TUI crate.

/// Events sent from the TUI back to the LeaderOrchestrator.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ContractResponse {
    Approve,
    Reject,
    Force,
    Override(Vec<String>),
    /// User selected a rewind candidate; seq_id is the target journal position.
    Rewind(u64),
}

/// Events pushed by the LeaderOrchestrator to the live operator TUI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TuiUpdate {
    Verdict {
        text: String,
        rubrics: Vec<(String, bool)>,
        h_sem: f32,
        velocity: f32,
        risk: f32,
        progress: f32,
        doom_prob: f32,
    },
    Arena {
        round: usize,
        winner: String,
        mutations: usize,
    },
    Trace(String),
    Ktrans(String),
    ApprovalRequest(String),
    Compaction(String),
    ContractNegotiated {
        description: String,
        criteria: Vec<(String, f32)>,
    },
    ContractApprovalRequest {
        round: usize,
        description: String,
        criteria: Vec<(String, f32)>,
    },
    PersonaTelemetry {
        scores: [f32; 4],
        telemetry_merges: u32,
        crdt_sync_frequency: f32,
        conflicts_count: u32,
        provenance_chain_length: u32,
        lock_states: Vec<(String, String, String, String)>,
    },
    ScaleTelemetry {
        total_tokens: usize,
        avg_latency_ms: u32,
        rotator_hits: u32,
        heals_resolved: u32,
    },
    /// Runtime surfaced rewind options after a doom-loop / failure detection.
    RewindAvailable(Vec<crate::recovery::RewindCandidate>),
}
