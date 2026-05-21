---
id: 0001
title: Project Kickoff
date: 2026-05-19
status: open
tags: [decision]
---

# 0001 — Project Kickoff

**Date:** 2026-05-19  
**Status:** Open

## Context

New isolated vault created for the Korg project.

## Decision

- Use a completely separate Obsidian vault (`~/Documents/Korg/`) so the knowledge graph, daily notes, and search stay clean and project-specific.
- Structure follows a lightweight but effective system:
  - `Index.md` as the single source of truth / dashboard
  - `Decisions/` for numbered architectural decisions
  - `Architecture/`, `Implementation/`, `Prompts/`, `Sessions/`, `Tests/`

## Rationale

Separation prevents pollution of the main personal vault (MINI) while still allowing rich linking and agent-friendly markdown.

## Consequences

- Will need to decide later whether to link back to other vaults (e.g. via `[[MINI/Note]]` style or file embeds) or keep it fully standalone.
- Easy to later enable the Local REST API plugin on this vault if live CLI control is desired.

## Next

- Flesh out the one-line description and top-level goals on the Index.
- Start capturing the first real technical decisions.
