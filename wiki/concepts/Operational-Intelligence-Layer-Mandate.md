---
title: "Operational-Intelligence-Layer-Mandate"
date: 2026-05-21
type: concept
tags: [semantic-decision, philosophy, korg, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---


# Semantic Decision: Korg as Operational Intelligence Layer (Not Documentation)

**Date:** 2026-05-19

## Decision

Korg must be optimized primarily for **agent retrieval, synthesis, pattern linking, and semantic reuse** — not for human reading pleasure.

### Core Requirements
- Small, focused documents (one clear semantic unit)
- Explicit, consistent typing and tagging
- Stable terminology and normalized naming across harnesses
- Dense, intentional cross-linking
- First-class treatment of operational and experimental knowledge

### High-Value Artifact Types (Priority)
- Failed experiments & dead ends
- Harness edge cases & tool behavior anomalies
- Semantic decisions & their rationale
- Session lifecycle patterns
- Orchestration / ACP / protocol evolution
- Emergent best practices and heuristics

Low-value (to be deprioritized):
- Random brainstorming
- Polished design docs disconnected from operational reality

## Rationale

In the domain of building Grok-native harnesses, orchestrators, and agent systems:
- Patterns emerge gradually through experimentation
- Semantics evolve
- Failures and edge cases are highly informative
- Normal code repositories destroy most of this signal (decisions disappear into commits, experiments get buried)

A dedicated, queryable operational memory becomes a major force multiplier as the number of harnesses and the complexity of ACP/TUI/runtime behavior grows.

The long-term highest-leverage use is **cross-harness pattern extraction** (session semantics, supervision/watch-mode models, artifact emission, recovery strategies, structured logging, etc.) across tools like cli-anything, api-anything, RedMicro, and future harnesses for Bun, Blender, FreeCAD, GIMP, Docker, Kubernetes, Playwright, etc.

## Consequences

- We will favor small, typed, densely linked notes over large readable documents.
- Typing and tagging discipline (`type:`, `harness:`, `domain:`) is now a first-order concern.
- `korg-save` and future synthesis skills must be tuned to surface the high-value categories above.
- The vault should feel like persistent agent-operational memory + experimentation corpus, not a wiki.

## Related

- [[_GROK.md]] (updated philosophy section)
- [[wiki/decisions/0002 - Lighter Pragmatic AI-First Approach for Korg]]
- First explicit articulation of the cross-harness pattern vision


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-19, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
