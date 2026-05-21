---
date: 2026-05-20
type: design
tags: [design, acp, reference-harness, protocol, orchestration]
harness: korg
domain: acp, orchestration, state-management
status: emerging
ai-first: true
---

# ACP Binding Design for the Korg Execution Kernel

This note defines the minimal Agent Client Protocol (ACP) message types and flows required to drive the three core Korg contracts across distributed workers, Brokers, and Leaders.

The goal is a small, stable wire protocol that lets completely different harnesses (TUI drivers, background daemons, swarm orchestrators, pipeline runners) participate in the same governed transactional memory layer without reinventing routing, isolation, or persistence semantics.

**Traceability**: This note is a faithful abstraction of the Grok 4.20 Heavy architecture described in [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]]. All major design decisions (Leader as policy brain, Broker/Worker isolation, ACP message surface, worktree usage, provenance chains, conflict resolution via arena, dynamic swarm sizing, blackboard + delta patching, capability-based permissions, and productive-death recovery) are directly derived from the real Grok Heavy system. The detailed ACP method semantics and observability signals in that document directly informed the message types and feedback mechanisms defined here.

---

## Guiding Principles

- **Contract fidelity first** — Every ACP message must map cleanly to one of the three mechanism contracts (`state-primitives.md`, `isolation-routing.md`, `transactional-memory.md`).
- **Minimal surface** — Only introduce messages that are strictly necessary for correctness and liveness. Everything else is an extension.
- **Language-agnostic** — The protocol describes *what* is exchanged, not how it is serialized (though we recommend a canonical JSON + CBOR dual-encoding strategy).
- **Failure is first-class** — STALLED, doom-loop termination, and contested outcomes must be explicit and observable.

---

## Core Roles

- **Leader** — Strategic planner. Emits routing decisions. Does not manage workers directly.
- **Broker** — Execution supervisor. Manages worktrees, enforces epoch windows, back-pressure, and termination. Owns the merge queue.
- **Worker** — Disposable execution unit. Runs inside an isolated worktree. Only ever communicates via `.ktrans` and ACP control messages.
- **Client** (optional) — Human or higher-level orchestrator that observes or drives the system.

---

## Minimal ACP Message Types

### 1. Routing & Work Dispatch (isolation-routing contract)

| Message | Direction          | Purpose |
|---------|--------------------|---------|
| `RouteWork` | Leader → Broker | Carries a routing payload: `routing_id`, capability requirements, `base_snapshot` hash, epoch deadline |
| `AckRoute` | Broker → Leader | Acknowledgment that the payload was accepted and a worker was allocated |
| `NackRoute` / `Stalled` | Broker → Leader | Work could not be started or the epoch window expired |

### 2. Worker Lifecycle & Heartbeat (isolation-routing + termination)

| Message | Direction | Purpose |
|---------|-----------|---------|
| `WorkerHello` | Worker → Broker | Registers a new worker instance with its `worker_id` and claimed capabilities |
| `Heartbeat` | Worker ↔ Broker | Periodic liveness + progress signal (includes current token velocity and last AST delta hash) |
| `RequestTerminate` | Broker → Worker | Instructs the worker to shut down (graceful or forced) |
| `TerminationReport` | Worker → Broker | Final status + `tx_id` of the terminal `.ktrans` that was (or will be) emitted |

### 3. Transaction Handoff (transactional-memory contract)

| Message | Direction | Purpose |
|---------|-----------|---------|
| `SubmitTransaction` | Worker → Broker | Carries a `.ktrans` payload (or reference + hash for large payloads) |
| `TransactionAccepted` | Broker → Worker | The `.ktrans` passed structural validation and entered the merge queue |
| `TransactionRejected` | Broker → Worker | Structural or provenance validation failed (with reason code) |

### 4. Epistemic & Arbitration Feedback (state-primitives + merge)

| Message | Direction | Purpose |
|---------|-----------|---------|
| `ContestedNotification` | Broker → Clients | A fact moved to `CONTESTED`. Includes `tx_ids` of conflicting transactions and authority vectors |
| `MergeOutcome` | Broker → Clients | A rebasement or authority resolution completed. Reports the winning side and any new `semantic-decision` created |
| `StatePromotion` | Broker → Clients | An artifact transitioned to `VERIFIED` (with verification evidence summary) |

---

## .ktrans Serialization Over ACP

- The canonical `.ktrans` format defined in `transactional-memory.md` is the payload.
- On the wire we recommend:
  - **JSON** for human-readable debugging and small transactions
  - **CBOR** for production efficiency (especially when provenance chains or large diffs are involved)
- Every `SubmitTransaction` message must include:
  - `tx_id` (UUIDv7)
  - `content_hash` (of the canonical serialization)
  - Either the full payload or a content-addressed reference + signature

The Broker is responsible for re-serializing and validating the hash before accepting the transaction into the merge queue.

---

## Key Lifecycle Flows

### Worker Startup & Work Execution

1. Broker receives `RouteWork` from Leader.
2. Broker creates isolated worktree + mounts verified snapshot.
3. Broker sends `WorkerHello` + work description to the allocated worker (or spawns one).
4. Worker emits `Heartbeat`s containing velocity/delta signals.
5. Worker emits zero or more `SubmitTransaction` messages (micro-transactions).
6. On completion or forced termination, worker emits `TerminationReport` + final `.ktrans`.

### Doom-Loop Termination (Broker-initiated)

1. Broker detects repeated high-velocity + zero-delta heartbeats.
2. Broker sends `RequestTerminate` (with reason `doom_loop`).
3. Worker must still attempt to flush its last coherent `.ktrans` before exiting.
4. Worker sends `TerminationReport` containing the `tx_id` of the diagnostic transaction.
5. Broker records the event and may emit a `ContestedNotification` for any claims that were in flight.

### Late Transaction Rebasement

1. Worker emits `SubmitTransaction` with an older `base_snapshot`.
2. Broker detects staleness during merge.
3. Broker performs the headless three-way rebase (see `transactional-memory.md`).
4. On conflict, emits `ContestedNotification` or `MergeOutcome`.

---

## Extension Points & Harness Styles

The protocol is deliberately minimal. Different harness styles can extend it:

- **TUI-style single-worker harness** — May collapse Leader + Broker into one process and add rich `ContestedNotification` rendering for the human operator.
- **Swarm orchestrator** — Runs many Brokers; uses `RouteWork` fan-out and aggregates `ContestedNotification` streams.
- **Pipeline harness** — Treats long-running workers as first-class and adds `ProgressUpdate` messages (still carrying velocity/delta).
- **Red-team wave coordinator** — Adds capability vectors and blast-radius metadata to `RouteWork`.

All extensions must still honor the five core responsibilities defined in the harness note.

---

## Open Questions & Future Work

- Exact binary encoding and authentication model for ACP (mTLS, signed messages, capability tokens?).
- How to handle very large `.ktrans` payloads (content-addressed object store + ACP reference?).
- Standardized error/reason codes for `Stalled`, `TransactionRejected`, and `RequestTerminate`.
- Leader-Broker-ACP-Model-for-Parallel-Agents (the detailed routing and supervision pattern that will sit on top of these messages).
- Metrics and observability schema for ACP traffic (to feed the live campaign views).

---

## Related

- [[wiki/reference-harness/ACP-v1.17-Wire-Format.md]] — The precise wire-level specification (JCS, signatures, streaming, schemas) that this binding is intended to target.

- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine and Merge-Arbitration Engine that ACP must surface.
- [[wiki/mechanisms/isolation-routing.md]] — Routing, worktree isolation, STALLED handling, and back-pressure that ACP messages must carry.
- [[wiki/mechanisms/transactional-memory.md]] — `.ktrans` format and rebasement protocol that ACP must transport reliably.
- [[Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md]] — The five responsibilities any ACP-speaking harness must implement.
- [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] — The signals (velocity, STALLED, contest pressure) that ACP must make observable.
- [[wiki/patterns/Cross-Harness-Pattern-Extraction.md]] — The long-term vision this protocol helps realize.
- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground truth production architecture of the Grok 4.20 Heavy Leader process and ACP (scheduling policy, blackboard concurrency, Arena conflict resolution, capability model, and headless usage) from which this protocol surface is derived.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — Practical pseudocode harness that actually speaks the ACP messages defined here.

This design note is a faithful abstraction of the Grok 4.20 Heavy architecture. The companion coordination model is described in [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]].

Concrete message schemas and wire formats are defined in [[wiki/reference-harness/ACP-Message-Schema.md]].

A real-world production usage of this entire approach is documented in [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]].