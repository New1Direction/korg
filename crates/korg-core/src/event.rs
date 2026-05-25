use serde::{Deserialize, Serialize};

/// Content-addressed reference for large payloads.
/// Store the payload outside the ledger; record only the digest + size here.
/// This keeps the ledger lightweight and replayable without carrying large blobs.
///
/// Defined in korg-core so that both korg-registry and the Adapter trait can
/// reference it without a circular dependency.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentRef {
    pub sha256: String,
    pub size_bytes: u64,
    pub label: String,
}

/// Normalized adapter output — the intake payload the ledger accepts.
///
/// This is what every Adapter::normalize() must produce regardless of wire format.
/// korg-runtime wraps it in a CapabilityEvent before appending to the journal.
///
/// Field names intentionally mirror AgentToolCallRequest in web.rs so that
/// the migration from private struct to public type is a rename-and-move, not
/// a redesign.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedEvent {
    /// Agent runtime identity. Convention: "agent:<name>@<version>" or "human:<id>".
    pub source_agent: String,
    /// Name of the tool called. Should match the agent's own tool registry name.
    pub tool_name: String,
    /// Tool arguments. Large values should be content-addressed via payload_refs.
    pub args: serde_json::Value,
    /// Tool result. Large values should be content-addressed via payload_refs.
    pub result: serde_json::Value,
    /// Content-addressed references for large payloads.
    #[serde(default)]
    pub payload_refs: Vec<ContentRef>,
    /// Whether the tool call succeeded.
    pub success: bool,
    /// Wall-clock duration of the tool call in milliseconds.
    pub duration_ms: u64,
    /// seq_id of the causally triggering event. None for root events (user_prompt).
    #[serde(default)]
    pub triggered_by: Option<u64>,
}
