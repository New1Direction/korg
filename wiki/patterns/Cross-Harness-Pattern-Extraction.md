---
title: "Cross-Harness-Pattern-Extraction"
date: 2026-05-21
type: concept
tags: [pattern, vision, long-term, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---



# Pattern: Cross-Harness Pattern Extraction

**Status:** Emerging long-term capability

## Description

The ability to identify, name, and reuse recurring structural and operational patterns across different CLI harnesses and agentic tools.

### Examples of recurring structures observed or anticipated
- Session semantics and lifecycle models
- Watch-mode / live supervision handling
- Process supervision and recovery strategies
- Structured artifact emission (logs, outputs, state)
- ACP (Agent Control Protocol) interaction models
- TUI state management and event loops
- Tool behavior anomaly handling

## Why This Matters

As the harness ecosystem grows (cli-anything family, api-anything, RedMicro orchestrators, future harnesses for editors, build tools, etc.), the same hard problems keep reappearing in slightly different forms.

Being able to:
- Recognize when a new harness is hitting a known pattern
- Retrieve prior solutions + failures
- Compose better implementations from proven parts

...becomes a major productivity and quality multiplier.

## Current State in Korg

We have begun capturing the raw material (semantic decisions, edge cases, session patterns, failed experiments) in a typed, linkable way so that future synthesis skills can perform this extraction.

**2026-05-20 milestones (triad completion):**
- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine + verification criteria + doom-loop detection + merge arbitration.
- [[wiki/mechanisms/isolation-routing.md]] — Leader-Broker routing, ephemeral worktree isolation, `.ktrans` handoff, differentiated back-pressure.
- [[wiki/mechanisms/transactional-memory.md]] — `.ktrans` schema (UUIDv7 + provenance + mutations with CONTEST action), code-rollback vs memory-persistence decoupling, headless Three-Way Merge & Rebasement Protocol.
- Two internal `korg-challenge` reviews performed before the final two extractions. All three notes now form a closed, consistent, harness-agnostic specification (epistemic states + physical boundaries + transactional handoff).
This triad is the minimal publishable foundation for Korg as a generalized semantic execution kernel.

The first human-facing explanation of the complete system lives at [[Human/Methodology/The-Korg-Triad.md]].

## Related

- [[wiki/patterns/Anthropic-Long-Running-Agent-Harnesses.md]] — Recent high-value extraction demonstrating the methodology in action.

- [[wiki/concepts/Operational-Intelligence-Layer-Mandate]]
- [[wiki/mechanisms/state-primitives.md]] — Epistemic State Machine + verification + doom-loop + merge arbitration
- [[wiki/mechanisms/isolation-routing.md]] — Leader-Broker routing, worktree isolation, `.ktrans`, differentiated back-pressure
- Korg's core mandate for retrieval + synthesis
- Future skills: pattern extraction / synthesis across harnesses


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[Conflict — Evaluation-Guardrail-Layer and Cross-Harness-Pattern-Extraction]] (dated 2026-05-21, confidence: low)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[Operational-Intelligence-Layer-Mandate]] (dated 2026-05-21, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
