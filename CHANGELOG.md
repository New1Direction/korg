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

### Security & correctness (multi-agent adversarial bug hunt — 15 real bugs fixed)

A fan-out review across every component, with each finding refuted-by-default by an independent verifier, surfaced **15 confirmed real bugs** (9 false positives filtered out). All fixed, with regression tests + the differential fuzzer extended to lock them:

- **CRITICAL — receipt signature forge (empty message).** `verify_tip_sig`/`verifyTipSig` verified an Ed25519 signature over a **0-byte message** when a receipt omitted its `tip`, letting an attacker mint a "validly signed" receipt over arbitrary events with their own key (reproduced empirically). Fixed in Rust + JS: the tip message must be exactly 32 bytes, and `verify_receipt`/`verifyReceipt` now **fail closed** when a signature is present but there's no tip, and reject zero-event receipts. (goldseal@v1 was not affected — its seal signs the canonical header, not a bare tip.)
- **CRITICAL — JS canonicalization key sort.** `verify.mjs` sorted object keys by UTF-16 code **unit**, diverging from Python/Rust (code **point**) on astral-plane keys → opposite VALID/INVALID verdicts on the same chain. Fixed with a code-point comparator (also applied to `deriveSummary`'s agent/file sorts).
- **Cross-impl number domain.** Unified across Rust/Python/JS: integers must be within ±(2⁵³−1) (JS `Number` loses precision beyond that) — now **rejected by all three**; Python `canonicalize` uses `allow_nan=False` (NaN/Infinity emitted non-standard JSON tokens Rust/JS couldn't parse). Finite floats are documented out of scope (SPEC §2) and the JS verifier now degrades gracefully (reports the event unverifiable instead of crashing) to keep capture of float-bearing tool args working. New `korg_ledger::canon_domain_error` + Python `_reject_out_of_domain`.
- **Producer/tooling fixes:** the ledger writer now **fails loud on mid-file corruption** (was silently returning a stale tip → forked chain; torn final line still tolerated); `korg-seal anchor` **preserves existing anchors** when re-anchoring (was dropping prior git-tip witnesses); `parse_github_repo` uses an **exact host match** (a suffix test accepted `notgithub.com/…` as the witness repo); `korg-seal mint --allow-unverified` now **surfaces the discarded chain errors** instead of sealing silently.
- **CI hardening:** the `verify-goldseal` PR-comment `report.mjs` now neutralizes Markdown/HTML in every seal-derived field (a crafted `claim`/path could inject into the privileged comment), and only updates a **Bot-authored** sticky comment whose trailing line is the marker (was a bare substring match).
- **Robustness:** the verifiers no longer crash on a `null`/non-object event (Python + JS guards across `verify_chain`/`verify_dag`/`verify_anchors`/`derive_summary`/`verify_structure`/`verify_seal`/`verifyGoldSeal`).

Gate: 41 Rust tests (incl. forge + proptest), 215 Python tests, JS conformance (incl. fuzz + receipt-forge regression), differential fuzz across 27 adversarial + edge-valid cases — **0 divergences**.

### Added
- **Adversarial hardening of the verifiers — fuzzed + differentially tested across all three impls.** Property-based fuzzing of the goldseal verifiers proves two security invariants: they never crash on hostile input, and never accept junk or any single-character hash/seal-signature flip. The fuzzers **found and fixed two real robustness bugs** — the Python and JS verifiers crashed (`AttributeError` / `TypeError`) on a `null`/non-object event; both now degrade gracefully to "invalid" like Rust always did (guards added across `verify_chain`/`verify_dag`/`verify_anchors`/`derive_summary`/`verify_structure`/`verify_seal` and the JS equivalents). Coverage: Python `test_goldseal_properties.py` (Hypothesis, ~1700 examples), Rust `crates/korg-verify/tests/fuzz.rs` (proptest, 5 properties), JS fuzz block in `conformance.mjs`. **Differential fuzzer** `spec/korg-ledger-v1/tools/diff_fuzz.py` runs 24 adversarial mutations of a Gold Seal through the Rust, Python, and JS verifiers and asserts all three return the **same** verdict (a divergence = a conformance bug) — **0 divergences**; gated in CI (Build & Test job). 211 Python tests.
- **Time-travel session explorer** ([`spec/korg-ledger-v1/web/explore.html`](spec/korg-ledger-v1/web/explore.html)) — a zero-install, client-side replay of any korg-ledger@v1 session or Gold Seal. Drop a ledger/seal and scrub the timeline: the **cumulative state** (agents, tools, files touched) is re-derived live over the slice up to the playhead, so you watch what the agent did evolve step by step, while the hash-chain verifies under every frame. A tampered chain lights up red and auto-jumps the playhead to the exact break; a Gold Seal additionally shows its claim + signer. Play/pause, scrub, click an event for full args/result. Same Web Crypto engine as the verifiers; linked from the landing page (a third card). Verified in a real browser across session / tampered / Gold Seal inputs.
- **Trusted *time* — `korg-seal anchor` + `korg-seal resolve` close the provenance triad (what + who + WHEN).** A Gold Seal proves *what* (re-derived summary) and *who* (Ed25519 seal) offline; it could not prove *when*. The new commands do: `korg-seal anchor <seal> --repo <url> --commit <sha>` binds a `git-tip` time anchor and re-signs (post-hoc flow: mint → publish/commit → anchor); `korg-seal resolve <seal>` performs the one network step — fetches each anchor's public commit (stdlib `urllib`, GitHub API) and confirms it *introduced* the anchored `entry_hash`, yielding a "the chain existed no later than `<commit date>`" bound. A public commit is immutable once mirrored, so an owner who rewrote the chain would have to force-push the witness (detectable). **Demoed live against the real repo:** the committed fixture's tip `bd8389e3…` is genuinely witnessed by public commit `0e566b0` (committed `2026-06-14T06:01:28Z`); `korg-seal resolve` confirms it over the live GitHub API, and the anchored seal still verifies offline. The resolver is injectable-fetcher-based so it's unit-tested hermetically (witnessed / not-witnessed / commit-404 / repo-URL parsing) + the anchor re-bind roundtrip. GOLDSEAL.md §6/§8 + positioning updated (time is now an explicit opt-in network step, not a gap). +5 korg-seal tests (14 total).
- **Verify a Gold Seal anywhere — hosted, in CI, and from the terminal.** The trust layer is now ubiquitous: (1) **GitHub Pages** publishes the zero-install browser verifiers + specs at <https://new1direction.github.io/korg/> (landing page, the Gold Seal verifier, and the raw-ledger verifier; `.github/workflows/pages.yml` deploys `spec/korg-ledger-v1/`). (2) A **`verify-goldseal` GitHub Action** (`.github/actions/verify-goldseal`) verifies seals/ledgers in CI — `uses: New1Direction/korg/.github/actions/verify-goldseal@…` with an optional `pin-pubkey`, failing the job on any tampered/unpinned artifact. It renders a **rich report** (a `report.mjs` reusing verify.mjs) into the job summary and, on a `pull_request`, **upserts a sticky PR comment** with the re-derived attestation inline — claim · who (issuer) · what (events/tools/files) · when (anchor) · integrity — so "verified agent work" shows up right on the PR. (3) A **dogfooding demo workflow** (`.github/workflows/goldseal-demo.yml`) mints a Gold Seal from a session ledger with `korg-seal`, verifies it (pinned to the issuer) via the Action, and proves a tampered copy is rejected — "verified agent work in CI". (4) The npm verifier (`@korgg/ledger-verify`) bumped to 0.2.0 — its `npx` CLI now verifies `goldseal@v1` too. The Gold Seal verifier's badge now links to the hosted page, and `korg-seal mint` prints the live verify URL.
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
