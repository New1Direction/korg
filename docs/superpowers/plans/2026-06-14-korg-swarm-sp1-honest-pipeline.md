# SP1 — Honest Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Benjamin's swarm work flow *honestly* end-to-end so a default, hermetic `korg campaign` produces a `.ktrans` ledger whose attested numbers are real measurements (acid test: attested `mutations_this_round` == real `git diff --numstat` file count; default path has zero fabricated telemetry).

**Architecture:** Three isolated units. **A** — a new `DeterministicProvider` in `korg-llm` (hermetic default; emits an applyable patch or an honest-null). **B** — the worker *child* (`harness.rs::run_task_in_worktree`) applies the patch, measures reality (`git diff --numstat`, 3-state `cargo check`, real tokens, `sysinfo`), and emits real telemetry into the `SwarmTelemetryPulse` `per_agent` JSON + plumbs the file count up to the leader. **C** — de-fabricate the blackboard's default-masking, gate the synthetic injectors behind `--inject-stress`, and derive the ledger fields from the real count.

**Tech Stack:** Rust (tokio, async-trait, serde_json, ed25519-dalek), the `korg-llm` + `korg-runtime` crates. New dep: `sysinfo`. Spec: `docs/superpowers/specs/2026-06-14-korg-swarm-sp1-honest-pipeline-design.md`.

**Branch:** `feat/swarm-honest-pipeline` (already created, spec committed).

**Cross-process note (read before starting Unit B):** A worker is a *separate* `korg worker` OS process. `run_task_in_worktree` runs in the **child**; it returns a `TaskResult` that `handle_route_work` packs into an ACP `SubmitTransaction` (over stdio). The **leader**'s `spawn_worker_process` rebuilds a `PersonaResult` from those ACP messages. So: telemetry for *scoring* travels child → `SwarmTelemetryPulse.per_agent` → `board.ingest_telemetry_pulse`; the file count for the *ledger* travels child → `SubmitTransaction.payload` → `spawn_worker_process` → `PersonaResult` → the persist call site. Both ends change.

**Test commands:** `cargo test -p korg-llm` (Unit A), `cargo test -p korg-runtime` (Units B/C), `cargo test --workspace` + `cargo fmt` + `cargo clippy` (final gate).

---

## Phase 0 — Fixture repo

### Task 0: Hermetic fixture crate Benjamin will "fix"

**Files:**
- Create: `fixtures/honest-demo-repo/Cargo.toml`
- Create: `fixtures/honest-demo-repo/src/lib.rs`

A minimal, self-contained cargo crate with one obvious bug and a failing test. The `DeterministicProvider`'s canonical patch (Task A2) rewrites `src/lib.rs` to fix it; applying it yields a real 1-file diff that compiles and makes the test pass.

- [ ] **Step 1: Create the fixture crate manifest**

`fixtures/honest-demo-repo/Cargo.toml`:
```toml
[package]
name = "honest-demo"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
```

- [ ] **Step 2: Create the buggy source**

`fixtures/honest-demo-repo/src/lib.rs`:
```rust
/// Adds two numbers. (Intentionally buggy: subtracts. Benjamin's canonical patch fixes it.)
pub fn add(a: i64, b: i64) -> i64 {
    a - b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(2, 3), 5);
    }
}
```

- [ ] **Step 3: Verify it builds but the test fails (the "before" state)**

Run: `cargo test --manifest-path fixtures/honest-demo-repo/Cargo.toml`
Expected: compiles, `adds` FAILS (`assert_eq!(add(2,3), 5)` → left `-1`, right `5`).

- [ ] **Step 4: Commit**

```bash
git add fixtures/honest-demo-repo
git commit -m "test(swarm): hermetic fixture crate for SP1 honest pipeline"
```

---

## Unit A — DeterministicProvider (`korg-llm`)

> A new `LlmProvider` that is the hermetic campaign default. Pure function of `(role, task, seed)`. For the fixture task it returns Benjamin's canonical applyable patch; for anything else it returns an **honest null** (`mutations: []`, low confidence). Recovers the role from the System-message text (`LlmRequest` has no role field). `MockProvider` stays untouched.

### Task A1: Extend Benjamin's mutation schema with an applyable field

**Files:**
- Modify: `Prompts/benjamin.md:18-28`

Today the mutation JSON is `{target, action, description}` where `description` is free-text prose — not machine-applyable. Add an explicit `content` field (full new-file bytes) so Unit B can apply it deterministically.

- [ ] **Step 1: Update the JSON action block in the prompt**

Replace the ```json example block (lines ~18-28) with:
```json
{
  "mutations": [
    {
      "target": "src/llm.rs",
      "action": "update",
      "content": "<the COMPLETE new contents of the target file>",
      "description": "Human-readable summary of the edit."
    }
  ]
}
```
And add one sentence below it: `` `content` MUST be the full file body to write (not a diff); `action` is `create` or `update`. ``

- [ ] **Step 2: Commit**

```bash
git add Prompts/benjamin.md
git commit -m "docs(swarm): add applyable 'content' field to Benjamin mutation schema"
```

### Task A2: DeterministicProvider — role recovery, canonical patch, honest-null

**Files:**
- Create: `crates/korg-llm/src/deterministic.rs`
- Modify: `crates/korg-llm/src/lib.rs` (add `mod deterministic; pub use deterministic::DeterministicProvider;` near the other module/exports at the top of the file)
- Test: in `crates/korg-llm/src/deterministic.rs` (`#[cfg(test)] mod tests`)

**Reference shapes (verbatim, do not guess):**
```rust
// trait (lib.rs:158, annotated #[async_trait])
pub trait LlmProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
    async fn complete_stream(&self, req: LlmRequest)
        -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError>;
}
// Message (lib.rs:29): { role: Role, content: String, name: Option<String>, tool_calls: Option<Vec<ToolCall>> }
// Role (lib.rs:22): enum { System, User, Assistant, Tool }
// LlmResponse (lib.rs:104): { content: String, usage: TokenUsage, model: String, finish_reason: FinishReason, tool_calls: Option<Vec<ToolCall>> }
// TokenUsage (lib.rs:88): { prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 }
```

- [ ] **Step 1: Write the failing tests**

`crates/korg-llm/src/deterministic.rs` (test module at the bottom):
```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p korg-llm deterministic 2>&1 | head -30`
Expected: FAIL — `DeterministicProvider` / `parse_for_test` not defined.

- [ ] **Step 3: Implement `DeterministicProvider`**

Top of `crates/korg-llm/src/deterministic.rs`:
```rust
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
use futures::stream::{self, Stream};
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
```

In `crates/korg-llm/src/lib.rs`, near the top module declarations / the `pub use` exports, add:
```rust
mod deterministic;
pub use deterministic::DeterministicProvider;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p korg-llm deterministic 2>&1 | tail -20`
Expected: PASS (4 tests). If `futures::stream` import errors, confirm `futures` is already a dep of `korg-llm` (it is — `MockProvider::complete_stream` uses `stream::iter`).

- [ ] **Step 5: Commit**

```bash
git add crates/korg-llm/src/deterministic.rs crates/korg-llm/src/lib.rs
git commit -m "feat(llm): DeterministicProvider — hermetic role-aware honest stub"
```

### Task A3: Make DeterministicProvider the hermetic campaign default

**Files:**
- Modify: `crates/korg-llm/src/lib.rs:2097-2208` (`build_provider_with` — add the `"deterministic"` arm)
- Modify: `crates/korg-llm/src/lib.rs:1853` (`from_env` default) and `:2051` (`load` default): `"mock"` → `"deterministic"`
- Test: `crates/korg-llm/src/lib.rs` tests module

> The wildcard `_ => Arc::new(MockProvider::new())` means flipping the default string alone is **inert** — the arm must exist. Both land together.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `lib.rs` (mirror `test_mock_provider_completes_offline`'s 14-field request boilerplate):
```rust
#[tokio::test]
async fn build_provider_default_is_deterministic_and_honest() {
    let mut cfg = KorgConfig::from_env();
    cfg.default_llm = "deterministic".to_string();
    let provider = build_provider(&cfg); // wrapped in ResilientLlmProvider
    let request = LlmRequest {
        messages: vec![
            Message { role: Role::System, content: "You are Benjamin, the Builder & Implementer.".to_string(), name: None, tool_calls: None },
            Message { role: Role::User, content: "Implement a distributed consensus protocol".to_string(), name: None, tool_calls: None },
        ],
        temperature: 0.3, max_tokens: None, tools: None, stop_sequences: None,
        multimodal: None, tx_id: None, session_id: None, policy_hash: None,
        top_p: None, presence_penalty: None, frequency_penalty: None,
    };
    let resp = provider.complete(request).await.unwrap();
    // honest null for an unknown task: empty mutations, NOT the mock echo string
    assert!(!resp.content.contains("[Mock Response to:"), "default must not be the mock echo");
    assert!(resp.content.contains("\"mutations\": []"), "unknown task → honest null");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p korg-llm build_provider_default_is_deterministic 2>&1 | tail -20`
Expected: FAIL — the `"deterministic"` string hits the wildcard → `MockProvider` → content contains `[Mock Response to:`.

- [ ] **Step 3: Add the `"deterministic"` arm and flip the defaults**

In `build_provider_with` (`lib.rs:2097`), add an arm before the wildcard `_ =>`:
```rust
        "deterministic" => Arc::new(DeterministicProvider::new()),
```
At `lib.rs:1853` (`from_env`):
```rust
            default_llm: std::env::var("KORG_DEFAULT_LLM").unwrap_or_else(|_| "deterministic".to_string()),
```
At `lib.rs:2051` (`load`):
```rust
            default_llm: default_llm.unwrap_or_else(|| "deterministic".to_string()),
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p korg-llm build_provider_default_is_deterministic 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Run the whole korg-llm suite (catch default-flip fallout)**

Run: `cargo test -p korg-llm 2>&1 | tail -20`
Expected: all PASS. `test_mock_provider_completes_offline` still passes (it constructs `MockProvider::new()` directly). If `test_toml_config_parsing_and_defaults` asserts a `"mock"` default anywhere, it does not (it asserts `"grok"` from a TOML literal) — leave it.

- [ ] **Step 6: Commit**

```bash
git add crates/korg-llm/src/lib.rs
git commit -m "feat(llm): make DeterministicProvider the hermetic campaign default"
```

---

## Unit B — Observation layer (worker child, `korg-runtime`)

> A new `observation.rs` module holds the pure, testable apply+measure primitives. `harness.rs::run_task_in_worktree` (the worker-child body) calls them after `run_persona`, emits the real metrics into the `SwarmTelemetryPulse.per_agent` JSON, and plumbs the file count up to the leader via the `SubmitTransaction` payload.

### Task B1: 3-state cargo check primitive

**Files:**
- Create: `crates/korg-runtime/src/observation.rs`
- Modify: `crates/korg-runtime/src/lib.rs` (add `pub mod observation;` with the other `mod`/`pub mod` declarations)
- Test: in `observation.rs`

> The existing `get_cargo_check_stderr` (`workers.rs:831`) returns `Option<String>` = `Some` only on failure, `None` on **both** success and cargo-absent — it cannot tell "passed" from "missing", which §6 requires. New 3-state primitive.

- [ ] **Step 1: Write the failing test**

`crates/korg-runtime/src/observation.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("korg-obs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn git(dir: &std::path::Path, args: &[&str]) {
        tokio::process::Command::new("git").args(args).current_dir(dir)
            .output().await.unwrap();
    }

    async fn init_crate(dir: &std::path::Path, lib_body: &str) {
        std::fs::write(dir.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"src/lib.rs\"\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), lib_body).unwrap();
        git(dir, &["init", "-q"]).await;
        git(dir, &["add", "-A"]).await;
        git(dir, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]).await;
    }

    #[tokio::test]
    async fn cargo_check_passed_on_valid_crate() {
        let d = tmp();
        init_crate(&d, "pub fn f() -> i64 { 1 }\n").await;
        assert!(matches!(cargo_check(&d).await, CargoCheck::Passed));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn cargo_check_failed_on_broken_crate() {
        let d = tmp();
        init_crate(&d, "pub fn f() -> i64 { \"nope\" }\n").await;
        assert!(matches!(cargo_check(&d).await, CargoCheck::Failed(_)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p korg-runtime observation::tests::cargo_check 2>&1 | head -20`
Expected: FAIL — `cargo_check` / `CargoCheck` not defined.

- [ ] **Step 3: Implement the 3-state primitive**

Top of `crates/korg-runtime/src/observation.rs`:
```rust
//! observation.rs — pure, testable "measure reality" primitives for the worker.
//!
//! The worker child applies a persona's patch to its git worktree, then measures
//! the real consequences (compile result, diff size, tokens, load) so the ledger
//! attests facts instead of fabricated numbers. Each primitive is independently
//! testable against a throwaway git repo.

use std::path::Path;

/// Three-state compile result. `Unavailable` (cargo missing / failed to spawn)
/// is distinct from `Passed` — §6 requires that distinction so a degraded host
/// records `tool_unavailable`, never a fabricated pass.
#[derive(Debug, Clone)]
pub enum CargoCheck {
    Passed,
    Failed(String),
    Unavailable,
}

/// Run `cargo check` in `worktree` and classify the outcome.
pub async fn cargo_check(worktree: &Path) -> CargoCheck {
    match tokio::process::Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(worktree)
        .output()
        .await
    {
        Ok(o) if o.status.success() => CargoCheck::Passed,
        Ok(o) => CargoCheck::Failed(String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(_) => CargoCheck::Unavailable, // cargo absent / failed to spawn
    }
}
```
Add `pub mod observation;` to `crates/korg-runtime/src/lib.rs` alongside the other module declarations.

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p korg-runtime observation::tests::cargo_check 2>&1 | tail -15`
Expected: PASS (2 tests). (These spawn real `cargo check` on a temp crate — a few seconds each.)

- [ ] **Step 5: Commit**

```bash
git add crates/korg-runtime/src/observation.rs crates/korg-runtime/src/lib.rs
git commit -m "feat(swarm): 3-state cargo_check observation primitive"
```

### Task B2: `git diff --numstat` primitive

**Files:**
- Modify: `crates/korg-runtime/src/observation.rs`

- [ ] **Step 1: Write the failing test**

Add to `observation.rs` tests (reuses the `tmp`/`git`/`init_crate` helpers from B1):
```rust
    #[tokio::test]
    async fn numstat_counts_a_modified_file() {
        let d = tmp();
        init_crate(&d, "pub fn f() -> i64 { 1 }\n").await;
        std::fs::write(d.join("src/lib.rs"), "pub fn f() -> i64 { 2 }\n").unwrap();
        let n = numstat(&d).await;
        assert_eq!(n.files, 1, "one file changed");
        assert!(n.added >= 1 && n.removed >= 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn numstat_counts_a_new_file() {
        let d = tmp();
        init_crate(&d, "pub fn f() -> i64 { 1 }\n").await;
        std::fs::write(d.join("src/extra.rs"), "pub fn g() {}\n").unwrap();
        let n = numstat(&d).await;
        assert_eq!(n.files, 1, "a newly created file counts");
        let _ = std::fs::remove_dir_all(&d);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p korg-runtime observation::tests::numstat 2>&1 | head -15`
Expected: FAIL — `numstat` not defined.

- [ ] **Step 3: Implement `numstat`**

Add to `observation.rs`:
```rust
/// Real diff size of the worktree vs HEAD. Stages all changes (`git add -A`) so
/// new *and* modified files are counted, then parses `git diff --cached --numstat`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Numstat {
    pub files: usize,
    pub added: u64,
    pub removed: u64,
}

pub async fn numstat(worktree: &Path) -> Numstat {
    let _ = tokio::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree)
        .output()
        .await;
    let out = tokio::process::Command::new("git")
        .args(["diff", "--cached", "--numstat"])
        .current_dir(worktree)
        .output()
        .await;
    let mut n = Numstat::default();
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let mut cols = line.split('\t');
            let added = cols.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0); // "-" (binary) → 0
            let removed = cols.next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            if cols.next().is_some() {
                n.files += 1;
                n.added += added;
                n.removed += removed;
            }
        }
    }
    n
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p korg-runtime observation::tests::numstat 2>&1 | tail -15`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/korg-runtime/src/observation.rs
git commit -m "feat(swarm): git diff --numstat observation primitive"
```

### Task B3: Apply-mutations primitive

**Files:**
- Modify: `crates/korg-runtime/src/observation.rs`

> Net-new: nothing applies persona mutations today. Parse `{target, action, content}` and write `content` to `target` (creating parent dirs). A mutation lacking an applyable `content` counts as a reject (honest — it cannot be applied).

- [ ] **Step 1: Write the failing test**

Add to `observation.rs` tests:
```rust
    #[tokio::test]
    async fn apply_writes_content_and_counts_rejects() {
        let d = tmp();
        std::fs::create_dir_all(d.join("src")).unwrap();
        let muts = vec![
            serde_json::json!({"target":"src/lib.rs","action":"update","content":"pub fn f() -> i64 { 2 }\n"}),
            serde_json::json!({"target":"src/x.rs","action":"update","description":"no content field"}),
        ];
        let outcome = apply_mutations(&d, &muts).await;
        assert_eq!(outcome.applied, 1);
        assert_eq!(outcome.rejected, 1, "a mutation with no applyable content is a reject");
        assert_eq!(std::fs::read_to_string(d.join("src/lib.rs")).unwrap(), "pub fn f() -> i64 { 2 }\n");
        assert!((outcome.conflict_rate - 0.5).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&d);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p korg-runtime observation::tests::apply 2>&1 | head -15`
Expected: FAIL — `apply_mutations` not defined.

- [ ] **Step 3: Implement `apply_mutations`**

Add to `observation.rs`:
```rust
/// Outcome of applying a persona's mutations to the worktree.
#[derive(Debug, Clone, Default)]
pub struct ApplyOutcome {
    pub applied: usize,
    pub rejected: usize,
    /// rejected / total — feeds the honest `conflict_rate` (0.0 = all clean).
    pub conflict_rate: f32,
}

/// Write each mutation's `content` to its `target` (relative to `worktree`).
/// A mutation without an applyable string `content` is rejected, not faked.
pub async fn apply_mutations(worktree: &Path, mutations: &[serde_json::Value]) -> ApplyOutcome {
    let mut out = ApplyOutcome::default();
    for m in mutations {
        let target = m.get("target").and_then(|v| v.as_str());
        let content = m.get("content").and_then(|v| v.as_str());
        match (target, content) {
            (Some(rel), Some(body)) => {
                let path = worktree.join(rel);
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&path, body).is_ok() {
                    out.applied += 1;
                } else {
                    out.rejected += 1;
                }
            }
            _ => out.rejected += 1, // no applyable content
        }
    }
    let total = (out.applied + out.rejected) as f32;
    out.conflict_rate = if total > 0.0 { out.rejected as f32 / total } else { 0.0 };
    out
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p korg-runtime observation::tests::apply 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/korg-runtime/src/observation.rs
git commit -m "feat(swarm): apply_mutations observation primitive (net-new)"
```

### Task B4: Honest metric mapping + `sysinfo` load proxy

**Files:**
- Modify: `crates/korg-runtime/Cargo.toml` (add `sysinfo`)
- Modify: `crates/korg-runtime/src/observation.rs`

> Composes the measurements into the `per_agent` JSON body the blackboard reads. Takes `cpu_load` as a parameter (caller probes `sysinfo`) so the mapping is deterministic + unit-testable.

- [ ] **Step 1: Add the `sysinfo` dependency**

In `crates/korg-runtime/Cargo.toml` under `[dependencies]`:
```toml
sysinfo = "0.30"
```
Run `cargo fetch -p korg-runtime` (or `cargo build -p korg-runtime`) to confirm it resolves.

- [ ] **Step 2: Write the failing test**

Add to `observation.rs` tests:
```rust
    #[test]
    fn honest_metrics_map_real_measurements() {
        // A clean apply that compiles → low risk, high confidence, positive verified delta.
        let m = honest_metrics(
            &ApplyOutcome { applied: 1, rejected: 0, conflict_rate: 0.0 },
            &CargoCheck::Passed,
            &Numstat { files: 1, added: 1, removed: 1 },
            120, 2.0, 0.30, "fixed add()",
        );
        assert!(m["risk_score"].as_f64().unwrap() < 0.4);
        assert!(m["epistemic_confidence"].as_f64().unwrap() > 0.6);
        assert_eq!(m["verified_count_delta"].as_i64().unwrap(), 1);
        assert!((m["token_velocity"].as_f64().unwrap() - 60.0).abs() < 1e-6); // 120 tok / 2.0 s
        assert_eq!(m["conflict_rate"].as_f64().unwrap(), 0.0);
        assert_eq!(m["gpu_util"].as_f64().unwrap(), 0.30);
    }

    #[test]
    fn honest_metrics_failed_compile_is_high_risk_zero_verified() {
        let m = honest_metrics(
            &ApplyOutcome { applied: 1, rejected: 0, conflict_rate: 0.0 },
            &CargoCheck::Failed("E0308".into()),
            &Numstat { files: 1, added: 5, removed: 0 },
            80, 1.0, 0.1, "broke the build",
        );
        assert!(m["risk_score"].as_f64().unwrap() > 0.6);
        assert_eq!(m["verified_count_delta"].as_i64().unwrap(), 0);
    }

    #[test]
    fn honest_metrics_unavailable_cargo_marks_tool_unavailable() {
        let m = honest_metrics(
            &ApplyOutcome { applied: 1, rejected: 0, conflict_rate: 0.0 },
            &CargoCheck::Unavailable,
            &Numstat { files: 1, added: 1, removed: 0 },
            10, 1.0, 0.0, "no cargo here",
        );
        assert_eq!(m["verified_count_delta"].as_i64().unwrap(), 0);
        assert_eq!(m["tool_unavailable"].as_bool(), Some(true));
    }
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p korg-runtime observation::tests::honest_metrics 2>&1 | head -15`
Expected: FAIL — `honest_metrics` / `cpu_load_proxy` not defined.

- [ ] **Step 4: Implement the mapping + the load proxy**

Add to `observation.rs` (named constants, no inline magic numbers):
```rust
// Honest mapping constants (tunable; named so the policy is explicit).
const RISK_PASS: f64 = 0.20;
const RISK_FAIL: f64 = 0.75;
const BLAST_PER_FILE: f64 = 0.05; // risk add per changed file, capped
const BLAST_CAP: f64 = 0.20;
const CONF_PASS: f64 = 0.85;
const CONF_FAIL: f64 = 0.25;

/// Probe real system CPU load as an honest compute-utilization proxy for the
/// `gpu_util` wire field. Returns 0.0 only if the probe yields nothing.
pub fn cpu_load_proxy() -> f64 {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    let cpus = sys.cpus();
    if cpus.is_empty() {
        return 0.0;
    }
    let avg = cpus.iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64;
    (avg / 100.0).clamp(0.0, 1.0)
}

/// Compose real measurements into the `per_agent` metrics JSON the blackboard
/// reads (keys must match `metrics_to_trace_event`). Every value is derived from
/// an observed fact — nothing invented.
#[allow(clippy::too_many_arguments)]
pub fn honest_metrics(
    apply: &ApplyOutcome,
    check: &CargoCheck,
    numstat: &Numstat,
    tokens: u32,
    elapsed_secs: f64,
    cpu_load: f64,
    surface_text: &str,
) -> serde_json::Value {
    let passed = matches!(check, CargoCheck::Passed);
    let unavailable = matches!(check, CargoCheck::Unavailable);
    let blast = (numstat.files as f64 * BLAST_PER_FILE).min(BLAST_CAP);
    let risk = if passed { RISK_PASS } else { RISK_FAIL } + if passed { blast } else { 0.0 };
    let confidence = if passed { CONF_PASS } else { CONF_FAIL };
    // verified delta: a real compile pass is +1; fail or unavailable is 0 (never faked).
    let verified: i64 = if passed { 1 } else { 0 };
    let velocity = if elapsed_secs > 0.0 { tokens as f64 / elapsed_secs } else { 0.0 };
    serde_json::json!({
        "phase": "complete",
        "risk_score": risk.clamp(0.0, 1.0),
        "epistemic_confidence": confidence,
        "conflict_rate": apply.conflict_rate,
        "token_velocity": velocity,
        "gpu_util": cpu_load.clamp(0.0, 1.0),
        "verified_count_delta": verified,
        "authority_improvement": if verified > 0 { 0.1 } else { 0.0 },
        "surface_text": surface_text,
        "files_changed": numstat.files,
        "tool_unavailable": unavailable,
    })
}
```

- [ ] **Step 5: Run it to verify it passes**

Run: `cargo test -p korg-runtime observation::tests::honest_metrics 2>&1 | tail -15`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/korg-runtime/Cargo.toml crates/korg-runtime/src/observation.rs
git commit -m "feat(swarm): honest_metrics mapping + sysinfo load proxy"
```

### Task B5: Wire apply+measure into the worker child + emit + plumb the count up

**Files:**
- Modify: `crates/korg-runtime/src/harness.rs:521-561` (`run_task_in_worktree`)
- Modify: `crates/korg-runtime/src/harness.rs:617-626` (`TaskResult` struct — add `files_changed`)
- Modify: `crates/korg-runtime/src/harness.rs:422-429` (completion pulse — emit real `per_agent`)
- Modify: `crates/korg-runtime/src/harness.rs` (`handle_route_work` `SubmitTransaction` payload — include `files_changed`)
- Modify: `crates/korg-runtime/src/personas.rs:61-88` (`PersonaResult` — add `files_changed: usize`, default 0 in `::new`)
- Modify: `crates/korg-runtime/src/workers.rs:653-779` (`spawn_worker_process` — read `files_changed` off the `SubmitTransaction` payload onto `PersonaResult`)
- Modify: `crates/korg-runtime/src/harness.rs:581-615` (delete dead `build_live_evolving_pulse`)

> This is the integration task (no isolated unit test — covered by the keystone end-to-end test in Task C5). Make the edits, then assert it compiles + the suite stays green.

- [ ] **Step 1: Add `files_changed` to `PersonaResult`**

In `personas.rs`, add the field to the struct (after `mutations`) and to `PersonaResult::new` (default `0`):
```rust
    pub mutations: Vec<serde_json::Value>,
    pub files_changed: usize,
```
```rust
            mutations: vec![],
            files_changed: 0,
```
The `PersonaResult { ... }` literal in `LlmPersona::think` (personas.rs ~312-322) must also gain `files_changed: 0,` (the worker child fills it in B5 step 2; the leader fills it in step 5).

- [ ] **Step 2: Apply + measure in `run_task_in_worktree`**

In `harness.rs::run_task_in_worktree`, between the `run_persona(...)` call and the `git add .`, insert apply+measure and capture `files_changed`:
```rust
        let persona_result = run_persona(persona, payload, "worker-task").await;

        // Honest observation: apply the persona's patch to THIS worktree, then
        // measure reality. The worktree is the process CWD (set at harness.rs:267).
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let started = std::time::Instant::now();
        let apply = crate::observation::apply_mutations(&cwd, &persona_result.mutations).await;
        let numstat = crate::observation::numstat(&cwd).await;
        let check = crate::observation::cargo_check(&cwd).await;
        let elapsed = started.elapsed().as_secs_f64().max(1e-3);
        let cpu = crate::observation::cpu_load_proxy();
        let surface = format!(
            "{} applied {} file(s), {} added / {} removed",
            persona.name(), numstat.files, numstat.added, numstat.removed
        );
        // Real token usage if the provider surfaced it (Unit A reports it); else 0.
        let tokens = persona_result.output.get("__tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        self.last_observation = Some(crate::observation::honest_metrics(
            &apply, &check, &numstat, tokens, elapsed, cpu, &surface,
        ));
        let files_changed = numstat.files;
```
Then add `files_changed` to the returned `TaskResult`:
```rust
        Ok(TaskResult {
            mutations: persona_result.mutations,
            doom_loop: false,
            provenance: vec![format!("persona:{}", persona.name())],
            confidence: persona_result.confidence,
            arena_scores: persona_result.arena_self_score.clone(),
            codebase_merkle_root,
            files_changed,
        })
```
Add the field to the `TaskResult` struct (harness.rs:617):
```rust
    codebase_merkle_root: String,
    files_changed: usize,
```
Add a field to `SingleWorkerHarness` to stash the observation for the completion pulse (near its other fields), e.g. `last_observation: Option<serde_json::Value>` defaulting to `None` in its constructor.

> Note on `__tokens`: Unit A's `DeterministicProvider` reports `usage` on `LlmResponse`, but `LlmPersona::think` discards it (personas.rs:279). To surface it without changing the provider contract, in `think` add the total to `output` before building `PersonaResult`: `if let Some(obj) = output.as_object_mut() { obj.insert("__tokens".into(), serde_json::json!(response.usage.total_tokens)); }` (immediately after the `parse_structured_response` call). This keeps the token count on the in-process `PersonaResult.output` the worker child reads.

- [ ] **Step 3: Emit the real metrics in the completion pulse**

In `harness.rs` (the completion pulse at ~422), replace the stubbed `per_agent`:
```rust
        let obs = self.last_observation.clone()
            .unwrap_or_else(|| serde_json::json!({"phase": "complete"}));
        let completion_pulse = AcpMessage::SwarmTelemetryPulse {
            agent_id: self.worker_id.clone(),
            per_agent: serde_json::json!({ self.worker_id.clone(): obs }),
            aggregate: serde_json::json!({}),
            scaling_recommendation: None,
        };
        let _ = client.send(&completion_pulse).await;
```

- [ ] **Step 4: Include `files_changed` in the `SubmitTransaction` payload**

In `handle_route_work` where the `SubmitTransaction` payload JSON is built (harness.rs ~403-412, the object carrying `mutations`/`doom_loop`/`provenance`/`codebase_merkle_root`), add:
```rust
            "files_changed": result.files_changed,
```

- [ ] **Step 5: Read `files_changed` off the transaction in the leader**

In `workers.rs::spawn_worker_process`, where `last_tx` is consumed onto `res`:
```rust
    if let Some(tx) = last_tx {
        res.output = tx.clone();
        if let Some(muts) = tx.get("mutations").and_then(|v| v.as_array()) {
            res.mutations = muts.clone();
        }
        if let Some(fc) = tx.get("files_changed").and_then(|v| v.as_u64()) {
            res.files_changed = fc as usize;
        }
    }
```

- [ ] **Step 6: Delete the dead synthetic helper**

Remove `build_live_evolving_pulse` (harness.rs ~581-615) — it is never called (confirmed by `grep -rn build_live_evolving_pulse crates/`) and exists only to fabricate sine-based pulses; leaving it invites accidental rewiring.

- [ ] **Step 7: Compile + run the korg-runtime suite**

Run: `cargo test -p korg-runtime 2>&1 | tail -25`
Expected: compiles; existing tests pass. Fix any `PersonaResult { ... }` / `TaskResult { ... }` literals the compiler flags for the new fields (the compiler lists each — `fallback_*` go through `::new` so they get the default).

- [ ] **Step 8: Commit**

```bash
git add crates/korg-runtime/src/harness.rs crates/korg-runtime/src/personas.rs crates/korg-runtime/src/workers.rs
git commit -m "feat(swarm): worker applies + measures real work, emits honest telemetry, plumbs file count"
```

---

## Unit C — Honest scoring + ledger (`korg-runtime`)

> De-fabricate the blackboard's default-masking, gate the synthetic injectors behind `--inject-stress`, and derive the ledger fields from the real file count plumbed up in Unit B.

### Task C1: De-fabricate `metrics_to_trace_event` (no signal → no event)

**Files:**
- Modify: `crates/korg-runtime/src/blackboard.rs:282-359` (`metrics_to_trace_event`)
- Test: `crates/korg-runtime/src/blackboard.rs` tests module (~line 392)

> Today every `unwrap_or` invents a plausible mid-range value (risk 0.35, gpu 0.45, velocity 70.0…) for a missing key — so a content-free `{"phase":"start"}` pulse becomes a fabricated trace event. Honest fix: a metrics object with **none** of the real signal keys yields **no** event (honest absence). Real `honest_metrics` pulses (all keys present) map through unchanged.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `blackboard.rs` (construct the `Blackboard` the same way the existing tests at ~line 392 do):
```rust
    #[test]
    fn content_free_pulse_yields_no_fabricated_event() {
        let mut bb = Blackboard::new(); // mirror the existing tests' constructor
        let stub = AcpMessage::SwarmTelemetryPulse {
            agent_id: "w".into(),
            per_agent: serde_json::json!({ "w": { "phase": "start" } }),
            aggregate: serde_json::json!({}),
            scaling_recommendation: None,
        };
        assert!(bb.ingest_telemetry_pulse(&stub, None).is_empty(),
            "a pulse with no real signal must not become an invented trace event");
    }

    #[test]
    fn real_signal_pulse_maps_through_with_observed_values() {
        let mut bb = Blackboard::new();
        let real = AcpMessage::SwarmTelemetryPulse {
            agent_id: "w".into(),
            per_agent: serde_json::json!({ "w": {
                "risk_score": 0.7, "epistemic_confidence": 0.3, "verified_count_delta": 0
            }}),
            aggregate: serde_json::json!({}),
            scaling_recommendation: None,
        };
        let evs = bb.ingest_telemetry_pulse(&real, None);
        assert_eq!(evs.len(), 1);
        assert!((evs[0].risk_score - 0.7).abs() < 1e-6);
        assert_eq!(evs[0].verified_count_delta, 0);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p korg-runtime content_free_pulse_yields_no_fabricated_event 2>&1 | tail -15`
Expected: FAIL — today the stub pulse produces one fully-defaulted event, so `.is_empty()` is false.

- [ ] **Step 3: Add the no-signal guard**

At the top of `metrics_to_trace_event` (`blackboard.rs:282`), before reading any field:
```rust
    fn metrics_to_trace_event(&self, agent_id: &str, m: &serde_json::Value) -> Option<TraceEvent> {
        // Honest absence: a metrics object carrying none of the real observation
        // keys is NOT a measurement — do not fabricate a mid-range event for it.
        const SIGNAL_KEYS: [&str; 6] = [
            "risk_score", "epistemic_confidence", "conflict_rate",
            "token_velocity", "gpu_util", "verified_count_delta",
        ];
        if !SIGNAL_KEYS.iter().any(|k| m.get(k).is_some()) {
            return None;
        }
        // (existing per-field extraction continues unchanged below)
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p korg-runtime _pulse 2>&1 | tail -15`
Expected: PASS (both tests). The empty-fallback in `ingest_telemetry_pulse` also returns `None` now for the `{"w":{...}}` shape (no top-level signal keys), so no event leaks through it.

- [ ] **Step 5: Commit**

```bash
git add crates/korg-runtime/src/blackboard.rs
git commit -m "fix(swarm): metrics_to_trace_event no longer fabricates defaults for signal-free pulses"
```

### Task C2: Gate the synthetic injectors behind `--inject-stress`

**Files:**
- Modify: `crates/korg-runtime/src/leader.rs` (`LeaderOrchestrator` struct + `new` — add `inject_stress: bool`; add `set_inject_stress`)
- Modify: `crates/korg-runtime/src/leader.rs:1517-1533` (stress_event), `:1714-1729` (per-round synthetic), `:897-921` (captain-async-planner)
- Modify: `src/main.rs` (Campaign / default command — add `--inject-stress` flag, thread into the leader)
- Test: `crates/korg-runtime/src/leader.rs` tests

> Default off → the evaluator scores only real `live_events`. The flag (for demos / fault-injection) re-enables the synthetic signal.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `leader.rs`:
```rust
    #[tokio::test]
    async fn inject_stress_defaults_off() {
        let leader = LeaderOrchestrator::new("task".to_string(), None);
        assert!(!leader.inject_stress, "the default campaign path must inject no synthetic signal");
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p korg-runtime inject_stress_defaults_off 2>&1 | tail -15`
Expected: FAIL — no `inject_stress` field.

- [ ] **Step 3: Add the field + setter, and gate every injector**

Add `pub inject_stress: bool,` to the `LeaderOrchestrator` struct; initialize `inject_stress: false,` in `LeaderOrchestrator::new`. Add:
```rust
    pub fn set_inject_stress(&mut self, on: bool) {
        self.inject_stress = on;
    }
```
Wrap the stress_event block (`leader.rs:1519-1533`):
```rust
            if self.inject_stress {
                let stress_event = TraceEvent { /* ...unchanged... */ };
                self.evaluator.ingest(stress_event);
            }
```
Wrap the per-round synthetic block (`leader.rs:1719-1729`):
```rust
                if self.inject_stress {
                    let synthetic = TraceEvent { /* ...unchanged... */ };
                    self.evaluator.ingest(synthetic);
                }
```
Wrap the captain-async-planner `tokio::spawn` (`leader.rs:899-920`) so the whole fabricating task only spawns under the flag:
```rust
            if self.inject_stress {
                let bb_clone = self.telemetry_blackboard.clone();
                let description_clone = description.clone();
                tokio::spawn(async move { /* ...unchanged... */ });
            }
```

- [ ] **Step 4: Thread the CLI flag**

In `src/main.rs`, add a `--inject-stress` boolean flag to the campaign/default command's args and, after constructing the leader, call `leader.set_inject_stress(args.inject_stress)` before running. (Follow the existing flag-parsing pattern in that command — e.g. how `--headless`/cognition mode are handled.)

- [ ] **Step 5: Run it to verify it passes + suite stays green**

Run: `cargo test -p korg-runtime inject_stress_defaults_off 2>&1 | tail -10 && cargo test -p korg-runtime 2>&1 | tail -15`
Expected: PASS. `test_playhead_steering_fork_campaign_reset` still passes (it does not assert on scores; `swarm_size >= 6` is unaffected by removing synthetic *telemetry*).

- [ ] **Step 6: Commit**

```bash
git add crates/korg-runtime/src/leader.rs src/main.rs
git commit -m "feat(swarm): gate synthetic telemetry injectors behind --inject-stress (default off)"
```

### Task C3: Drop the TUI jitter; keep the real scores

**Files:**
- Modify: `crates/korg-runtime/src/leader.rs:1607-1612` (the `scores:` array in `PersonaTelemetry`)
- Modify: `crates/korg-runtime/src/leader.rs:1613-1683` (synthetic `crdt_sync_frequency`/`conflicts_count`/`lock_states`)

> `real_scores` is genuine; only the `+ sin()/cos()*0.02` jitter is cosmetic fabrication. Send the raw real scores. The lock/CRDT sub-fields have no real subsystem — zero them on the default path (gating the whole `try_send` would blank the real panel). Keep the `ScaleTelemetry` block after (`:1684-1705`) — it reads real atomics.

- [ ] **Step 1: Send raw real scores (remove jitter)**

Replace the `scores:` array:
```rust
                        scores: real_scores,
```

- [ ] **Step 2: Zero the fabricated sub-fields on the default path**

Replace the synthetic `crdt_sync_frequency`, `conflicts_count`, and `lock_states` with honest values when stress is off (there is no real lock/CRDT subsystem feeding them):
```rust
                        telemetry_merges: (round * 12) as u32,
                        crdt_sync_frequency: if self.inject_stress { 1.2 + (round as f32 * 0.15) } else { 0.0 },
                        conflicts_count: if self.inject_stress { (round / 3) as u32 } else { 0 },
                        provenance_chain_length: (round + 1) as u32,
                        lock_states: if self.inject_stress {
                            vec![ /* ...the existing 4 fabricated tuples, unchanged... */ ]
                        } else {
                            vec![]
                        },
```

- [ ] **Step 3: Compile + run the suite**

Run: `cargo test -p korg-runtime 2>&1 | tail -15`
Expected: compiles, green. (TUI rendering is visual; the keystone is that the dashboard still receives the real scores, just without jitter.)

- [ ] **Step 4: Commit**

```bash
git add crates/korg-runtime/src/leader.rs
git commit -m "fix(swarm): TUI shows raw real scores (no jitter); fabricated lock/CRDT sub-fields gated"
```

### Task C4: Ledger attests the real file count

**Files:**
- Modify: `crates/korg-runtime/src/leader.rs:1451` (compute `real_files_changed` from `results`)
- Modify: `crates/korg-runtime/src/leader.rs` (`LeaderOrchestrator` — add `total_real_mutations: usize`, init 0)
- Modify: `crates/korg-runtime/src/leader.rs:1739-1747` (call site — pass real count, drop `+ round%2`)
- Modify: `crates/korg-runtime/src/leader.rs:612-719` (`persist_campaign_ktrans` — add `total_mutations_so_far` param; replace BOTH `(round+1)*5`)
- Modify: `crates/korg-runtime/src/leader.rs:1603-1604` (TUI `Arena.mutations` — use the real count for consistency)

> `results` (now carrying `.files_changed` from Unit B) is in scope from `:1451` through the persist call at `:1740` in the same loop iteration. `blast_radius` is already a function of `mutations_this_round`, so it becomes real for free once the count is real. The two `(round+1)*5` literals (payload + struct) MUST change together or the `tx_hash` won't match.

- [ ] **Step 1: Write the failing test**

Add to `leader.rs` tests:
```rust
    #[test]
    fn real_mutation_count_replaces_synthetic_formula() {
        // The honest count is the sum of per-worker files_changed, NOT
        // arena_outcome["mutations"].unwrap_or(3) + round%2.
        use crate::personas::{Persona, PersonaResult};
        let mut a = PersonaResult::new(Persona::Benjamin, "r".into());
        a.files_changed = 1;
        let mut b = PersonaResult::new(Persona::Harper, "r".into());
        b.files_changed = 0;
        let results = vec![a, b];
        let real: usize = results.iter().map(|r| r.files_changed).sum();
        assert_eq!(real, 1, "the attested count is the real per-worker sum");
    }
```

- [ ] **Step 2: Run it to verify it fails (compile-driven)**

Run: `cargo test -p korg-runtime real_mutation_count_replaces 2>&1 | tail -15`
Expected: FAIL to compile until `PersonaResult.files_changed` exists (it does after Unit B). If Unit B is committed, this test PASSES immediately — it documents the invariant; proceed to wire it into the campaign loop.

- [ ] **Step 3: Compute the real count in the loop**

After `let results = final_results;` (`leader.rs:1451`), add:
```rust
            let real_files_changed: usize = results.iter().map(|r| r.files_changed).sum();
            self.total_real_mutations += real_files_changed;
```
Add `pub total_real_mutations: usize,` to the struct; init `total_real_mutations: 0,` in `new`.

- [ ] **Step 4: Pass the real count at the persist call site**

Replace `leader.rs:1743`:
```rust
                    arena_outcome["mutations"].as_u64().unwrap_or(3) as usize + (round % 2),
```
with:
```rust
                    real_files_changed,
```
And the TUI `Arena.mutations` at `:1603-1604`:
```rust
                        mutations: real_files_changed,
```

- [ ] **Step 5: Make `total_mutations_so_far` real (both literals)**

Add a parameter to `persist_campaign_ktrans`:
```rust
    async fn persist_campaign_ktrans(
        &mut self,
        round: usize,
        arena_winner: String,
        arena_confidence: f32,
        mutations_this_round: usize,
        total_mutations_so_far: usize,
        verdict: &EvaluationVerdict,
    ) {
        // capture for the spawn_blocking closure (alongside the other locals)
        // ... existing captures ...
```
Capture it for the closure (it's a plain `usize`, `Copy`), and replace **both** `total_mutations_so_far: (round + 1) * 5,` literals (in `CampaignKtransPayload` ~:680 and `CampaignKtrans` ~:707) with `total_mutations_so_far,`. Update the two call sites: the per-round call (`:1740`) passes `self.total_real_mutations`; `persist_final_summary_ktrans` (~:800) passes `self.total_real_mutations` (round 999).

- [ ] **Step 6: Run the suite**

Run: `cargo test -p korg-runtime 2>&1 | tail -20`
Expected: compiles, green.

- [ ] **Step 7: Commit**

```bash
git add crates/korg-runtime/src/leader.rs
git commit -m "feat(swarm): ledger attests the real diff file count (mutations + running total)"
```

### Task C5: Keystone test — the attested count equals the real diff

**Files:**
- Create: `crates/korg-runtime/tests/honest_pipeline.rs`

> The full multi-process campaign demo is SP4. SP1's keystone proof composes Unit A (provider) + Unit B (observation) on the real fixture: the DeterministicProvider's patch, applied to a copy of `fixtures/honest-demo-repo`, yields a real 1-file diff that compiles — so the count the ledger would attest equals the real diff. Hermetic (needs `git` + `cargo`, which CI has).

- [ ] **Step 1: Write the test**

`crates/korg-runtime/tests/honest_pipeline.rs`:
```rust
//! Keystone: prove the honest pipeline attests a TRUE fact — the mutation count
//! the ledger would record equals the real git diff file count.

use korg_llm::{DeterministicProvider, LlmProvider, LlmRequest, Message, Role};
use korg_runtime::observation::{apply_mutations, cargo_check, honest_metrics, numstat, CargoCheck};

fn req(system: &str, user: &str) -> LlmRequest {
    LlmRequest {
        messages: vec![
            Message { role: Role::System, content: system.into(), name: None, tool_calls: None },
            Message { role: Role::User, content: user.into(), name: None, tool_calls: None },
        ],
        temperature: 0.3, max_tokens: None, tools: None, stop_sequences: None,
        multimodal: None, tx_id: None, session_id: None, policy_hash: None,
        top_p: None, presence_penalty: None, frequency_penalty: None,
    }
}

async fn git(dir: &std::path::Path, args: &[&str]) {
    tokio::process::Command::new("git").args(args).current_dir(dir).output().await.unwrap();
}

#[tokio::test]
async fn honest_pipeline_attests_real_diff_count() {
    // 1. Copy the committed fixture crate into a temp git repo (the "before" state).
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/honest-demo-repo");
    let dir = std::env::temp_dir().join(format!("korg-keystone-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::copy(format!("{src}/Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    std::fs::copy(format!("{src}/src/lib.rs"), dir.join("src/lib.rs")).unwrap();
    git(&dir, &["init", "-q"]).await;
    git(&dir, &["add", "-A"]).await;
    git(&dir, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "base"]).await;

    // 2. Ask the hermetic default provider (as Benjamin) for the fixture patch.
    let provider = DeterministicProvider::new();
    let resp = provider.complete(req(
        "You are Benjamin, the Builder & Implementer.",
        "Fix the add function in src/lib.rs so it adds",
    )).await.unwrap();

    // 3. Parse mutations the way the worker does, then APPLY them to the worktree.
    let (output, _c, _f) = korg_runtime::personas::parse_structured_response(&resp.content);
    let muts = output.get("mutations").and_then(|m| m.as_array()).cloned().unwrap_or_default();
    let apply = apply_mutations(&dir, &muts).await;
    assert_eq!(apply.applied, 1, "the canonical patch applies one file");

    // 4. Measure reality.
    let n = numstat(&dir).await;
    let check = cargo_check(&dir).await;

    // 5. The honest invariant: attested mutation count == real diff file count == 1,
    //    and the fix compiles (verified_count_delta == 1).
    assert_eq!(n.files, 1, "exactly one real file changed");
    assert!(matches!(check, CargoCheck::Passed), "the applied fix compiles");
    let metrics = honest_metrics(&apply, &check, &n, resp.usage.total_tokens, 1.0,
                                 0.0, "keystone");
    assert_eq!(metrics["verified_count_delta"].as_i64(), Some(1));
    assert_eq!(metrics["files_changed"].as_u64(), Some(1));

    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Ensure `parse_structured_response` and `observation` are reachable**

`parse_structured_response` is already `pub` (personas.rs:91); `observation` is `pub mod` (Task B1). Confirm `korg_runtime::personas` and `korg_runtime::observation` are public paths (they are if `pub mod personas;` / `pub mod observation;` in `lib.rs`). `korg-llm` is already a dependency of `korg-runtime`.

- [ ] **Step 3: Run the keystone test**

Run: `cargo test -p korg-runtime --test honest_pipeline 2>&1 | tail -20`
Expected: PASS — one file changed, compiles, verified delta 1.

- [ ] **Step 4: Commit**

```bash
git add crates/korg-runtime/tests/honest_pipeline.rs
git commit -m "test(swarm): keystone — attested mutation count equals the real diff"
```

---

## Final gate

- [ ] **Step 1: Full workspace test + lint**

Run:
```bash
cargo test --workspace 2>&1 | tail -25
cargo fmt --all
cargo clippy --workspace 2>&1 | grep -E "warning|error" | head
```
Expected: all tests pass; fmt clean; no new clippy warnings in the touched crates.

- [ ] **Step 2: Manual honesty check (the whole point)**

Run a default campaign against the fixture and confirm the ledger attests the real count with zero synthetic contamination:
```bash
# from a checkout of fixtures/honest-demo-repo (or point the campaign at it)
KORG_DEFAULT_LLM=deterministic cargo run -- campaign --headless "Fix the add function in src/lib.rs so it adds" 2>&1 | tail -40
```
Expected: no `stress-test-worker` event in the evaluator window; the persisted `.ktrans` `mutations_this_round` equals the real diff file count (1). With `--inject-stress`, the synthetic signal returns (labeled).

- [ ] **Step 3: Commit any fmt changes**

```bash
git add -A && git commit -m "chore(swarm): fmt" || true
```

---

## Self-review notes (author)

- **Spec coverage:** Unit A (A1–A3) = provider + default flip + applyable schema. Unit B (B1–B5) = 3-state cargo / numstat / apply / honest mapping / wire+plumb + dead-code delete + sysinfo dep. Unit C (C1–C5) = de-fabricate defaults / gate synthetics / TUI jitter / real ledger count / keystone. All §8 integration rows are covered.
- **Known confirm-at-implementation points (not placeholders — real unknowns flagged honestly):** (a) the exact CLI flag-parsing struct in `src/main.rs` for `--inject-stress` (follow the existing `--headless` pattern); (b) `SingleWorkerHarness`'s constructor field list for `last_observation` (add alongside existing fields); (c) the `SubmitTransaction` payload-build site line in `handle_route_work` (~403-412) for `files_changed`; (d) `Blackboard`'s test constructor (mirror existing `blackboard.rs` tests). Each names exactly what to find and the pattern to follow.
- **Type consistency:** `files_changed: usize` is consistent across `PersonaResult`, `TaskResult`, the `SubmitTransaction` payload, and the leader sum. `CargoCheck`/`Numstat`/`ApplyOutcome`/`honest_metrics` signatures match between `observation.rs` and the keystone test.
- **Deliberate deviation from spec §4 Unit C #3:** the spec described the gated path *loading a seeded scenario file* (`fixtures/stress-scenarios/baseline.json`) of recorded adverse traces. This plan instead **gates the existing inline constants** behind `--inject-stress`. The honesty goal — *the default path carries zero fabricated telemetry* — is fully met either way; loading from a file is a cosmetic improvement deferred to keep the thin spine tight. If the reviewer wants the file-based scenario, it is a small additive task (read+deserialize `Vec<TraceEvent>`, ingest each under the flag).
- **Out of scope (SP2–4):** generalizing apply+measure to all five personas, inter-persona data-flow, warm boot, and the full multi-process demo recording.
