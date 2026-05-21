---
title: "isolation-routing"
date: 2026-05-21
type: concept
tags: [pattern, isolation, routing, leader-broker, worktree, back-pressure, concurrency, headless-execution, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---



# Leader-Broker Routing & Worktree Isolation Contracts

This contract defines the **physical execution boundaries** that make the Epistemic State Machine (see [[wiki/mechanisms/state-primitives.md]]) safe under concurrency. It specifies how the Leader-Broker dispatches work to headless workers and how the filesystem layer guarantees zero cross-contamination between workers and the primary Korg vault.

Without these guarantees, concurrent offensive workers will corrupt the shared memory space (or deadlock the pipeline) long before the Merge-Arbitration Engine or epistemic verification criteria can be applied.

These primitives are the operational shield for any headless harness, background reconnaissance daemon, or multi-node swarm.

---

## 1. The Leader-Broker Routing Protocol

### The Contract

The **Leader** (strategic planning and capability orchestration) never directly manipulates global vault state or worker execution contexts. It emits discrete, capability-matched routing payloads to one or more **Broker** daemons. Each Broker is responsible for a bounded set of workers and maintains its own isolated view of pending work.

The Leader maps high-level tasks (e.g., "map authentication surface on target X", "fuzz SSRF vectors on discovered endpoints") to worker capability vectors. The mapping is recorded as a first-class routing decision artifact.

### Routing Matrix & Failure Semantics

- Routing payloads are emitted asynchronously over bounded channels.
- Every payload carries a unique `routing_id`, capability requirements, epoch deadline, and a reference to the minimal read-only snapshot of verified state the worker is allowed to consume.
- A Broker must acknowledge receipt of a routing payload within the negotiated **epoch window** (implementation-defined but recorded in the routing artifact).
- On failure to acknowledge (or explicit negative ack):
  - The Broker drops the routing handle.
  - The target asset (or attack surface component) is marked `STALLED` in the routing metadata.
  - A `failed-experiment` or `edge-case` artifact is written with `routing_failure: true`, `reason: "ack_timeout"`, and the original `routing_id`.
  - Dependent `INFERRED` or `OBSERVED` claims that were waiting on this work path may be transitioned toward `CONTESTED` (see interaction rules in §3).

`STALLED` is a routing-level status. It does not directly replace any Epistemic State Machine state but is a first-class trigger that can cause dependent artifacts to re-enter verification or become contested.

The Leader-Broker contract therefore provides both liveness (no silent hangs) and auditability (every routing failure becomes queryable knowledge).

---

## 2. Ephemeral Worktree Isolation

### Mechanism

Every tool invocation or sub-agent execution occurs inside a **transient, cryptographically isolated** execution environment:

- Default namespace: `/tmp/korg/worktrees/$WORKER_ID` (or equivalent platform-native isolated volume).
- The environment is created fresh for the worker instance, mounted with a read-only snapshot of the minimal verified Korg state required for the task (never the live `wiki/` or `Human/` trees).
- Workers **never** hold write handles to the primary vault structures.
- On completion (success or failure), the worker emits exactly **one** deterministic transaction artifact (`.ktrans` file) containing:
  - Proposed state mutations (additions, updates, or contestations).
  - Full provenance (original routing_id, worker_id, tool outputs, hashes of inputs).
  - Cryptographic signature or content hash of the transaction for later verification.
- The worktree is then torn down. No residual filesystem state remains.

This is the concrete realization of the "immutable snapshot → worktree/sandbox mutation → verification → candidate write-back" flow referenced in the Epistemic State Machine.

The `.ktrans` format is the canonical handoff between isolated execution and the Merge-Arbitration Engine. Its precise schema is defined in the forthcoming Transactional Memory primitive; until then, all implementations must produce artifacts that are self-describing and contain at minimum the fields above.

### Cryptographic & Namespace Isolation Requirements

- Worktree roots must be unguessable or access-controlled (randomized subdirectories + restrictive permissions or namespaced mounts).
- Read-only snapshots are content-addressed or hash-verified against the last known VERIFIED state.
- Any attempt by a worker to escape its namespace or write outside its worktree must be treated as a security anomaly and immediately produce a `tool-behavior` + `edge-case` artifact.

---

## 3. Bounded-Buffer Back-Pressure (Drop-Oldest vs. Block)

### The Problem

An aggressive offensive worker (large-scale endpoint discovery, broad fuzzing campaign, or verbose telemetry emitter) can flood the system with artifacts. Without differentiated back-pressure, this leads to memory exhaustion, loss of high-value structural state, or pipeline deadlock.

### The Strategy

Korg applies **state-aware, differentiated buffering**:

| Artifact Epistemic Class | Buffer Policy     | Rationale |
|--------------------------|-------------------|-----------|
| `INFERRED` / ephemeral telemetry / raw logs | **Drop-Oldest** | High-volume, low-trust observations. Losing some intermediate inferences is acceptable if it keeps the system responsive. |
| `OBSERVED` (raw tool outputs that have not yet been classified) | **Drop-Oldest** (with sampling) | Volume can be extreme; sampling preserves signal while bounding memory. |
| Candidate `VERIFIED` or `CONTESTED` mutations (i.e., `.ktrans` payloads or merge-queue entries) | **Hard Block** with back-pressure propagation to the Leader | These are structural claims that have already passed initial validation or require arbitration. Loss here damages the epistemic integrity of the vault. The Leader is notified and must either throttle the producing workers, reallocate capacity, or explicitly relax the current campaign scope. |

The Broker is responsible for enforcing these policies on its local channels. When a hard Block occurs on a structural mutation path, the routing metadata for the affected workers is updated and a back-pressure signal is sent upstream.

This policy directly protects the integrity of the Epistemic State Machine: high-velocity noise cannot drown out the low-velocity, high-authority facts that deserve permanent vault promotion.

---

## Interaction with the Epistemic State Machine

- All routing events, acknowledgments, `STALLED` marks, and back-pressure actions must themselves be emitted as observable artifacts that can enter the `OBSERVED → INFERRED → VERIFIED → CONTESTED` lifecycle.
- A `STALLED` routing failure on a critical path is a legitimate reason to contest or re-verify dependent claims.
- The Merge-Arbitration Engine (state-primitives §3) only sees candidate writes that have successfully exited an isolated worktree and passed the Broker's back-pressure filter.

These three contracts (Routing, Isolation, Differentiated Back-Pressure) together close the loop between the abstract epistemic lifecycle and the physical reality of concurrent headless execution.

---

## Related

- [[wiki/mechanisms/state-primitives.md]] — The epistemic lifecycle this contract physically protects. The Broker role, `CONTESTED` holding area, and authority-vector merge logic are direct consumers of the isolation guarantees defined here.
- [[wiki/mechanisms/transactional-memory.md]] — The precise `.ktrans` handoff format, persistence decoupling, and Three-Way Rebasement rules that all isolated workers must emit and that the Broker must consume.
- [[Human/Methodology/The-Korg-Triad.md]] — Narrative walkthrough of how isolation and routing feel when you're actually running parallel workers.
- [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] — What STALLED events, back-pressure, and worker lifecycle signals actually look like to a live operator.
- [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]] — Real-world production implementation of the same isolation, STALLED handling, and recovery patterns.
- [[wiki/patterns/Cross-Harness-Pattern-Extraction]] — Parent vision. These three mechanism notes are the first complete triad of de-TUI-ified execution contracts.
- [[wiki/concepts/Operational-Intelligence-Layer-Mandate]] — Why runtime isolation, routing, and transactional handoff behavior must be first-class, queryable knowledge.
- Future supporting notes (detailed variants, metrics, ACP bindings):
  - [[wiki/reference-harness/ACP-Binding-Design.md]] — First concrete protocol sketch
  - [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]] — Coordination model that uses the protocol at scale
  - [[wiki/reference-harness/ACP-Message-Schema.md]] — Exact message schemas and wire formats
  - Worktree-Isolation-Per-Subagent
  - Bounded-Buffers-and-Drop-Oldest-Synchronization
  - Immutability-Engine-and-Transactional-Write-Back
  - Three-Way-Merge-Resolution-and-State-Rebasement-Heuristics (now partially realized in transactional-memory.md)

**Status note (2026-05-20):** This primitive was extracted immediately after state-primitives.md while the concurrency model was still fresh from the source traces. It intentionally front-loads the physical safety layer so that subsequent harness implementations (headless or otherwise) have a complete, consistent contract to implement against.

---

**Last updated:** 2026-05-20 — Extracted under the Cross-Harness Pattern Extraction program as the physical execution substrate for the Epistemic State Machine.

## See Also

- [[Synthesis — Transactional Memory]]


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-21, confidence: medium)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-21, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
