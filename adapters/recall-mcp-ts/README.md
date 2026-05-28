# @korg/recall-mcp

> **TypeScript port of korg-recall-mcp for npx distribution.**

Cross-session semantic memory for Claude Code (or any MCP client), as
an npm package. Same `recall` tool, same `korg:introspect@v1` document,
same ledger format as the Python reference implementation — but
installable in one command:

```bash
npx -y @korg/recall-mcp
```

## Why a TypeScript port

Claude Code itself is TypeScript. The canonical MCP SDK is TypeScript.
`npx`-installable distribution is dramatically less friction than
`pipx + venv + pip install` for a one-time user. This port targets the
install-friction case specifically — the Python version stays as the
reference implementation with the higher-quality `fastembed` backend.

| | `@korg/recall-mcp` (TS) | `korg-recall-mcp` (Python) |
|---|---|---|
| **Install** | `npx -y @korg/recall-mcp` | `pipx install 'korg-recall-mcp[semantic]'` |
| **Embedding backend** | `@xenova/transformers` (BGE-small, ONNX, optional dep) | `fastembed` (BGE-small, ONNX) |
| **Bundle weight** | ~10MB + 50MB model on first semantic call | ~30MB + 130MB model |
| **Substring fallback** | always available, zero-dep | always available, zero-dep |
| **MCP protocol** | official `@modelcontextprotocol/sdk` (v1.29) | hand-rolled JSON-RPC over stdio |
| **Tool schema** | byte-identical to Python (structurally) | reference |
| **`command_id`** | `korg-recall-mcp.recall` | `korg-recall-mcp.recall` |
| **`korg:introspect@v1`** | yes | yes |

Both versions read the **same ledger file** and emit the same tool
results. You can run either; you can run both side-by-side under
different MCP server names.

## Register with Claude Code

Add to `~/.claude.json`:

```json
{
  "mcpServers": {
    "korg-recall": {
      "command": "npx",
      "args": ["-y", "@korg/recall-mcp", "--ledger", "/Users/you/.korg/claude-events.jsonl"]
    }
  }
}
```

Restart Claude Code. The `recall` tool is now in the agent's toolset.

For continuous capture (the other half of the loop), pair with the
Python `korg-ingest-claude --tail` running separately, or use
`korg-setup` to install everything at once.

## CLI usage

```bash
# Run as MCP server on stdio (default — what Claude Code calls)
korg-recall-mcp

# One-shot CLI search (great for piping into other tools)
korg-recall-mcp --query "rust borrow checker" --mode substring --top-n 5

# Emit the korg:introspect@v1 document and exit
korg-recall-mcp --introspect

# Custom ledger paths (repeatable)
korg-recall-mcp --ledger ~/.korg/claude-events.jsonl \
                --ledger ~/.korg/legacy-events.jsonl
```

## The `recall` tool

One MCP tool. Same input schema as the Python version:

```json
{
  "name": "recall",
  "inputSchema": {
    "type": "object",
    "required": ["query"],
    "properties": {
      "query":       { "type": "string" },
      "top_n":       { "type": "integer", "default": 5, "minimum": 1, "maximum": 50 },
      "min_score":   { "type": "number",  "default": 0.30, "minimum": 0.0, "maximum": 1.0 },
      "mode":        { "type": "string",  "enum": ["auto", "semantic", "substring"], "default": "auto" },
      "tool_filter": { "type": "array",   "items": { "type": "string" } }
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

## Embedding modes

| Mode | What it does | When to use |
|---|---|---|
| `auto` (default) | Semantic if `@xenova/transformers` is loadable, else substring | The right answer for most users |
| `semantic` | Embedding-backed cosine ranking via transformers.js. Throws if dep missing. | When you NEED conceptual matching |
| `substring` | AND-of-lowercased-terms over the embedding text. Zero dependencies. | Tiny ledgers, quick keyword lookups, no network on first run |

Semantic mode downloads `Xenova/bge-small-en-v1.5` (~50MB) to
`~/.cache/huggingface/` on first call. Subsequent embeds are
sub-millisecond.

## Architecture

```
src/
├── text.ts          # event → embedding text (port from Python)
├── event-index.ts   # incremental JSONL reader, byte-offset state
├── search.ts        # RecallEngine: substring + semantic
├── introspect.ts    # the korg:introspect@v1 document
├── server.ts        # MCP wiring via @modelcontextprotocol/sdk
├── cli.ts           # CLI entry point + arg parsing
└── index.ts         # public API for programmatic use
```

Zero required runtime dependencies beyond `@modelcontextprotocol/sdk`.
The semantic embedding backend is an **optional** dependency that
loads on first semantic call.

## Tests

49 tests covering:

- **text.test.ts** (9) — per-event-type extraction, trimming, fallbacks
- **event-index.test.ts** (9) — incremental load, malformed lines,
  partial-write tolerance, multi-file, new-file pickup, triggered_by
  preservation
- **search.test.ts** (11) — substring AND-of-terms, top_n, tool_filter,
  case-insensitivity, auto-fallback when embedder missing, explicit
  semantic raises without embedder, automatic refresh
- **introspect.test.ts** (12) — canonical schema id, command_id
  matches Python reference, MCP↔introspect drift check, exit-code
  table shape, JSON round-trip
- **server.test.ts** (8) — handleRecallCall happy path + error paths,
  format renderer
- **smoke-mcp.test.ts** (1) — end-to-end SDK client spawns the
  compiled CLI as a subprocess, drives full initialize → tools/list →
  tools/call. Proves the MCP wire format works.

Run them:

```bash
npm test
```

## Build + publish

```bash
npm install
npm run build       # tsc → dist/
npm test            # all 49 tests
npm publish --access public   # publishes @korg/recall-mcp
```

The `bin` field in package.json registers `korg-recall-mcp` as the
shell command. After global install (or `npx`), the binary is on PATH.

## Status

- v0.1.0 — substring-only path is solid (zero deps, fully tested).
  Semantic path is wired but the transformers.js install is left as
  the user's choice via `npm install @xenova/transformers` (or
  `npm install @korg/recall-mcp` will pull it as an optionalDependency).
- Not yet:
  - **Persistent embeddings cache to disk.** Embeddings recompute on
    cold start. For multi-thousand-event ledgers, a sidecar cache
    file would speed it up.
  - **Streaming recall results.** Today `tools/call` returns one text
    blob. A future version could chunk into multiple content items.

## License

MIT.
