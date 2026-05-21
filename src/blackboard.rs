//! Blackboard — the persistent, versioned LWW-Element-Set CRDT surface for the harness.
//!
//! Primary new responsibility for this increment:
//!   - Ingest real SwarmTelemetryPulse messages (from workers over ACP stdio)
//!   - Map them into TraceEvent records the Evaluator can consume
//!   - Provide drain hooks so the Leader can feed genuine swarm data into the 5-rubric Evaluator
//!
//! This is the bridge that makes the Evaluator operate on live telemetry instead of synthetic events.

use crate::acp::AcpMessage;
use crate::evaluator::TraceEvent;
use chrono::Utc;
use std::collections::VecDeque;
use uuid::Uuid;

/// Simple but real blackboard that the Leader owns (upgrade path to full CRDT + vector clocks later).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Blackboard {
    /// Ring buffer of recent normalized telemetry observations.
    /// The Evaluator's window is fed from here.
    pub trace_buffer: VecDeque<TraceEvent>,

    /// Raw recent pulses (for debugging / .ktrans provenance).
    pub recent_pulses: VecDeque<serde_json::Value>,

    /// Session this blackboard belongs to.
    pub session_id: Uuid,
}

impl Default for Blackboard {
    fn default() -> Self {
        Self::new(Uuid::now_v7())
    }
}

impl Blackboard {
    pub fn new(session_id: Uuid) -> Self {
        Self {
            trace_buffer: VecDeque::with_capacity(64),
            recent_pulses: VecDeque::with_capacity(32),
            session_id,
        }
    }

    /// The core mapping the user requested.
    /// Takes a SwarmTelemetryPulse (the ACP message) and returns the derived TraceEvent(s).
    /// Supports both the flat per_agent shape and the richer aggregate+per_agent form from the spec.
    pub fn ingest_telemetry_pulse(&mut self, pulse: &AcpMessage) -> Vec<TraceEvent> {
        let mut events = vec![];

        if let AcpMessage::SwarmTelemetryPulse {
            per_agent,
            aggregate,
            ..
        } = pulse
        {
            // Primary path: per_agent usually contains the detailed agent metrics
            if let Some(agent_obj) = per_agent.as_object() {
                for (agent_id, metrics) in agent_obj {
                    if let Some(te) = self.metrics_to_trace_event(agent_id, metrics) {
                        events.push(te);
                    }
                }
            }

            // Also try to extract a leader / aggregate view if present
            if let Some(agg) = aggregate.as_object() {
                if let Some(te) = self
                    .metrics_to_trace_event("aggregate", &serde_json::Value::Object(agg.clone()))
                {
                    // Mark it clearly
                    let mut agg_te = te;
                    agg_te.agent_id = "swarm-aggregate".to_string();
                    events.push(agg_te);
                }
            }

            // Fallback: if the pulse was sent in a flatter shape (common in early workers)
            if events.is_empty() {
                if let Some(te) = self.metrics_to_trace_event("unknown-agent", per_agent) {
                    events.push(te);
                }
            }
        }

        // Store raw pulse for provenance
        if let Ok(raw) = serde_json::to_value(pulse) {
            if self.recent_pulses.len() >= 32 {
                self.recent_pulses.pop_front();
            }
            self.recent_pulses.push_back(raw);
        }

        // Append to the ring buffer the Evaluator will read
        for event in &events {
            if self.trace_buffer.len() >= 64 {
                self.trace_buffer.pop_front();
            }
            self.trace_buffer.push_back(event.clone());
        }

        events
    }

    /// Robust extractor that turns arbitrary JSON metrics (from a pulse) into a TraceEvent.
    /// Handles missing fields gracefully with Heavy-Tier defaults.
    fn metrics_to_trace_event(&self, agent_id: &str, m: &serde_json::Value) -> Option<TraceEvent> {
        // Common field names we expect from the spec + worker emission
        let risk = m
            .get("risk_score")
            .and_then(|v| v.as_f64())
            .or_else(|| m.get("risk").and_then(|v| v.as_f64()))
            .unwrap_or(0.35) as f32;

        let conf = m
            .get("epistemic_confidence")
            .and_then(|v| v.as_f64())
            .or_else(|| m.get("confidence").and_then(|v| v.as_f64()))
            .unwrap_or(0.72) as f32;

        let conflict = m
            .get("conflict_rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.12) as f32;

        let velocity = m
            .get("token_velocity")
            .and_then(|v| v.as_f64())
            .or_else(|| m.get("tokens_per_sec").and_then(|v| v.as_f64()))
            .unwrap_or(70.0) as f32;

        let gpu = m
            .get("gpu_util")
            .and_then(|v| v.as_f64())
            .or_else(|| m.get("gpu").and_then(|v| v.as_f64()))
            .unwrap_or(0.45) as f32;

        let verified = m
            .get("verified_count_delta")
            .and_then(|v| v.as_i64())
            .or_else(|| m.get("verified_delta").and_then(|v| v.as_i64()))
            .unwrap_or(1) as i32;

        let authority = m
            .get("authority_improvement")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.11) as f32;

        let surface = m
            .get("surface_text")
            .and_then(|v| v.as_str())
            .or_else(|| m.get("last_action").and_then(|v| v.as_str()))
            .unwrap_or("agent emitted telemetry pulse")
            .to_string();

        let content_hash = m
            .get("content_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("sha256:telemetry")
            .to_string();

        Some(TraceEvent {
            agent_id: agent_id.to_string(),
            timestamp: Utc::now().to_rfc3339(),
            risk_score: risk.clamp(0.0, 1.0),
            epistemic_confidence: conf.clamp(0.0, 1.0),
            conflict_rate: conflict.clamp(0.0, 1.0),
            token_velocity: velocity.max(0.0),
            gpu_util: gpu.clamp(0.0, 1.0),
            verified_count_delta: verified,
            authority_improvement: authority.clamp(-1.0, 1.0),
            semantic_entropy_raw: m
                .get("semantic_entropy")
                .and_then(|v| v.as_f64())
                .map(|x| x as f32),
            content_hash,
            ast_delta_hash: m
                .get("ast_delta_hash")
                .and_then(|v| v.as_str())
                .unwrap_or("sha256:delta")
                .to_string(),
            surface_text: surface,
        })
    }

    /// Drains newly arrived events for the Evaluator.
    /// The Leader typically calls this right before running an evaluation pass.
    pub fn drain_new_trace_events(&mut self) -> Vec<TraceEvent> {
        self.trace_buffer.drain(..).collect()
    }

    /// Returns a view of the current window (non-consuming) — useful for the Evaluator if it wants to peek.
    pub fn current_window(&self) -> Vec<TraceEvent> {
        self.trace_buffer.iter().cloned().collect()
    }

    /// Convenience: feed a batch of events directly (used by Leader when it has synthetic or external data).
    pub fn ingest_trace_events(&mut self, events: Vec<TraceEvent>) {
        for event in events {
            if self.trace_buffer.len() >= 64 {
                self.trace_buffer.pop_front();
            }
            self.trace_buffer.push_back(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::AcpMessage;

    #[test]
    fn pulse_ingestion_produces_real_trace_events() {
        let mut bb = Blackboard::new(Uuid::now_v7());

        let pulse = AcpMessage::SwarmTelemetryPulse {
            agent_id: "benjamin-pkg-01".into(),
            per_agent: serde_json::json!({
                "benjamin-pkg-01": {
                    "risk_score": 0.61,
                    "epistemic_confidence": 0.49,
                    "conflict_rate": 0.29,
                    "token_velocity": 165.0,
                    "gpu_util": 0.78,
                    "verified_count_delta": 0,
                    "authority_improvement": 0.04,
                    "surface_text": "tried three different approaches, none verified yet"
                }
            }),
            aggregate: serde_json::json!({}),
            scaling_recommendation: None,
        };

        let events = bb.ingest_telemetry_pulse(&pulse);
        assert!(!events.is_empty());
        let first = &events[0];
        assert_eq!(first.agent_id, "benjamin-pkg-01");
        assert!(first.risk_score > 0.5);
        assert!(first.token_velocity > 100.0);
        assert!(first.verified_count_delta == 0);
    }
}
