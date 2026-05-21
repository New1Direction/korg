---
title: "0002 - Lighter Pragmatic AI-First Approach for Korg"
date: 2026-05-21
type: decision
tags: [decision, methodology, korg, reconciled, yvaeh-mode]
status: reconciled
ai-first: true
confidence: high
---


# 0002 — Lighter Pragmatic AI-First Approach for Korg

**Date:** 2026-05-19  
**Status:** Active

## Decision

Adopt a **lighter, pragmatic version** of the AI-first + wiki-style system for the Korg vault, at least in the early phase.

### What we keep
- Wiki-style folder structure (`wiki/entities/`, `wiki/concepts/`, `wiki/projects/`, `wiki/daily/`, `wiki/decisions/`, `wiki/synthesis/`, `raw/`)
- `_GROK.md` as the living operating manual at the vault root
- Spirit of AI-first: self-contained notes, good `[[wikilinks]]`, sensible propagation on important writes
- Use of dedicated Grok skills (`korg-challenge`, `korg-save`, etc.)

### What we intentionally soften / defer for now
- Mandatory long `## For future Grok` preambles on every note
- Strict recency markers on *every* external claim
- Heavy "update five (or seven) places on every single write" rule
- Full bi-temporal timeline tracking on every fact change

## Rationale

The vault currently has very little lived history. Applying the full strict obsidian-second-brain-style contract on day one risks over-structuring and bureaucratic overhead before the vault has earned it.

We want real signal from actually using the vault with lighter rules first. We can always tighten the conventions later once we have 30–50 real notes and have run `/korg-challenge` multiple times on actual content.

## Context

This decision came directly out of the first successful run of `/korg-challenge` on the topic "the decision to go wiki-style + AI-first for Korg".

The challenge surfaced the risk of premature heavy structure and recommended starting lighter.

## Consequences

- Faster iteration in the early days
- Lower friction when capturing thoughts during deep work
- The structure (`_GROK.md`, wiki folders, skills) is still in place and can be hardened later without major rework
- We will periodically re-evaluate the strictness as the vault grows

## Related

- [[wiki/decisions/0001 - Project Kickoff]]
- [[wiki/projects/Korg]]
- First run of `korg-challenge` (2026-05-19)
- Creation of `korg-save` skill

---

**Review trigger:** Revisit this decision once the vault has accumulated meaningful history (target: ~40–50 real notes or after 3–4 major work sessions).


## Reconciled History

- **Reconciled on:** 2026-05-21 by Yvaeh Mode
- **Winner Source:** [[AI-First Vault Principles]] (dated 2026-05-19, confidence: high)
- **Resolution:** Auto-resolved contradictions in favor of the chronologically superior source.
