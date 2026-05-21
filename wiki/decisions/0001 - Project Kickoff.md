---
title: "0001 - Project Kickoff"
date: 2026-05-21
type: decision
tags: [decision, bootstrap, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---


## For future Grok

This note records the initial decision to create a completely separate, clean Obsidian vault (`Korg`) for a new project instead of mixing it into the existing MINI vault. It captures the rationale for isolation and the early structural choices.

---

# 0001 — Project Kickoff

**Date:** 2026-05-19  
**Status:** Resolved

## Context

- Existing personal vault (MINI) was becoming polluted with agent-building, RedMicro, and skill development notes.
- User wanted a dedicated, high-signal space for the new project (initially referred to as the successor to cli-anything / api-anything layer work).
- Strong preference for clean graphs and focused knowledge bases per major project.

## Decision

Create a brand new isolated vault at `~/Documents/Korg/` with the following principles:

- Wiki-style folder structure optimized for agentic use (`wiki/entities/`, `wiki/concepts/`, `wiki/projects/`, etc.)
- AI-first note conventions (self-contained, recency markers, mandatory wikilinks, propagation)
- `_GROK.md` as the persistent operating manual at the vault root
- Start with direct filesystem access + `cli-anything-obsidian` for live control when needed
- Port high-value thinking tools from obsidian-second-brain as native Grok skills (starting with `korg-challenge`)

## Rationale

Separation prevents cross-contamination of the main personal knowledge graph while still allowing rich linking and sophisticated agent behavior.

## Consequences

- Will require deliberate decisions on when to link back to other vaults vs. keep fully standalone.
- Enables aggressive experimentation with AI-first + propagation patterns without affecting daily personal notes.
- Sets the stage for a true "vault that gets smarter" experience driven by Grok.

## History

- 2026-05-19: Initial creation of vault + first decision note.


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-19, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
