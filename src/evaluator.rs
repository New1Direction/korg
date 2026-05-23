//! Evaluator persona — the harsh adversarial guardrail (5 binary rubrics + semantic entropy).
//!
//! This is the production-grade implementation of the Evaluation-Guardrail-Layer pattern.
//! It consumes TraceEvent (derived from SwarmTelemetryPulse) and produces EvaluationVerdict
//! that the LeaderOrchestrator uses to scale, revise, or terminate.
//!
//! Key polish: `semantic_entropy()` is a first-class method on Evaluator that the five
//! rubric check_* methods call directly. No duplication in evaluate().

use crate::embeddings::{cosine_similarity, EmbeddingModel};
use std::collections::VecDeque;
use uuid::Uuid;

const WINDOW_SIZE: usize = 24; // ~5-30s of 1-5Hz telemetry

static EVAL_SEMAPHORE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

/// Tunable thresholds for the harsh critic (Heavy-Tier defaults).
#[derive(Debug, Clone)]
pub struct RubricConfig {
    pub min_useful_rate: f32, // verified / total in window
    pub max_risk_score: f32,
    pub max_conflict_rate: f32,
    pub min_epistemic_confidence: f32,
    pub max_token_velocity: f32,
    pub max_gpu_util: f32,
    pub entropy_threshold_for_doom: f32, // semantic_entropy above this + other signals → doom
    pub min_authority_improvement: f32,
    pub velocity_drift_threshold: f32,
}

impl Default for RubricConfig {
    fn default() -> Self {
        Self {
            min_useful_rate: 0.35,
            max_risk_score: 0.65,
            max_conflict_rate: 0.25,
            min_epistemic_confidence: 0.55,
            max_token_velocity: 180.0,
            max_gpu_util: 0.92,
            entropy_threshold_for_doom: 0.78,
            min_authority_improvement: 0.08,
            velocity_drift_threshold: 0.45,
        }
    }
}

/// One normalized observation from the swarm (mapped from SwarmTelemetryPulse).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TraceEvent {
    pub agent_id: String,
    pub timestamp: String,
    pub risk_score: f32,
    pub epistemic_confidence: f32,
    pub conflict_rate: f32,
    pub token_velocity: f32,
    pub gpu_util: f32,
    pub verified_count_delta: i32,
    pub authority_improvement: f32,
    pub semantic_entropy_raw: Option<f32>, // optional precomputed from pulse
    pub content_hash: String,
    pub ast_delta_hash: String,
    /// Short surface text we can embed for live semantic_entropy (thoughts, diffs, outputs).
    pub surface_text: String,
}

impl Default for TraceEvent {
    fn default() -> Self {
        Self {
            agent_id: "unknown".into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            risk_score: 0.3,
            epistemic_confidence: 0.7,
            conflict_rate: 0.1,
            token_velocity: 60.0,
            gpu_util: 0.4,
            verified_count_delta: 1,
            authority_improvement: 0.15,
            semantic_entropy_raw: None,
            content_hash: "sha256:deadbeef".into(),
            ast_delta_hash: "sha256:cafebabe".into(),
            surface_text: "agent made incremental progress on the contract".into(),
        }
    }
}

/// Rich verdict returned to the Leader (and written via blackboard + .ktrans).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvaluationVerdict {
    pub verdict_id: Uuid,
    pub session_id: Uuid,
    pub timestamp: String,
    pub overall: String, // "PASS" | "NEEDS_REVISION" | "TERMINATE"
    pub passed_rubrics: u8,
    pub total_rubrics: u8,
    pub justifications: Vec<String>,
    pub recommended_action: String, // "scale_up" | "hold" | "revise" | "terminate_and_rollback"
    pub semantic_entropy: f32,      // the live value used for this evaluation
    pub doom_loop_detected: bool,
    pub productive_death: bool,
}

pub struct Evaluator {
    pub config: RubricConfig,
    window: VecDeque<TraceEvent>,
    embedding_model: std::sync::Arc<dyn EmbeddingModel>,
    embedding_cache: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<f32>>>>,
}

impl Evaluator {
    pub fn new(config: Option<RubricConfig>) -> Self {
        let cfg = config.unwrap_or_default();

        // === Production path: prefer real Candle embeddings for semantic_entropy ===
        // Falls back to FakeEmbeddingModel when:
        //   - `candle` feature is not enabled, or
        //   - model files are not present / HF download fails (CI, quick testing)
        let embedding_model: std::sync::Arc<dyn crate::embeddings::EmbeddingModel> =
            match crate::embeddings::CandleEmbeddingModel::load() {
                Ok(real) => {
                    println!("[Evaluator] Loaded real CandleEmbeddingModel (all-MiniLM-L6-v2)");
                    std::sync::Arc::new(real)
                }
                Err(e) => {
                    println!(
                        "[Evaluator] Using FakeEmbeddingModel (Candle not available: {})",
                        e
                    );
                    std::sync::Arc::new(crate::embeddings::FakeEmbeddingModel::default())
                }
            };

        Self {
            config: cfg,
            window: VecDeque::with_capacity(WINDOW_SIZE),
            embedding_model,
            embedding_cache: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Public API: push a fresh telemetry observation (from blackboard / SwarmTelemetryPulse).
    pub fn ingest(&mut self, event: TraceEvent) {
        if self.window.len() == WINDOW_SIZE {
            self.window.pop_front();
        }
        self.window.push_back(event);
    }

    /// Helper method to score semantic similarity of a candidate text against a reference text.
    pub async fn score_similarity(&self, reference: &str, candidate: &str) -> f32 {
        let ref_emb = {
            let r_cache = self.embedding_cache.read().await;
            r_cache.get(reference).cloned()
        };
        let ref_emb = match ref_emb {
            Some(e) => Some(e),
            None => {
                let model = self.embedding_model.clone();
                let ref_str = reference.to_string();
                let res = tokio::task::spawn_blocking(move || model.embed(&ref_str)).await;
                if let Ok(Ok(e)) = res {
                    let mut w_cache = self.embedding_cache.write().await;
                    w_cache.insert(reference.to_string(), e.clone());
                    Some(e)
                } else {
                    None
                }
            }
        };

        let cand_emb = {
            let r_cache = self.embedding_cache.read().await;
            r_cache.get(candidate).cloned()
        };
        let cand_emb = match cand_emb {
            Some(e) => Some(e),
            None => {
                let model = self.embedding_model.clone();
                let cand_str = candidate.to_string();
                let res = tokio::task::spawn_blocking(move || model.embed(&cand_str)).await;
                if let Ok(Ok(e)) = res {
                    let mut w_cache = self.embedding_cache.write().await;
                    w_cache.insert(candidate.to_string(), e.clone());
                    Some(e)
                } else {
                    None
                }
            }
        };

        if let (Some(r), Some(c)) = (ref_emb, cand_emb) {
            cosine_similarity(&r, &c)
        } else {
            0.3
        }
    }


    /// The key polish requested: semantic_entropy is now a first-class method
    /// that any rubric can call directly. It uses the pluggable embedding model.
    ///
    /// Takes recent surface texts (or falls back to window), embeds them, computes
    /// the pairwise formula from Evaluation-Guardrail-Layer.md:
    ///   H_sem = 1 - (2 / (N*(N-1))) * Σ_{i<j} S_ij
    pub async fn semantic_entropy(&self, texts: Vec<String>) -> f32 {
        if texts.len() < 2 {
            // Not enough signal — treat as low entropy (stable)
            return 0.15;
        }

        let _permit = EVAL_SEMAPHORE.acquire().await.unwrap();
        let start_time = std::time::Instant::now();

        let mut embeddings = Vec::with_capacity(texts.len());
        for t in &texts {
            let cached_opt = {
                let r_cache = self.embedding_cache.read().await;
                r_cache.get(t).cloned()
            };

            if let Some(e) = cached_opt {
                embeddings.push(e);
            } else {
                let model = self.embedding_model.clone();
                let t_clone = t.clone();
                let res = tokio::task::spawn_blocking(move || model.embed(&t_clone)).await;
                if let Ok(Ok(e)) = res {
                    let mut w_cache = self.embedding_cache.write().await;
                    w_cache.insert(t.clone(), e.clone());
                    embeddings.push(e);
                }
            }
        }

        if embeddings.len() < 2 {
            return 0.25;
        }

        // Offload the pairwise calculations to spawn_blocking
        let calculate_res = tokio::task::spawn_blocking(move || {
            let mut sum_sim = 0.0f32;
            let mut pairs = 0usize;

            for i in 0..embeddings.len() {
                for j in (i + 1)..embeddings.len() {
                    let a = &embeddings[i];
                    let b = &embeddings[j];
                    let mut dot = 0.0f32;
                    for (x, y) in a.iter().zip(b.iter()) {
                        dot += x * y;
                    }
                    sum_sim += dot.clamp(-1.0, 1.0);
                    pairs += 1;
                }
            }
            (sum_sim, pairs)
        }).await;

        let h = match calculate_res {
            Ok((sum_sim, pairs)) => {
                if pairs == 0 {
                    0.3
                } else {
                    let avg_sim = sum_sim / (pairs as f32);
                    let val = 1.0 - avg_sim;
                    val.clamp(0.0, 1.0)
                }
            }
            Err(_) => 0.3,
        };

        let duration_ms = start_time.elapsed().as_millis() as u64;
        tracing::info!(
            target: "korg::metrics",
            duration_ms = duration_ms,
            texts_count = texts.len(),
            entropy = h,
            "Semantic Entropy calculated"
        );

        h
    }

    /// Convenience wrapper that uses the current window's surface_text values.
    pub async fn semantic_entropy_from_window(&self) -> f32 {
        let texts: Vec<String> = self.window.iter().map(|e| e.surface_text.clone()).collect();
        self.semantic_entropy(texts).await
    }

    // ---------- The five harsh binary rubrics (self-contained, call semantic_entropy directly) ----------

    fn check_trajectory_efficiency(&self, entropy: f32) -> (bool, String) {
        if self.window.is_empty() {
            return (
                true,
                "No data — default PASS (insufficient evidence)".into(),
            );
        }

        let verified_rate = self.average_verified_rate();
        let avg_velocity = self.average_token_velocity();

        let entropy_penalty = if entropy > self.config.entropy_threshold_for_doom * 0.9 {
            0.25
        } else {
            0.0
        };

        let fail = (verified_rate + entropy_penalty) < self.config.min_useful_rate
            || (avg_velocity > self.config.max_token_velocity * 1.4 && verified_rate < 0.25);

        let justification = format!(
            "Trajectory: verified_rate={:.2} (min {:.2}), live_entropy={:.3}, velocity={:.1}. {}",
            verified_rate,
            self.config.min_useful_rate,
            entropy,
            avg_velocity,
            if fail {
                "HARSH FAIL — high semantic churn with low verified progress. Classic early doom signal."
            } else {
                "PASS"
            }
        );
        (!fail, justification)
    }

    fn check_epistemic_integrity(&self, entropy: f32) -> (bool, String) {
        if self.window.is_empty() {
            return (true, "No data — default PASS".into());
        }

        let avg_conf = self.average_epistemic_confidence();
        let avg_conflict = self.average_conflict_rate();

        let fail = avg_conf < self.config.min_epistemic_confidence
            && avg_conflict > self.config.max_conflict_rate * 0.6;

        // Extra harsh: high entropy + low confidence is treated as epistemic collapse
        let extra_fail = entropy > self.config.entropy_threshold_for_doom * 0.85 && avg_conf < 0.48;

        let justification = format!(
            "Epistemic: avg_confidence={:.2} (min {:.2}), conflict_rate={:.2}, entropy={:.3}. {}",
            avg_conf,
            self.config.min_epistemic_confidence,
            avg_conflict,
            entropy,
            if fail || extra_fail {
                "HARSH FAIL — low confidence combined with rising conflict and semantic drift. Agent is guessing, not knowing."
            } else {
                "PASS"
            }
        );
        (!(fail || extra_fail), justification)
    }

    fn check_tool_use_precision(&self) -> (bool, String) {
        if self.window.is_empty() {
            return (true, "No data — default PASS".into());
        }

        let avg_risk = self.average_risk_score();
        let avg_velocity = self.average_token_velocity();
        let avg_authority = self.average_authority_improvement();

        let fail = avg_risk > self.config.max_risk_score * 0.9
            && avg_velocity > self.config.max_token_velocity * 0.7
            && avg_authority < self.config.min_authority_improvement;

        let justification = format!(
            "Tool-Use: risk={:.2}, velocity={:.1}, authority_improvement={:.3}. {}",
            avg_risk,
            avg_velocity,
            avg_authority,
            if fail {
                "HARSH FAIL — aggressive tool calls (high velocity + risk) without measurable authority gain. Wasting tokens and increasing blast radius."
            } else {
                "PASS"
            }
        );
        (!fail, justification)
    }

    fn check_semantic_adherence(&self, entropy: f32) -> (bool, String) {
        if self.window.is_empty() {
            return (true, "No data — default PASS".into());
        }

        let velocity_drift = self.velocity_drift();

        let fail = entropy > self.config.entropy_threshold_for_doom
            || (velocity_drift > self.config.velocity_drift_threshold && entropy > 0.55);

        let justification = format!(
            "Semantic: live_H_sem={:.3} (threshold {:.2}), velocity_drift={:.2}. {}",
            entropy,
            self.config.entropy_threshold_for_doom,
            velocity_drift,
            if fail {
                "HARSH FAIL — semantic entropy high or rapidly drifting. The agent is no longer coherently following the contract; productive death vs doom-loop differentiator triggered."
            } else {
                "PASS"
            }
        );
        (!fail, justification)
    }

    fn check_resource_utilization(&self, entropy: f32) -> (bool, String) {
        if self.window.is_empty() {
            return (true, "No data — default PASS".into());
        }

        let avg_gpu = self.average_gpu_util();
        let avg_risk = self.average_risk_score();
        let avg_conf = self.average_epistemic_confidence();
        let avg_velocity = self.average_token_velocity();

        let fail = (avg_gpu > self.config.max_gpu_util && avg_risk > 0.55)
            || (avg_velocity > self.config.max_token_velocity * 1.1 && avg_conf < 0.5)
            || (entropy > 0.72 && avg_gpu > 0.8);

        let justification = format!(
            "Resource: gpu={:.2}, risk={:.2}, conf={:.2}, velocity={:.1}, entropy={:.3}. {}",
            avg_gpu,
            avg_risk,
            avg_conf,
            avg_velocity,
            entropy,
            if fail {
                "HARSH FAIL — unsustainable resource burn (GPU/velocity) while epistemic and semantic signals degrade. Immediate scaling or termination candidate."
            } else {
                "PASS"
            }
        );
        (!fail, justification)
    }

    // ---------- Helpers (used by rubrics) ----------

    fn average_verified_rate(&self) -> f32 {
        if self.window.is_empty() {
            return 0.5;
        }
        let total: i32 = self
            .window
            .iter()
            .map(|e| e.verified_count_delta.max(0))
            .sum();
        let steps = self.window.len() as i32;
        (total as f32 / steps as f32).clamp(0.0, 1.0)
    }

    fn average_epistemic_confidence(&self) -> f32 {
        self.average_field(|e| e.epistemic_confidence)
    }

    fn average_conflict_rate(&self) -> f32 {
        self.average_field(|e| e.conflict_rate)
    }

    fn average_risk_score(&self) -> f32 {
        self.average_field(|e| e.risk_score)
    }

    fn average_token_velocity(&self) -> f32 {
        self.average_field(|e| e.token_velocity)
    }

    fn average_gpu_util(&self) -> f32 {
        self.average_field(|e| e.gpu_util)
    }

    fn average_authority_improvement(&self) -> f32 {
        self.average_field(|e| e.authority_improvement)
    }

    fn average_field<F: Fn(&TraceEvent) -> f32>(&self, f: F) -> f32 {
        if self.window.is_empty() {
            return 0.5;
        }
        let sum: f32 = self.window.iter().map(f).sum();
        sum / self.window.len() as f32
    }

    fn velocity_drift(&self) -> f32 {
        if self.window.len() < 3 {
            return 0.0;
        }
        let velocities: Vec<f32> = self.window.iter().map(|e| e.token_velocity).collect();
        let first_half = &velocities[0..velocities.len() / 2];
        let second_half = &velocities[velocities.len() / 2..];
        let avg1 = first_half.iter().sum::<f32>() / first_half.len() as f32;
        let avg2 = second_half.iter().sum::<f32>() / second_half.len() as f32;
        ((avg2 - avg1).abs() / avg1.max(1.0)).min(1.0)
    }

    /// Main entry point: run all five rubrics, compute live semantic_entropy,
    /// decide overall harsh verdict, and return a rich EvaluationVerdict.
    #[tracing::instrument(skip(self), fields(session_id = %session_id))]
    pub async fn evaluate(&mut self, session_id: Uuid) -> EvaluationVerdict {
        let live_entropy = self.semantic_entropy_from_window().await;

        let checks = vec![
            ("Trajectory Efficiency", self.check_trajectory_efficiency(live_entropy)),
            ("Epistemic Integrity", self.check_epistemic_integrity(live_entropy)),
            ("Tool-Use Precision", self.check_tool_use_precision()),
            ("Semantic Adherence", self.check_semantic_adherence(live_entropy)),
            ("Resource Utilization", self.check_resource_utilization(live_entropy)),
        ];

        let mut passed = 0u8;
        let mut justifications = vec![];

        for (name, (ok, j)) in &checks {
            if *ok {
                passed += 1;
            }
            justifications.push(format!("[{}] {}", name, j));
            tracing::debug!(rubric = name, passed = ok, justification = j, "rubric_evaluated");
        }

        let total = 5u8;
        let doom = live_entropy > self.config.entropy_threshold_for_doom && (5 - passed) >= 3;

        let productive_death =
            (5 - passed) >= 2 && live_entropy < 0.45 && self.average_verified_rate() > 0.6;

        let (overall, action) = if passed == total {
            ("PASS".to_string(), "scale_up".to_string())
        } else if doom {
            (
                "TERMINATE".to_string(),
                "terminate_and_rollback".to_string(),
            )
        } else if passed <= 2 {
            ("NEEDS_REVISION".to_string(), "revise".to_string())
        } else {
            ("NEEDS_REVISION".to_string(), "hold".to_string())
        };

        tracing::info!(
            overall = %overall,
            passed_rubrics = passed,
            total_rubrics = total,
            semantic_entropy = live_entropy,
            doom_loop_detected = doom,
            recommended_action = %action,
            "evaluation_verdict"
        );

        crate::metrics::record_evaluator_verdict(&overall, doom, live_entropy);

        EvaluationVerdict {
            verdict_id: Uuid::now_v7(),
            session_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            overall,
            passed_rubrics: passed,
            total_rubrics: total,
            justifications,
            recommended_action: action,
            semantic_entropy: live_entropy,
            doom_loop_detected: doom,
            productive_death,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn evaluator_produces_harsh_verdicts_with_real_semantic_entropy() {
        let mut ev = Evaluator::new(None);

        // Seed with some low-quality, high-churn events
        for i in 0..12 {
            let mut evt = TraceEvent::default();
            evt.risk_score = 0.78;
            evt.epistemic_confidence = 0.38;
            evt.conflict_rate = 0.42;
            evt.token_velocity = 210.0;
            evt.surface_text = format!("noisy churn iteration {} — changing mind constantly", i);
            evt.verified_count_delta = if i % 3 == 0 { 0 } else { -1 };
            ev.ingest(evt);
        }

        let v = ev.evaluate(Uuid::now_v7()).await;
        assert!(
            v.semantic_entropy > 0.5,
            "High churn should produce high entropy"
        );
        assert!(
            v.passed_rubrics < 4,
            "Harsh critic should fail multiple rubrics on bad data"
        );
        assert!(v.overall != "PASS");
    }

    #[tokio::test]
    async fn semantic_entropy_method_is_directly_callable() {
        let ev = Evaluator::new(None);
        let texts = vec![
            "we are building a clean refactored module".to_string(),
            "the new design separates concerns beautifully".to_string(),
        ];
        let h = ev.semantic_entropy(texts).await;
        assert!(h >= 0.0 && h <= 1.0);
    }
}
