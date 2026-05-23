# korg

**The first deterministic cognitive runtime.**
*Every decision your AI agent makes is logged, causally ordered, and reversible — like Git, but for cognition.*

[![Crates.io](https://img.shields.io/crates/v/korg.svg?style=flat-square&color=fc8d62)](https://crates.io/crates/korg)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg?style=flat-square)](https://opensource.org/licenses/MIT)
[![Build](https://img.shields.io/badge/build-passing-brightgreen.svg?style=flat-square)](https://github.com/New1Direction/korg)
[![Rust 2021](https://img.shields.io/badge/rust-2021-93450a.svg?style=flat-square)](https://www.rust-lang.org)
[![130 tests](https://img.shields.io/badge/tests-130%20passing-brightgreen.svg?style=flat-square)](https://github.com/New1Direction/korg)

---

![korg demo — rewind, fork, and replay AI agent decisions in real time](demo.gif)

---

> AI agents are black boxes. When they fail, you can't debug. When they succeed, you can't reproduce it.
> When they do something wrong, you can't undo it.
>
> **Korg fixes this.**

---

## What Korg Does

Korg is a **cognitive hypervisor** — a runtime layer that sits beneath your AI agents and governs every decision they make.

It doesn't replace your LLM. It governs what the LLM does.

```
Foundation Model          →  predicts, suggests, generates
────────────────────────────────────────────────────────────
Korg Cognitive Runtime    →  schedules, validates, isolates,
                             reconciles, replays, heals, governs
```

Every agent action is:
- **Appended** to an immutable, cryptographically-signed ledger
- **Ordered** with Hybrid Logical Clocks (causal, deterministic, globally consistent)
- **Replayable** — rebuild exact state at any point in history
- **Reversible** — rewind, fork, or branch any decision

---

## The Demo

```bash
# Run an autonomous agent campaign
korg run "Fix the authentication bug in src/auth.rs"

# Every decision is logged with a causal timestamp
# Watch the ktrans ledger stream in real time...

# Agent goes down a wrong path? Rewind it.
korg rewind --seq 4
# Workspace snaps back instantly (Git Merkle O(1) restore)

# Fork from that point with a different strategy
korg fork --from 4 --goal "Try a stateless JWT approach"

# Both branches run. Best result wins. Full audit trail.
korg merge --winner branch-2
```

> *No other AI agent runtime lets you do this.*

---

## Core Architecture

Korg is built on the same theoretical foundations that make databases and operating systems reliable — applied to AI cognition for the first time.

| Invariant | What it means |
|:---|:---|
| **Append-only WAL** | Every cognitive event is a ledger entry. Nothing is mutated, only appended. Like a database WAL, but for AI thought. |
| **HLC Causal Ordering** | Hybrid Logical Clocks guarantee globally consistent, causally ordered event streams — even across distributed swarm workers. |
| **Deterministic Replay** | Any campaign can be replayed byte-for-byte from the ledger. Same inputs, same outputs, every time. |
| **Speculative Branches** | Fork execution into parallel hypothetical paths. Preview before committing. Discard freely. |
| **Execution Checkpoints** | Snapshot the entire runtime state: ledger offset, projection views, lease maps, workspace tree. Restore in O(1). |
| **Micro-Healing** | Transient failures (lock conflicts, stale state) are automatically healed at the effect level, with full retry audit trails. |
| **Semantic Governance** | Swarm actions are validated against BERT embedding cosine similarity — semantic alignment, not keyword matching. |

```
┌────────────────────────────────────────────────────────────────┐
│  korg v0.1.0  │  session: 019e5333-efc9-7c70  │  ● ACTIVE      │
├───────────────────────────────┬────────────────────────────────┤
│  SWARM PLAN                   │  LIVE MERKLE LEDGER            │
│  ├─ [●] Captain  [PLANNING]   │  (tx_00)→(tx_01)→[tx_02]→...  │
│  ├─ [●] Harper   [RESEARCH]   │                                │
│  ├─ [●] Benjamin [SYNTHESIS]  │  TELEMETRY                     │
│  └─ [○] Lucas    [IDLE]       │  ├─ Velocity  85.2 t/s  ▇▆▄▂█  │
│                               │  ├─ Entropy    0.451     ▄▃▂▃▄  │
│  GOVERNANCE GATES             │  └─ Progress  68.7 %    ▂▃▄▅▆▇  │
│  ├─ 🟡 Amber Security [IDLE]  │                                │
│  ├─ 🟢 Consensus     [ACTIVE] │  LEDGER STREAM                 │
│  └─ 🔵 Steering Fork [IDLE]   │  [tx_03] Benjamin: patch auth  │
└───────────────────────────────┴────────────────────────────────┘
```

---

## Quick Start

### Install

```bash
cargo install korg
```

### Or build from source

```bash
git clone https://github.com/New1Direction/korg
cd korg
cargo build --release
./target/release/korg --help
```

### Run your first campaign

```bash
# Interactive TUI dashboard
korg campaign --tui --prompt "Refactor the auth layer to use JWTs"

# Web cockpit at localhost:8080
korg campaign --web --prompt "Optimize the database connection pool"

# Pure autonomous goal mode
korg goal "Write and validate a full test suite for src/parser.rs"

# Preview without committing (speculative sandbox)
korg run --preview "Refactor the main event loop"
```

### Rewind & Fork

```bash
# Rewind to a specific ledger sequence point
korg rewind --seq 4

# List all checkpoints in the current session
korg checkpoints list

# Restore from a specific checkpoint
korg checkpoints restore --id <checkpoint-uuid>
```

---

## Cognition Modes

Korg adapts its intelligence tier based on task complexity. Modes are governed exclusively through the capability resolver — every switch is ledger-logged.

| Mode | Best for |
|:---|:---|
| `instant` | Ultra-low latency. Bypasses negotiation. Optimistic execution. |
| `balanced` | Default. Structured multi-round contract negotiation. |
| `heavy` | Deep multi-agent deliberation. Multiple evaluation rounds. |
| `research` | Wide divergent exploration. Semantic index scanning across all crates. |
| `recovery` | Safe rollback mode. Creates checkpoints before every mutation. |
| `autonomous` | Full goal-mode. Self-steering with automatic re-planning. |
| `heavy-consciousness` | Maximum depth. Full HeavyConsciousness context injection. |

```bash
korg run --mode research "Explore alternative approaches to the rate limiter"
korg run --mode recovery "Carefully migrate the database schema"
```

---

## Why Korg Exists

Current AI coding agents are probabilistic black boxes. They:
- **Can't be replayed** — same prompt, different output, every time
- **Can't be rewound** — one wrong action and you're manually diffing git history
- **Can't be audited** — no record of what the agent decided and why
- **Can't be governed** — no way to set policy boundaries at runtime

Korg treats AI cognition the same way a hypervisor treats compute and Git treats code:

> **If it's not in the ledger, it didn't happen.**

---

## Comparison

| Capability | Korg | LangChain / LangGraph | CrewAI | Standard CLI Agents |
|:---|:---:|:---:|:---:|:---:|
| Deterministic replay | ✅ | ❌ | ❌ | ❌ |
| Causal HLC ordering | ✅ | ❌ | ❌ | ❌ |
| Rewind execution | ✅ | ❌ | ❌ | ❌ |
| Speculative branches | ✅ | ❌ | ❌ | ❌ |
| Execution checkpoints | ✅ | ❌ | ❌ | ❌ |
| Cryptographic audit trail | ✅ | ❌ | ❌ | ❌ |
| Micro-healing | ✅ | ❌ | ❌ | ❌ |
| Model-agnostic | ✅ | ✅ | ✅ | ✅ |

> **Korg is not an agent framework. It's the governance kernel that runs beneath all of them.**

---

## Technical Stack

| Component | Technology |
|:---|:---|
| Core runtime | Rust 2021, Tokio async |
| Ledger ordering | Hybrid Logical Clocks (HLC) |
| Workspace snapshots | Git Merkle tree (O(1) restore via `write-tree` / `read-tree`) |
| Cryptographic attestation | Ed25519 (ed25519-dalek) |
| Semantic governance | BERT cosine similarity (Candle / Hugging Face) |
| TUI dashboard | Ratatui + Crossterm |
| Web cockpit | Axum + SSE |
| Syntax highlighting | Syntect + tree-sitter |

---

## Architecture Deep Dive

→ **[Read the full technical write-up](https://github.com/New1Direction/korg/blob/main/ARCHITECTURE.md)** *(coming soon)*

The short version:

1. **CapabilityResolver** — the single authority for all runtime state. All reads and writes flow through it. No secondary state stores.
2. **CapabilityJournal** — the append-only WAL. Every cognitive event is sealed here with an HLC timestamp, causation chain, and cryptographic signature.
3. **ProjectionEngine** — pure state folds over the journal. Any read model can be rebuilt deterministically from the raw event stream.
4. **ExecutionCheckpoint** — snapshot of `{ledger_offset, projection_state, lease_map, workspace_tree_hash}`. Restores full runtime state in O(1) without replaying the entire event stream.
5. **CapabilityExecutor** — executes the physical effect DAG. Failures trigger automatic micro-healing before escalating.

---

## Status

Korg is in active development. Current test coverage: **130 tests, 0 failures**.

- [x] Append-only cognitive ledger with HLC ordering
- [x] Deterministic replay and projection rebuilds
- [x] Speculative execution + preview mode
- [x] Execution checkpoints (O(1) restore)
- [x] Micro-healing effect layer
- [x] Multi-agent swarm orchestration (Captain, Harper, Benjamin, Lucas)
- [x] TUI dashboard + Web cockpit
- [x] Cryptographic provenance attestation
- [x] Single-authority CognitionMode governance
- [ ] `cargo install korg` on crates.io
- [ ] Remote swarm workers
- [ ] WASM backends
- [ ] IDE language server integration
- [ ] Distributed checkpoint synchronization

---

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.

---

<p align="center">
  <sub>Built with Rust. Governed by invariants. No black boxes.</sub>
</p>