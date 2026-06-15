# claude-code-adapter

Translate Claude Code session JSONL files into korg `AgentToolCall` events.

## What it produces

Each Claude Code session is captured into a **verifiable per-session ledger** at
`~/.korg/sessions/<session_id>.jsonl` — a `korg-ledger@v1` hash-chain you can
independently verify with `korg-verify` (or the JS verifier in a browser). Capture
is passive and zero-config once `korg-setup` registers the `korg-hook`
PostToolUse/Stop hook; `korg-backfill` re-derives the same verifiable ledgers for
every historical session.

> The earlier flat `~/.korg/claude-events.jsonl` (one un-chained `{seq, ...}` line
> per event, written by the legacy `korg-ingest-claude --tail` daemon and
> `make_jsonl_emit`) is **legacy** — it carried no hash chain and is superseded by
> the per-session verifiable ledgers. `korg-backfill --migrate-flat <path>` converts
> an existing flat ledger into the per-session format.

## What this proves

Claude Code is the production-default Anthropic CLI. Its session files
(`~/.claude/projects/<dir>/<uuid>.jsonl`) are written by every user of the
tool, every day, with no integration required from us — they exist whether
korg knows about it or not. If those files round-trip through korg's ledger
with full causal coherence, **any existing Claude Code user can have a
retroactive korg ledger of every session they've ever run**, with zero
behavior change.

This completes the three-transport proof trio for korg's universality claim:

| Adapter | Transport | Tool shape | Source |
|---|---|---|---|
| [`codex-ws`](../codex-ws/) | WebSocket (mitmproxy capture) | OpenAI function_call + custom_tool_call | OpenAI Codex CLI |
| [`grok-heavy`](../grok-heavy/) | NDJSON stream | XML-ish tool blocks, 16-agent fan-out | Grok Heavy `/_data/v1/a/t/` |
| `claude-code` (this) | JSONL on disk | Anthropic message + content blocks (tool_use / tool_result) | `~/.claude/projects/*.jsonl` |

If all three round-trip cleanly, korg's "transport-agnostic capability
ledger" claim is real, not aspirational.

## Causal mapping

| Claude Code event | korg event | triggered_by |
|---|---|---|
| `type: user` with plain-string content (first one) | `user_prompt` (root) | `None` |
| `type: user` with plain-string content (follow-ups) | `user_prompt` | prior `llm_inference` |
| `type: user` with `tool_result` block only | — (attaches to prior tool call) | — |
| `type: assistant` (any) | `llm_inference` | prior `llm_inference`, or root `user_prompt` if first |
| `tool_use` block inside an assistant message | tool name verbatim (`Read`, `Bash`, `Edit`, …) | enclosing `llm_inference` |

The `llm_inference → llm_inference` chain (skipping over intervening
`user_prompt` and `tool_call` events) follows spec §2a. The result is
that replaying any Claude Code session reconstructs an audit-coherent
graph identical in shape to what korgex's own agent loop produces.

## Two ways to use it

> **Recommended live path: `korg-setup` + `korg-hook`.** The verifiable,
> zero-config capture model (PostToolUse/Stop hook → per-session verifiable
> ledger at `~/.korg/sessions/<id>.jsonl`) is described in
> [What it produces](#what-it-produces). The `korg-ingest-claude --tail/--once`
> CLI below still works, but it emits the **legacy un-chained flat ledger**;
> run `korg-backfill --migrate-flat <path>` to upgrade an existing flat ledger
> to the per-session format.

### Legacy flat-file tail (`korg-ingest-claude`)

The adapter watches `~/.claude/projects/**/*.jsonl` and streams new events into
a **legacy flat (un-chained)** korg ledger as Claude Code writes them. Install,
run once, get a continuously-growing flat ledger:

```bash
pip install -e ./adapters/claude-code

# Watch ~/.claude/projects, append events to ~/.korg/claude-events.jsonl (legacy flat ledger)
korg-ingest-claude --tail

# Just print events to stdout (no file write):
korg-ingest-claude --tail --stub

# Custom locations:
korg-ingest-claude --tail \
    --projects-dir ~/.claude/projects \
    --out         ~/.korg/claude-events.jsonl \
    --state       ~/.korg/claude-tail-state.json \
    --poll-interval 0.5
```

The byte-offset state is persistent — kill the process and restart any
time; it picks up exactly where it left off. The state file at
`~/.korg/claude-tail-state.json` survives reboots.

### One-shot backfill of history

For the existing pile of session files on your disk right now, use
`korg-backfill` — it re-derives the **verifiable per-session ledgers** under
`~/.korg/sessions/` for every historical session.

The legacy `korg-ingest-claude --once` still exists, but it writes the **legacy
un-chained flat ledger** instead:

```bash
korg-ingest-claude --once --out ~/.korg/claude-events.jsonl   # legacy flat ledger
# [korg-ingest-claude] one-shot pass complete · files_active=N events=M ...
```

Runs once, ingests everything you haven't ingested yet, exits.

### Library usage

For embedding into your own ledger plumbing:

```python
from pathlib import Path
from claude_code_adapter import ClaudeCodeAdapter, TailIngester


def emit(body: dict) -> int | None:
    # write to korg-bridge, korg-server, your own DB, etc.
    ...


# Single-session replay:
adapter = ClaudeCodeAdapter(emit, source_agent="agent:claude-code@2.1.0")
with Path("~/.claude/projects/-Users-you-Documents-foo/abc.jsonl").expanduser().open() as f:
    stats = adapter.ingest(f)

# Or multi-session tail:
ingester = TailIngester(emit=emit, projects_dir=Path("~/.claude/projects").expanduser())
ingester.run(poll_interval_s=1.0)   # blocks; Ctrl-C to stop
```

The library exports `make_jsonl_emit(path)` and `make_stub_emit()` as
ready-made `emit` callables for testing or when you just want a flat
JSONL ledger on disk.

## Input shape

The adapter accepts any iterable of:
- raw JSONL strings (e.g. an open file handle), **or**
- pre-parsed dicts (e.g. for tests).

Mixed iterables also work. Malformed lines and blank lines are skipped
silently.

## What gets skipped (intentional)

- `system` / `last-prompt` / `permission-mode` / `ai-title` /
  `file-history-snapshot` / `attachment` / `queue-operation` — session
  metadata that doesn't participate in causality.
- `thinking` content blocks — captured as part of the `llm_inference`
  result, not emitted as separate events.
- Streaming intermediates — Claude Code only writes complete messages
  to the JSONL, so this is moot.

## Tail-mode invariants

These are property-tested in `tests/test_tail.py` and underpin the
"install once, ledger forever" promise:

- **No duplicate emission.** Byte offsets are persisted atomically per
  file via tmp-rename; restarts resume at the exact byte they left off.
- **Cross-poll causal coherence.** Per-file `ClaudeCodeAdapter` instances
  retain both chain state (`prompt_seq`, `llm_seq`) and parser state
  (`pending_tool_calls`, `seen_first_user`) across polls — so a
  `tool_use` written in one poll cycle and its `tool_result` written in
  the next still attach correctly.
- **Mid-write tolerance.** A line being written without a trailing `\n`
  at poll time is held back until the next poll. The adapter never
  emits a half-formed line.
- **Multi-session isolation.** Each `.jsonl` file (each session) gets
  its own adapter; their `seq` chains can't interfere.
- **New-file pickup.** Files appearing in `projects_dir` between polls
  are discovered automatically on the next pass.

## Known limitations

- **Sidechains (`isSidechain: true`).** Claude Code's `Task` tool spawns
  sub-agents that share the parent's JSONL file. korg's single-parent
  `triggered_by` model can't represent true sub-agent fan-out structurally
  — the sidechain events become inline siblings of their invoking thread.
  If multi-agent becomes common, the natural extension is a
  `caused_by: [seq_ids]` array on `llm_inference` events (same proposed
  resolution as [`grok-heavy`](../grok-heavy/#known-limitation-chatroom_send-cross-edges)).
- **`tool_result` payload size.** Results larger than 8000 chars are
  truncated inline with `…[truncated]` and `success` is preserved. For
  full fidelity, integrate `payload_refs` (sha256, size_bytes) — this
  adapter doesn't do content addressing for v1.
- **Wall-clock timing.** Timestamps from the JSONL are not propagated
  into the ledger; the ledger uses its own HLC. Topological order is
  preserved; absolute timing is not.
