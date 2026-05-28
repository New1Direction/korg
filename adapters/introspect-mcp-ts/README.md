# @korg/introspect-mcp

> **Bridge any `--introspect`-aware CLI to Claude Code via MCP. TypeScript edition.**

One MCP server, any binary in the ecosystem. After installing once,
adding a new tool surface to Claude Code's agent is **just one line of
config per binary**. The TS port of `korg-introspect-mcp` — same wire
contract, same safety gating, same `command_id` conventions as the
Python reference, distributed via npm for `npx` install.

```bash
npx -y @korg/introspect-mcp thump --list-tools
# [korg-introspect-mcp] thump v0.2.0 — 9 tool(s):
#   thump.tui            side_effects=fs_read   ...
#   thump.generate       side_effects=fs_write  ... (would be denied)
#   ...
```

## Why a TypeScript port

The Python `korg-introspect-mcp` works perfectly — but it requires
`pipx install` + a Python venv. For Claude Code (a TypeScript app)
users on Mac/Linux, the canonical "install a new MCP server" idiom is
`npx -y @some/package`. This TS port closes that distribution gap.

The Python version stays as the reference implementation. Both ports
emit the same MCP wire format, accept the same args, and produce the
same tool results.

## Register with Claude Code

Add one entry per binary you want to expose:

```json
{
  "mcpServers": {
    "korg-thump": {
      "command": "npx",
      "args": ["-y", "@korg/introspect-mcp", "thump"],
      "env": { "KORG_INTROSPECT_MCP_ALLOW": "fs_write" }
    },
    "korg": {
      "command": "npx",
      "args": ["-y", "@korg/introspect-mcp", "/usr/local/bin/korg"],
      "env": { "KORG_INTROSPECT_MCP_ALLOW": "all" }
    },
    "korg-korgex": {
      "command": "npx",
      "args": ["-y", "@korg/introspect-mcp", "korgex"]
    }
  }
}
```

Restart Claude Code. **30+ tools across the entire korg ecosystem**
land in the agent's toolset. Tool names match each binary's
`command_id` from its `--introspect` document, so the
recall→re-invoke loop (when paired with `@korg/recall-mcp`) is
deterministic.

## What it does

1. At startup, runs `<binary> --introspect` and parses the
   `korg:introspect@v1` document.
2. Registers one MCP tool per `Callable`, using the introspect
   `input_schema` directly as the MCP `inputSchema` (no re-encoding).
3. When the agent calls a tool, the bridge:
   - maps MCP arguments to CLI argv (kebab-case long flags,
     bool flag-on-true, arrays repeat the flag, command_id segments
     become subcommand path),
   - execs the binary as a subprocess,
   - formats stdout per the declared `output_mode` (envelope JSON
     parsed + pretty-printed, stream returned raw, session refused
     as unsupported in v1, none → "ok"),
   - honors `capabilities.side_effects` — refuses
     `fs_write` / `network` / `ledger_write` invocations unless
     explicitly allowed.

## Safety gate

By default only `none` and `fs_read` callables are invocable.
Everything else requires explicit opt-in via the
`KORG_INTROSPECT_MCP_ALLOW` env var (set in the MCP server's
`env` block):

| Value | Allows |
|---|---|
| (unset) | `none`, `fs_read` (safe default) |
| `fs_write` | + file writes |
| `network` | + network access |
| `ledger_write` | + writing to the korg ledger |
| `fs_write,network,ledger_write` | full access (combine) |
| `all` or `*` | everything |

When the agent calls a denied tool, the response explains exactly
which env var to set. **The default is stricter than running the
binary directly via Bash** — defense in depth for MCP-driven
invocations.

## Argv mapping convention

| MCP argument | CLI argv |
|---|---|
| `"query": "x"` | `--query x` |
| `"top_n": 5` | `--top-n 5` |
| `"quiet": true` | `--quiet` |
| `"quiet": false` | *(omitted)* |
| `"tags": ["a", "b"]` | `--tags a --tags b` |
| `command_id: "thump.bun.script.run"` | `bun script run` as subcommand path |

This convention matches clap + argparse with kebab-case long flags,
which the entire korg ecosystem uses. If a binary deviates, the right
fix is on the binary side — keep this mapper boring.

## CLI usage

```bash
# Sanity-check what an agent will see (no MCP server launched):
npx -y @korg/introspect-mcp thump --list-tools

# Run as MCP server on stdio (default — what Claude Code calls):
npx -y @korg/introspect-mcp thump --allow fs_write

# Pass an absolute path or PATH name:
npx -y @korg/introspect-mcp /usr/local/bin/korgex --allow all
```

## What's deliberately NOT supported in v1

- **`output_mode: session`** — long-lived stateful sessions over stdio
  MCP need persistent bidirectional I/O. Agents that try to call a
  session-mode callable get a clear error explaining why.
- **Streaming progress mid-call.** Stdout is buffered and returned at
  completion. A v2 could stream chunks back via MCP notifications.
- **Auto-rerun on tool drift.** If `<binary> --introspect` changes
  between bridge invocations, restart the MCP server to pick up
  the new schema.

## Tests

56 tests covering:
- **args.test.ts** (15) — kebab conversion, bool/array/null handling,
  command_id subcommand-path splitting, naked binary, nested
  subcommand paths, dashed names not split, fallback for
  missing-prefix command_ids.
- **safety.test.ts** (12) — default deny, env-var parsing
  (single/comma/`all`/`*`/whitespace), case-insensitivity, factories,
  denial-message contents.
- **discovery.test.ts** (10) — binary lookup (absolute/PATH),
  `--introspect` invocation, malformed JSON, schema mismatch,
  duplicate IDs, missing required fields, valid minimal document,
  end-to-end with a fixture binary.
- **invoker.test.ts** (6) — envelope JSON pretty-print, kebab-case
  flag flow-through, array flag repetition, session-mode refused,
  non-zero exit reports stderr+code, timeout.
- **server.test.ts** (6) — `buildToolsList` produces correct MCP
  shape, tags capabilities in description, passes input_schema
  verbatim, end-to-end SDK client → compiled CLI → fixture binary
  roundtrip (init + list + call), default policy refuses fs_write
  with a helpful env-var hint.
- **_sanity.test.ts** (1) — fixture binary spawns correctly.

Run them:
```bash
npm test
```

## Architecture

```
src/
├── discovery.ts   # exec <binary> --introspect, validate doc
├── args.ts        # MCP arguments → CLI argv (uniform convention)
├── safety.ts      # Policy: side_effects allow-list
├── invoker.ts     # exec the binary, format by output_mode
├── server.ts      # MCP wiring via @modelcontextprotocol/sdk
├── cli.ts         # CLI entry: <binary> [--allow] [--list-tools]
└── index.ts       # public API for programmatic use
```

Zero required runtime dependencies beyond `@modelcontextprotocol/sdk`.
Pure stdlib for everything else.

## License

MIT.
