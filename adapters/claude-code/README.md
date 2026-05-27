# claude-code-adapter

Translate Claude Code session JSONL files into korg `AgentToolCall` events.

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

## Usage

```python
from pathlib import Path
import requests
from claude_code_adapter import ClaudeCodeAdapter


def emit(body: dict) -> int | None:
    r = requests.post("http://localhost:8080/api/agent/tool-call", json=body)
    return r.json().get("seq_id") if r.ok else None


adapter = ClaudeCodeAdapter(emit, source_agent="agent:claude-code@2.1.150")

# Replay one session file:
with Path.home().joinpath(".claude/projects/-Users-you-Documents-foo/abc.jsonl").open() as f:
    stats = adapter.ingest(f)

print(stats)
# IngestStats(user_prompts=12, llm_rounds=87, tool_calls=152, dropped=0)
```

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
