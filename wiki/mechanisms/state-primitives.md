---
title: "state-primitives"
date: 2026-05-21
type: concept
tags: [pattern, state-primitives, epistemic-governance, headless-protocol, doom-loop, merge-arbitration, orchestration, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---



# Harness-Agnostic State Primitives

**This note extracts the core state governance and orchestration mechanics observed in the Grok Build TUI into universal, harness-independent Execution Primitive Modules.** These primitives define how *any* headless offensive worker, long-running agent pipeline, or multi-node swarm interacts with the Korg transactional memory layer when operating without human UI intervention.

The goal is to prove Korg as a generalized semantic execution kernel rather than a TUI-specific case study. All subsequent headless harnesses (CLI daemons, background recon workers, swarm coordinators) inherit these behaviors by reference.

---

## 1. The Epistemic State Machine (Headless Protocol)

Headless agents must advance artifacts through a deterministic 4-state lifecycle without relying on human review gates:

`OBSERVED → INFERRED → VERIFIED → CONTESTED`

### State Transitions

- **OBSERVED**: Raw artifact ingested from tool output, transcript segment, or external source. No semantic processing applied yet. Always the entry point for any new datum.
- **INFERRED**: Initial classification, hypothesis generation, or semantic extraction performed by LLM or heuristic. Still provisional; carries explicit `inference_confidence` and provenance.
- **VERIFIED**: Artifact has passed one or more deterministic verification criteria. Only VERIFIED facts may be promoted into the permanent Korg vault as first-class memory (Hard Discovered Constraints, Aborted Hypotheses with final status, etc.).
- **CONTESTED**: Contradiction detected (new evidence conflicts with prior VERIFIED claim, or cross-agent synthesis produces EPISTEMIC_CONTRADICTION). Triggers soft/hard purge paths and boundary relaxation.

### Deterministic Entry Criteria for VERIFIED

An artifact transitions to VERIFIED only when at least one of the following holds (recorded in the artifact's `verification` frontmatter or metadata block):

- **Cryptographic hash match**: The exact byte content (or canonical serialization) matches a previously observed immutable snapshot or reference hash from a trusted source.
- **Multi-tool validation pass**: Two or more independent tools (static analyzer + dynamic fuzzer + network oracle, etc.) produce congruent results on the same claim with no divergence in key fields.
- **Cross-agent consensus with provenance**: N-of-M agents independently arrived at the identical conclusion via different tool paths, with full lineage preserved (no circular self-reference).
- **Explicit authority-vector override**: A higher-authority source (see §3) explicitly stamps the fact (e.g., a deterministic static-analysis engine declaring an endpoint shape).

Any transition must emit a state delta that is itself recorded for doom-loop detection (see §2).

---

## 2. Non-Interactive Doom-Loop Detection

In a TUI a human can click "Cancel." Headless environments have no such escape hatch. The system must self-terminate before token waste or context poisoning becomes catastrophic.

### Mechanism

The Leader-Broker (or equivalent orchestrator) continuously monitors the agent's emitted state-delta stream for two coupled signals:

1. **Token-velocity ceiling breach** — sustained high token consumption with low semantic novelty (tracked via rolling window on delta embeddings).
2. **AST-mutational delta tracking** — the structural (AST-level) difference between consecutive emitted artifacts or plan steps. When semantic similarity of the *natural-language description* of three consecutive state deltas exceeds **0.98** (cosine on embedding) **and** the underlying tool-output / AST delta is **zero** (exact structural match), the sequence is classified as a tautological loop.

### Termination Rule

On detection of **3 consecutive** qualifying deltas:

- The Broker issues a hard `SIGKILL` (or equivalent process termination) against the offending worker's context window.
- The current partial run is serialized as a `failed-experiment` or `edge-case` artifact (with `doom_loop: true` flag).
- The failure is injected back into Observational Memory under the Epistemic Constraint Boundary mechanism so future agents see the exact loop signature and the token-velocity profile that preceded it.

This is the headless analogue of the human "Cancel" + synthesis purge path. It protects both compute budget and the long-term integrity of the vault.

---

## 3. The Headless Merge-Arbitration Engine

When multiple parallel offensive workers write back to the central Korg vault concurrently (e.g., one worker mapping authentication endpoints while another fuzzes SSRF vectors on overlapping attack surface), a deterministic merge strategy is required.

### Core Rule

All concurrent writes are staged through the Transactional Memory layer (immutable snapshot → worktree/sandbox mutation → verification → candidate write-back). The Merge-Arbitration Engine then applies an **authority-vector resolution matrix** before any vault mutation is committed.

### Authority Vector Precedence (highest first)

- **Deterministic static analysis** (exact AST diff, type-checked output, cryptographic provenance of the analyzer binary itself)
- **Multi-tool corroborated dynamic evidence** (fuzzer + network oracle + timing side-channel that all agree on a concrete fact)
- **Single-tool high-fidelity observation** (with full request/response captured)
- **Probabilistic LLM inference** (lowest precedence; always treated as `INFERRED` until elevated by one of the above)

When two candidates conflict on the same key (e.g., "parameter `id` is an integer" vs "parameter `id` accepts arbitrary SSRF payload"):

- The higher authority vector always wins.
- The losing candidate is recorded as `CONTESTED` with a pointer to the winning artifact and the authority justification.
- If authorities are equal, the merge is marked for later human (or higher-tier orchestrator) arbitration and the conflicting facts remain in a `CONTESTED` holding area (never promoted to VERIFIED).

### Implementation Notes for Headless Harnesses

- The Broker maintains a short-lived merge queue keyed by `(attack_surface_id, claim_key)`.
- Resolution is atomic with respect to the vault write transaction.
- Every arbitration decision itself becomes a first-class `semantic-decision` artifact with `harness: korg` and `domain: merge-arbitration`.

This engine is what allows safe horizontal scaling of offensive workers without corrupting the shared epistemic state.

---

## Related

- [[wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md]] — External confirmation that dedicated verification loops and adversarial evaluation (rather than self-assessment) are essential for long-running agent work — directly supporting the Epistemic State Machine and Merge-Arbitration Engine.

- [[wiki/patterns/Cross-Harness-Pattern-Extraction]] — The parent vision for decontextualizing TUI-derived patterns.
- [[wiki/concepts/Operational-Intelligence-Layer-Mandate]] — Why these primitives are first-class citizens.
- [[wiki/mechanisms/isolation-routing.md]] — The concrete physical contracts (Leader-Broker routing with STALLED semantics, ephemeral worktree isolation + `.ktrans` handoff, and state-aware back-pressure).
- [[wiki/mechanisms/transactional-memory.md]] — The `.ktrans` schema, rollback-decoupled persistence, and headless Three-Way Merge & Rebasement Protocol that operationalizes all mutations against the Epistemic State Machine.
- [[Human/Methodology/The-Korg-Triad.md]] — Human-readable explanation of why these states and rules exist and what they feel like in practice.
- [[Human/Methodology/Reviewing-and-Resolving-Contested-Facts.md]] — The practical operator workflow for the `CONTESTED` state and Merge-Arbitration Engine.
- [[wiki/patterns/SuperGrok-Heavy-Multi-Agent-Workflows.md]] — One of the strongest external validations of doom-loop detection, recovery, and epistemic governance.
- [[wiki/patterns/Leader-Broker-ACP-Model-for-Parallel-Agents]], [[wiki/patterns/Worktree-Isolation-Per-Subagent]], and [[wiki/patterns/Bounded-Buffers-and-Drop-Oldest-Synchronization]] (future detailed variants) — Implementation patterns that realize the contracts.
- [[wiki/tooling/Synthesis-Failure-Modes-and-Memory-Purging]] (future) — How CONTESTED and routing-failure artifacts feed the epistemic immune system.

These primitives are the minimal contract any new harness (headless CLI, swarm daemon, RedMicro wave coordinator, etc.) must implement or delegate to the Korg state layer.

---

**Last updated:** 2026-05-20 — Extracted from Grok Build TUI operational traces into harness-agnostic form as the first concrete step of the Cross-Harness Pattern Extraction program.

## See Also

- [[Synthesis — Transactional Memory]]


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]] (dated 2026-05-21, confidence: low)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-21, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
