---
title: "transactional-memory"
date: 2026-05-21
type: concept
tags: [pattern, transactional-memory, ktrans, three-way-merge, rebasement, rollback-isolation, persistence, concurrency, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---



# Transactional Memory Serialization Contract (`.ktrans`)

This contract completes the foundational triad of the Korg headless runtime specification:

- [[wiki/mechanisms/state-primitives.md]] defines *what* epistemic states an artifact may occupy and the deterministic criteria for promotion.
- [[wiki/mechanisms/isolation-routing.md]] defines *where* and *how* concurrent workers execute safely without contaminating the primary vault.
- This note defines *exactly what data package* moves between isolated execution and the Broker's Merge-Arbitration Engine so that state changes are persisted durably and correctly.

The `.ktrans` (Korg Transaction) is the canonical, self-describing handoff artifact. It is the sole mechanism by which any headless worker, sub-agent, or background daemon may propose mutations to the governed memory layer.

---

## 1. The `.ktrans` Transaction Anatomy

### Schema Contract

A valid `.ktrans` payload **must** be a well-formed, parseable document (JSON or YAML with a canonical Markdown+frontmatter variant for human inspection) containing at minimum the following fields. The Broker rejects any transaction that fails this structural contract before it enters the merge queue.

**Required top-level fields:**

- `tx_id`: UUIDv7 (time-sortable, lexicographically ordered for replay and rebasement safety)
- `worker_id`: Stable identifier of the producing worker / sub-agent instance
- `routing_id`: Reference to the Leader-Broker routing payload that authorized this work (from isolation-routing)
- `timestamp`: ISO-8601 UTC of transaction creation
- `base_snapshot`: Cryptographic hash (or content-addressed reference) of the verified state snapshot the worker used as its starting point
- `provenance_chain`: Array of upstream hashes, tool output digests, and prior `tx_id` values that directly contributed to the mutations in this payload
- `mutations`: Array of mutation descriptors (see below)

**Mutation descriptor (each element of `mutations`):**

- `target_path`: Relative vault path (e.g., `wiki/decisions/0042 - SSRF Surface.md` or a namespaced key for non-file artifacts)
- `action`: One of `INSERT`, `UPDATE`, `CONTEST`
- `payload`: The raw semantic content or diff being proposed (Markdown fragment, structured object, or minimal patch)
- `confidence`: Optional inference confidence if the mutation originated from an `INFERRED` stage
- `authority_vector`: Reference or inline declaration of the authority level claimed for this mutation (used by the Merge-Arbitration Engine)

Additional optional fields (e.g., `doom_loop_detected`, `exit_reason`, `resource_usage`) are permitted and encouraged for diagnostic `.ktrans` files emitted on worker termination.

The Broker treats the `tx_id` as the primary ordering key for all rebasement and replay operations.

---

## 2. Decoupling Code Rollback from Memory Persistence

### The Core Problem

In conventional agent architectures, when a sub-agent process crashes, panics, receives `SIGKILL` (doom-loop termination), or is otherwise terminated, all intermediate findings that lived only in its local memory or scratch space are lost. The only surviving artifacts are whatever the parent orchestrator happened to log before the kill.

### The Korg Contract

**Memory persistence is independent of execution lifetime.**

Every worker is required to treat state-relevant updates as flushable micro-transactions:

- As soon as a worker has produced a coherent, provenance-backed observation or inference that could affect the vault, it **must** emit a `.ktrans` payload containing that update (even if the overall task is incomplete).
- On any termination path — graceful exit, panic, explicit `SIGKILL` from the Broker, or external signal — the worker (or its supervising Broker shim) is responsible for flushing a final `.ktrans` that captures the *state of the failure*.
  - For doom-loop terminations (see state-primitives §2), this final transaction **must** include the loop signature, token-velocity profile, and the last stable semantic state before the kill.
- The Broker accepts these terminal transactions with the same validation rules as normal ones. The resulting artifact is serialized into the vault (typically under `wiki/sessions/` or as a `failed-experiment` with `doom_loop: true`).

**Consequence:** A worker may be killed for resource or correctness reasons, yet every hard-won, verifiable piece of knowledge it produced survives as a first-class, queryable, mergeable fact. This is the mechanism that turns "crashed recon worker" into "enriched vault with documented failure mode."

The isolation layer (worktree + single `.ktrans` output) combined with this persistence rule guarantees that no valuable epistemic work is lost to transient process death.

---

## 3. The Three-Way Merge & Rebasement Protocol

### The Late-Arrival Problem

A `.ktrans` payload may arrive after its `base_snapshot` has become stale because:
- The worker was delayed inside an epoch window (isolation-routing).
- Network / scheduling jitter caused the transaction to be processed after other higher-throughput workers updated the same `target_path`.
- A higher-authority worker (per the authority-vector matrix) has already committed a conflicting change.

### The Contract

When the Broker receives a `.ktrans` whose `base_snapshot` no longer matches the current head of the target artifact(s), it performs a **headless semantic rebase** before applying the Merge-Arbitration Engine:

1. **Base comparison**: Compute the semantic delta between the worker's `base_snapshot` and the current vault state for every `target_path` in the mutations.
2. **Authority check**: For each mutation, compare the incoming `authority_vector` against any intervening changes.
   - If the intervening change came from a **strictly higher** authority vector, the incoming mutation is automatically re-targeted (if semantically safe) or moved to the `CONTESTED` holding area.
   - If the intervening change came from an equal or lower authority, the incoming mutation participates in normal three-way resolution (ancestor = base_snapshot, Mine = current vault, Theirs = the proposed mutation from the late `.ktrans`).
3. **Conflict outcome**:
   - Clean rebase possible → transaction is applied and a new `semantic-decision` records the rebasement.
   - Non-trivial conflict or lower authority → the entire conflicting subset of the transaction (or the whole payload) is written as a `CONTESTED` artifact. The original `tx_id` is preserved for audit. A first-class `semantic-decision` is created documenting the rebasement decision and the authority justification.
4. **Ordering**: All rebasement decisions respect the global `tx_id` (UUIDv7) partial order where possible, falling back to wall-clock timestamp + authority tie-breaker.

This protocol ensures that late-arriving but high-value transactions from slow or long-running workers are never silently discarded, while still protecting the integrity of higher-authority facts that arrived earlier.

The Three-Way Merge & Rebasement Protocol is the concrete realization of the "Three-Way Merge Resolution & State Rebasement Heuristics" referenced across the other mechanism notes.

---

## Cross-Triad Consistency Rules

- Every `.ktrans` that reaches the Broker has already passed the isolation and back-pressure filters defined in isolation-routing.md.
- Every mutation inside a `.ktrans` is interpreted through the Epistemic State Machine (state-primitives.md). An `INSERT` or `UPDATE` with sufficient verification evidence may advance artifacts to `VERIFIED`; a `CONTEST` action directly feeds the `CONTESTED` holding area and may trigger boundary relaxation.
- The Merge-Arbitration Engine (state-primitives §3) is the single point of authority for applying rebased `.ktrans` payloads. No worker or Broker may bypass it.
- All rebasement and arbitration decisions themselves become first-class, queryable `semantic-decision` artifacts with `domain: merge-arbitration`.

These three notes together form a closed, internally consistent specification for safe, concurrent, headless interaction with a governed transactional memory layer.

---

## Related

- [[wiki/mechanisms/state-primitives.md]] — The epistemic states and Merge-Arbitration Engine that consume and validate `.ktrans` payloads.
- [[wiki/mechanisms/isolation-routing.md]] — The physical execution environment that produces `.ktrans` artifacts and the back-pressure rules that protect the merge queue.
- [[Human/Methodology/The-Korg-Triad.md]] — Human explanation of the full worker lifecycle and why the `.ktrans` + rebasement design actually matters in practice.
- [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] — How operators actually monitor .ktrans quality, rebasement pressure, and contest outcomes in real time.
- [[Human/Methodology/Reviewing-and-Resolving-Contested-Facts.md]] — The detailed review and resolution workflow for the contested mutations that `.ktrans` payloads can produce.
- [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]] — Real-world usage of worktree isolation, `.ktrans`-style artifacts, and merge/conflict resolution at scale.
- [[wiki/patterns/Cross-Harness-Pattern-Extraction]] — The parent program. These three mechanism notes are the first complete, publishable triad of harness-agnostic execution primitives.
- [[wiki/concepts/Operational-Intelligence-Layer-Mandate]] — Why the transaction format, rollback decoupling, and rebasement rules must be explicit, typed, and queryable rather than implicit in any single harness implementation.

**Status (2026-05-20):** This note was extracted immediately after the prior two primitives, completing the minimal architectural triad required for any headless Korg-compatible harness or swarm. All three notes were subjected to internal `korg-challenge` review before finalization.

---

**Last updated:** 2026-05-20 — Third and final pillar of the core headless runtime contracts. The `.ktrans` handoff, persistence decoupling, and headless three-way rebasement protocol are now specified.

## See Also

- [[Synthesis — Transactional Memory]]


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[Conflict — isolation-routing and transactional-memory]] (dated 2026-05-21, confidence: low)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-21, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
