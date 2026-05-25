use korg_registry::log::{CapabilityEvent, CapabilityJournal};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RewindScope {
    LocalUndo,
    StrategicReset,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RewindCandidate {
    pub seq_id: u64,
    pub rationale: String,
    pub scope: RewindScope,
    /// Seq IDs that will be removed when this rewind executes. Populated now for v0.2 invalidation preview.
    pub invalidates: Vec<u64>,
}

/// Compute recovery candidates from the journal at the moment of failure.
///
/// Returns up to two candidates ordered from surgical to broad:
///   1. LocalUndo  — rewind to the last successful tool call before failure
///   2. StrategicReset — rewind to before the root of the triggered_by causal chain
pub fn rewind_candidates(journal: &CapabilityJournal, failure_seq: u64) -> Vec<RewindCandidate> {
    let mut candidates = Vec::new();

    if let Some(local_seq) = find_last_successful_tool_call(journal, failure_seq) {
        let invalidates: Vec<u64> = ((local_seq + 1)..=failure_seq).collect();
        candidates.push(RewindCandidate {
            seq_id: local_seq,
            rationale: format_local_undo_rationale(journal, local_seq),
            scope: RewindScope::LocalUndo,
            invalidates,
        });
    }

    let chain_root = find_chain_root(journal, failure_seq);
    let reset_target = chain_root.saturating_sub(1);

    let already_covered = candidates.iter().any(|c| c.seq_id == reset_target);
    if !already_covered && reset_target < failure_seq {
        let invalidates: Vec<u64> = ((reset_target + 1)..=failure_seq).collect();
        candidates.push(RewindCandidate {
            seq_id: reset_target,
            rationale: format_strategic_reset_rationale(journal, chain_root),
            scope: RewindScope::StrategicReset,
            invalidates,
        });
    }

    candidates
}

/// Walk backward through events to find the last AgentToolCall that succeeded before failure_seq.
fn find_last_successful_tool_call(journal: &CapabilityJournal, before_seq: u64) -> Option<u64> {
    journal
        .events
        .iter()
        .rev()
        .find(|e| {
            e.seq_id < before_seq
                && matches!(
                    &e.event,
                    CapabilityEvent::AgentToolCall { success: true, .. }
                )
        })
        .map(|e| e.seq_id)
}

/// Follow triggered_by pointers back to the root (the event with no parent).
fn find_chain_root(journal: &CapabilityJournal, from_seq: u64) -> u64 {
    let mut current = from_seq;
    for _ in 0..1000 {
        let parent = journal
            .events
            .iter()
            .find(|e| e.seq_id == current)
            .and_then(|e| e.metadata.triggered_by);
        match parent {
            Some(p) => current = p,
            None => break,
        }
    }
    current
}

fn format_local_undo_rationale(journal: &CapabilityJournal, seq_id: u64) -> String {
    let tool_name = journal
        .events
        .iter()
        .find(|e| e.seq_id == seq_id)
        .and_then(|e| match &e.event {
            CapabilityEvent::AgentToolCall { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        });

    match tool_name {
        Some(name) => format!(
            "Rewind to before the {} call at seq {} — undoes just the failed mutation and retries from your last clean state",
            name, seq_id
        ),
        None => format!(
            "Rewind to seq {} — undoes the immediate failure and retries from the previous checkpoint",
            seq_id
        ),
    }
}

fn format_strategic_reset_rationale(journal: &CapabilityJournal, chain_root: u64) -> String {
    let root_desc = journal
        .events
        .iter()
        .find(|e| e.seq_id == chain_root)
        .map(|e| match &e.event {
            CapabilityEvent::AgentToolCall { tool_name, .. } => format!("{} call", tool_name),
            _ => "planning step".to_string(),
        });

    match root_desc {
        Some(desc) => format!(
            "Rewind to before the {} that started this chain (seq {}) — abandons the current approach entirely and lets the evaluator start fresh",
            desc, chain_root
        ),
        None => format!(
            "Rewind to before the root of the causal chain (seq {}) — abandons the current approach and starts a clean campaign iteration",
            chain_root
        ),
    }
}
