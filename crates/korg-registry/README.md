# korg-registry

The capability journal, ledger, and transition engine at the core of korg.

`korg-registry` is the kernel crate of the workspace. It owns the append-only
event journal (`CapabilityJournal`), the `korg-ledger@v1` tamper-evident
hash-chain, the single state-mutation authority (`CapabilityResolver`), and the
event-folding read models (`ProjectionEngine`). Every layer above it —
`korg-runtime` (the leader/scheduler), `korg-server` (HTTP + MCP), and
`korg-bridge` (the PyO3 wrapper for `korgex`) — reads and mutates runtime state
*exclusively* through types defined here.

It depends only on [`korg-core`](../korg-core) (for `ContentRef`,
`SubscriptionTier`, `paths`, and `metrics`) plus `serde`, `uuid`, `chrono`,
`tracing`, `fs2` (advisory file locks), `sha2`, and `hmac`. No async runtime, no
network, no LLM.

## What it does

Three things, in layers:

1. **A persisted, hash-chained event log.** `CapabilityJournal` appends
   `JournalEvent` envelopes to `journal.json` under an `fs2` advisory lock, each
   carrying an HLC timestamp, causal metadata, and a `prev_hash`/`entry_hash`
   chain link (`ledger_chain`). The chain is the integrity substrate the whole
   workspace inherits.

2. **A single mutation authority.** `CapabilityResolver` is the *only* place
   capability state changes. A `TransitionRequest` runs through a fixed
   Plan → Validate → Commit → Apply → Journal pipeline; every phase emits an
   event, and a mid-flight failure triggers rollback effects and a
   `TransitionRolledBack` event.

3. **Deterministic read models.** `ProjectionEngine` folds the raw event stream
   into materialized state (e.g. `campaign_projection`) via pure `apply`
   functions, so any view can be rebuilt from history with `rebuild_all`.

## Key modules and types

| Module | Public surface | Role |
|:---|:---|:---|
| `log` | `CapabilityJournal`, `CapabilityEvent`, `JournalEvent`, `EventMetadata`, `EventTier`, `HlcTimestamp`, `CapabilitySnapshot`, `ContentRef`, `IS_PREVIEW_MODE` | The append-only WAL, its event enum, and the on-disk envelope. |
| `ledger_chain` | `canonicalize`, `chain_hash`, `verify_chain`, `verify_dag`, `GENESIS_HASH` | `korg-ledger@v1` reference implementation (see below). |
| `projection` | `ProjectionEngine` | Pure event-fold read models; `new` / `rebuild_all` are the public entry points. |
| `types` | `CapabilityNode`, `CapabilityState`, `Category`, `CognitionMode`, `ProjectionMap`, `TransitionRequest`, `TransitionResponse` | The capability-DAG node model and the resolver's request/response contract. |
| `plan` | `TransitionState` | The seven-phase transition lifecycle enum (`Planned` → `Applied` / `RolledBack` / `Failed`). |
| (crate root) | `CapabilityResolver` | The single state-mutation authority. |

`pub(crate)` modules `checkpoint`, `executor`, `planner`, and `validator` are
implementation detail: they build immutable transition plans, run the effect
DAG, enforce static (cycle/dangling-reference) and dynamic
(dependency/conflict/lease) safety checks, and snapshot resolver state. They are
not part of the public API.

### `CapabilityResolver`

The facade over the capability DAG. `default_resolver()` registers 12 built-in
capability nodes (`docker_sandbox`, `cognition_mode`, `semantic_embeddings`,
`provenance_attestation`, etc.), each declaring dependencies, conflicts, and a
`ProjectionMap` of where it surfaces (CLI flag, LSP command, SDK config path, UI
toggle). State changes go through one of:

- `transition(id, state)` / `handle_transition_request(req)` — the full
  pipeline; the latter is what the web/TUI layers forward to.
- `acquire_lease` / `release_lease` — time-bound exclusive locks that gate
  transitions by `owner_id` (correlation id).
- `cognition_mode()` / `set_cognition_mode(str)` — the single read/write path
  for the active `CognitionMode`; `korg-runtime`'s leader reads it here and never
  caches a copy.
- `create_checkpoint` / `restore_checkpoint` — capture/restore
  `{ledger_offset, projection_state, lease_map, active_states}`; restore rewinds
  the journal and rebuilds projections.
- `authorize_tool_use(tier, tool)` — coarse subscription gate that rejects
  high-blast-radius tools (`Bash`, `docker_sandbox`) on the `Standard` tier.

### `korg-ledger@v1` hash-chain (`ledger_chain`)

This module is the **Rust reference** for the frozen `korg-ledger@v1` spec
(canonical text in [`spec/korg-ledger-v1`](../../spec/korg-ledger-v1); vendored
golden vectors under `tests/conformance/`). Each event carries `prev_hash` (the
previous event's `entry_hash`, `GENESIS_HASH` for the first) and `entry_hash`
(SHA-256, or HMAC-SHA256 when `KORG_LEDGER_HMAC_KEY` is set, over the canonical
preimage). `canonicalize` reproduces Python's
`json.dumps(sort_keys=True, separators=(",",":"), ensure_ascii=True)` byte for
byte, so a journal written here verifies identically under korgex's Python
`verify_chain`. `tests/conformance.rs` pins this against frozen tip hashes
computed by the Python reference — diverge by one byte and it fails.

`verify_chain` returns an empty `Vec<String>` iff the chain is intact; otherwise
each entry names the offending `seq_id` (content tampered, or
inserted/deleted/reordered). `verify_dag` separately checks `seq_id` uniqueness
and that every `triggered_by` points to a strictly-earlier event.

## How it fits the workspace

```
korg-bridge (PyO3)  ─┐
korg-server (HTTP/MCP)├─►  korg-registry::CapabilityResolver / CapabilityJournal
korg-runtime (leader)─┘            │
                                   └─►  korg-core (ContentRef, SubscriptionTier, paths, metrics)
```

- **[`korg-bridge`](../korg-bridge)** wraps `CapabilityJournal` so `korgex`
  (Python) writes `AgentToolCall` events to the same `journal.json` the server
  reads — and inherits the hash-chain for free.
- **[`korg-server`](../korg-server)** holds an
  `Arc<Mutex<CapabilityResolver>>`, forwards `TransitionRequest`s, and streams
  the journal via `to_json_lines_filtered`.
- **`korg-runtime`** keeps its `CapabilityResolver` as the authoritative store
  of `CognitionMode` and drives rewind/recovery through `rewind` and the
  checkpoint API.

The `AgentToolCall` variant (schema `1.0`) is the universal external-agent
event: any MCP-compatible runtime (korgex, Claude Code, Codex) emits it, with
large payloads content-addressed via `ContentRef` rather than inlined.

## Usage

Append agent activity to a chained journal and verify it:

```rust
use korg_registry::{CapabilityJournal, CapabilityEvent};
use std::path::PathBuf;

let mut journal = CapabilityJournal::new(
    PathBuf::from(".korg/journal.json"),
    PathBuf::from(".korg/snapshots.json"),
    10,                                  // snapshot interval
    PathBuf::from(".korg/journal.lock"),
);

journal.append(CapabilityEvent::AgentToolCall {
    source_agent: "korgex".into(),
    tool_name:    "Edit".into(),
    args:         serde_json::json!({ "file_path": "src/routes.rs" }),
    result:       serde_json::json!({ "success": true }),
    payload_refs: vec![],
    success:      true,
    duration_ms:  142,
    timestamp:    chrono::Utc::now(),
});

// Empty == intact. Honours KORG_LEDGER_HMAC_KEY if set.
assert!(journal.verify_chain().is_empty());
```

Drive a governed capability transition through the resolver:

```rust
use korg_registry::{CapabilityResolver, CapabilityState};

let mut resolver = CapabilityResolver::default_resolver();
resolver.transition("docker_sandbox", CapabilityState::Enabled)?;
# Ok::<(), String>(())
```

## Notable gaps / stubs

Honest about what is real vs. illustrative:

- **Effect execution is mocked.** `executor::run_effect` does *not* spawn real
  Docker containers or run real tools — it prints what it would do and uses
  hardcoded container-name substrings (`fail_first`, `fail_always`) to simulate
  failures. The transactional plan/validate/rollback/journal machinery and its
  event trail are real; the physical side effects are not.
- **Micro-healing is pattern-matched and narrow.** `attempt_micro_healing`
  recognizes exactly two error substrings (container collision → `docker rm -f`,
  stale lockfile → `remove_file`) and retries once. It is not a general healing
  layer.
- **`authorize_tool_use` is a coarse allowlist** (string match on `Bash` /
  `docker_sandbox`), not a real permission model.
- **One projection ships.** Only `CampaignProjection` is registered in
  `ProjectionEngine`; the engine is built to hold many but currently holds one.
- **`triggered_by` index is in-memory.** Rebuilt on every load; a TODO notes
  incremental persistence is deferred to v2 if startup exceeds 100ms.
- **`verify_integrity` is post-facto.** It detects dangling `ContentRef` blobs
  after the fact but does not enforce blob-before-event crash-safety; the source
  documents this explicitly.

## Tests

Unit tests live inline (transition safety, leases, HLC monotonicity under clock
drift, projection folding/rebuild, micro-healing, chain tamper-detection).
Cross-language conformance against the frozen Python oracle is in
[`tests/conformance.rs`](tests/conformance.rs).

```bash
cargo test -p korg-registry
```
