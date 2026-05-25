# korg-core Public API — Frozen Contract

**Frozen after step 5 (2026-05-25). Do not add or remove items without a recorded rationale.**

This document is the contract. Every item listed here is explicitly load-bearing.
Every item absent here was intentionally excluded.

---

## `korg_core::adapter`

```rust
pub trait Adapter: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn source_agent_prefix(&self) -> &str;
    fn validate(&self, raw: &serde_json::Value) -> anyhow::Result<()>;
    fn normalize(&self, raw: serde_json::Value) -> anyhow::Result<NormalizedEvent>;
}
```

**Stability:** Frozen. korg-runtime will implement this trait for all in-process adapters.
`version()` stamps the adapter binary version into event metadata for audit/replay.
`validate()` is a cheap necessary-but-not-sufficient pre-flight; `normalize()` is authoritative.

**Decision:** Adapter stays in korg-core (not a separate `korg-adapter` crate) for v1.
Rationale: the trait is small, already here, and adapters are load-bearing extensibility that
belongs with foundational types. Cost to move later is low (one trait). Revisit if the
"stranger consumer who wants just the schema" use case becomes real.

---

## `korg_core::event`

```rust
pub struct ContentRef {
    pub sha256: String,
    pub size_bytes: u64,
    pub label: String,
}

pub struct NormalizedEvent {
    pub source_agent: String,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub result: serde_json::Value,
    pub payload_refs: Vec<ContentRef>,
    pub success: bool,
    pub duration_ms: u64,
    pub triggered_by: Option<u64>,
}
```

**Stability:** Frozen. `ContentRef` is the canonical definition — `korg_registry::log::ContentRef`
is a `pub use korg_core::ContentRef` re-export. Do not redeclare it in any other crate.
`NormalizedEvent` is what every `Adapter::normalize()` must produce.

---

## `korg_core::metrics`

All `record_*` functions are pub. Some are not yet wired at all call sites — that is a gap
in the callers, not a reason to remove the function. The full set is the contract.

```rust
// Wired
pub fn record_campaign_completed()
pub fn record_evaluator_verdict(overall: &str, doom: bool, entropy: f32)
pub fn record_ktrans_persisted(round: usize)
pub fn record_transition_applied(capability_id: &str)
pub fn record_transition_failed(capability_id: &str, error: &str)
pub fn record_transition_rejected(capability_id: &str, reason: &str)
pub fn record_worker_completed(persona: &str)
pub fn record_worker_crashed(persona: &str)
pub fn record_worker_timeout(worker_id: &str)
pub fn record_workspace_completed(persona: &str, exit_ok: bool)
pub fn record_workspace_created(persona: &str)
pub fn record_workspace_destroyed(persona: &str)
pub fn record_agent_tool_invocation(tool_name: &str)
pub fn snapshot() -> MetricsSnapshot

// Stub — defined, not yet wired at all call sites
pub fn record_campaign_started()
pub fn record_campaign_round(round: usize, winner: &str, action: &str)
pub fn record_vision_policy_redacted()
pub fn record_vision_policy_blocked()
pub fn record_llm_request()
pub fn record_llm_failure(provider: &str, status: u16)
pub fn record_sse_event()

pub struct MetricsSnapshot { /* all fields pub, serializes to JSON for /api/metrics */ }
```

**NOT pub (excluded by design):** The raw `AtomicU64` statics. All mutation goes through
`record_*` functions; all reads go through `snapshot()`. This keeps the counter update
semantics (Relaxed ordering, tracing side effects) encapsulated.

---

## `korg_core::paths`

All path resolution functions are pub. The only excluded items are `config_dir()` and
`data_dir()` which are `pub(crate)` — no downstream crate needs the raw XDG root dirs;
they should use the specific path functions instead.

```rust
pub fn cache_dir() -> PathBuf
pub fn campaign_dir(session_id: &uuid::Uuid) -> PathBuf
pub fn state_blobs_dir(session_id: &uuid::Uuid) -> PathBuf
pub fn blackboard_dir() -> PathBuf
pub fn blackboard_json() -> PathBuf
pub fn contracts_dir() -> PathBuf
pub fn ktrans_dir() -> PathBuf
pub fn worktree_dir(persona_name: &str, routing_id: &str, suffix: &str) -> PathBuf
pub fn worktree_dir_harness(worker_id: &str, routing_id: &str) -> PathBuf
pub fn forks_dir(tx_id: usize) -> PathBuf
pub fn semantic_merge_path(session_id: &uuid::Uuid) -> PathBuf
pub fn project_root() -> PathBuf
pub fn project_root_string() -> String
pub fn prompts_dir() -> PathBuf
pub fn temp_patch_path() -> PathBuf
```

---

## `korg_core::subscription`

```rust
pub enum SubscriptionTier { Standard, Premium, Enterprise }
impl SubscriptionTier {
    pub fn as_str(&self) -> &'static str
}
```

**Stability:** Frozen. Lives in korg-core (not korg-auth or korg-registry) to break the
auth↔registry circular dependency. Both crates depend on korg-core.

---

## `korg_core::telemetry`

```rust
pub fn init_tracing()

#[macro_export] macro_rules! trace_transition! { ... }
#[macro_export] macro_rules! trace_round! { ... }
```

**Stability:** Frozen. `init_tracing()` is called once at binary startup. The macros emit
structured spans — use them in hot paths instead of raw `tracing::info!` to keep the
event schema consistent with Architecture/Overview.md.

---

## What does NOT belong in korg-core

Per the inclusion criterion in `src/lib.rs`:

1. A module must have **zero internal korg-crate dependencies**.
2. A module must be consumed by **three or more** other korg crates.

Current exclusions and why:
- `provenance.rs` — depends on `acp::canonicalize` and ACP message types. Lives in korg-runtime.
- Any orchestration logic — belongs in korg-runtime or higher.
- Any HTTP client or ML inference — belongs in korg-llm or korg-embeddings.
