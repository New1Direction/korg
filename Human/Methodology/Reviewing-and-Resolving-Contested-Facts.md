---
date: 2026-05-20
title: Reviewing and Resolving Contested Facts
type: methodology
tags: [methodology, contested-facts, merge-arbitration, operator-workflow, epistemic-lifecycle]
---

# Reviewing and Resolving Contested Facts

One of the most important (and most misunderstood) parts of operating a Korg system is what happens when facts become `CONTESTED`.

This is not a bug. It is one of the kernel’s core protection mechanisms. When two or more sources disagree on something that matters, the system refuses to silently pick a winner. Instead, it moves the conflicting claims into the `CONTESTED` holding area and surfaces them for review.

A good operator does not treat contested facts as noise or failure. They treat them as the system doing its job — flagging places where the evidence is genuinely in tension and human judgment (or higher-authority automation) is required.

---

## What a CONTESTED Artifact Actually Is

A fact becomes `CONTESTED` when:

- New evidence arrives that conflicts with a previously `VERIFIED` claim.
- Two `.ktrans` payloads propose incompatible mutations on the same target (e.g., one says “parameter `id` is an integer,” another says “it accepts arbitrary SSRF payloads”).
- A late-arriving transaction fails the headless three-way rebase because a higher-authority change has already been committed.
- Cross-agent synthesis detects an `EPISTEMIC_CONTRADICTION`.

The artifact itself is not deleted or overwritten. It is moved to a holding state with full provenance of both (or all) sides of the conflict, the authority vectors involved, and the exact moment the contradiction was detected.

This is the epistemic immune system in action.

---

## Why the System Creates Contested Facts

The kernel deliberately errs on the side of **visible conflict** rather than silent corruption.

Without the `CONTESTED` state and the Merge-Arbitration Engine, you would eventually accumulate a vault full of “probably true” statements that slowly become untrustworthy. The moment two workers (or two runs) disagree on something important, the system forces the disagreement into the open.

This gives operators two powerful things:

1. **Auditability** — You can always see exactly why something is contested and what the competing claims were.
2. **Safety** — High-authority facts (especially deterministic static analysis) are protected from being overwritten by lower-authority inferences.

---

## The Operator’s Review Workflow

Not every contested fact needs immediate attention. The skill is knowing which ones matter and when.

### 1. Triage by Authority Differential
- **High differential** (e.g., deterministic static analysis vs. LLM inference): Usually safe to let the Merge-Arbitration Engine resolve automatically. The higher authority wins.
- **Low or equal differential**: These are the ones worth looking at. Two roughly equal sources are in tension.

### 2. Look at Provenance and Recency
- How fresh is each side of the conflict?
- What was the `base_snapshot` of the late-arriving transaction?
- Did one side come from a worker that later produced a rich final `.ktrans`, or from one that died in a doom-loop?

### 3. Assess Blast Radius
- Is this fact used by many downstream claims?
- Would accepting the wrong side quietly poison a large part of the attack surface model?

High blast-radius contested facts should rise to the top of the review queue.

### 4. Decide on Resolution Path
- Let the engine rebase / apply authority rules (default for most cases)
- Manually promote one side to `VERIFIED` (with a `semantic-decision` record)
- Trigger a boundary relaxation or additional verification work
- Escalate into a first-class human `semantic-decision`

---

## How Authority Vectors and the Merge-Arbitration Engine Actually Decide

The engine follows a strict, deterministic hierarchy (highest first):

1. Deterministic static analysis (with cryptographic provenance of the analyzer itself)
2. Multi-tool corroborated dynamic evidence
3. Single-tool high-fidelity observation with full request/response capture
4. Probabilistic LLM inference (lowest precedence)

When a `.ktrans` arrives with a `CONTEST` action or triggers a rebase conflict, the engine compares the authority vector of the new claim against any existing claims on the same target.

- Clear higher authority → the lower claim is moved to `CONTESTED` (or stays there) and the higher one wins.
- Equal or near-equal authority → the conflict remains contested and a `semantic-decision` artifact is created documenting the situation.

The engine never guesses. It only applies the recorded authority rules and the three-way rebase logic.

---

## Common Patterns in Contested Facts

**Pattern 1: Static Analysis vs. Dynamic Fuzzing**
- Static analysis says “this endpoint only accepts integers.”
- A fuzzer found a path that sent a string and got a 200.
- Usually the fuzzer result is lower authority unless it was multi-tool corroborated. The engine often resolves this correctly without human help.

**Pattern 2: Two Different Workers, Same Surface, Different Conclusions**
- Worker A (mapping auth) says “no authentication required.”
- Worker B (deeper recon) says “authentication is required but can be bypassed via header X.”
- These are often complementary rather than contradictory. A human review can turn this into a richer `VERIFIED` fact instead of a simple win/lose.

**Pattern 3: Late High-Value Transaction**
- A slow, thorough worker finally emits a `.ktrans` that contradicts several faster, shallower workers.
- This is one of the most valuable kinds of contest. It often means the faster workers were wrong or incomplete.

**Pattern 4: Doom-Loop Worker Produces a Contested Claim**
- A worker that was eventually killed for looping still managed to emit a `.ktrans` before death.
- These should usually be treated with extra skepticism unless the final diagnostic `.ktrans` shows the loop happened *after* the contested claim was made.

---

## When to Escalate to a Human Semantic-Decision

You should create an explicit `semantic-decision` when:

- The conflict involves two sources of roughly equal authority and the resolution has significant downstream impact.
- You are choosing to relax an existing boundary or override a previous `VERIFIED` fact.
- You want to record a deliberate policy decision (“we will treat any finding from this specific static analyzer as authoritative for the next 30 days”).
- The automated engine has left a cluster of contested facts that are blocking progress on a high-value attack surface.

Every human `semantic-decision` should include:
- The exact facts being resolved
- The rationale (including why the authority rules were insufficient or needed augmentation)
- The date and the operator

This keeps the vault honest about where human judgment was injected.

---

## Forward Pointers

Once you are comfortable reviewing contested facts, the next natural areas to deepen are:

- Metrics around contest volume, resolution time, and authority conflict rates
- Building a “contested facts dashboard” that surfaces the highest-blast-radius items first
- How ACP can let different harnesses contribute to the same contested queue → [[wiki/reference-harness/ACP-Binding-Design.md]] (first protocol sketch)
- Writing playbooks for specific recurring contest patterns (static vs. dynamic, late high-value transactions, etc.)

The `CONTESTED` state is where the kernel’s claim to being a *governed* memory layer is most visible. It is also where skilled operators add the most long-term value.

---

## Related

- [[Human/Methodology/The-Korg-Triad.md]] — The epistemic states (especially `VERIFIED` → `CONTESTED`) this workflow operates on.
- [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] — How contest pressure and the contested queue appear in real-time monitoring.
- [[wiki/mechanisms/state-primitives.md]] — The formal definition of the Epistemic State Machine and the Merge-Arbitration Engine.
- [[wiki/mechanisms/transactional-memory.md]] — How `CONTEST` actions in `.ktrans` payloads feed this workflow.
- [[wiki/mechanisms/isolation-routing.md]] — How late-arriving or `STALLED` transactions often create the contests you will review.

This note completes a strong four-piece foundation for the human layer. The next phase will likely shift toward reference implementations and deeper technical guidance.