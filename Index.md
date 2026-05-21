---
title: Korg
type: dashboard
tags: [project, active, korg]
ai-first: true
status: setup
created: 2026-05-19
updated: 2026-05-20
---

# Korg

**One-line:** Isolated Grok-native knowledge base for the new project (successor-layer work, agent patterns, API/harness experiments).

**Vault Style:** Wiki-style + AI-first (see [[_GROK.md]])

---

## Current Status

- **Phase:** Initial structure + AI-first foundation + Cross-Harness primitives extraction
- **Last worked on:** 2026-05-20
- **Next actions:**
  - The core triad (`state-primitives`, `isolation-routing`, `transactional-memory`) is complete. Next work may include detailed variant notes, metrics, or ACP bindings.
  - Execute a full cross-triad `korg-challenge` + `korg-reconcile` on all three mechanism notes together.
  - Begin the first `Human/Methodology/` narrative describing operator use of the complete system.
  - Prototype or design the first reference headless harness implementation against the published contracts.

---

## How to Work in This Vault

1. Read `[[_GROK.md]]` first on any new session.
2. Use `korg-challenge` (and future skills) for thinking.
3. Always propagate changes (project note + daily + log).
4. Prefer updating existing high-quality notes over creating new ones.

---

## Key Locations

- **Product Documentation:**
  - [[README.md]] — Project landing page & quick start
  - [[PRODUCT_OVERVIEW.md]] — Market positioning & value proposition
  - [[ARCHITECTURE.md]] — Deep-dive system design & swarming topologies
  - [[USER_GUIDE.md]] — 6-pane TUI cockpit manual & playhead controls
  - [[INSTALLATION_GUIDE.md]] — Homebrew, Docker, and source build instructions
  - [[RELEASE_NOTES.md]] — Version changelogs & Phase 2 roadmap
- **Operating Manual:** `[[_GROK.md]]`
- **Audit Log:** `[[log.md]]`
- **Daily Notes:** `wiki/daily/`
- **Projects:** `wiki/projects/`
- **Concepts & Patterns:** `wiki/concepts/` and `wiki/patterns/`
  - New: `Evaluation-Guardrail-Layer.md` — five binary grading dimensions for the Evaluator persona + semantic entropy doom-loop heuristics. Canonical reference for the Heavy-Adversarial guardrail layer.
  - `Anthropic-Long-Running-Agent-Harnesses.md` — Generator/Evaluator adversarial loops, persistent artifacts, Planner + Generator + Evaluator roles, and verification patterns from Anthropic (May 2026). Strong mappings to Korg Arena, `.ktrans`, and Leader-Broker-ACP.
- **Mechanisms (Harness-Agnostic Primitives):** `wiki/mechanisms/`
- **Reference Harness & ACP Designs:** `wiki/reference-harness/`
  - New canonical reference: `ACP-v1.17-Wire-Format.md` — exact JCS + Ed25519 wire format, message schemas, and error taxonomy from the v1.17 spec.
  - Flagship practical artifact: `Minimal-ACP-Client-Pseudocode.md` — complete Grok Build-style end-to-end with session recovery, human gates, Arena scoring, and semantic merge (the note people actually build harnesses from).
- **Reference Implementations:** `reference-implementations/rust/grok-acp-harness/` — first executable Rust skeleton directly derived from the pseudocode (ACP client + SingleWorkerHarness).
- **Human-Facing Narratives & Methodology:** `Human/`
  - New specialized guide: `How-to-Watch-a-Live-16-Agent-Campaign.md` — operator manual for real Grok 4.20 Heavy swarms using the 4-persona lens (Captain, Harper, Benjamin, Lucas), Arena signals, token-bucket, human gates, and recovery.
- **Decisions:** `wiki/decisions/`
- **Syntheses:** `wiki/synthesis/`
  - [[Synthesis — ACP Protocol]]
  - [[Synthesis — Adversarial Loop]]
  - [[Synthesis — Blackboard]]
  - [[Synthesis — Contract Negotiation]]
  - [[Synthesis — Evaluator Persona]]
  - [[Synthesis — Semantic Entropy]]
  - [[Synthesis — Transactional Memory]]
- **Raw Sources:** `raw/`

---

## Goals

- Build a second brain that actually compounds across long agent sessions
- Make high-quality thinking tools (challenge, reconcile, synthesize) native to Grok
- Keep the graph clean and agent-optimized while remaining usable by humans

---

## Recent Activity

(See `[[log.md]]` for the full trail)

---

**This vault is meant to get smarter, not just bigger.**

*Structure inspired by obsidian-second-brain patterns, adapted for Grok + direct filesystem + cli-anything-obsidian.*