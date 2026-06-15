# korg-core

Foundational types and traits for the korg cognitive runtime.

`korg-core` is the bottom crate in the workspace dependency graph. It holds the
shared vocabulary — types, traits, path resolution, metrics, and telemetry — that
sibling crates need in common, and nothing else.

## Inclusion criterion

A module lives in `korg-core` **only if** it satisfies both rules (enforced by
convention, documented in `lib.rs`):

1. **Zero internal deps** — it compiles with third-party crates only, no `use korg_*::`.
2. **Consumed by three or more other korg crates.**

A module used by exactly one crate belongs *in that crate*. A module with internal
deps belongs in the lowest crate that satisfies them. This is why, for example,
`provenance` lives in `korg-runtime` (it depends on `acp` message types) and nothing
referencing `korg-registry` or higher can appear here.

`korg-core` itself depends only on `serde`, `serde_json`, `anyhow`, `uuid`, `chrono`,
`tracing`/`tracing-subscriber`, and `directories`. It is the only crate every other
korg crate can depend on without risking a cycle. Current consumers: `korg-auth`,
`korg-llm`, `korg-registry`, `korg-runtime`, `korg-server`, `korg-tui`, and
`korg-bridge`.

## Modules

### `event` — the ledger intake payload

```rust
pub struct NormalizedEvent {
    pub source_agent: String,     // "agent:<name>@<version>" | "human:<id>"
    pub tool_name: String,
    pub args: serde_json::Value,
    pub result: serde_json::Value,
    pub payload_refs: Vec<ContentRef>,
    pub success: bool,
    pub duration_ms: u64,
    pub triggered_by: Option<u64>, // seq_id of the causal parent; None for roots
}

pub struct ContentRef {           // content-addressed reference for large blobs
    pub sha256: String,
    pub size_bytes: u64,
    pub label: String,
}
```

`NormalizedEvent` is the canonical, wire-format-agnostic shape every adapter must
produce; `korg-runtime` is intended to wrap it in a `CapabilityEvent` before appending
to the journal. `ContentRef` keeps the ledger lightweight: large payloads are stored
out of band and referenced here by digest + size. Both types live in `korg-core`
specifically so `korg-registry` and the `Adapter` trait can reference them without a
circular dependency — `korg-registry` re-exports `ContentRef` (`pub use korg_core::ContentRef`)
as its own canonical content-addressing type.

### `adapter` — the `Adapter` trait

```rust
pub trait Adapter: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn version(&self) -> &str;            // SemVer; caller stamps it into event metadata
    fn source_agent_prefix(&self) -> &str; // e.g. "agent:korgex@" for routing
    fn validate(&self, raw: &serde_json::Value) -> anyhow::Result<()>;
    fn normalize(&self, raw: serde_json::Value) -> anyhow::Result<NormalizedEvent>;
}
```

Plugin contract for **in-process Rust** wire-format translators: one `Adapter` per
external agent protocol (Codex WebSocket frames, Grok NDJSON, etc.), translating raw
JSON into a `NormalizedEvent`. The trait is object-safe (`Box<dyn Adapter>` works).
`validate` is a documented cheap pre-flight: `validate(x).is_err()` implies
`normalize(x)` would also fail, but the converse does **not** hold — `normalize` is the
authoritative check. The out-of-process Python adapters under `adapters/` follow the
same conceptual contract but as HTTP clients posting to `/api/agent-tool-call`; they
get runtime validation only, no trait, no compile-time guarantee.

### `paths` — centralized path resolution

XDG-aware runtime directory resolution via the `directories` crate — no hardcoded
paths. Public functions resolve the cache root and session-scoped artifact
directories: `cache_dir()`, `campaign_dir(session_id)`, `state_blobs_dir`,
`blackboard_dir`/`blackboard_json`, `contracts_dir`, `ktrans_dir`,
`worktree_dir(...)`/`worktree_dir_harness(...)`, `forks_dir(tx_id)`,
`semantic_merge_path`, `prompts_dir`, `temp_patch_path`, and
`project_root()`/`project_root_string()`. This is the most widely consumed module in
the crate — it is what replaced the old hardcoded `/Users/.../Korg` references
across the runtime. `config_dir()` and `data_dir()` exist but are `pub(crate)` until a
downstream crate needs them.

### `metrics` — lock-free runtime counters

Global `AtomicU64` counters with zero-allocation `record_*()` call sites for the hot
paths (campaigns, transitions, workers, workspaces, LLM requests, vision-policy
redactions, etc.). Each record call also emits a structured `tracing` event.
`snapshot()` returns a `MetricsSnapshot` that serializes straight to JSON for the
server's `GET /api/metrics` endpoint.

### `telemetry` — structured tracing setup

`init_tracing()` installs the global `tracing-subscriber` stack. Call it exactly once
at the top of `main()`; later calls (e.g. in tests) are no-ops via `try_init`. Honors
`KORG_LOG` (default `info`) for the env-filter and `KORG_LOG_JSON=1` to switch from
pretty human output to machine-parseable JSON. Also exports the `trace_transition!`
and `trace_round!` convenience macros.

### `subscription` — `SubscriptionTier`

```rust
pub enum SubscriptionTier { Standard, Premium, Enterprise }
```

Lives here so `korg-auth` (which stores it on `UserSession`) and `korg-registry`
(which gates tool authorization on it) share one type without depending on each other.

## Where it sits in the workspace

```
korg-core   ← types, traits, paths, metrics, telemetry (this crate; no internal deps)
   ├── korg-auth        (UserSession + SubscriptionTier)
   ├── korg-llm
   ├── korg-registry    (CapabilityJournal; re-exports ContentRef)
   │      └── korg-bridge   (PyO3 → CapabilityJournal; see crates/korg-bridge/README.md)
   ├── korg-runtime     (wraps NormalizedEvent → CapabilityEvent)
   ├── korg-server      (/api/metrics, tool authorization by tier)
   └── korg-tui
```

## Status / gaps

- `paths`, `metrics`, `telemetry`, `SubscriptionTier`, and `ContentRef` are all live
  and consumed across the workspace.
- **`NormalizedEvent` and the `Adapter` trait are a forward-declared intake contract,
  not yet wired up.** As of this writing there are **no concrete `impl Adapter`** in
  the workspace and no caller wraps `NormalizedEvent` into a `CapabilityEvent` — the
  in-process adapter path described above is the intended design, but ingestion today
  goes through `korg-registry`/`korg-bridge` and the HTTP `/api/agent-tool-call`
  route. The field names on `NormalizedEvent` intentionally mirror the server's
  `AgentToolCallRequest` so adopting it later is a rename-and-move, not a redesign.

Unit tests cover `paths`, `metrics`, and `telemetry`; `event`, `adapter`, and
`subscription` are plain type definitions and are exercised by their consumers.

## License

Licensed under either of MIT or Apache-2.0 at your option, consistent with the
workspace.
