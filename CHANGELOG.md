# Changelog

All notable changes to `korg` are documented here.

Korg uses **two parallel version namespaces** because the PyO3 extension is
versioned in lockstep with the Python clients (korgex, korgchat), not with
the underlying Rust runtime:

- `vX.Y.Z` — the korg runtime (overall release).
- `bridge-vX.Y.Z` — the `korg-bridge` PyO3 crate published as a Python wheel.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) —
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Changed
- README cleaned up: placeholder crates.io / docs.rs badges dropped (not yet published), install path corrected, test count updated to 175 (162 cargo + 13 pytest).
- `cargo fmt` cleanup across korg-bridge.
- `.gitignore` extended for transient `*_report.html` artifacts.

---

## [bridge-v0.3.2] — 2026-05-27

### Added
- `record_llm_call` accepts an optional `assistant_text` kwarg that gets persisted into the journal entry's `result.text` field. Agent transcripts now replay with the model's actual reply.

---

## [bridge-v0.3.1] — 2026-05-26

### Added
- `payload_refs` plumbed through `Bridge.record_*` calls as `{sha256, size_bytes, label}` triples for content-addressed large blobs.

---

## [bridge-v0.3.0] — 2026-05-26

### Added
- **`korg-bridge` PyO3 extension.** In-process WAL adapter for Python clients (korgex, korgchat). Builds as a Python wheel via `maturin develop`. Removes the HTTP roundtrip for journal writes.
- Phase D subscription-registry invariant property tests.
- Stream-JSON v1.2 sub-agent spine (§2b VALIDATED).
- On-demand rewind (`Ctrl-R`) in `korg-tui`.

### Fixed
- 3 Critical + 3 High findings from the 2026-05-25 ecosystem audit closed.
- 7 Medium + 5 Python-Medium findings closed.
- 11 Low findings closed.
- Adapter cross-spine link bug closed.

### Changed
- Thumper execution substrate folded into `korg-runtime` as the `execution/` submodule.
- §2a causal chain rule clarified — `llm_inference` always points at the prior `llm_inference`, never at intervening tool calls.

---

## [0.1.0] — 2026-05-23

### Added

**Core Runtime**
- Append-only capability event ledger (`src/registry/log.rs`) with cryptographic integrity sealing
- Hybrid Logical Clock (HLC) causal ordering — deterministic, globally consistent, drift-resistant
- `korg rewind --seq N` — deterministic workspace restore to any ledger sequence in O(1) via `git read-tree`
- Execution checkpoints: full runtime snapshot (ledger offset, workspace, projections, lease map, branch lineage)
- Speculative execution branches — fork from any checkpoint, run in parallel, discard or merge

**Orchestration**
- Multi-persona adversarial swarm: `captain` (planning), `harper` (critique), `benjamin` (coding), `lucas` (synthesis), `evaluator` (judge)
- `LeaderOrchestrator` — async campaign execution with HLC-ordered event log
- `CapabilityResolver` — single authority for cognition mode transitions, fully ledger-logged
- `ExecutionDag` — topological level-order campaign scheduling with cycle detection
- Human approval gates with timeout and auto-approve (`--goal` mode)

**Evaluation**
- 5-rubric semantic evaluator: Trajectory, Epistemic, Coherence, Efficiency, Safety
- Semantic entropy scoring (Candle optional feature)
- Multi-round Arena with ELO-style verdict accumulation
- Doom-loop detection with automatic revision gating

**Infrastructure**
- Model-agnostic LLM provider layer (OpenAI, Anthropic, Google, Ollama, rotator)
- Free-tier rotator with 60-second cooldown and round-robin failover
- Cryptographic provenance attestation (`korg verify-provenance`)
- Real-time TUI dashboard (Ratatui) with live ledger ticker
- SSE web cockpit (`korg --web`) with glassmorphism UI
- Interactive developer shell (`korg shell`) with `/read`, `/edit`, `/goal`, `/reconcile`
- LSP server (`korg lsp`) — read-only semantic navigation over stdio
- Semantic vector index (`korg index`) via Tree-sitter + Candle embeddings
- Signed `.ktrans` transaction persistence with compaction and fast recovery

**Quality**
- 130 tests, 0 failures
- HLC monotonicity proof under backward clock drift (`test_hlc_monotonicity_with_backward_time_drift`)
- Git worktree isolation test suite
- CI: build, test, clippy, rustfmt on every push

---

See [ROADMAP.md](ROADMAP.md) for planned features.

[0.1.0]: https://github.com/New1Direction/korg/releases/tag/v0.1.0
[bridge-v0.3.0]: https://github.com/New1Direction/korg/releases/tag/bridge-v0.3.0
[bridge-v0.3.1]: https://github.com/New1Direction/korg/releases/tag/bridge-v0.3.1
[bridge-v0.3.2]: https://github.com/New1Direction/korg/releases/tag/bridge-v0.3.2
