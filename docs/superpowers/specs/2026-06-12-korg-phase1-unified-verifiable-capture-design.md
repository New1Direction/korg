# Phase 1 — Unified Verifiable Ledger + Zero-Config Claude Code Capture

**Status:** Design (approved)
**Date:** 2026-06-12
**Program:** "Zero-config adoption" (Track A, Phase 1 of 3). Phase 2 = trust hardening; Phase 3 = publish verifier + WASM + UI. Track B (swarm) follows Track A.
**One-line goal:** Make every Claude Code session land — passively, cross-platform — in a *single, hash-chained, independently verifiable* ledger, so `korg-verify` can validate a real agent session and there is exactly **one** ledger format in the project.

---

## 1. Problem & Context

Korg currently has **two ledgers that never join**:

- **Flat capture ledger** — the Claude Code tail adapter writes `~/.korg/claude-events.jsonl` as plain JSONL `{seq, source_agent, tool_name, args, result, success, duration_ms, triggered_by?}` with **no hash chain, no HLC, no signatures** (`adapters/claude-code/src/claude_code_adapter/tail.py:256`). This is what `recall-mcp` reads.
- **Canonical verifiable ledger** — `korg-server` / `korg-bridge` write `JournalEvent` records (hash-chained `entry_hash`/`prev_hash`, HLC timestamps, causal metadata) that `korg-verify` actually checks (`crates/korg-registry/src/log.rs:310`, `crates/korg-ledger/src/lib.rs:115`).

The consequence: the most-adopted capture path produces records that **cannot be verified**, and the verifiable format is produced only by paths that require a running server or a built PyO3 extension. The project's headline claim ("tamper-evident, independently verifiable") does not apply to the data most users actually generate.

Two secondary problems:
- **"Just on" is macOS-only.** Passive capture relies on a `launchd` daemon; Linux is a manual `tmux` paste, Windows nothing (`adapters/korg-setup/src/korg_setup/launchd.py`, `setup.py:8`).
- **The richest capture path writes the wrong format.** The file-tail **parser** already produces a causally-correct, rich event stream — `llm_inference` events with token + cache breakdown, tool calls, and the spec §2a causal spine (`adapters/claude-code/.../parser.py:117`, `adapter.py:84`). That asset is currently funneled into the non-verifiable flat file.

## 2. Goal & Non-Goals

**Goal:** One canonical, append-friendly, hash-chained ledger format; a cross-platform, zero-config, *full-richness* Claude Code capture path that writes it; all readers (`recall-mcp`, `korg-verify`, TUI) consuming it; a one-time migration of existing flat ledgers; and the Phase-2 trust primitives *reserved* in the format so we never re-publish it.

**Non-goals (explicitly deferred):**
- **Phase 2:** implementing per-event signing, RFC-3161 / transparency-log anchoring, proptest + cargo-fuzz, deterministic-replay CI gate. Phase 1 only *reserves the slots*.
- **Phase 3:** publishing `korg-verify` to crates.io, the WASM in-browser verifier, npx-able MCP servers, the time-travel explorer UI.
- Unifying the *physical location* of the per-workspace server journal (`.korg/capability_journal.json`) with the user-global capture ledger. Phase 1 unifies the **format** (both become `JournalEvent` JSONL, both verifiable by the same tool); merging files is out of scope.
- Codex/Grok live capture (those adapters remain frame-dump library tools).

## 3. Architecture Overview

```
Claude Code tool call / turn end
        │  (PostToolUse / Stop / SubagentStop hook fires)
        ▼
  korg-hook  ──reads transcript_path delta (offset-tracked per session)
        │
        ▼
  existing parser (parser.py) ──► NormalizedEvents (llm_inference, tool calls, spine)
        │
        ▼
  canonical emit ──► builds full JournalEvent (synthesized metadata + HLC)
        │
        ▼
  korg_ledger (pure-Python conformant writer) ──► hash-chains + appends one line
        │
        ▼
  ~/.korg/ledger.jsonl   ← THE single canonical verifiable ledger (JSONL)
        │
        ├──► recall-mcp (search)
        ├──► korg-verify (independent verification)
        └──► korg-tui (display)
```

The hook does **not** reimplement parsing — it *pumps* the existing parser over the session-file delta. This keeps full data richness (`llm_inference`, tokens, causal spine) while gaining cross-platform, daemon-free, real-time capture.

## 4. Components (isolated units)

Each unit below has: a single purpose, a defined interface, and named dependencies.

### 4.1 `korg-ledger` format extension (Rust — `crates/korg-ledger`, `crates/korg-registry`)
**Purpose:** Reserve Phase-2 trust slots without breaking existing conformance vectors.
**Changes:**
- Add `event_sig: Option<String>` to `JournalEvent`; add `"event_sig"` to `HASH_FIELDS` (`crates/korg-ledger/src/lib.rs:23`) so it is excluded from the hash preimage exactly like `entry_hash`. `#[serde(default)]` ⇒ existing events and frozen vectors deserialize and verify unchanged.
- Add additive variant `CapabilityEvent::LedgerRewind { target_seq_id, invalidated_through, rewound_by, reason, timestamp }`. It is appended (never truncates), so it lives *inside* the hash preimage and is itself tamper-evident. This is the non-destructive replacement for today's `Vec::retain` rewind (`crates/korg-registry/src/log.rs:618`) — Phase 1 adds the variant + makes `rewind()` append it; Phase 2 builds replay semantics on it.
- Reserve out-of-band anchor file path `.korg/anchors.jsonl` (records `{seq_id, entry_hash, anchor_proof, anchored_at}`). Phase 1: define the path + make readers tolerate its presence/absence. Phase 2 writes it. **Zero** preimage impact.
**Interface (unchanged):** `chain_hash`, `verify_chain`, `verify_dag`.
**Verification:** the existing frozen conformance vectors (`spec/korg-ledger-v1/conformance.json`) must still pass byte-for-byte after these additions.

### 4.2 JSONL on-disk format for `CapabilityJournal` (Rust — `crates/korg-registry`)
**Purpose:** Make the canonical ledger append-friendly and concurrency-tolerant.
**Change:** Replace the "rewrite a pretty-printed JSON array on every flush" behavior (`log.rs:684`) with **one `JournalEvent` per line** (JSONL). `append_with_metadata` does an atomic line append (write to a temp + `rename`, or `O_APPEND` under the existing `fs2` lock). `load` reads line-by-line. In-memory chain logic is unchanged.
**Interface:** `append_with_metadata(...) -> seq_id`, `load(path)`, `verify`.
**Compat:** provide a one-shot reader that accepts the legacy JSON-array file and rewrites it as JSONL on first load (so existing workspace journals migrate transparently).

### 4.3 `korg_ledger` — pure-Python conformant writer (new Python module)
**Purpose:** Produce verifiable `JournalEvent` JSONL from Python with **stdlib only** (no server, no compiled extension). Becomes a *third independent conformant producer* of `korg-ledger@v1`.
**Implementation:** reuse the canonicalization + `chain_hash` already in `spec/korg-ledger-v1/conformance.py` (sorted keys, compact, `\uXXXX` escaping, surrogate pairs, optional HMAC via `KORG_LEDGER_HMAC_KEY`).
**Interface:**
```python
class LedgerWriter:
    def __init__(self, path: Path, hmac_key: bytes | None = None): ...
    def append(self, *, event: dict, metadata: dict) -> int:  # returns global seq_id
        # assigns seq_id, ticks HLC, sets prev_hash from last entry_hash (or GENESIS),
        # computes entry_hash over canonical(JournalEvent minus HASH_FIELDS),
        # enforces causality (metadata.triggered_by strictly-earlier or None),
        # appends one JSON line under an exclusive file lock (fcntl/msvcrt).
    def tip(self) -> tuple[int, str]:  # (seq_id, entry_hash) for resume
```
**Dependencies:** `hashlib`, `json`, `uuid`, `fcntl`/`msvcrt` (stdlib).
**Verification:** unit test that the writer reproduces the frozen tip hashes in `spec/korg-ledger-v1/conformance.json` (basic, HMAC, non-BMP).

### 4.4 Canonical emit (Python — `claude-code` adapter)
**Purpose:** Turn a parsed `NormalizedEvent` into a full `JournalEvent` and append it via 4.3. Replaces `make_jsonl_emit` (`tail.py:256`).
**Field synthesis (from the audited mapping):**

| `JournalEvent` field | Source |
|---|---|
| `event.{source_agent,tool_name,args,result,success,duration_ms}` | parser output (verbatim) |
| `event.event_type` | `"AgentToolCall"` (Phase 1 captures tool/LLM/prompt events) |
| `event.timestamp` | session-record timestamp if present, else capture wall-clock |
| `event.payload_refs` | `[]` (content-ref large payloads is a later optimization) |
| `metadata.event_id` | fresh `uuid4` |
| `metadata.triggered_by` | **global** seq_id returned by the parent's `append()` (see §5) |
| `metadata.causation_id` | parent event's `event_id` (tracked in per-session map) |
| `metadata.root_event_id` | first event's `event_id` in the session |
| `metadata.actor_id` | `"korg:claude-hook"` (hook path) / `"korg:claude-tail"` (daemon path) |
| `metadata.emitted_at` | HLC assigned by the writer |
| `metadata.{correlation_id,campaign_id}` | nil UUID; `tier` = `Telemetry`; `speculative` = false; `retry_count` = 0; `tags` = {} |

The parser, `NormalizedEvent`, and `triggered_by` assignment logic are **reused unchanged**.

### 4.5 `korg-hook` — the capture driver (new Python entrypoint)
**Purpose:** On each hook firing, capture the session-file delta into the canonical ledger. Cross-platform, daemon-free.
**Behavior:**
1. Read hook JSON from stdin: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, and (for `PostToolUse`) `tool_name`/`tool_input`/`tool_response`.
2. Open `transcript_path`; seek to the saved byte offset in `~/.korg/hook-state/<session_id>.json`; read complete lines only.
3. Run the existing parser over the delta → `NormalizedEvent`s → 4.4 emit → 4.3 append.
4. Persist the new offset + the local→global seq map atomically.
5. **Always exit 0** and never block meaningfully — errors are swallowed to `~/.korg/logs/korg-hook.log`; capture must never break or slow a Claude session.
**Why the transcript, not the hook payload:** the `transcript_path` delta contains the `assistant` records that carry `llm_inference` + token usage, which the raw `PostToolUse` payload lacks. `Stop`/`SubagentStop` firings flush the trailing turn that has no following tool call.
**Interface:** stdin JSON → side effects (ledger append + state update); stdout unused; exit 0.
**Dependencies:** 4.3, 4.4, existing `parser.py`.

### 4.6 `korg-setup` settings.json hook registration (Python — `korg-setup`)
**Purpose:** Auto-register the hook so capture is "just on," cross-platform.
**Change:** new `korg_setup/claude_settings.py::ensure_hook_registered()` that read-modify-atomic-writes `~/.claude/settings.json` under `hooks.PostToolUse`, `hooks.Stop`, `hooks.SubagentStop`, adding `{"matcher": ".*", "hooks": [{"type": "command", "command": "<abs path to korg-hook>"}]}`. Idempotent (no duplicate entries); backup to `~/.claude/settings.json.korg-backup`.
**Note:** `~/.claude/settings.json` (hooks) is a *different file* from `~/.claude.json` (MCP servers) — the existing MCP registration in `claude_config.py` is kept as-is.
**Daemon:** demoted to optional. `launchd`/manual tail still works (writing the same canonical ledger via 4.4) for users who prefer always-on tailing, but it is no longer required and no longer the default.

### 4.7 Migration + optional backfill (Python)
**Purpose:** Bring existing data into the canonical ledger.
- **Migrate (one-shot, idempotent):** convert an existing flat `~/.korg/claude-events.jsonl` → `~/.korg/ledger.jsonl` using the audited field mapping; synthesize UUIDs/HLC/timestamps; chain hashes in `seq` order. Backs up the original; lossy fields (real timestamps, original UUIDs) are documented as synthesized. The output is canonical and `verify_chain`-valid.
- **Backfill (optional `--backfill`):** run the parser over all historical `~/.claude/projects/**/*.jsonl` once → canonical ledger, giving retroactive verifiable history with no behavior change.

### 4.8 Readers unification
- **`recall-mcp`** (`index.py:62`): update `IndexedEvent`/`EventIndex.refresh` to read canonical JSONL, where fields are nested (`event.tool_name`, `event.args`, `metadata.triggered_by`, top-level `seq_id`). Provide a thin accessor so a migrated file reads identically.
- **`korg-verify`:** confirm it reads JSONL (it already verifies the canonical format); add the capture ledger path to its accepted inputs.
- **`korg-tui`:** point at `~/.korg/ledger.jsonl`.

### 4.9 Conformance & end-to-end tests
- Python writer (4.3) reproduces the frozen conformance tip hashes.
- E2E: a committed sample Claude session fixture → hook driver → `~/.korg/ledger.jsonl` → `korg-verify` exits 0; tampering one event fails verification at the right `seq_id`.
- Cross-producer: a ledger produced by the Python writer verifies identically under the Rust `korg-verify` and the JS verifier (extends the existing 3-language conformance to the *producer* side).
- Wire these into CI (`.github/workflows/ci.yml`).

### 4.10 Honest framing (ride-along from the "credibility" track)
Update `adapters/claude-code/README.md`, `INSTALLATION_GUIDE.md`, and the README capture section to describe *one* verifiable, cross-platform, zero-config capture path. Stop describing the flat file as a ledger. Keep claims to what Phase 1 ships (capture + verification); signing/anchoring are clearly labeled Phase 2.

## 5. Data Flow & Causality

The canonical ledger has a **single global monotonic `seq_id`**, assigned by the writer (4.3). `metadata.triggered_by` must reference a strictly-earlier global `seq_id`.

The parser assigns causal links in *local* terms (per spec §2a: `llm_inference` chains to the prior `llm_inference`; `tool_in_round` chains to its enclosing `llm_round`). The hook driver processes a session's events in file order and maintains a per-session **local→global seq map** in `~/.korg/hook-state/<session_id>.json`: each `append()` returns the global `seq_id`, which becomes the `triggered_by` value for its children. Because the writer is the sole seq authority and appends under an exclusive lock, monotonicity and the strictly-earlier invariant hold even with multiple concurrent Claude sessions writing to the one ledger.

## 6. Error Handling

- **Hook robustness:** the driver wraps all work in a top-level guard, logs to `~/.korg/logs/korg-hook.log`, and exits 0 unconditionally. A capture failure must never abort or visibly slow a Claude tool call.
- **Offset/state corruption:** if `<session_id>.json` is unreadable, re-derive conservatively from the ledger tip for that session (idempotent — duplicate suppression by `(session_id, source record id)`), preferring missed-capture over double-capture-then-dedupe.
- **Concurrency:** exclusive file lock (`fcntl`/`msvcrt`) around append serializes writers; brief bounded retry on contention.
- **Large payloads:** Phase 1 stores `args`/`result` inline; values over a threshold are truncated with a marker (content-addressed `payload_refs` is a later optimization, slot already exists).

## 7. Testing Strategy

Per repo testing rules (≥80%, behavioral): unit tests for the Python writer (conformance vectors, causality rejection, HMAC mode, resume-from-tip), unit tests for field synthesis, an integration test for the hook driver over a fixture session, the cross-producer conformance test, and the migration round-trip test (flat → canonical → `verify_chain` clean). Rust: JSONL load/append round-trip, legacy-array→JSONL migration, `LedgerRewind` append + chain integrity, and re-run of frozen vectors with `event_sig` added.

## 8. Risks & Open Questions (resolve during implementation)

1. **Exact `PostToolUse`/`Stop` payload fields.** The design assumes `transcript_path`, `session_id`, `cwd`, `hook_event_name` are present. Verify against the installed Claude Code version before finalizing 4.5; if `transcript_path` is absent, derive the session file from `session_id` + project dir.
2. **Hook latency.** Reading + parsing the delta must be sub-perceptible. Offset-tracking keeps it O(delta); if a pathological delta is large, cap work and let the next firing (or `Stop`) catch up.
3. **Two physical ledgers during transition.** Capture ledger (`~/.korg/ledger.jsonl`, user-global) vs per-workspace server journal both use the format but are separate files. Acceptable for Phase 1; note for a later "single store" step.
4. **`recall-mcp` schema change** is a breaking read change; the thin accessor + migration must land together so recall keeps working across the cutover.
5. **`event_sig` in `HASH_FIELDS`** must be added in lockstep across Rust, Python, and JS implementations or the 3-language conformance breaks. Land all three in one change.

## 9. Phase Boundary Recap

- **Phase 1 (this spec):** one format (JSONL `JournalEvent` + reserved slots), cross-platform zero-config hook capture reusing the parser, pure-Python conformant writer, readers unified, migration + optional backfill, conformance/E2E tests, honest docs.
- **Phase 2:** implement `event_sig` signing, anchor writing (RFC-3161 / transparency log), `LedgerRewind` replay semantics, proptest + cargo-fuzz, deterministic-replay CI gate.
- **Phase 3:** publish `korg-verify` (crates.io) + WASM in-browser self-verifying receipt, npx-able MCP servers, time-travel explorer UI.
