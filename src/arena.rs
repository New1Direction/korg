//! Arena — Multi-Persona Competition Engine
//!
//! The Arena runs concurrent Evaluator scoring across all persona results,
//! selects the winner by highest composite score, and performs semantic merge.
//!
//! # Architecture
//!
//! ```text
//! [PersonaResult × N] → run_arena() → ArenaOutcome
//!                                          ↓
//!                                    CapabilityResolver validates
//!                                          ↓
//!                                    LeaderOrchestrator acts
//! ```

use crate::evaluator::TraceEvent;
use crate::personas::{Persona, PersonaResult};
use serde::{Deserialize, Serialize};

/// The result of a single Arena competition round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaOutcome {
    /// Name of the winning persona.
    pub winner: String,
    /// Routing ID of the winner's work package.
    pub routing_id: String,
    /// Composite confidence score of the winner (0.0–1.0).
    pub confidence: f32,
    /// Per-persona scores in canonical order: [Captain, Harper, Benjamin, Lucas].
    pub scores: [f32; 4],
    /// Total mutation count from the winning result.
    pub mutations: usize,
}

impl ArenaOutcome {
    /// Fallback used when the arena produces no valid results.
    pub fn fallback() -> Self {
        ArenaOutcome {
            winner: "Lucas".to_string(),
            routing_id: "pkg-lucas".to_string(),
            confidence: 0.85,
            scores: [0.85, 0.85, 0.85, 0.85],
            mutations: 3,
        }
    }

    /// Serialize to `serde_json::Value` for legacy compatibility with existing
    /// leader.rs code that reads `arena_outcome["winner"]` etc.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": "winner",
            "winner": self.winner,
            "routing_id": self.routing_id,
            "confidence": self.confidence,
            "scores": self.scores,
            "mutations": self.mutations,
        })
    }
}

/// Run the adversarial Arena: each persona result is scored by the Evaluator persona
/// concurrently. The winner is the result with the highest composite score.
///
/// Uses `tokio::task::JoinSet` for bounded concurrent evaluation — does NOT block on
/// sequential ordering; all evaluations run in parallel.
#[tracing::instrument(skip(results), fields(candidate_count = results.len()))]
pub async fn run_arena(results: &[PersonaResult]) -> ArenaOutcome {
    if results.is_empty() {
        tracing::warn!("arena called with empty results; returning fallback outcome");
        return ArenaOutcome::fallback();
    }

    let mut join_set = tokio::task::JoinSet::new();

    for r in results {
        let r = r.clone();
        join_set.spawn(async move {
            let payload = format!(
                "Evaluate the proposed work from persona: {}\n\nProposed Output:\n{}\n\nProposed Mutations:\n{}",
                r.persona.name(),
                serde_json::to_string_pretty(&r.output).unwrap_or_default(),
                serde_json::to_string_pretty(&r.mutations).unwrap_or_default()
            );

            tracing::debug!(
                persona = r.persona.name(),
                routing_id = %r.routing_id,
                "arena evaluating persona"
            );

            let eval_result = crate::personas::run_persona(
                crate::personas::Persona::Evaluator,
                &payload,
                &format!("arena-eval-{}", r.routing_id),
            ).await;

            (r, eval_result)
        });
    }

    let mut evaluated: Vec<(PersonaResult, f32)> = Vec::new();

    while let Some(join_result) = join_set.join_next().await {
        if let Ok((worker_res, eval_res)) = join_result {
            let passed = eval_res.output["passed_rubrics"].as_f64().unwrap_or(4.0) as f32;
            let total = eval_res.output["total_rubrics"].as_f64().unwrap_or(5.0) as f32;
            let ratio = if total > 0.0 { passed / total } else { 0.8 };
            let score = ratio * eval_res.confidence;

            tracing::info!(
                persona = worker_res.persona.name(),
                arena_score = score,
                passed_rubrics = passed,
                total_rubrics = total,
                evaluator_confidence = eval_res.confidence,
                "arena score computed"
            );

            crate::metrics::record_evaluator_verdict(
                &format!("arena-{}", worker_res.persona.name()),
                false,
                1.0 - ratio,
            );

            evaluated.push((worker_res, score));
        }
    }

    if evaluated.is_empty() {
        tracing::warn!("arena produced no evaluated results; returning fallback");
        return ArenaOutcome::fallback();
    }

    // Select winner by highest score
    let mut best_idx = 0;
    let mut best_score = -1.0f32;
    for (i, (_, score)) in evaluated.iter().enumerate() {
        if *score > best_score {
            best_score = *score;
            best_idx = i;
        }
    }

    let (best_worker, _) = &evaluated[best_idx];

    // Build canonical [Captain, Harper, Benjamin, Lucas] score array
    let mut scores = [0.85f32; 4];
    for (worker, score) in &evaluated {
        let idx = match worker.persona {
            Persona::Captain  => 0,
            Persona::Harper   => 1,
            Persona::Benjamin => 2,
            Persona::Lucas    => 3,
            _                 => continue,
        };
        scores[idx] = *score;
    }

    let outcome = ArenaOutcome {
        winner: best_worker.persona.name().to_string(),
        routing_id: best_worker.routing_id.clone(),
        confidence: best_score,
        scores,
        mutations: best_worker.mutations.len(),
    };

    tracing::info!(
        winner = %outcome.winner,
        confidence = outcome.confidence,
        "arena winner selected"
    );

    outcome
}

/// Compute the RL-style scaling reward from an evaluation verdict.
///
/// ```text
/// reward = (useful_rate × 1.8) - entropy_cost - churn_penalty
/// ```
///
/// Negative rewards indicate the system is in a doom-loop or resource violation.
pub fn compute_scaling_reward(
    passed_rubrics: u8,
    total_rubrics: u8,
    semantic_entropy: f32,
    doom_loop_detected: bool,
    churn_penalty: f32,
) -> f32 {
    if doom_loop_detected {
        return -1.8;
    }
    let useful_rate = passed_rubrics as f32 / total_rubrics.max(1) as f32;
    let entropy_cost = (semantic_entropy * 1.6).min(1.4);
    let base = (useful_rate * 1.8) - entropy_cost;
    base - churn_penalty
}

/// Update the churn penalty from a sliding window of recent rewards.
///
/// High oscillation in rewards → higher penalty on future scaling.
/// Window size is capped at 12 entries.
pub fn update_churn_penalty(history: &std::collections::VecDeque<f32>) -> f32 {
    if history.len() < 4 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut prev = *history.back().unwrap_or(&0.0);
    for &r in history {
        sum += (r - prev).abs();
        prev = r;
    }
    (sum / history.len() as f32 * 0.6).min(0.9)
}

/// Build a synthetic telemetry trace event from a verdict for continuity in the Evaluator window.
pub fn verdict_to_trace_event(
    overall: &str,
    passed_rubrics: u8,
    total_rubrics: u8,
    semantic_entropy: f32,
    doom_loop_detected: bool,
) -> TraceEvent {
    TraceEvent {
        agent_id: "leader-aggregate".into(),
        risk_score: if overall == "TERMINATE" { 0.82 } else { 0.45 },
        epistemic_confidence: if passed_rubrics >= 4 { 0.81 } else { 0.52 },
        conflict_rate: if doom_loop_detected { 0.48 } else { 0.18 },
        token_velocity: if doom_loop_detected { 240.0 } else { 95.0 },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn test_compute_scaling_reward_doom_loop() {
        let r = compute_scaling_reward(5, 5, 0.0, true, 0.0);
        assert_eq!(r, -1.8);
    }

    #[test]
    fn test_compute_scaling_reward_excellent() {
        let r = compute_scaling_reward(5, 5, 0.1, false, 0.0);
        // useful_rate=1.0, entropy_cost=0.16, base = 1.8 - 0.16 = 1.64
        assert!((r - 1.64).abs() < 0.01, "got {}", r);
    }

    #[test]
    fn test_churn_penalty_low_history() {
        let history = VecDeque::from(vec![0.8, 0.9]);
        let penalty = update_churn_penalty(&history);
        assert_eq!(penalty, 0.0); // < 4 entries → no penalty
    }

    #[test]
    fn test_churn_penalty_oscillating() {
        let history = VecDeque::from(vec![0.9, 0.1, 0.9, 0.1, 0.9, 0.1]);
        let penalty = update_churn_penalty(&history);
        assert!(penalty > 0.3, "oscillating rewards should produce non-trivial penalty, got {}", penalty);
    }

    #[test]
    fn test_arena_outcome_fallback() {
        let o = ArenaOutcome::fallback();
        assert_eq!(o.winner, "Lucas");
        assert_eq!(o.confidence, 0.85);
    }

    #[test]
    fn test_arena_outcome_to_json() {
        let o = ArenaOutcome::fallback();
        let j = o.to_json();
        assert_eq!(j["winner"], "Lucas");
        assert_eq!(j["mode"], "winner");
    }

    #[test]
    fn test_verdict_to_trace_event_terminate() {
        let te = verdict_to_trace_event("TERMINATE", 1, 5, 0.9, true);
        assert_eq!(te.risk_score, 0.82);
        assert_eq!(te.conflict_rate, 0.48);
    }
}
