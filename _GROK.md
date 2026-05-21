# _GROK.md — Korg Vault Operating Manual

**Vault:** `~/Documents/Korg`  
**Mandate:** Operational Intelligence Layer for Grok-native harness & agent development

**Primary Optimization Goals (in order):**
- Retrieval (fast, precise, low-token)
- Synthesis (cross-source pattern discovery)
- Pattern linking (especially cross-harness)
- Semantic reuse (stable concepts that survive experimentation)

This vault exists to compound **agent-operational knowledge** across long time horizons. It is **not** documentation for humans.

Every Grok session that touches this vault must read this file first.

---

## 0. Core Principle: Operational Intelligence Layer

This is an **agent-native experimentation and operational memory corpus**, not a human wiki.

We optimize for what future Grok instances (and subagents) will need when building, debugging, and evolving harnesses, orchestrators, ACP protocols, TUIs, and multi-agent systems.

### What This Means in Practice

- **Small, focused documents** — one clear semantic unit per note.
- **Explicit, stable typing and tagging** — retrieval and synthesis depend on consistent `type:` and tags.
- **Normalized naming** and stable terminology across harnesses.
- **Dense, intentional cross-linking** so patterns can be traversed.
- **First-class treatment** of high-value artifacts:
  - Failed experiments & dead ends
  - Harness edge cases & tool behavior anomalies
  - Semantic decisions & their rationale ("why this abstraction")
  - Session lifecycle patterns
  - Orchestration / ACP / protocol evolution
  - Emergent best practices and heuristics

Random brainstorming and polished design docs are **low priority**. They add noise.

### Artifact Typing (Current Taxonomy)

Preferred `type:` values:
- `semantic-decision`
- `failed-experiment`
- `edge-case`
- `pattern`
- `session-lifecycle`
- `tool-behavior`
- `acp-evolution`
- `orchestration-heuristic`
- `best-practice`
- `harness-idiom`

Use additional tags like:
- `harness: <name>` (cli-anything, api-anything, korg, redmicro, etc.)
- `domain: <area>` (orchestration, supervision, artifact-emission, recovery, acp, tui, session-model, etc.)

### Current Lighter Approach (May 2026)

We are currently running a **pragmatic lighter regime**:
- Strong wiki-style structure and `_GROK.md` as the contract
- Good self-contained notes + wikilinks
- High-value propagation (project + daily + log)
- **No** mandatory long preambles or exhaustive recency markers on every claim yet

We will tighten typing, tagging, and density as the corpus grows. The structure exists so we can evolve the strictness without chaos.

### Long-Term Vision: Cross-Harness Pattern Extraction

As the number of harnesses grows (Bun, Blender, FreeCAD, GIMP, Docker, Kubernetes, Playwright, etc.), recurring structures will appear in:
- Session semantics & lifecycle
- Watch-mode / supervision models
- Artifact emission patterns
- Recovery and fallback strategies
- Structured logging & observability
- ACP interaction models

The vault's highest leverage will come from making these patterns first-class and queryable.

---

## 1. Folder Structure (Wiki Style)

```
Korg/
├── _GROK.md                 # This file
├── Index.md                 # Human + agent dashboard
├── log.md                   # Append-only audit trail of all writes
│
├── raw/                     # IMMUTABLE originals (never modify)
│   ├── articles/
│   ├── transcripts/
│   ├── videos/
│   └── screenshots/
│
├── wiki/
│   ├── entities/            # People, companies, tools (one file each)
│   ├── concepts/            # Frameworks, patterns, ideas
│   ├── projects/            # Active work (status tracked)
│   ├── daily/               # YYYY-MM-DD.md
│   ├── sessions/            # Long agent runs, deep work logs
│   ├── decisions/           # ADRs and major choices
│   ├── synthesis/           # Auto-generated connection notes
│   └── mechanisms/          # Harness-agnostic Execution Primitive Modules (state machines, doom-loop detection, merge arbitration, etc.) — the core contracts for headless and multi-node operation
│
├── reference-harness/       # Reference implementations, ACP protocol designs, and concrete harness sketches that realize the triad contracts.
│
├── Human/                   # Human-facing narratives and methodology (the "living room" layer). Explains the why, the feel, and the operator experience built on top of the dense technical specs in wiki/.
│   └── Methodology/         # Core explanations of the system, operator playbooks, and "how to think with Korg"
│
├── research/                # External research pulled in
├── prompts/                 # Reusable system prompts & agent instructions
├── .obsidian/               # Obsidian config
└── (legacy folders being migrated)
```

**Flat is better for agents.** `wiki/entities/` and `wiki/concepts/` are deliberately flat.

---

## 2. Key Files

- **`Index.md`** — High-level map + current priorities. Read this early for navigation.
- **`log.md`** — Every significant write operation appends here with timestamp and summary.
- **`wiki/daily/YYYY-MM-DD.md`** — The daily note. Used for context and as a scratch space.

---

## 3. Note Type Schemas (Minimum)

All notes get the universal fields above.

### `type: project`
```yaml
status: active | planning | completed | on-hold | archived
priority: high | medium | low
```

### `type: entity`
```yaml
role: "..."
company: "..."
timeline: [...]   # use for role/company history
```

### `type: decision`
```yaml
status: open | resolved | superseded
```

### `type: session`
```yaml
participants: [Grok, ...]
duration: "..."
```

### `type: synthesis`
```yaml
sources: ["[[wiki/...]]", ...]
confidence: high | medium | speculation
```

---

## 4. Propagation Rules (Mandatory)

When you create or significantly update something, also touch:

- The main **project note** it belongs to
- **Today's daily note**
- **`log.md`**
- Any relevant **synthesis** or **concept** note

Use parallel subagents when multiple things need updating.

---

## 5. Current Skills & Commands (Korg Edition)

This vault is operated via Grok skills. Current high-value ones:

- `korg-challenge` — Red-team the current idea against this vault's history (highly recommended before big decisions)
- `korg-reconcile` — Find and resolve contradictions
- `korg-save` — Extract and propagate everything worth keeping from the current conversation
- `korg-synthesize` — Find unnamed patterns and write connection notes

More skills will be added over time.

---

## 6. Working Style with This Vault

- Always search before creating new notes (avoid duplicates).
- Prefer updating existing high-quality notes over creating new ones.
- When ingesting external material, save the raw version in `raw/` and derive wiki pages from it.
- Use subagents aggressively for parallel work (people extraction, decision extraction, contradiction scanning, etc.).
- After any non-trivial work, append to `log.md`.

---

## 7. Migration Notes (May 2026)

This vault was bootstrapped from a clean start and is being evolved toward the AI-first wiki-style system inspired by obsidian-second-brain.

Legacy folders (`Architecture/`, `Decisions/`, `Implementation/`, etc.) will be gradually migrated into `wiki/`.

---

**End of operating manual.**

When in doubt, re-read this file and `Index.md`.

The goal is a vault that gets *smarter* over time, not just bigger.