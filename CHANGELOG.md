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
- **Trusted *time* — `korg-seal anchor` + `korg-seal resolve` close the provenance triad (what + who + WHEN).** A Gold Seal proves *what* (re-derived summary) and *who* (Ed25519 seal) offline; it could not prove *when*. The new commands do: `korg-seal anchor <seal> --repo <url> --commit <sha>` binds a `git-tip` time anchor and re-signs (post-hoc flow: mint → publish/commit → anchor); `korg-seal resolve <seal>` performs the one network step — fetches each anchor's public commit (stdlib `urllib`, GitHub API) and confirms it *introduced* the anchored `entry_hash`, yielding a "the chain existed no later than `<commit date>`" bound. A public commit is immutable once mirrored, so an owner who rewrote the chain would have to force-push the witness (detectable). **Demoed live against the real repo:** the committed fixture's tip `bd8389e3…` is genuinely witnessed by public commit `0e566b0` (committed `2026-06-14T06:01:28Z`); `korg-seal resolve` confirms it over the live GitHub API, and the anchored seal still verifies offline. The resolver is injectable-fetcher-based so it's unit-tested hermetically (witnessed / not-witnessed / commit-404 / repo-URL parsing) + the anchor re-bind roundtrip. GOLDSEAL.md §6/§8 + positioning updated (time is now an explicit opt-in network step, not a gap). +5 korg-seal tests (14 total).
- **Verify a Gold Seal anywhere — hosted, in CI, and from the terminal.** The trust layer is now ubiquitous: (1) **GitHub Pages** publishes the zero-install browser verifiers + specs at <https://new1direction.github.io/korg/> (landing page, the Gold Seal verifier, and the raw-ledger verifier; `.github/workflows/pages.yml` deploys `spec/korg-ledger-v1/`). (2) A **`verify-goldseal` GitHub Action** (`.github/actions/verify-goldseal`) verifies seals/ledgers in CI — `uses: New1Direction/korg/.github/actions/verify-goldseal@…` with an optional `pin-pubkey`, failing the job on any tampered/unpinned artifact. (3) A **dogfooding demo workflow** (`.github/workflows/goldseal-demo.yml`) mints a Gold Seal from a session ledger with `korg-seal`, verifies it (pinned to the issuer) via the Action, and proves a tampered copy is rejected — "verified agent work in CI". (4) The npm verifier (`@korgg/ledger-verify`) bumped to 0.2.0 — its `npx` CLI now verifies `goldseal@v1` too. The Gold Seal verifier's badge now links to the hosted page, and `korg-seal mint` prints the live verify URL.
- **`goldseal@v1` — the public, independently-verifiable certificate layer (the "Gold Seal").** A portable, signed certificate of an AI-agent session that anyone re-verifies offline with zero trust in the issuer. A strict superset of `korgex-receipt@v1`: it embeds the full event chain + an Ed25519 issuer seal + a human-legible summary that is **re-derived from the events at verify time**, so the "files touched / tools used / steps" a person actually reads *cannot lie* (the legacy tip-signature left the summary unprotected; the seal signs claim + summary + tip together). Three conformant implementations: Python (`korg_ledger.goldseal` stdlib derivation + `korg_ledger.signing.mint_seal`/`verify_seal`), Rust (`korg_verify::verify_goldseal` + the `korg-verify` binary, which renders the attestation), and JS (`verify.mjs` `verifyGoldSeal`/`deriveSummary`). Cross-impl proof: a frozen fixture (`crates/korg-verify/tests/fixtures/goldseal-v1.json`) **minted by Python, verified byte-identically by Rust and JS** (seed `[42;32]`, deterministic re-mint). Adversarially tested in all three: a lying summary, a moved claim, a tampered event, a stripped seal (a downgrade — fails), and a wrong pinned issuer key are all rejected. Adds read-time `verify_dag` to the Python `korg_ledger` package (it previously enforced causality only at write time). Spec: [`GOLDSEAL.md`](spec/korg-ledger-v1/GOLDSEAL.md). Headline artifact: a zero-install in-browser Gold Seal verifier ([`spec/korg-ledger-v1/web/seal.html`](spec/korg-ledger-v1/web/seal.html)) — drop a seal and watch the summary re-derive (and the gold seal crack red when tampered), entirely client-side via Web Crypto. Positioning: [`docs/goldseal/POSITIONING.md`](docs/goldseal/POSITIONING.md). **Anchors are bound into the seal** (the signed header includes `anchors`, so an anchor cannot be stripped, added, or forged — while staying structurally chain-bound), removing the one documented limit; the only remaining external step is the *network* resolution of a git-tip anchor for trusted time. 191 Python tests + 7 Rust goldseal tests + JS conformance (incl. bound-anchor) check.
- **`adapters/korg-seal/` v0.1.0 — the producer-side Gold Seal minter (closes the capture→mint→verify loop).** `korg-seal mint <session.jsonl> --claim "..."` turns a captured korg-ledger@v1 session into a signed `goldseal@v1` certificate; `korg-seal verify` / `korg-seal key` round it out. Manages a local Ed25519 issuer key at `~/.korg/issuer.ed25519` (`0600`, generated on first use) whose public half is the issuer identity a relying party pins. **Refuses to seal a chain that does not verify** (no Gold Seal on a tampered history). Reuses the conformant `korg_ledger.goldseal`/`signing` cores; verification stays the dependency-light Rust/JS/browser path. Proven end-to-end: a freshly-keyed CLI-minted seal verifies VALID under the independent Rust `korg-verify` binary AND `verify.mjs`. 9 tests (key lifecycle + 0600 perms, mint, broken-chain refusal, determinism, full CLI mint→verify→pin roundtrip). `cryptography`-gated (CI installs only pytest, so they skip there; cross-impl agreement is gated by the verifiers).
- **`adapters/introspect-mcp/` v0.1.0 — generic `--introspect` → MCP bridge.** The leverage move that closes the loop: one MCP server that takes any `--introspect`-aware binary and exposes every `Callable` in its document as a typed MCP tool. After installing once, `korg-introspect-mcp thump`, `korg-introspect-mcp korg`, `korg-introspect-mcp korgex` unlock **30+ tools** across the ecosystem in Claude Code with zero per-binary MCP wiring. Honors `capabilities.side_effects` — default policy refuses `fs_write` / `network` / `ledger_write` unless `KORG_INTROSPECT_MCP_ALLOW` opts in. Argv mapping is convention-based (kebab-case long flags, bool flag-on-true, arrays repeat the flag, command_id `.` segments become subcommand path) — works for every binary in the ecosystem with no config. Tool names use the introspect `command_id` directly, so the recall→re-execute loop is deterministic (recall returns events with tool_name=`thump.generate`; the bridge serves an MCP tool by the exact same name). 67 tests covering discovery, args mapping, safety gating, invoker output-mode handling, and the full MCP protocol roundtrip with mocked + real binary fixtures. End-to-end smoke-tested against real `thump` and `korg` Rust binaries.
- **`adapters/recall-mcp/` — Foundry-style `--introspect` + Capabilities, sharing one source of truth with MCP `tools/list`.** New `korg_recall_mcp.introspect` module defines `Callable` and `Capabilities` dataclasses; the same `Callable` instance projects to both an MCP tool descriptor AND an `--introspect` document entry. End-to-end smoke-tested: the input schema served via MCP `tools/list` is byte-identical to the one in `--introspect`. Document includes stable `command_id`, declared `side_effects`/`output_mode`/`long_running`/`stateful`/`reads_stdin` capabilities, and a canonical 8-code `exit_codes` table. Tagged `korg:introspect@v1`. The pattern is now ready to apply to thumper, korg, korgex — anywhere we want CLI introspection AND MCP descriptors to stay in sync without two parallel sources. 17 new tests including a contract test that fails CI if the two surfaces drift.
- **`adapters/korg-setup/` v0.1.0 — one-command install for the Claude Code loop.** Collapses the five-step manual setup (verify binaries → create ledger dir → edit `~/.claude.json` → install launchd plist → load it) into `korg-setup`. Atomic writes with a `.korg-backup` copy of the prior Claude config; idempotent re-runs; `--dry-run` preview; `status` subcommand shows what's installed and running; `uninstall` reverses everything except the ledger itself. macOS launchd is native; Linux gets a manual one-liner (systemd-user native install is a follow-up). 45 tests covering claude-config edits, launchd integration (mocked subprocess), the orchestrator, and the status reporter.
- **`adapters/recall-mcp/` v0.1.0 — cross-session semantic memory MCP server.** The first output-side adapter, complementing the three input adapters. Wraps the korg ledger with semantic recall and exposes it via the Model Context Protocol so Claude Code (or any MCP client) can call `recall(query)` and get the top-N relevant events from every prior session. The wedge: ChatGPT Memory is OpenAI-only, Anthropic ships nothing equivalent, Cursor's memory is tool-locked — korg-recall-mcp is the only memory layer that spans *across* vendor boundaries because the ledger format is vendor-agnostic. 49 tests covering text flattening, incremental index, substring + semantic search, and the full MCP JSON-RPC surface.
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

## [bridge-v0.3.3] — 2026-06-04

### Added
- `record_llm_call` accepts the optional prompt-cache breakdown (`cache_read_tokens` / `cache_creation_tokens` / `uncached_input_tokens`) and folds it onto the event `args` — but only when caching is active. A prompt-cache hit is now provable from a bridge-written journal too, at parity with korgex's local-journal and HTTP transports. A cold turn keeps the legacy two-field `args`, so older readers and the hash-chain over historical events are undisturbed (field order is irrelevant — `args` canonicalize with sorted keys at hash time).

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
[bridge-v0.3.3]: https://github.com/New1Direction/korg/releases/tag/bridge-v0.3.3
