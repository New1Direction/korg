# Changelog

All notable changes to `korg` are documented here.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) —
versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

## [Unreleased]

See [ROADMAP.md](ROADMAP.md) for planned features.

[0.1.0]: https://github.com/New1Direction/korg/releases/tag/v0.1.0
