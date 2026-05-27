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

### Added
- **`adapters/claude-code/` v0.2.0 — live tail mode.** The adapter now ships a `korg-ingest-claude --tail` CLI that watches `~/.claude/projects/**/*.jsonl` and streams new events into a korg ledger as Claude Code writes them. Byte-offset state persists across restarts; per-file adapter instances retain chain state across polls so a `tool_use` in one poll and its `tool_result` in the next still attach correctly. **"Install once, ledger forever."** The positioning flips from "replay your history" to "korg follows you in real time" with the same parser. 16 new tests covering: no-duplicate emission, cross-poll causal coherence, mid-write tolerance, restart safety, multi-session isolation, new-file pickup.
- **`adapters/claude-code/` v0.1.0** — translates Claude Code session JSONL files (`~/.claude/projects/<dir>/<uuid>.jsonl`) into korg `AgentToolCall` events. Any existing Claude Code user can now have a retroactive korg ledger of every session they've ever run, with zero behavior change. Smoke-tested against a real 3,141-event session (103 user prompts + 1893 LLM rounds + 1145 tool calls, zero dropped). 23 tests.
- **`adapters/codex-ws/`** — translates OpenAI Codex CLI WebSocket frames into korg `AgentToolCall` events. Validates the transport-agnostic claim against an architecturally-different stack (WebSocket, OpenAI tool-shape, custom_tool_call freeform-text). 11 tests.
- **`adapters/grok-heavy/`** — translates Grok Heavy 16-agent NDJSON into korg events. Stress-tests korg's single-parent `triggered_by` causal model under a 16-way fan-out. Documents two known limitations (ingest order vs wall-clock concurrency, and cross-agent chatroom edges). 14 tests.
- **Transport-agnostic proof trio complete.** Three adapter PoCs across three architectural extremes (stdio JSONL, WebSocket, NDJSON streaming) all round-trip cleanly through the ledger with spec §2a causal coherence — moving the "universal capability ledger" claim from aspirational to demonstrated.

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
