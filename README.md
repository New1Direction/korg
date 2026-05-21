# korg

**A minimal, elegant reference implementation of the Korg Heavy-Tier agent orchestration system.**

`korg` is a clean, production-oriented reference implementation of the full Korg architecture — including the Agent Control Protocol (ACP), adversarial evaluation, multi-round Arena, signed transactional memory, and real-time observability.

It is designed to be studied, extended, and used as a foundation for serious long-running agent systems.

---

## Features

- **Live Ratatui TUI** — a focused, real-time operator dashboard
- **ACP v1.17** — signed messages, JCS canonicalization, Ed25519 verification
- **5 Adversarial Rubrics** — rigorous, combinatorial evaluation with semantic entropy
- **Multi-Round Arena** — aggregation and selection across agent outputs
- **Signed `.ktrans`** — tamper-evident transactional memory with compaction and base-snapshot recovery
- **Human-in-the-Loop** — approval gates for high-stakes decisions
- **Live Streaming & Replay** — every event is observable and fully reproducible
- **Global CLI** — single binary, works from any directory

---

## Installation

```bash
cargo install --path .
```

After installation, the `korg` command is available globally.

---

## Usage

```bash
# Launch the interactive operator dashboard
korg tui

# Run a complete observable campaign
korg campaign

# Run the leader in demo mode
korg leader --demo

# Replay a previous campaign with full verification
korg leader --replay latest
```

### Flags

| Flag                | Description                                      |
|---------------------|--------------------------------------------------|
| `--tui`             | Launch inside the Ratatui dashboard              |
| `--non-interactive` | Run without human approval prompts               |
| `--live-stream`     | Output live `.ktrans` events to stdout           |

---

## The Dashboard

Running `korg tui` opens a clean, technical terminal interface that displays:

- Real-time verdict evaluations and rubric status
- Arena round history and outcomes
- Live `.ktrans` and TraceEvent streams
- Human approval requests as modal overlays
- Compaction and recovery status

The interface prioritizes clarity and operational awareness over visual flair.

---

## Architecture

Korg is built around three core ideas:

- **Epistemic State Machine** — explicit tracking of knowledge confidence
- **Transactional Memory** — every change is recorded in signed, replayable `.ktrans` artifacts
- **Continuous Guardrails** — an independent Evaluator that can pause or redirect work

The reference implementation demonstrates how these components work together in a real system, using the Agent Control Protocol (ACP) as the communication layer.

---

## Development

```bash
# Run from source
cargo run -- tui

# Format and lint
cargo fmt
cargo clippy
```

The project is intentionally kept as a reference implementation. It is not a turnkey daemon, but a clear and auditable foundation.

---

## Status

This repository contains a complete and working reference of the Korg Heavy-Tier system, including:

- Full ACP messaging and signing
- Adversarial multi-agent evaluation
- Arena-based output aggregation
- Signed, compactable transactional memory
- Real-time TUI with human oversight

It is intended for teams and researchers building long-running, high-stakes agent systems.

---

## License

MIT License

---

*Minimal. Technical. Serious.*