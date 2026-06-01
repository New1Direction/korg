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

/// Lifecycle phase of a single worker, as a structured signal (not a display
/// string). Emitted alongside the human-readable `TuiUpdate::Trace` lines so the
/// operator TUI can build a live leader → worker tree from real state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WorkerLifecycle {
    /// Worker is being spawned (process/handshake in flight).
    Spawning,
    /// Worker is actively running.
    Running,
    /// Worker completed successfully (or was self-healed).
    Ok,
    /// Worker process crashed — queued for recovery.
    Crashed,
    /// Worker exceeded the timeout budget.
    TimedOut,
    /// Worker failed to spawn at all.
    SpawnError,
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
    /// Structured worker-lifecycle signal feeding the live swarm tree. Emitted at
    /// the same real lifecycle points as the worker `Trace` lines — never parsed
    /// from a display string, never fabricated.
    WorkerState {
        node_id: String,
        persona: String,
        state: WorkerLifecycle,
        elapsed_ms: u64,
    },
}
