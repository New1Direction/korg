//! Persona implementations for the native 4-agent topology.
//!
//! This module contains the logic for Captain, Harper, Benjamin, and Lucas.
//! It is used both by the SingleWorkerHarness (when running as subprocess)
//! and directly by the in-process LeaderOrchestrator for the full campaign demo.

use crate::evaluator::{Evaluator, TraceEvent};
use crate::llm::{LlmProvider, LlmRequest, Message, Role};
use serde_json::json;
use std::fs;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persona {
    Captain,   // Grok / Reasoning & Orchestration
    Harper,    // Critique & Research
    Benjamin,  // Tool-Use / Generator (builder)
    Lucas,     // Synthesis & Arena participation
    Evaluator, // Harsh adversarial critic (Anthropic-style Generator/Evaluator loop)
}

impl Persona {
    pub fn from_capabilities(caps: &[String]) -> Self {
        if caps
            .iter()
            .any(|c| c.contains("captain") || c.contains("reasoning"))
        {
            Persona::Captain
        } else if caps
            .iter()
            .any(|c| c.contains("harper") || c.contains("critique"))
        {
            Persona::Harper
        } else if caps
            .iter()
            .any(|c| c.contains("lucas") || c.contains("synthesis"))
        {
            Persona::Lucas
        } else if caps
            .iter()
            .any(|c| c.contains("evaluator") || c.contains("critic"))
        {
            Persona::Evaluator
        } else {
            Persona::Benjamin // default execution persona
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Persona::Captain => "Captain",
            Persona::Harper => "Harper",
            Persona::Benjamin => "Benjamin",
            Persona::Lucas => "Lucas",
            Persona::Evaluator => "Evaluator",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PersonaResult {
    pub persona: Persona,
    pub routing_id: String,
    pub output: serde_json::Value,
    pub confidence: f32,
    pub mutations: Vec<serde_json::Value>,
    pub arena_self_score: serde_json::Value,
    pub crashed: bool,
    pub error_msg: Option<String>,
}

impl PersonaResult {
    pub fn new(persona: Persona, routing_id: String) -> Self {
        Self {
            persona,
            routing_id,
            output: json!({}),
            confidence: 0.85,
            mutations: vec![],
            arena_self_score: json!({}),
            crashed: false,
            error_msg: None,
        }
    }
}

/// Standard custom response parser for Markdown + YAML frontmatter + JSON action block
pub fn parse_structured_response(response: &str) -> (serde_json::Value, f32, serde_json::Value) {
    let mut frontmatter = serde_json::Map::new();
    let mut confidence = 0.85;
    let mut output = json!({});

    // Parse frontmatter
    let parts: Vec<&str> = response.split("---").collect();
    if parts.len() >= 3 {
        let fm_block = parts[1];
        for line in fm_block.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(pos) = line.find(':') {
                let key = line[..pos].trim();
                let value_str = line[pos + 1..].trim();

                // Parse value
                if let Ok(num) = value_str.parse::<f64>() {
                    if let Some(n) = serde_json::Number::from_f64(num) {
                        frontmatter.insert(key.to_string(), serde_json::Value::Number(n));
                    }
                } else if value_str.eq_ignore_ascii_case("true") {
                    frontmatter.insert(key.to_string(), serde_json::Value::Bool(true));
                } else if value_str.eq_ignore_ascii_case("false") {
                    frontmatter.insert(key.to_string(), serde_json::Value::Bool(false));
                } else {
                    let stripped = value_str.trim_matches(|c| c == '\'' || c == '"');
                    frontmatter.insert(key.to_string(), serde_json::Value::String(stripped.to_string()));
                }
            }
        }
    }

    // Extract confidence
    if let Some(val) = frontmatter.get("confidence") {
        if let Some(f) = val.as_f64() {
            confidence = f as f32;
        } else if let Some(s) = val.as_str() {
            if let Ok(f) = s.parse::<f32>() {
                confidence = f;
            }
        }
    }

    // Parse JSON Block
    let mut json_str = String::new();
    if let Some(start_json) = response.find("```json") {
        let sub = &response[start_json + 7..];
        if let Some(end_json) = sub.find("```") {
            json_str = sub[..end_json].trim().to_string();
        }
    } else if let Some(start_code) = response.find("```") {
        let sub = &response[start_code + 3..];
        if let Some(end_code) = sub.find("```") {
            let potential = sub[..end_code].trim();
            if potential.starts_with('{') {
                json_str = potential.to_string();
            }
        }
    }

    // Fallback bracket match
    if json_str.is_empty() {
        if let Some(first_bracket) = response.find('{') {
            if let Some(last_bracket) = response.rfind('}') {
                if last_bracket > first_bracket {
                    json_str = response[first_bracket..=last_bracket].to_string();
                }
            }
        }
    }

    if !json_str.is_empty() {
        if let Ok(val) = serde_json::from_str(&json_str) {
            output = val;
        }
    }

    (output, confidence, serde_json::Value::Object(frontmatter))
}

/// Dynamic prompt loader from filesystem overrides or static fallbacks
pub fn load_prompt_for_persona(persona: Persona) -> String {
    let filename = match persona {
        Persona::Captain => "captain.md",
        Persona::Harper => "harper.md",
        Persona::Benjamin => "benjamin.md",
        Persona::Lucas => "lucas.md",
        Persona::Evaluator => "evaluator.md",
    };
    let path = format!("/Users/clubpenguin/Documents/Korg/Prompts/{}", filename);
    if let Ok(content) = fs::read_to_string(&path) {
        content
    } else {
        match persona {
            Persona::Captain => {
                "---\npersona: Captain\nrole: Swarm Orchestrator & Planner\ndescription: Decomposes tasks, designs execution DAGs.\n---\n".to_string()
            }
            Persona::Harper => {
                "---\npersona: Harper\nrole: Adversarial Researcher & Reviewer\ndescription: Audits codebases, identifies vulnerabilities.\n---\n".to_string()
            }
            Persona::Benjamin => {
                "---\npersona: Benjamin\nrole: Builder & Implementer\ndescription: Executing implementation work.\n---\n".to_string()
            }
            Persona::Lucas => {
                "---\npersona: Lucas\nrole: Synthesizer & Reconciler\ndescription: Combines parallel swarm edits.\n---\n".to_string()
            }
            Persona::Evaluator => {
                "---\npersona: Evaluator\nrole: Guardrail Evaluator & Critic\ndescription: Adversarial check against 5 rubrics.\n---\n".to_string()
            }
        }
    }
}

/// Unified wrapper containing active LLM provider and template context
pub struct LlmPersona {
    pub persona: Persona,
    pub provider: Arc<dyn LlmProvider>,
    pub system_prompt: String,
    pub temperature: f32,
}

impl LlmPersona {
    pub fn new(persona: Persona, provider: Arc<dyn LlmProvider>) -> Self {
        let system_prompt = load_prompt_for_persona(persona);
        let temperature = match persona {
            Persona::Captain => 0.2,
            Persona::Harper => 0.5,
            Persona::Benjamin => 0.3,
            Persona::Lucas => 0.4,
            Persona::Evaluator => 0.1,
        };
        Self {
            persona,
            provider,
            system_prompt,
            temperature,
        }
    }

    pub async fn think(&self, payload: &str, routing_id: &str) -> Result<PersonaResult, crate::llm::LlmError> {
        let messages = vec![
            Message {
                role: Role::System,
                content: self.system_prompt.clone(),
                name: None,
                tool_calls: None,
            },
            Message {
                role: Role::User,
                content: format!("Routing ID: {}\nPayload: {}", routing_id, payload),
                name: None,
                tool_calls: None,
            },
        ];

        let request = LlmRequest {
            messages,
            temperature: self.temperature,
            max_tokens: Some(4096),
            tools: None,
            stop_sequences: None,
            multimodal: None,
            tx_id: Some(routing_id.to_string()),
            session_id: None,
            policy_hash: None,
        };

        let response = self.provider.complete(request).await?;
        let (output, confidence, frontmatter) = parse_structured_response(&response.content);

        let mut mutations = vec![];
        if let Some(muts) = output.get("mutations") {
            if let Some(arr) = muts.as_array() {
                mutations = arr.clone();
            }
        } else if let Some(muts) = frontmatter.get("mutations") {
            if let Some(arr) = muts.as_array() {
                mutations = arr.clone();
            }
        }

        let self_score = output.get("arena_self_score").cloned().unwrap_or_else(|| {
            frontmatter.get("arena_self_score").cloned().unwrap_or_else(|| {
                let score = frontmatter.get("self_score").cloned().unwrap_or(json!(0.85));
                json!({
                    "correctness": score,
                    "completeness": 0.85,
                    "novelty": 0.70,
                    "minimal_diff": 0.80,
                    "provenance_strength": 0.90
                })
            })
        });

        Ok(PersonaResult {
            persona: self.persona,
            routing_id: routing_id.to_string(),
            output,
            confidence,
            mutations,
            arena_self_score: self_score,
            crashed: false,
            error_msg: None,
        })
    }
}

pub fn fallback_captain(payload: &str, routing_id: &str) -> PersonaResult {
    eprintln!("[Captain Simulation] Decomposing and planning: {}", payload);
    let mut result = PersonaResult::new(Persona::Captain, routing_id.to_string());
    result.output = json!({
        "plan": "High-level DAG for the task",
        "work_packages": 4
    });
    result.confidence = 0.92;
    result.mutations = vec![json!({"target": "plan.md", "action": "create"})];
    result.arena_self_score = json!({
        "correctness": 0.93, "completeness": 0.88, "novelty": 0.65,
        "minimal_diff": 0.80, "provenance_strength": 0.90
    });
    result
}

pub fn fallback_harper(payload: &str, routing_id: &str) -> PersonaResult {
    eprintln!("[Harper Simulation] Researching and critiquing: {}", payload);
    let mut result = PersonaResult::new(Persona::Harper, routing_id.to_string());
    result.output = json!({
        "evidence": ["prior_art_1", "security_risk_X"],
        "concerns": 3
    });
    result.confidence = 0.87;
    result.mutations = vec![json!({"target": "research.md", "action": "append"})];
    result.arena_self_score = json!({
        "correctness": 0.91, "completeness": 0.95, "novelty": 0.72,
        "minimal_diff": 0.60, "provenance_strength": 0.94
    });
    result
}

pub fn fallback_benjamin(payload: &str, routing_id: &str) -> PersonaResult {
    eprintln!("[Benjamin Simulation] Executing implementation work: {}", payload);
    let mut result = PersonaResult::new(Persona::Benjamin, routing_id.to_string());
    result.output = json!({
        "files_changed": 7,
        "tests_passing": true
    });
    result.confidence = 0.83;
    result.mutations = vec![
        json!({"target": "src/auth.rs", "action": "update"}),
        json!({"target": "tests/auth_test.rs", "action": "create"}),
    ];
    result.arena_self_score = json!({
        "correctness": 0.85, "completeness": 0.80, "novelty": 0.55,
        "minimal_diff": 0.90, "provenance_strength": 0.75
    });
    result
}

pub fn fallback_lucas(payload: &str, routing_id: &str) -> PersonaResult {
    eprintln!("[Lucas Simulation] Synthesizing and preparing Arena: {}", payload);
    let mut result = PersonaResult::new(Persona::Lucas, routing_id.to_string());
    result.output = json!({
        "synthesis": "Combined best elements from all agents",
        "hybrid_ready": true
    });
    result.confidence = 0.89;
    result.mutations = vec![json!({"target": "synthesis.md", "action": "create"})];
    result.arena_self_score = json!({
        "correctness": 0.88, "completeness": 0.91, "novelty": 0.78,
        "minimal_diff": 0.82, "provenance_strength": 0.85
    });
    result
}

pub async fn fallback_evaluator(payload: &str, routing_id: &str) -> PersonaResult {
    eprintln!("[Evaluator Simulation] Performing harsh adversarial review: {}", payload);
    let mut ev = Evaluator::new(None);
    let base_text = payload.to_string();
    for i in 0..8 {
        let mut te = TraceEvent {
            agent_id: format!("worker-{}", i % 4),
            ..TraceEvent::default()
        };
        if base_text.contains("churn") || base_text.contains("noisy") || base_text.contains("fail") {
            te.risk_score = 0.71 + (i as f32 * 0.02);
            te.epistemic_confidence = 0.41;
            te.conflict_rate = 0.33;
            te.token_velocity = 195.0;
            te.verified_count_delta = if i % 4 == 0 { 0 } else { -1 };
            te.authority_improvement = 0.03;
            te.surface_text = format!("{} — iteration {} — semantic drift detected", base_text, i);
        } else {
            te.risk_score = 0.32;
            te.epistemic_confidence = 0.78;
            te.conflict_rate = 0.09;
            te.token_velocity = 85.0;
            te.verified_count_delta = 2;
            te.authority_improvement = 0.18;
            te.surface_text = format!("{} — steady verified progress {}", base_text, i);
        }
        ev.ingest(te);
    }
    let verdict = ev.evaluate(Uuid::now_v7()).await;
    let mut result = PersonaResult::new(Persona::Evaluator, routing_id.to_string());
    result.output = json!({
        "verdict_id": verdict.verdict_id.to_string(),
        "overall": verdict.overall,
        "passed_rubrics": verdict.passed_rubrics,
        "total_rubrics": verdict.total_rubrics,
        "semantic_entropy": verdict.semantic_entropy,
        "doom_loop_detected": verdict.doom_loop_detected,
        "productive_death": verdict.productive_death,
        "recommended_action": verdict.recommended_action,
        "justifications": verdict.justifications,
        "critique": format!("Harsh critic (5 rubrics) evaluated the generator output. Live H_sem = {:.3}", verdict.semantic_entropy),
    });
    result.confidence = if verdict.overall == "PASS" { 0.88 } else { 0.94 };
    result.arena_self_score = json!({
        "correctness": 0.96, "completeness": 0.93, "novelty": 0.78,
        "minimal_diff": 0.62, "provenance_strength": 0.91
    });
    result.mutations = vec![json!({
        "target": "evaluation_report.md",
        "action": "create",
        "payload": verdict.justifications.join("\n")
    })];
    result
}

pub async fn run_persona(persona: Persona, payload: &str, routing_id: &str) -> PersonaResult {
    let cfg = crate::llm::KorgConfig::load();
    let provider = crate::llm::build_provider(&cfg);
    run_persona_with_provider(persona, payload, routing_id, provider).await
}

pub async fn run_persona_with_provider(
    persona: Persona,
    payload: &str,
    routing_id: &str,
    provider: Arc<dyn LlmProvider>,
) -> PersonaResult {
    let lp = LlmPersona::new(persona, provider);
    match lp.think(payload, routing_id).await {
        Ok(res) => res,
        Err(e) => {
            eprintln!("[Persona] Live LLM execution failed for {}: {}. Falling back to simulation.", persona.name(), e);
            let mut res = match persona {
                Persona::Captain => fallback_captain(payload, routing_id),
                Persona::Harper => fallback_harper(payload, routing_id),
                Persona::Benjamin => fallback_benjamin(payload, routing_id),
                Persona::Lucas => fallback_lucas(payload, routing_id),
                Persona::Evaluator => fallback_evaluator(payload, routing_id).await,
            };
            res.error_msg = Some(e.to_string());
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{MockProvider, Role, LlmResponse};
    use serde_json::json;

    #[test]
    fn test_parse_structured_response() {
        let raw_response = r#"---
confidence: 0.95
self_score: 0.88
plan_name: "Test Planning Swarm"
---
Here is some thinking markdown...
```json
{
  "key": "value",
  "mutations": [
    {"target": "src/llm.rs", "action": "update"}
  ]
}
```
And some post-script explanation."#;

        let (output, confidence, frontmatter) = parse_structured_response(raw_response);
        
        assert_eq!(confidence, 0.95);
        assert_eq!(frontmatter.get("plan_name").unwrap().as_str().unwrap(), "Test Planning Swarm");
        assert_eq!(output.get("key").unwrap().as_str().unwrap(), "value");
        
        let mutations = output.get("mutations").unwrap().as_array().unwrap();
        assert_eq!(mutations.len(), 1);
        assert_eq!(mutations[0].get("target").unwrap().as_str().unwrap(), "src/llm.rs");
    }

    #[tokio::test]
    async fn test_llm_persona_think_success() {
        let mock_provider = Arc::new(MockProvider::new());
        mock_provider.set_response(Ok(LlmResponse {
            content: r#"---
confidence: 0.92
self_score: 0.94
---
```json
{
  "task_completed": true,
  "mutations": [
    {"target": "plan.md", "action": "create"}
  ]
}
```"#.to_string(),
            usage: crate::llm::TokenUsage {
                prompt_tokens: 15,
                completion_tokens: 10,
                total_tokens: 25,
            },
            model: "mock-model-v1".to_string(),
            finish_reason: crate::llm::FinishReason::Stop,
            tool_calls: None,
        }));

        let persona = LlmPersona::new(Persona::Captain, mock_provider);
        let result = persona.think("Decompose task", "test-routing").await.unwrap();

        assert_eq!(result.persona, Persona::Captain);
        assert_eq!(result.confidence, 0.92);
        assert_eq!(result.output.get("task_completed").unwrap().as_bool().unwrap(), true);
        assert_eq!(result.mutations.len(), 1);
    }
}
