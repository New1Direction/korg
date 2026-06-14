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
/// no structured role field, so we match a stable marker the persona prompt
/// contains (Benjamin's prompt says "Builder & Implementer").
fn role_marker(req: &LlmRequest) -> &'static str {
    let system = req
        .messages
        .iter()
        .find(|m| matches!(m.role, Role::System))
        .map(|m| m.content.as_str())
        .unwrap_or("");
    if system.contains("Builder & Implementer") || system.contains("Benjamin") {
        "benjamin"
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

/// Build the structured response text (markdown frontmatter + ```json block),
/// matching what `parse_structured_response` in korg-runtime expects.
fn render(confidence: f32, mutations_json: &str) -> String {
    format!(
        "---\nconfidence: {confidence}\nself_score: {confidence}\n---\n\n```json\n{{\n  \"mutations\": {mutations_json}\n}}\n```\n\nDeterministic honest provider output.\n"
    )
}

pub struct DeterministicProvider {
    name: &'static str,
}

impl DeterministicProvider {
    pub fn new() -> Self {
        Self { name: "deterministic-honest" }
    }

    /// Pure rendering core (no async, no I/O) so it is trivially testable.
    fn render_for(&self, req: &LlmRequest) -> String {
        let role = role_marker(req);
        let task = task_text(req).to_ascii_lowercase();
        // Recognize the fixture task by a stable signature.
        let is_fixture = role == "benjamin"
            && task.contains("add function")
            && task.contains("src/lib.rs");
        if is_fixture {
            let content = serde_json::Value::String(FIXTURE_LIB_RS.to_string());
            let mutations = format!(
                "[{{\"target\":\"src/lib.rs\",\"action\":\"update\",\"content\":{},\"description\":\"Fix add to use addition\"}}]",
                content
            );
            render(0.95, &mutations)
        } else {
            // Honest null: no fabricated mutations, low confidence.
            render(0.20, "[]")
        }
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
        let prompt_tokens: u32 = req.messages.iter().map(|m| estimate_tokens(&m.content)).sum();
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
        let delta = LlmDelta { content, tool_calls: None, finish_reason: Some(FinishReason::Stop) };
        Ok(Box::pin(stream::iter(vec![Ok(delta)])))
    }
}

/// Test-only re-export of korg-runtime's structured parser shape so the unit
/// tests here can assert on the emitted mutations without depending on
/// korg-runtime. Mirrors the ```json-block extraction used downstream.
#[cfg(test)]
pub(crate) fn parse_for_test(response: &str) -> (serde_json::Value, f32, serde_json::Value) {
    let mut confidence = 0.85_f32;
    if let Some(line) = response.lines().find(|l| l.trim_start().starts_with("confidence:")) {
        if let Some(v) = line.split(':').nth(1) {
            if let Ok(f) = v.trim().parse::<f32>() { confidence = f; }
        }
    }
    let mut output = serde_json::json!({});
    if let Some(s) = response.find("```json") {
        let sub = &response[s + 7..];
        if let Some(e) = sub.find("```") {
            if let Ok(v) = serde_json::from_str(sub[..e].trim()) { output = v; }
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
                Message { role: Role::System, content: system.to_string(), name: None, tool_calls: None },
                Message { role: Role::User, content: user.to_string(), name: None, tool_calls: None },
            ],
            temperature: 0.3, max_tokens: None, tools: None, stop_sequences: None,
            multimodal: None, tx_id: None, session_id: None, policy_hash: None,
            top_p: None, presence_penalty: None, frequency_penalty: None,
        }
    }

    #[tokio::test]
    async fn fixture_task_returns_canonical_applyable_patch() {
        let p = DeterministicProvider::new();
        // Benjamin system marker + the fixture task signature
        let r = p.complete(req("You are Benjamin, the Builder & Implementer.",
                               "Fix the add function in src/lib.rs so it adds")).await.unwrap();
        let (output, _conf, _fm) = crate::deterministic::parse_for_test(&r.content);
        let muts = output.get("mutations").and_then(|m| m.as_array()).expect("mutations array");
        assert_eq!(muts.len(), 1, "exactly one applyable mutation");
        let m0 = &muts[0];
        assert_eq!(m0.get("target").and_then(|v| v.as_str()), Some("src/lib.rs"));
        let content = m0.get("content").and_then(|v| v.as_str()).expect("applyable content field");
        assert!(content.contains("a + b"), "the patch must actually fix the bug");
    }

    #[tokio::test]
    async fn unknown_task_returns_honest_null_not_fabricated_success() {
        let p = DeterministicProvider::new();
        let r = p.complete(req("You are Benjamin, the Builder & Implementer.",
                               "Implement a distributed consensus protocol")).await.unwrap();
        let (output, conf, _fm) = crate::deterministic::parse_for_test(&r.content);
        let muts = output.get("mutations").and_then(|m| m.as_array()).expect("mutations array");
        assert!(muts.is_empty(), "honest null: no fabricated mutations for an unknown task");
        assert!(conf < 0.5, "honest null reports low confidence, got {conf}");
    }

    #[tokio::test]
    async fn output_is_byte_identical_for_same_inputs() {
        let p = DeterministicProvider::new();
        let a = p.complete(req("You are Benjamin, the Builder & Implementer.", "Fix the add function in src/lib.rs so it adds")).await.unwrap();
        let b = p.complete(req("You are Benjamin, the Builder & Implementer.", "Fix the add function in src/lib.rs so it adds")).await.unwrap();
        assert_eq!(a.content, b.content, "deterministic: same inputs → byte-identical output");
    }

    #[tokio::test]
    async fn reports_truthful_nonzero_token_usage() {
        let p = DeterministicProvider::new();
        let r = p.complete(req("You are Benjamin, the Builder & Implementer.", "Fix the add function in src/lib.rs so it adds")).await.unwrap();
        assert!(r.usage.total_tokens > 0);
        assert_eq!(r.usage.total_tokens, r.usage.prompt_tokens + r.usage.completion_tokens);
    }
}
