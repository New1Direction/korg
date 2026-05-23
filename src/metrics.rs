//! Korg Metrics — Atomic Runtime Counters
//!
//! Provides lightweight, lock-free counters for system-level observability.
//! All metrics are global atomics — zero allocation per record call.
//!
//! # Exposed via API
//!
//! ```text
//! GET /api/metrics → { campaigns_started: 3, transitions_applied: 47, ... }
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use serde::Serialize;

// =========================================================================
// Global Atomic Counters
// =========================================================================

static CAMPAIGNS_STARTED:         AtomicU64 = AtomicU64::new(0);
static CAMPAIGNS_COMPLETED:       AtomicU64 = AtomicU64::new(0);
static CAMPAIGN_ROUNDS_TOTAL:     AtomicU64 = AtomicU64::new(0);
static TRANSITIONS_APPLIED:       AtomicU64 = AtomicU64::new(0);
static TRANSITIONS_REJECTED:      AtomicU64 = AtomicU64::new(0);
static TRANSITIONS_FAILED:        AtomicU64 = AtomicU64::new(0);
static WORKER_TIMEOUTS:           AtomicU64 = AtomicU64::new(0);
static EVALUATOR_VERDICTS:        AtomicU64 = AtomicU64::new(0);
static DOOM_LOOPS_DETECTED:       AtomicU64 = AtomicU64::new(0);
static KTRANS_PERSISTED:          AtomicU64 = AtomicU64::new(0);
static VISION_POLICY_REDACTED:    AtomicU64 = AtomicU64::new(0);
static VISION_POLICY_BLOCKED:     AtomicU64 = AtomicU64::new(0);
static LLM_REQUESTS_TOTAL:        AtomicU64 = AtomicU64::new(0);
static LLM_REQUESTS_FAILED:       AtomicU64 = AtomicU64::new(0);
static AGENT_TOOL_INVOCATIONS:    AtomicU64 = AtomicU64::new(0);
static SSE_EVENTS_BROADCAST:      AtomicU64 = AtomicU64::new(0);
static WORKERS_COMPLETED:         AtomicU64 = AtomicU64::new(0);
static WORKERS_CRASHED:           AtomicU64 = AtomicU64::new(0);
static WORKSPACES_CREATED:        AtomicU64 = AtomicU64::new(0);
static WORKSPACES_COMPLETED:      AtomicU64 = AtomicU64::new(0);
static WORKSPACES_DESTROYED:      AtomicU64 = AtomicU64::new(0);

// =========================================================================
// Record Functions (call sites in hot paths)
// =========================================================================

#[inline]
pub fn record_campaign_started() {
    CAMPAIGNS_STARTED.fetch_add(1, Ordering::Relaxed);
    tracing::info!(counter = "campaigns_started", "campaign_started");
}

#[inline]
pub fn record_campaign_completed() {
    CAMPAIGNS_COMPLETED.fetch_add(1, Ordering::Relaxed);
    tracing::info!(counter = "campaigns_completed", "campaign_completed");
}

#[inline]
pub fn record_campaign_round(round: usize, winner: &str, action: &str) {
    CAMPAIGN_ROUNDS_TOTAL.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        counter = "campaign_rounds",
        round,
        arena_winner = winner,
        leader_action = action,
        "campaign_round_complete"
    );
}

#[inline]
pub fn record_transition_applied(capability_id: &str) {
    TRANSITIONS_APPLIED.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(
        counter = "transitions_applied",
        capability_id,
        "capability_transition_applied"
    );
}

#[inline]
pub fn record_transition_rejected(capability_id: &str, reason: &str) {
    TRANSITIONS_REJECTED.fetch_add(1, Ordering::Relaxed);
    tracing::warn!(
        counter = "transitions_rejected",
        capability_id,
        reason,
        "capability_transition_rejected"
    );
}

#[inline]
pub fn record_transition_failed(capability_id: &str, error: &str) {
    TRANSITIONS_FAILED.fetch_add(1, Ordering::Relaxed);
    tracing::error!(
        counter = "transitions_failed",
        capability_id,
        error,
        "capability_transition_failed"
    );
}

#[inline]
pub fn record_worker_timeout(worker_id: &str) {
    WORKER_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
    tracing::warn!(
        counter = "worker_timeouts",
        worker_id,
        "worker_timeout"
    );
}

#[inline]
pub fn record_evaluator_verdict(overall: &str, doom: bool, entropy: f32) {
    EVALUATOR_VERDICTS.fetch_add(1, Ordering::Relaxed);
    if doom {
        DOOM_LOOPS_DETECTED.fetch_add(1, Ordering::Relaxed);
    }
    tracing::info!(
        counter = "evaluator_verdicts",
        overall,
        doom_loop_detected = doom,
        semantic_entropy = entropy,
        "evaluator_verdict"
    );
}

#[inline]
pub fn record_ktrans_persisted(round: usize) {
    KTRANS_PERSISTED.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(counter = "ktrans_persisted", round, "ktrans_persisted");
}

#[inline]
pub fn record_vision_policy_redacted() {
    VISION_POLICY_REDACTED.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_vision_policy_blocked() {
    VISION_POLICY_BLOCKED.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_llm_request() {
    LLM_REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_llm_failure(provider: &str, status: u16) {
    LLM_REQUESTS_FAILED.fetch_add(1, Ordering::Relaxed);
    tracing::warn!(
        counter = "llm_failures",
        provider,
        http_status = status,
        "llm_request_failed"
    );
}

#[inline]
pub fn record_agent_tool_invocation(tool_name: &str) {
    AGENT_TOOL_INVOCATIONS.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(counter = "tool_invocations", tool_name, "agent_tool_invoked");
}

#[inline]
pub fn record_sse_event() {
    SSE_EVENTS_BROADCAST.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn record_worker_completed(persona: &str) {
    WORKERS_COMPLETED.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(counter = "workers_completed", persona, "worker_completed");
}

#[inline]
pub fn record_worker_crashed(persona: &str) {
    WORKERS_CRASHED.fetch_add(1, Ordering::Relaxed);
    tracing::warn!(counter = "workers_crashed", persona, "worker_crashed");
}

#[inline]
pub fn record_workspace_created(persona: &str) {
    WORKSPACES_CREATED.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(counter = "workspaces_created", persona, "workspace_created");
}

#[inline]
pub fn record_workspace_completed(persona: &str, exit_ok: bool) {
    WORKSPACES_COMPLETED.fetch_add(1, Ordering::Relaxed);
    tracing::info!(counter = "workspaces_completed", persona, exit_ok, "workspace_completed");
}

#[inline]
pub fn record_workspace_destroyed(persona: &str) {
    WORKSPACES_DESTROYED.fetch_add(1, Ordering::Relaxed);
    tracing::debug!(counter = "workspaces_destroyed", persona, "workspace_destroyed");
}

// =========================================================================
// Snapshot (for /api/metrics)
// =========================================================================

/// A point-in-time snapshot of all runtime metrics.
/// Serializes cleanly to JSON for the `/api/metrics` endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub campaigns_started: u64,
    pub campaigns_completed: u64,
    pub campaign_rounds_total: u64,
    pub transitions_applied: u64,
    pub transitions_rejected: u64,
    pub transitions_failed: u64,
    pub worker_timeouts: u64,
    pub evaluator_verdicts: u64,
    pub doom_loops_detected: u64,
    pub ktrans_persisted: u64,
    pub vision_policy_redacted: u64,
    pub vision_policy_blocked: u64,
    pub llm_requests_total: u64,
    pub llm_requests_failed: u64,
    pub agent_tool_invocations: u64,
    pub sse_events_broadcast: u64,
    pub workers_completed: u64,
    pub workers_crashed: u64,
    pub workspaces_created: u64,
    pub workspaces_completed: u64,
    pub workspaces_destroyed: u64,
}

/// Collect a point-in-time snapshot of all metrics. Lock-free.
pub fn snapshot() -> MetricsSnapshot {
    MetricsSnapshot {
        campaigns_started:      CAMPAIGNS_STARTED.load(Ordering::Relaxed),
        campaigns_completed:    CAMPAIGNS_COMPLETED.load(Ordering::Relaxed),
        campaign_rounds_total:  CAMPAIGN_ROUNDS_TOTAL.load(Ordering::Relaxed),
        transitions_applied:    TRANSITIONS_APPLIED.load(Ordering::Relaxed),
        transitions_rejected:   TRANSITIONS_REJECTED.load(Ordering::Relaxed),
        transitions_failed:     TRANSITIONS_FAILED.load(Ordering::Relaxed),
        worker_timeouts:        WORKER_TIMEOUTS.load(Ordering::Relaxed),
        evaluator_verdicts:     EVALUATOR_VERDICTS.load(Ordering::Relaxed),
        doom_loops_detected:    DOOM_LOOPS_DETECTED.load(Ordering::Relaxed),
        ktrans_persisted:       KTRANS_PERSISTED.load(Ordering::Relaxed),
        vision_policy_redacted: VISION_POLICY_REDACTED.load(Ordering::Relaxed),
        vision_policy_blocked:  VISION_POLICY_BLOCKED.load(Ordering::Relaxed),
        llm_requests_total:     LLM_REQUESTS_TOTAL.load(Ordering::Relaxed),
        llm_requests_failed:    LLM_REQUESTS_FAILED.load(Ordering::Relaxed),
        agent_tool_invocations: AGENT_TOOL_INVOCATIONS.load(Ordering::Relaxed),
        sse_events_broadcast:   SSE_EVENTS_BROADCAST.load(Ordering::Relaxed),
        workers_completed:      WORKERS_COMPLETED.load(Ordering::Relaxed),
        workers_crashed:        WORKERS_CRASHED.load(Ordering::Relaxed),
        workspaces_created:     WORKSPACES_CREATED.load(Ordering::Relaxed),
        workspaces_completed:   WORKSPACES_COMPLETED.load(Ordering::Relaxed),
        workspaces_destroyed:   WORKSPACES_DESTROYED.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_snapshot_is_monotonic() {
        let before = snapshot();
        record_campaign_started();
        record_transition_applied("cognition_mode");
        record_transition_rejected("docker_sandbox", "dependency not met");
        let after = snapshot();

        assert!(after.campaigns_started >= before.campaigns_started + 1);
        assert!(after.transitions_applied >= before.transitions_applied + 1);
        assert!(after.transitions_rejected >= before.transitions_rejected + 1);
    }

    #[test]
    fn test_metrics_serialize_to_json() {
        let snap = snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("campaigns_started"));
        assert!(json.contains("transitions_applied"));
    }
}
