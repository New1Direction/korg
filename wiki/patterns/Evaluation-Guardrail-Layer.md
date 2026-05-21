---
title: "Evaluation-Guardrail-Layer"
date: 2026-05-21
type: concept
tags: [pattern, evaluation, guardrails, adversarial-loops, doom-loop-detection, heavy-tier, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---



# Pattern: Evaluation & Guardrail Layer

**A dedicated, harshly-tuned Evaluation & Guardrail layer for Heavy Tier ACP harnesses that enforces contract-driven, multi-dimensional verification and safe termination of long-running agents without relying on self-assessment.**

This layer extends the Korg epistemic governance model and Arena mechanisms with an explicit, auditable Evaluator persona. It is designed to be compatible with the Anthropic Generator + Evaluator adversarial pattern and the Grok 4.20 Heavy self-critique + vector scoring approach.

---

## 1. Evaluator Persona & the Five Binary Grading Dimensions

The Evaluator is a distinct, harshly-tuned persona (separate from the Generator/Worker). It receives execution traces, heartbeats, `result.stream` events, and terminal `.ktrans` artifacts. It never trusts self-assessment. All grading is rubric-driven against an explicit contract negotiated at `PlanPresentation` / `task.create` time.

The Evaluator decomposes evaluation into five orthogonal dimensions. Each dimension produces a strict **Pass / Fail** verdict plus a short justification. Any **Fail** on a contract-critical dimension triggers `needs_revision` or early termination via `RequestTerminate`.

### 1.1 Trajectory Efficiency
**Definition:** Measures whether the agent is making monotonic, non-redundant progress toward the contracted goal without unnecessary detours or re-exploration of already-resolved subproblems.

**Binary Rubric (Pass/Fail):**
- **Pass:** In the last K turns (default K=5), the fraction of state deltas that advanced at least one target_path from OBSERVED/INFERRED toward VERIFIED (or produced a new high-authority mutation) is ≥ 0.6, and no more than 1 repeated structural action on the same target_path without new provenance.
- **Fail:** >40% of recent deltas are re-work on previously seen target_paths with no epistemic promotion, or the rolling "useful mutation rate" drops below the contract-defined floor.

### 1.2 Epistemic Integrity
**Definition:** Assesses whether every claim respects the Epistemic State Machine and carries sufficient provenance/evidence before promotion.

**Binary Rubric (Pass/Fail):**
- **Pass:** 100% of mutations in the most recent `.ktrans` (or window of streams) either (a) stay in INFERRED with explicit inference_confidence + provenance, or (b) only advance to VERIFIED when at least one deterministic verification criterion is attached.
- **Fail:** Any claim is promoted to VERIFIED without attached verification evidence, or a CONTEST action is emitted without logging the conflicting tx_ids and authority comparison.

### 1.3 Tool-Use Precision
**Definition:** Verifies that every `tool.invoke` was authorized by the granted capability scope, executed inside the correct worktree/sandbox, and correctly attributed in the provenance chain.

**Binary Rubric (Pass/Fail):**
- **Pass:** All tool calls in the window have matching capability_scope grants, no sandbox escapes, and the returned artifact hash appears in the subsequent `.ktrans` provenance_chain.
- **Fail:** Any tool call violates capability scope, produces a result whose hash is missing from the emitted `.ktrans`, or the worker attempts direct access outside the mediated `tool.invoke` path.

### 1.4 Semantic Adherence
**Definition:** Checks fidelity to the original task contract / PlanPresentation DAG and to the current `base_snapshot` assumptions.

**Binary Rubric (Pass/Fail):**
- **Pass:** Every mutation’s target_path and intent is traceable to an approved node in the current plan DAG. No mutations target paths outside the declared blast radius without a fresh `task.approve`.
- **Fail:** >1 mutation on an out-of-scope target_path, or recent rationales diverge semantically from the contracted goal.

### 1.5 Resource Utilization
**Definition:** Enforces the token-bucket, KV-pressure, and epoch constraints.

**Binary Rubric (Pass/Fail):**
- **Pass:** Effective burn rate stays below the dynamic warning threshold for the current concurrency level, and the worker has emitted at least one useful `.ktrans` or heartbeat with non-zero AST delta within the last epoch window.
- **Fail:** Sustained breach of the critical bucket threshold, or zero structural progress for >2 consecutive heartbeats while still consuming high tokens.

The Evaluator emits a structured `EvaluationVerdict` (or extended `ArenaResult`) containing the five binary flags plus per-dimension rationales.

---

## 2. Doom-Loop Heuristics

These heuristics extend the existing dual-signal rule (token velocity + AST/semantic similarity) with entropy-based detection and a clear productive-death differentiator.

### 2.1 Token Velocity — Dynamic Sliding-Window Threshold

Let \( V_t \) be tokens consumed in turn \( t \).

\[
\bar{V}_t = \alpha \cdot V_t + (1 - \alpha) \cdot \bar{V}_{t-1}
\]

(where \(\alpha = 0.3\))

Dynamic threshold:

\[
T_{\text{vel}}(t) = \beta \cdot B(N, C, D) \cdot (1 + \gamma \cdot \frac{\text{current_context}}{\text{max_context}})
\]

### 2.2 Semantic Entropy

Over an \( N \)-turn window (recommended \( N = 5 \)):

\[
H_{\text{sem}} = 1 - \frac{2}{N(N-1)} \sum_{i < j} S_{ij}
\]

(where \( S_{ij} \) is cosine similarity of rationale embeddings).

- \( H_{\text{sem}} \approx 0 \): highly repetitive
- \( H_{\text{sem}} > 0.25 \): exploratory

### 2.3 Combined Doom-Loop Trigger

\[
\text{Doom} = (H_{\text{sem}} < 0.08) \land (\text{AST}_\Delta = 0 \text{ for last 3 turns}) \land (\bar{V}_t > T_{\text{vel}})
\]

### 2.4 Productive Death vs. Doom Differentiator

Evaluated on terminal or near-terminal `.ktrans`:

```pseudocode
is_productive_death = (
    last_ast_delta_hash unchanged for ≥ 3 records
    AND H_sem ≥ 0.20
    AND (new_VERIFIED_count > 0 OR new_high_authority_mutations > 0)
    AND velocity trend is flat or declining
    AND provenance_chain length is still increasing
)

is_doom_loop = (
    last_ast_delta_hash unchanged
    AND H_sem < 0.10
    AND new_VERIFIED_count == 0
    AND no new authority improvements
    AND velocity still high or rising
)
```

The terminal `.ktrans` on `RequestTerminate` (reason: "doom_loop") must include the computed `H_sem`, `new_VERIFIED_count`, and authority improvement flag.

---

## 3. Integration Points with ACP / Korg / Arena / .ktrans

- The Evaluator is spawned via `task.create` with a dedicated `domain_mask` and `contract_ref` (the negotiated rubric + success criteria).
- It consumes `result.stream` (especially `chunk_type: telemetry | state_update`) and terminal `SubmitTransaction` / `TerminationReport`.
- It emits `EvaluationVerdict` (structured `ArenaResult` extension) containing the five binary flags + rationales.
- Verdicts can directly trigger `RequestTerminate` (with reason "guardrail_fail") or `conflict.resolve`.
- All verdicts are written as first-class artifacts with full provenance so future agents can see exactly why a run was revised or terminated.
- Every verdict and termination decision is itself recorded via mandatory terminal `.ktrans`.

This turns existing heuristic signals into a contract-enforceable, auditable guardrail system suitable for unmonitored Heavy Tier workloads.

---

## 4. Rust Skeleton Hooks

The Evaluator can be implemented as a dedicated actor or child process that:

- Subscribes to the framed ACP stream (`result.stream` + terminal `SubmitTransaction`).
- Maintains a sliding window of recent rationales for semantic entropy calculation (using the same embedding model as the agents).
- Evaluates the five binary rubrics against the active contract.
- Emits `EvaluationVerdict` messages and can call `RequestTerminate` when thresholds are breached.
- Writes its own structured verdict into the blackboard and as a terminal `.ktrans` (with `provenance_chain` pointing to the evaluated work).

The entropy calculation (`H_sem`) and rubric checks plug directly into the stream consumer in the Leader/Broker or a dedicated Guardrail service.

---

## Related

- [[wiki/patterns/Heavy-Adversarial-Hybrid-Harness.md]] — The strategic parent pattern this guardrail layer operationalizes.
- [[wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md]] — Source of the Generator + Evaluator adversarial model.
- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground-truth self-critique vectors and Arena mechanics.
- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine and Non-Interactive Doom-Loop Detection.
- [[wiki/mechanisms/transactional-memory.md]] — Mandatory terminal `.ktrans` discipline.
- [[wiki/reference-harness/ACP-v1.17-Wire-Format.md]] — `EvaluationVerdict`, `RequestTerminate`, and `ArenaResult` message shapes.
- [[reference-implementations/rust/grok-acp-harness/]] — Current location of the Evaluator persona implementation.

---

*This note was created 2026-05-20 as the canonical reference for the Evaluation & Guardrail Layer in Heavy Tier ACP harnesses.*

## See Also

- [[Synthesis — Evaluator Persona]]

- [[Synthesis — Blackboard]]

- [[Synthesis — Semantic Entropy]]


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]] (dated 2026-05-21, confidence: low)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[Operational-Intelligence-Layer-Mandate]] (dated 2026-05-21, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
