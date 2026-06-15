# korg-recall-mcp

**Cross-session semantic memory MCP server over the korg ledger.**

Install once. Every Claude Code (or any MCP-speaking client) prompt can
now recall relevant moments from **every prior session you've ever
recorded**. ChatGPT Memory but local, visible, auditable, and **spans
all your AI tools** — not just one vendor.

## What this completes

The transport-agnostic proof trio (`codex-ws`, `grok-heavy`,
`claude-code`) showed korg can *ingest* events from any agent. Live-tail
mode (`korg-ingest-claude --tail`) made the ingest continuous. This MCP
server is the **output side**: anyone querying the ledger gets semantic
recall over all of it from inside their existing AI tool.

```
                                                    ┌─────────────────┐
~/.claude/projects/**/*.jsonl  ──┐                   │  Claude Code    │
                                  ▼                   │  (or any MCP    │
                  korg-ingest-claude --tail           │   client)       │
                                  ▼                   └──────┬──────────┘
                  ~/.korg/claude-events.jsonl                ▼
                                  ▼                  ┌─────────────────┐
                            korg-recall-mcp  ◀───────│  recall(query)  │
                                  ▼                   └─────────────────┘
                       top-N relevant events
                       across ALL prior sessions
```

## Why this is differentiated

| Product | Memory scope | Tools covered | Format |
|---|---|---|---|
| ChatGPT Memory | per-account | OpenAI only | proprietary |
| Anthropic | — | — | none ships today |
| Cursor memories | per-project | Cursor only | proprietary |
| **korg-recall-mcp** | **per-machine** | **any tool with a korg adapter** (Claude Code, Codex, Grok Heavy, KorgChat, korgex) | **open jsonl** |

The fact that the ledger is a flat append-only JSONL means it's
greppable, version-controllable, replayable in `korg-tui`, and
introspectable in any other adapter without parsing tricks. Nobody else
ships a memory layer that spans across vendor boundaries because nobody
else has a vendor-agnostic capture format.

## Install

```bash
# From the Korg workspace:
pip install -e ./adapters/recall-mcp[semantic]
```

The `[semantic]` extra pulls `fastembed` (ONNX, no torch dep). Without
it the server falls back to substring matching — still useful for
keyword lookups but loses the conceptual recall.

## Register with Claude Code

Edit `~/.claude.json` (or whichever Claude Code MCP config file your
version uses) and add:

```json
{
  "mcpServers": {
    "korg-recall": {
      "command": "korg-recall-mcp",
      "args": ["--ledger", "~/.korg/claude-events.jsonl"]
    }
  }
}
```

Restart Claude Code. The `recall` tool now appears in the agent's
toolset; the model decides when to use it (typically before
attempting something that might have been done before).

For continuous capture, run this in another shell:

```bash
korg-ingest-claude --tail
```

Together they form a closed loop: every session you run gets recorded;
the next session can recall from all of them.

## CLI usage (without an MCP client)

```bash
# Default: read ~/.korg/claude-events.jsonl, semantic if fastembed installed
korg-recall-mcp --query "how did i solve the auth bug last week"

# Multiple ledger sources (mix files and directories):
korg-recall-mcp --query "rate limiter design" \
    --ledger ~/.korg/claude-events.jsonl \
    --ledger ~/.korg/korgchat-journals/

# Substring only (no embedding):
korg-recall-mcp --query "TODO rate limiter" --mode substring

# Tighter recall (high cosine floor):
korg-recall-mcp --query "anything about async" --min-score 0.55 --top-n 3
```

## `--introspect`: agent-native discovery

Foundry-style discovery: emit a `korg:introspect@v1` document describing
every callable, its input schema, declared side-effects, output mode,
and stable command ID. **The same source of truth that the MCP
`tools/list` endpoint serves from** — so the JSON Schema an agent
discovers via `--introspect | jq` is byte-identical to what it'll get
back from an MCP call. One source, two surfaces.

```bash
$ korg-recall-mcp --introspect | jq '.callables[0].capabilities'
{
  "output_mode": "stream",
  "side_effects": "fs_read",
  "requires_project": false,
  "long_running": false,
  "stateful": false,
  "reads_stdin": false,
  "supports_output_path": false
}
```

Agents can use this **before** invoking anything to decide whether a
callable is safe and how to consume its output. The `callables_declared:
true` flag is the truthfulness signal — the package commits to documenting
its capabilities rather than emitting an empty stub.

Stable `command_id` (here `korg-recall-mcp.recall`) means agents can pin
to this identifier across version bumps. The `exit_codes` table at the
document root is the canonical agent contract (string-keyed on the wire
since JSON has no integer keys).

## The MCP tool surface

One tool: `recall`. Schema (abbreviated):

```json
{
  "name": "recall",
  "description": "Search across all prior AI sessions recorded in the korg ledger…",
  "inputSchema": {
    "type": "object",
    "required": ["query"],
    "properties": {
      "query":       {"type": "string"},
      "top_n":       {"type": "integer", "default": 5, "minimum": 1, "maximum": 50},
      "min_score":   {"type": "number",  "default": 0.30, "minimum": 0.0, "maximum": 1.0},
      "mode":        {"type": "string",  "enum": ["auto", "semantic", "substring"], "default": "auto"},
      "tool_filter": {"type": "array",   "items": {"type": "string"}}
    }
  }
}
```

Result text format (one line per match, designed for direct LLM consumption):

```
[recall · semantic] 3 match(es):
  · seq=128 score=0.84 agent=claude-code#abc tool=user_prompt :: can you show me how to handle async errors in tokio
  · seq=129 score=0.72 agent=claude-code#abc tool=llm_inference :: Use anyhow::Result or thiserror for ergonomic error propagation.
  · seq=42  score=0.61 agent=claude-code#xyz tool=Bash         :: Bash command=cargo test → 5 passed
```

## Architecture

- **`text.py`** — flattens any event shape into a single embeddable string,
  preferring signal over noise (tool name + key args + result preview).
- **`index.py`** — `EventIndex` reads one or more `.jsonl` files
  incrementally, tracking byte offsets so each `refresh()` only loads
  new events. Picks up new files appearing in watched directories.
- **`search.py`** — `RecallEngine.search(query, mode=…)`. Substring is
  pure-stdlib AND-of-lowercase-terms; semantic uses fastembed +
  cosine. Embeddings cache on the event instance.
- **`server.py`** — minimal JSON-RPC over stdio. Implements `initialize`,
  `tools/list`, `tools/call`, `ping`, and the `notifications/initialized`
  notification. No third-party MCP SDK required.

## Tests

Tests cover everything end-to-end:

- 9 tests for the text flattener (per-event-type extraction, trimming, fallbacks).
- 11 tests for the index (incremental load, malformed-line tolerance,
  partial-line hold-back, multi-file, new-file pickup, triggered_by preservation).
- 16 tests for the recall engine (substring + semantic, top_n, min_score,
  tool_filter, automatic refresh, auto-mode fallback, explicit-semantic
  raising without fastembed).
- 16 tests for the MCP server (full initialize/list/call roundtrip,
  unknown-method errors, notification handling, ping, tool_filter through
  the JSON-RPC surface, malformed-line tolerance, serve_stdio loop).
- 17 tests for the introspect document (korg:introspect@v1 schema,
  callables, exit_codes, command_id stability).

Run them with the Korg workspace venv (which has fastembed):

```bash
PYTHONPATH=src /path/to/Korg/.venv/bin/python3 -m pytest -q
```

## Status

v0.1.0 — works end-to-end against real fixtures and a synthetic MCP
roundtrip. Not yet:

- **Embedding cache persistence to disk.** Today, embeddings are
  recomputed on every server start. For long-running servers this is
  fine (cache is hot in memory); for repeated CLI invocations or
  short-lived servers, persisting `{file:seq → vector}` to a sidecar
  JSON would speed cold starts. Easy add when needed.
- **Filtering by `--since DUR`.** Events don't carry HLC timestamps in
  the flat-jsonl format yet; once they do, `--since 24h` is one filter
  predicate away.
- **Subscribe-style streaming results.** MCP supports server-initiated
  notifications. A "context update" event when a new highly-relevant
  match appears in the ledger would be magical. Out of scope for v1.

## License

MIT.
