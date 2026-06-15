//! DeterministicProvider — a hermetic, offline, role-aware LlmProvider.
//!
//! It is the default campaign provider: a pure function of (role, task) that
//! emits role-shaped, *applyable* structured output for tasks it recognizes,
//! and an honest null (zero mutations, low confidence) for tasks it does not.
//! It never fabricates success. A real model (`--provider ollama`) handles
//! arbitrary tasks; this stub's contract is honesty + reproducibility.

use crate::{
    FinishReason, LlmDelta, LlmError, LlmProvider, LlmRequest, LlmResponse, Role, TokenUsage,
};
use async_trait::async_trait;
use futures_util::stream::{self, Stream};
use std::pin::Pin;

/// Word-count token estimate — truthful for the bytes we actually emit.
fn estimate_tokens(s: &str) -> u32 {
    s.split_whitespace().count() as u32
}

/// Recover the persona role from the System message text. `LlmRequest` carries
/// no structured role field, so we match a stable marker each persona prompt
/// contains. The markers below are the `role:` lines from `Prompts/*.md`
/// (e.g. Benjamin = "Builder & Implementer", Captain = "Swarm Orchestrator &
/// Planner"); the persona name is a secondary fallback.
fn role_marker(req: &LlmRequest) -> &'static str {
    let system = req
        .messages
        .iter()
        .find(|m| matches!(m.role, Role::System))
        .map(|m| m.content.as_str())
        .unwrap_or("");
    // Check the most-specific role titles first; persona names are a fallback for
    // truncated/overridden prompts that drop the `role:` frontmatter line.
    if system.contains("Builder & Implementer") || system.contains("Benjamin") {
        "benjamin"
    } else if system.contains("Swarm Orchestrator & Planner") || system.contains("Captain") {
        "captain"
    } else if system.contains("Adversarial Researcher & Reviewer") || system.contains("Harper") {
        "harper"
    } else if system.contains("Synthesizer & Reconciler") || system.contains("Lucas") {
        "lucas"
    } else if system.contains("Guardrail Evaluator & Critic") || system.contains("Evaluator") {
        "evaluator"
    } else {
        "unknown"
    }
}

/// Last user message = the task payload.
fn task_text(req: &LlmRequest) -> &str {
    req.messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
        .map(|m| m.content.as_str())
        .unwrap_or("")
}

/// The canonical fix for the fixture crate (`fixtures/honest-demo-repo`):
/// rewrite src/lib.rs so `add` actually adds. Full file body (applyable as-is).
const FIXTURE_LIB_RS: &str = "/// Adds two numbers.\npub fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn adds() {\n        assert_eq!(add(2, 3), 5);\n    }\n}\n";

/// Build the structured response text (markdown frontmatter + a JSON fence),
/// matching what `parse_structured_response` in korg-runtime expects. `body`
/// is the *complete* JSON object body (already serialized) to place inside the
/// `json` fence, letting each persona emit its own role-shaped schema.
fn render_artifact(confidence: f32, body: &str) -> String {
    format!(
        "---\nconfidence: {confidence}\nself_score: {confidence}\n---\n\n```json\n{body}\n```\n\nDeterministic honest provider output.\n"
    )
}

/// Convenience for Benjamin's mutations-only artifact (kept byte-for-byte
/// compatible with the SP1 output the apply path already understands).
fn render(confidence: f32, mutations_json: &str) -> String {
    render_artifact(
        confidence,
        &format!("{{\n  \"mutations\": {mutations_json}\n}}"),
    )
}

/// Serialize a JSON value into the body string for `render_artifact`,
/// pretty-printed for stability and human-auditability.
fn artifact_body(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

pub struct DeterministicProvider {
    name: &'static str,
}

impl DeterministicProvider {
    pub fn new() -> Self {
        Self {
            name: "deterministic-honest",
        }
    }

    /// Pure rendering core (no async, no I/O) so it is trivially testable.
    ///
    /// For the *fixture-class* task (the `add`-bug fix on `src/lib.rs`) every
    /// persona emits a real, deterministic, role-shaped artifact matching its
    /// documented `Prompts/*.md` output schema. For any other task each persona
    /// returns an honest null (empty role-shaped artifact + low confidence) —
    /// it never fabricates success for a task this stub cannot actually do.
    fn render_for(&self, req: &LlmRequest) -> String {
        let role = role_marker(req);
        let task = task_text(req).to_ascii_lowercase();
        // Recognize the fixture task by a stable signature, independent of role,
        // so the whole swarm collaborates on it (not just Benjamin).
        let is_fixture = task.contains("add function") && task.contains("src/lib.rs");
        if is_fixture {
            self.fixture_artifact(role)
        } else {
            self.honest_null(role)
        }
    }

    /// Real role-shaped artifact for the fixture task. Each branch mirrors the
    /// JSON schema documented in `Prompts/{persona}.md`.
    fn fixture_artifact(&self, role: &str) -> String {
        match role {
            "captain" => {
                let body = serde_json::json!({
                    "work_packages": [
                        {
                            "id": 1,
                            "title": "Fix the broken `add` in src/lib.rs",
                            "assigned_to": "Benjamin",
                            "description": "Rewrite `add` in src/lib.rs so it returns a + b, and keep a unit test asserting add(2,3)==5.",
                            "dependencies": []
                        }
                    ],
                    "acceptance_criteria": [
                        "src/lib.rs compiles cleanly",
                        "add(2, 3) returns 5",
                        "the adds() unit test passes"
                    ]
                });
                render_artifact(0.95, &artifact_body(&body))
            }
            "harper" => {
                let body = serde_json::json!({
                    "concerns": [
                        {
                            "severity": "high",
                            "description": "The current `add` in src/lib.rs does not perform addition; the fix must use a + b, not a - b or a hardcoded constant.",
                            "file_path": "src/lib.rs"
                        }
                    ],
                    "risk_assessment": "low",
                    "prior_art_checked": [
                        "fixtures/honest-demo-repo src/lib.rs add bug"
                    ],
                    "recommendations": [
                        "Keep the change to src/lib.rs minimal — only the add body and its test"
                    ]
                });
                render_artifact(0.9, &artifact_body(&body))
            }
            "benjamin" => {
                let content = serde_json::Value::String(FIXTURE_LIB_RS.to_string());
                let mutations = format!(
                    "[{{\"target\":\"src/lib.rs\",\"action\":\"update\",\"content\":{},\"description\":\"Fix add to use addition\"}}]",
                    content
                );
                render(0.95, &mutations)
            }
            "lucas" => {
                let body = serde_json::json!({
                    "synthesis": "Captain's plan and Harper's concern both point at the same one-file fix; Benjamin's implement step rewrites src/lib.rs add to return a + b. No conflicts to reconcile.",
                    "hybrid_ready": true,
                    "resolutions": [
                        {
                            "topic": "src/lib.rs add fix",
                            "decision": "Adopt Benjamin's implement step (add returns a + b) — consistent with Captain's acceptance criteria and Harper's concern."
                        }
                    ]
                });
                render_artifact(0.9, &artifact_body(&body))
            }
            "evaluator" => {
                let body = serde_json::json!({
                    "overall": "PASS",
                    "passed_rubrics": [
                        "correctness",
                        "completeness",
                        "minimal_diff",
                        "provenance_strength"
                    ],
                    "total_rubrics": 5,
                    "justifications": [
                        "Correctness: add now returns a + b and the adds() test asserts 5.",
                        "Minimal diff: only src/lib.rs is touched."
                    ],
                    "recommended_action": "hold"
                });
                render_artifact(0.9, &artifact_body(&body))
            }
            _ => self.honest_null(role),
        }
    }

    /// Honest null for tasks this stub cannot do: an empty role-shaped artifact
    /// with low confidence. Never fabricated success.
    fn honest_null(&self, role: &str) -> String {
        let body = match role {
            "captain" => serde_json::json!({ "work_packages": [], "acceptance_criteria": [] }),
            "harper" => serde_json::json!({
                "concerns": [],
                "risk_assessment": "unknown",
                "prior_art_checked": [],
                "recommendations": []
            }),
            "benjamin" => serde_json::json!({ "mutations": [] }),
            "lucas" => {
                serde_json::json!({ "synthesis": "", "hybrid_ready": false, "resolutions": [] })
            }
            "evaluator" => serde_json::json!({
                "overall": "NEEDS_REVISION",
                "passed_rubrics": [],
                "total_rubrics": 5,
                "justifications": [],
                "recommended_action": "hold"
            }),
            // Unknown persona: fall back to the historical mutations-only null.
            _ => serde_json::json!({ "mutations": [] }),
        };
        render_artifact(0.20, &artifact_body(&body))
    }
}

impl Default for DeterministicProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LlmProvider for DeterministicProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let prompt_tokens: u32 = req
            .messages
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();
        let content = self.render_for(&req);
        let completion_tokens = estimate_tokens(&content);
        Ok(LlmResponse {
            usage: TokenUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
            content,
            model: "deterministic-honest-v1".to_string(),
            finish_reason: FinishReason::Stop,
            tool_calls: None,
        })
    }

    async fn complete_stream(
        &self,
        req: LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError> {
        let content = self.render_for(&req);
        let delta = LlmDelta {
            content,
            tool_calls: None,
            finish_reason: Some(FinishReason::Stop),
        };
        Ok(Box::pin(stream::iter(vec![Ok(delta)])))
    }
}

/// Test-only re-export of korg-runtime's structured parser shape so the unit
/// tests here can assert on the emitted mutations without depending on
/// korg-runtime. Mirrors the ```json-block extraction used downstream.
#[cfg(test)]
pub(crate) fn parse_for_test(response: &str) -> (serde_json::Value, f32, serde_json::Value) {
    let mut confidence = 0.85_f32;
    if let Some(line) = response
        .lines()
        .find(|l| l.trim_start().starts_with("confidence:"))
    {
        if let Some(v) = line.split(':').nth(1) {
            if let Ok(f) = v.trim().parse::<f32>() {
                confidence = f;
            }
        }
    }
    let mut output = serde_json::json!({});
    if let Some(s) = response.find("```json") {
        let sub = &response[s + 7..];
        if let Some(e) = sub.find("```") {
            if let Ok(v) = serde_json::from_str(sub[..e].trim()) {
                output = v;
            }
        }
    }
    (output, confidence, serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LlmRequest, Message, Role};

    fn req(system: &str, user: &str) -> LlmRequest {
        LlmRequest {
            messages: vec![
                Message {
                    role: Role::System,
                    content: system.to_string(),
                    name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: user.to_string(),
                    name: None,
                    tool_calls: None,
                },
            ],
            temperature: 0.3,
            max_tokens: None,
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: None,
            session_id: None,
            policy_hash: None,
            top_p: None,
            presence_penalty: None,
            frequency_penalty: None,
        }
    }

    #[tokio::test]
    async fn fixture_task_returns_canonical_applyable_patch() {
        let p = DeterministicProvider::new();
        // Benjamin system marker + the fixture task signature
        let r = p
            .complete(req(
                "You are Benjamin, the Builder & Implementer.",
                "Fix the add function in src/lib.rs so it adds",
            ))
            .await
            .unwrap();
        let (output, _conf, _fm) = crate::deterministic::parse_for_test(&r.content);
        let muts = output
            .get("mutations")
            .and_then(|m| m.as_array())
            .expect("mutations array");
        assert_eq!(muts.len(), 1, "exactly one applyable mutation");
        let m0 = &muts[0];
        assert_eq!(
            m0.get("target").and_then(|v| v.as_str()),
            Some("src/lib.rs")
        );
        let content = m0
            .get("content")
            .and_then(|v| v.as_str())
            .expect("applyable content field");
        assert!(
            content.contains("a + b"),
            "the patch must actually fix the bug"
        );
    }

    #[tokio::test]
    async fn unknown_task_returns_honest_null_not_fabricated_success() {
        let p = DeterministicProvider::new();
        let r = p
            .complete(req(
                "You are Benjamin, the Builder & Implementer.",
                "Implement a distributed consensus protocol",
            ))
            .await
            .unwrap();
        let (output, conf, _fm) = crate::deterministic::parse_for_test(&r.content);
        let muts = output
            .get("mutations")
            .and_then(|m| m.as_array())
            .expect("mutations array");
        assert!(
            muts.is_empty(),
            "honest null: no fabricated mutations for an unknown task"
        );
        assert!(conf < 0.5, "honest null reports low confidence, got {conf}");
    }

    // --- Slice 1: role-aware fixture artifacts (one per persona) ---

    /// A persona system message + the fixture task. Helper keeps the role
    /// markers in one place so the assertions below read cleanly.
    async fn fixture_output_for(system: &str) -> serde_json::Value {
        let p = DeterministicProvider::new();
        let r = p
            .complete(req(
                system,
                "Plan the work to fix the add function in src/lib.rs so it adds",
            ))
            .await
            .unwrap();
        let (output, conf, _fm) = crate::deterministic::parse_for_test(&r.content);
        assert!(
            conf > 0.5,
            "fixture artifact should carry real (high) confidence, got {conf}"
        );
        output
    }

    #[tokio::test]
    async fn captain_fixture_output_has_nonempty_work_packages() {
        let out =
            fixture_output_for("You are the Captain, the Swarm Orchestrator & Planner.").await;
        let wps = out
            .get("work_packages")
            .and_then(|v| v.as_array())
            .expect("captain artifact has work_packages array");
        assert!(
            !wps.is_empty(),
            "captain fixture work_packages must be non-empty"
        );
        assert!(
            out.get("acceptance_criteria")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false),
            "captain fixture must include acceptance_criteria"
        );
    }

    #[tokio::test]
    async fn harper_fixture_output_has_nonempty_concerns() {
        let out =
            fixture_output_for("You are Harper, the Adversarial Researcher & Reviewer.").await;
        let concerns = out
            .get("concerns")
            .and_then(|v| v.as_array())
            .expect("harper artifact has concerns array");
        assert!(
            !concerns.is_empty(),
            "harper fixture concerns must be non-empty"
        );
        assert!(
            out.get("risk_assessment")
                .and_then(|v| v.as_str())
                .is_some(),
            "harper fixture must include a risk_assessment string"
        );
    }

    #[tokio::test]
    async fn benjamin_fixture_output_has_applyable_mutation() {
        let out = fixture_output_for("You are Benjamin, the Builder & Implementer.").await;
        let muts = out
            .get("mutations")
            .and_then(|v| v.as_array())
            .expect("benjamin artifact has mutations array");
        assert_eq!(
            muts.len(),
            1,
            "benjamin emits exactly one applyable mutation"
        );
        let content = muts[0]
            .get("content")
            .and_then(|v| v.as_str())
            .expect("applyable content field");
        assert!(content.contains("a + b"), "benjamin patch must fix the bug");
    }

    #[tokio::test]
    async fn lucas_fixture_output_has_nonempty_resolutions() {
        let out = fixture_output_for("You are Lucas, the Synthesizer & Reconciler.").await;
        let resolutions = out
            .get("resolutions")
            .and_then(|v| v.as_array())
            .expect("lucas artifact has resolutions array");
        assert!(
            !resolutions.is_empty(),
            "lucas fixture resolutions must be non-empty"
        );
    }

    #[tokio::test]
    async fn evaluator_fixture_output_has_nonempty_passed_rubrics() {
        let out =
            fixture_output_for("You are the Evaluator, the Guardrail Evaluator & Critic.").await;
        let passed = out
            .get("passed_rubrics")
            .and_then(|v| v.as_array())
            .expect("evaluator artifact has passed_rubrics array");
        assert!(
            !passed.is_empty(),
            "evaluator fixture passed_rubrics must be non-empty"
        );
        assert!(
            out.get("recommended_action")
                .and_then(|v| v.as_str())
                .is_some(),
            "evaluator fixture must include a recommended_action"
        );
    }

    // --- Slice 1: honest-null for an unknown task, per persona ---

    async fn honest_null_output_for(system: &str) -> (serde_json::Value, f32) {
        let p = DeterministicProvider::new();
        let r = p
            .complete(req(
                system,
                "Build a real-time multiplayer physics engine from scratch",
            ))
            .await
            .unwrap();
        let (output, conf, _fm) = crate::deterministic::parse_for_test(&r.content);
        (output, conf)
    }

    fn array_is_empty(out: &serde_json::Value, key: &str) -> bool {
        out.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn every_persona_returns_honest_null_for_unknown_task() {
        let (cap, c) =
            honest_null_output_for("You are the Captain, the Swarm Orchestrator & Planner.").await;
        assert!(c < 0.5, "captain honest-null low confidence, got {c}");
        assert!(
            array_is_empty(&cap, "work_packages"),
            "captain unknown → no work_packages"
        );

        let (har, c) =
            honest_null_output_for("You are Harper, the Adversarial Researcher & Reviewer.").await;
        assert!(c < 0.5, "harper honest-null low confidence, got {c}");
        assert!(
            array_is_empty(&har, "concerns"),
            "harper unknown → no concerns"
        );

        let (ben, c) = honest_null_output_for("You are Benjamin, the Builder & Implementer.").await;
        assert!(c < 0.5, "benjamin honest-null low confidence, got {c}");
        assert!(
            array_is_empty(&ben, "mutations"),
            "benjamin unknown → no mutations"
        );

        let (luc, c) = honest_null_output_for("You are Lucas, the Synthesizer & Reconciler.").await;
        assert!(c < 0.5, "lucas honest-null low confidence, got {c}");
        assert!(
            array_is_empty(&luc, "resolutions"),
            "lucas unknown → no resolutions"
        );

        let (eva, c) =
            honest_null_output_for("You are the Evaluator, the Guardrail Evaluator & Critic.")
                .await;
        assert!(c < 0.5, "evaluator honest-null low confidence, got {c}");
        assert!(
            array_is_empty(&eva, "passed_rubrics"),
            "evaluator unknown → no passed_rubrics"
        );
    }

    #[tokio::test]
    async fn output_is_byte_identical_for_same_inputs() {
        let p = DeterministicProvider::new();
        let a = p
            .complete(req(
                "You are Benjamin, the Builder & Implementer.",
                "Fix the add function in src/lib.rs so it adds",
            ))
            .await
            .unwrap();
        let b = p
            .complete(req(
                "You are Benjamin, the Builder & Implementer.",
                "Fix the add function in src/lib.rs so it adds",
            ))
            .await
            .unwrap();
        assert_eq!(
            a.content, b.content,
            "deterministic: same inputs → byte-identical output"
        );
    }

    #[tokio::test]
    async fn reports_truthful_nonzero_token_usage() {
        let p = DeterministicProvider::new();
        let r = p
            .complete(req(
                "You are Benjamin, the Builder & Implementer.",
                "Fix the add function in src/lib.rs so it adds",
            ))
            .await
            .unwrap();
        assert!(r.usage.total_tokens > 0);
        assert_eq!(
            r.usage.total_tokens,
            r.usage.prompt_tokens + r.usage.completion_tokens
        );
    }
}
