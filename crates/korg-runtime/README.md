# korg-runtime

The orchestration engine: leader, workers, DAG scheduler, personas, blackboard, and ACP wire protocol.

This is the crate that actually *runs* a campaign. Given a root task, `korg-runtime`
spawns a swarm of agent personas, dispatches work to them as concurrent child
processes, scores their competing results in an Arena, runs an adversarial
Evaluator guardrail, and persists each round as a signed `.ktrans` transaction.
It sits above the capability kernel (`korg-registry`) and the model/embedding
providers (`korg-llm`, `korg-embeddings`), and below the operator surfaces
(`korg-tui`, `korg-server`) that drive it.

It is a single connected component shipped as one crate — the same way `bevy_ecs`
ships as one crate. The modules below are tightly coupled by design; splitting
them further would force every internal interface to become a public API (see the
note at the top of `src/lib.rs`).

## Where it sits in the workspace

```text
korg-tui / korg-server / korg (binary)   ← drive a campaign
        ↓
korg-runtime   ← THIS CRATE: leader, workers, arena, evaluator, DAG, healing
        ↓
korg-registry (CapabilityResolver / CapabilityJournal)   ← single state authority + WAL
korg-llm (LlmProvider)         ← model calls
korg-embeddings (cosine sim)   ← semantic scoring
korg-auth                      ← identity
korg-core                      ← paths, metrics
```

Consumers in the repo: `src/main.rs` (the `korg` CLI), `korg-tui`, and
`korg-server` all construct a `leader::LeaderOrchestrator` and call into it.
`korg-tui` and `korg-server` additionally re-export `recovery::RewindCandidate`,
`tui_bridge::ContractResponse`, and `tui_bridge::TuiUpdate`.

## Modules

The crate is a flat cluster of `pub mod`s (all re-exported from `lib.rs`). The
load-bearing ones:

| Module | Responsibility |
|:---|:---|
| `leader` | `LeaderOrchestrator` — the full campaign loop. Spawns the four personas as `korg worker` subprocesses over stdio, sends `RouteWork`, collects `SubmitTransaction`, runs the Arena, drives the `CampaignPhase` state machine, and signs/persists `.ktrans` rounds. By far the largest module. |
| `workers` | Concurrent worker fan-in (`dispatch_level`, `spawn_worker_process`) built on `tokio::task::JoinSet` with a per-worker `WORKER_TIMEOUT` (300s) and a non-blocking retry queue. The module header documents the three fan-in correctness bugs it was extracted to fix. |
| `personas` | The four-agent topology — `Persona::{Captain, Harper, Benjamin, Lucas}` plus an `Evaluator` critic — and `run_persona*` entry points. Used both in-process by the leader and by the standalone worker harness. |
| `agent` | `run_agent_loop` — the real tool-use loop (read/edit files, run shell, feed results back to the LLM, repeat up to `MAX_AGENT_TURNS = 40`). This is what makes `korg "fix the auth module"` do work. |
| `arena` | `run_arena` / `ArenaOutcome` — concurrent Evaluator scoring across persona results, winner selection by composite score, and semantic merge. |
| `evaluator` | The harsh adversarial guardrail: 5 binary rubrics + `semantic_entropy()` (cosine distance over a 24-sample telemetry window) producing an `EvaluationVerdict` with a `doom_loop_detected` flag. |
| `campaign` | `CampaignPhase` state machine (`Initializing → Planning → Contracting → Dispatching → Evaluating → Committing → Complete`, with `Aborted` reachable from anywhere) and `.ktrans` persistence (JCS hashing + Ed25519 signing of `CampaignKtrans`). |
| `acp` | ACP wire format (targeting v1.17): `MessageEnvelope<P>`, `AcpMessage`, JCS (RFC 8785) canonicalization, `sign_payload` / Ed25519 over the canonical form, and the typed payloads exchanged between leader and workers. |
| `blackboard` | Persistent LWW-Element-Set CRDT surface with a `VectorClock`. Ingests `SwarmTelemetryPulse` messages from workers and maps them to `TraceEvent`s the Evaluator consumes. |
| `session` | `SessionBackend` trait decoupling orchestration from process launch. `SubprocessBackend` (default, spawns `korg worker`) and `DockerBackend`. Workers emit typed `WorkerEvent`s, not raw stdout. |
| `workspace` | `WorkspaceManager` / `WorkspaceId` — the single authority over git-worktree creation and destruction. One isolated worktree per agent run, with a `Created → Provisioned → Active → Completed\|Failed → Destroyed` lifecycle. |
| `runtime` | `RuntimeCoordinator` / `CampaignRuntime` — campaign-level cancellation token, process-group `ExecutionSupervisor`, concurrency semaphore, `RetryBudget` (doom-loop guard), and the guaranteed cleanup/rollback sequence. |
| `recovery` | Rewind-candidate computation. `rewind_candidates` / `on_demand_candidates` walk the journal's `triggered_by` causal chain to propose a `LocalUndo` and a `StrategicReset` target (with invalidation preview). |
| `provenance` | `CampaignAttestation` — SHA-256 hash-chaining and offline Ed25519 verification of campaign traces; backs the `korg verify` CLI command. |
| `code_indexer` | Crawls a workspace, splits files into code blocks, embeds them via `korg-embeddings`, and serves cosine-similarity queries (`query_codebase`). |
| `code_intel` | Tree-sitter syntax layer: `KorgLanguage::{Rust, Python}`, AST symbol extraction, S-expression structural search, and pre-flight syntax validation. |
| `skills` | "Yvaeh Mode" reconcile/synthesize over an Obsidian-style markdown vault (`run_reconcile`, `run_synthesize`). |
| `vision_policy` | `PolicyVerdict::{Approved, Redacted, Blocked}` — screenshot/vision-attachment redaction policy over `korg_llm::VisionPolicyConfig`. |
| `harness` | `SingleWorkerHarness` — the stdio worker entry point (`run_as_stdio_worker`) used by the `korg worker` subcommand. |
| `tui_bridge` | Wire types shared with the operator UI (`ContractResponse`, `TuiUpdate`). Lives here so leader/workers can reference them without depending on the TUI crate. |
| `dag` | **Re-export shim only.** Forwards `crate::dag::*` call sites to `execution::dag` and `execution::recovery::heal_node_with_context`. |
| `execution/` | The folded-in "thumper" substrate (see below). |

### `execution/` — speculative DAG substrate

`execution/` is the former `thumper` execution engine, folded in so it can stay
tightly coupled without a public API boundary. It is surfaced to the rest of the
crate through the `dag` re-export shim.

- `execution::dag` — `ExecutionDag` / `DagNode` / `NodeStatus` / `SpeculativeScheduler`. Topologically sorts, then runs independent nodes concurrently via `tokio::spawn`.
- `execution::recovery` — `heal_node` / `heal_node_with_context`: closed-loop self-healing that parses real compiler/test stderr (e.g. missing-semicolon regexes) and attempts local repair before escalating.
- `execution::pool` — warm rolling sandbox pool (`Sandbox`, `SandboxStatus`) with pre-mounted toolchains and warm LSP/compiler daemons.
- `execution::events` — `BunEvent` / `BunOutcome` / `BunEventOrOutcome`, the NDJSON event contract mirroring the Python `thump/events.py` harness.

## Public API entry points

The crate exposes its modules directly (no curated prelude). The entry points
external crates actually use:

- **`leader::LeaderOrchestrator`** — `new(root_task, session_id)`, then `run_full_campaign()` or `run_observable_campaign()`.
- **`agent::run_agent_loop(prompt, provider, tui_tx)`** — single-agent tool-use loop.
- **`personas::run_persona(persona, payload, routing_id)`** — invoke one persona directly.
- **`harness::SingleWorkerHarness::run_as_stdio_worker(id)`** — the worker-process side.
- **`recovery::on_demand_candidates()`** — compute rewind options against the current journal tail (drives Ctrl-R in the TUI).
- **`provenance::verify_cli_command(path)`** — offline attestation verification.

### Example: run a campaign

```rust
use korg_runtime::leader::LeaderOrchestrator;

# async fn run() -> anyhow::Result<()> {
let mut leader = LeaderOrchestrator::new(
    "Refactor the auth layer to use JWTs".to_string(),
    None, // generate a fresh session UUID
);

// Spawns the persona workers, runs the arena/evaluator loop,
// and persists a signed .ktrans per round under .korg/.
leader.run_full_campaign().await?;
# Ok(())
# }
```

`LeaderOrchestrator::new` builds a fresh per-campaign Ed25519 signing key, a
`RuntimeCoordinator` (4 concurrent workers, 10-workspace quota, retry budget 3),
and a `CapabilityResolver` from `korg-registry`. State is read from / written to
the journal and workspace paths resolved by `korg_core::paths`.

## Status and known gaps

This crate is under active development and pre-1.0. Things to be aware of when
reading or building on it:

- **`lib.rs` blanket-allows** `dead_code`, `unused_imports`, `unused_mut`, `unused_variables`, and `unused_assignments` across the whole crate. Not all code paths are wired into a shipping command yet.
- **`harness::SingleWorkerHarness::run` is a legacy stub path** (so labelled in the source). The live worker path is `run_as_stdio_worker`.
- **`crate::dag` is purely a re-export shim.** The real implementation is in `execution::dag` / `execution::recovery`; the shim exists to preserve existing `crate::dag::*` call sites after the thumper fold-in.
- **Codebase Merkle capture degrades silently.** `campaign::capture_codebase_merkle_root` runs `git write-tree`; if git fails it falls back to the literal string `"sha256:codebase-fallback"` rather than erroring.
- **`DockerBackend` exists** alongside the default `SubprocessBackend`, but the subprocess backend is what `build_backend()` selects by default.
- **Single-user / local scope.** As with the rest of korg v1, the orchestration path assumes a trusted local workspace; there are no network auth or permission bounds on worker spawning.

## License

Licensed under either of MIT or Apache-2.0 at your option (workspace default).
