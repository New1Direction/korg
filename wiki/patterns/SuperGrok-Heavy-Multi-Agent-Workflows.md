---
title: "SuperGrok-Heavy-Multi-Agent-Workflows"
date: 2026-05-21
type: concept
tags: [pattern, multi-agent, orchestration, workflows, personas, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---


# Pattern: SuperGrok Heavy Multi-Agent Workflows

**A mature, production-grade set of personas, dynamic orchestrator patterns, and operational disciplines for running large-scale parallel agent systems with explicit output contracts, failure recovery, and knowledge preservation.**

This pattern was originally developed and refined for the Grok CLI in "SuperGrok Heavy" mode. It represents one of the most complete real-world expressions of the problems Korg was designed to govern.

---

## Description

SuperGrok Heavy is an operational framework for coordinating multiple specialized sub-agents. It emphasizes:

- Explicit, named **personas** with strict output contracts (specific `.md` files)
- A **Master Orchestrator** that dynamically selects and composes workflows
- Strong **monitoring and recovery** disciplines, especially around doom loops
- Systematic use of **worktree isolation** and conflict resolution across parallel agents

The framework treats multi-agent execution as a governed process rather than ad-hoc parallelism.

### Core Personas and Output Contracts

Each persona is defined with a clear role and a mandatory output artifact:

- **researcher** → `investigation_findings.md`
- **planner** → `implementation_plan.md`
- **implementer** → `implementation_summary.md`
- **reviewer** → `review_notes.md`
- **selector** (Best-of-N) → `selected_strategy.md`
- **synthesizer** → `synthesis.md`
- **conflict_resolver** / **merger** → `merge_decisions.md`
- **validator** → `validation_report.md`
- **test_writer** → `test_summary.md` + test files
- **security_auditor** → `security_audit.md`
- **integrator** → `integration_summary.md`

These output contracts create a predictable, queryable artifact graph — exactly the kind of structure Korg is optimized to capture and retrieve.

---

## Reusable Workflow Templates

The guide defines several composable high-level patterns:

- **Research → Implement → Review** (linear chain with `resume_from`)
- **Best-of-N Exploration → Selection → Implementation** (parallel researchers + selector)
- **Plan → Multiple Parallel Implementers → Integrator** (planner decomposes, multiple implementers own files, integrator merges)
- **Parallel Specialists → Synthesizer**
- **Explore → Test → Fix Loop**

The Master Orchestrator prompt instructs the parent agent to analyze task complexity and risk, then declare and execute the most appropriate workflow.

---

## Doom-Loop Detection and Recovery Strategies

One of the strongest contributions of the pattern is its explicit handling of agent failure modes:

**Detection signals:**
- No visible progress
- Repetitive behavior / same errors
- High token consumption with low semantic novelty
- Stuck in the same loop across multiple turns

**Recovery strategies:**
- Immediately cancel the stuck agent
- Tighten scope and retry
- Switch to a different persona
- Change the workflow entirely
- Partial worktree rollback
- Human escalation with clear options

**Parent rule:** "Never let a failing agent continue indefinitely."

This is one of the most direct real-world implementations of the non-interactive doom-loop primitive.

---

## Monitoring Checklist

The framework includes a disciplined parent-agent monitoring loop (reviewed every 5–15 minutes):

- Agent health (stuck, looping, cancelled, errored)
- Output contract compliance (are the required `.md` files being produced?)
- Worktree hygiene (stale changes, cross-worktree conflicts)
- Coordination hygiene (`resume_from` usage, persona alignment)

This checklist is a practical operationalization of the live campaign monitoring signals defined in Korg.

---

## Mapping to Korg Primitives

| SuperGrok Heavy Element                  | Korg Primitive / Contract                                                                 | Strength |
|------------------------------------------|-------------------------------------------------------------------------------------------|----------|
| Doom-loop detection + immediate cancel + recovery | Non-Interactive Doom-Loop Detection + Broker SIGKILL + terminal `.ktrans`                | Extremely High |
| Worktree isolation + conflict resolution | Ephemeral Worktree Isolation + Three-Way Merge & Rebasement Protocol + Conflict Resolver persona | Extremely High |
| Master Orchestrator / dynamic workflow selection | Leader role + dynamic `RouteWork` dispatch in the Leader-Broker-ACP Model                | Very High |
| Explicit personas + output contracts     | Typed artifacts + AI-first note contracts + Observational Memory synthesis               | Very High |
| Monitoring checklist (velocity, stuck agents, worktree state) | Live campaign signals (token velocity, AST delta, STALLED, contested queue)             | Very High |
| Parallel implementers + Integrator / Synthesizer | Parallel execution + Merge-Arbitration Engine + authority-vector resolution              | High |
| Best-of-N + Selector                     | Selection as a first-class epistemic step; feeds into `VERIFIED` or `CONTESTED`          | High |
| Staged application + validation gates    | Transactional write-back + `CONTESTED` holding area + human `semantic-decision` gate    | High |

---

## Why This Matters for Korg

SuperGrok Heavy demonstrates that sophisticated practitioners are already building exactly the kinds of systems Korg was designed to support — but they are doing so with ad-hoc conventions, implicit contracts, and manual recovery processes.

By extracting this pattern, Korg gains:

- A rich, battle-tested set of reusable workflow templates
- Concrete examples of the exact failure modes (doom loops, worktree conflicts, knowledge loss) that our primitives were built to solve
- A driving use case for the reference-harness and ACP work

This pattern shows that the Korg kernel is not theoretical — real operators are already feeling the pain it was designed to eliminate.

---

## Related

- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine and Non-Interactive Doom-Loop Detection
- [[wiki/mechanisms/isolation-routing.md]] — Worktree isolation, STALLED handling, and back-pressure
- [[wiki/mechanisms/transactional-memory.md]] — `.ktrans` handoff and merge/rebase guarantees
- [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]] — The formal coordination model this pattern approximates
- [[wiki/reference-harness/ACP-Binding-Design.md]] — The protocol surface needed to make these workflows first-class and governed
- [[Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md]] — How harness authors can implement SuperGrok-Heavy-style orchestration against the Korg contracts
- [[Human/Methodology/How-to-Watch-a-Live-Campaign.md]] — The monitoring discipline this pattern makes explicit

**Status:** High-value, battle-tested pattern ready for reuse and formalization within Korg. This is currently one of the strongest external data points validating the entire Korg architecture.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[Operational-Intelligence-Layer-Mandate]] (dated 2026-05-21, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
