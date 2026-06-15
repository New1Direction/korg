# Roadmap

> **Current release:** [v0.1.0](https://github.com/New1Direction/korg/releases/tag/v0.1.0) — a tamper-evident, verifiable ledger for AI agent cognition.

---

## v0.2 — Autonomous Recovery (`korg autoheal`)

**Target:** Q3 2026

The checkpoint layer is established. v0.2 closes the loop by making the system
self-correcting — agents that encounter transient failures recover automatically
without human intervention.

| Feature | Description |
|---|---|
| `korg autoheal` | Autonomous recovery loop: detect failure → identify root cause → rewind to last clean checkpoint → retry with corrected strategy |
| Incremental checkpoints | Background checkpoint compression — only delta since last checkpoint is stored |
| Recovery confidence scoring | Each autoheal attempt is scored; low-confidence recoveries escalate to human approval |
| Doom-loop circuit breaker | Hard limit on recursive recovery attempts with structured escalation |

---

## v0.3 — SDK + Language Bindings

**Target:** Q4 2026

Korg's governance kernel should be usable from any language, not just Rust CLIs.

| Feature | Description |
|---|---|
| Python SDK (`pip install korg`) | `korg.Session`, `korg.rewind()`, `korg.fork()` — full runtime access from Python |
| JavaScript/TypeScript SDK | Node.js + Deno compatible; works with LangChain, Vercel AI SDK |
| gRPC API | Language-agnostic wire protocol for embedding Korg in existing agent frameworks |
| REST API | `POST /api/campaign`, `GET /api/ledger/:seq`, `POST /api/rewind` |

---

## v0.4 — Plugin Capabilities

**Target:** Q1 2027

| Feature | Description |
|---|---|
| Custom capability registration | Agents can register domain-specific capabilities (e.g. `database_query`, `image_generate`) with full ledger governance |
| Capability marketplace | Community-contributed capability plugins, version-locked and hash-verified |
| Sandboxed capability execution | Each capability runs in an isolated workspace with blast-radius bounds |

---

## Beyond

- **Cloud sync** — share sessions and ledger state across team members
- **Distributed swarm** — multi-machine campaign execution with network-aware HLC
- **Formal verification** — TLA+ spec for the core ledger invariants

---

## How to Contribute

See [CONTRIBUTING.md](CONTRIBUTING.md) for architecture invariants and how to propose new capabilities.

Open a [discussion](https://github.com/New1Direction/korg/discussions) if you want to
influence the roadmap — especially for the SDK design in v0.3.
