# korg-introspect-mcp

**The bridge that makes any `--introspect`-aware binary callable from Claude Code.**

One MCP server, any binary in the ecosystem. Adding a new tool to the
agent surface is now: implement `--introspect` (Capabilities document)
on your binary → register `korg-introspect-mcp <your-binary>` in
`~/.claude.json` → done. Claude Code now has every callable as a typed MCP tool.

## What it does

1. At startup, runs `<binary> --introspect` and parses the `korg:introspect@v1` document.
2. Registers one MCP tool per `Callable`, using the introspect `input_schema` directly as the MCP `inputSchema` (no re-encoding).
3. When the agent calls a tool, the bridge:
   - maps the MCP arguments to CLI argv (kebab-case long flags, bool flag-on-true, arrays repeat the flag, command_id segments become subcommand path),
   - execs the binary,
   - formats stdout per the declared `output_mode` (envelope JSON parsed + pretty-printed, stream returned raw, session refused as unsupported in v1, none collapsed to "ok"),
   - honors `capabilities.side_effects` — refuses `fs_write` / `network` / `ledger_write` invocations unless explicitly allowed.

## Install

```bash
pip install -e ./adapters/introspect-mcp
```

## Register with Claude Code

Add one entry per binary you want to expose:

```json
{
  "mcpServers": {
    "korg-thump": {
      "command": "korg-introspect-mcp",
      "args": ["thump"],
      "env": {"KORG_INTROSPECT_MCP_ALLOW": "fs_write"}
    },
    "korg-binary": {
      "command": "korg-introspect-mcp",
      "args": ["korg"],
      "env": {"KORG_INTROSPECT_MCP_ALLOW": "all"}
    },
    "korg-korgex": {
      "command": "korg-introspect-mcp",
      "args": ["korgex"]
    }
  }
}
```

Restart Claude Code. **30+ tools** from across the ecosystem are now
in the agent's toolset. Tool names match the `command_id` from each
binary's introspect document (`thump.generate`, `korg.rewind`,
`korgex.agent`, …) — same identifier that the recall MCP server
returns when finding past sessions, so the recall→re-execute loop is
deterministic.

## Safety: side-effects gating

By default only `none` and `fs_read` callables can be invoked. Everything
else (file writes, network, ledger writes) requires explicit opt-in via
`KORG_INTROSPECT_MCP_ALLOW`:

| value | allows |
|---|---|
| (unset) | `none`, `fs_read` (safe default) |
| `fs_write` | + file writes |
| `network` | + network access |
| `ledger_write` | + writing to the korg ledger |
| `fs_write,network,ledger_write` | full access (combine) |
| `all` or `*` | everything |

When the agent calls a denied tool, the response explains exactly which
env var to set. **The default is more conservative than running the
binary directly via Bash** — strictly safer than the baseline.

## Subcommand inspection

For sanity-checking what an agent will see without launching the server:

```bash
korg-introspect-mcp thump --list-tools
```

Prints the binary's full callable list with their declared capabilities
and a `(would be denied)` flag for any that the current policy blocks.

## Argv mapping conventions

The bridge converts MCP arguments to CLI argv with this fixed convention
(matches clap + argparse with kebab-case long flags, which the entire korg
ecosystem uses):

| MCP argument | CLI argv |
|---|---|
| `"query": "x"` | `--query x` |
| `"top_n": 5` | `--top-n 5` *(snake→kebab)* |
| `"quiet": true` | `--quiet` *(flag only)* |
| `"quiet": false` | *(omitted)* |
| `"tags": ["a", "b"]` | `--tags a --tags b` |
| `"path": "/x"` (with `command_id: "thump.bun.script.run"`) | `bun script run --path /x` |

The `command_id` is split on `.` and the segments after the binary name
become the subcommand path. This works without ANY per-binary
configuration — every callable across thumper, korg, korgex follows the
same convention.

If a binary deviates (e.g. positional args, `--no-foo` for booleans,
single-dash flags), the right fix is on the binary side. Keep this
mapper boring.

## What's deliberately NOT supported in v1

- **`output_mode: session`** — long-lived stateful sessions over stdio MCP need
  bidirectional persistent I/O, which a one-shot `tools/call` doesn't fit.
  Agents that try to call a session-mode callable get a clear error explaining
  why. Run the binary directly for these.
- **Streaming progress notifications mid-call.** Stdout is buffered and
  returned at completion. MCP supports server→client notifications; a future
  version could stream chunks back, but the simple one-shot model covers
  90% of cases.
- **Auto-rerun on tool drift.** If `<binary> --introspect` changes between
  bridge invocations, you have to restart the MCP server to pick up the new
  schema. Easy add when needed.

## Tests

67 tests covering:
- **discovery** (14): binary lookup (absolute/relative/PATH), `--introspect` invocation, timeout, malformed JSON, schema mismatch, duplicate IDs, missing required fields, valid minimal document, end-to-end with a fixture binary.
- **args** (15): kebab conversion, bool/array/None handling, command_id subcommand-path splitting, naked binary (no subcommand), nested subcommand paths, dashed names not split, fallback for missing-prefix command_ids.
- **safety** (12): default deny, env-var parsing (single/comma/`all`/`*`/whitespace), case-insensitivity, factories, frozen dataclass, denial-message contents.
- **invoker** (8): envelope JSON pretty-print, kebab-case flag flow-through, array flag repetition, session-mode refused, non-zero exit reports stderr+code, timeout, binary path actually exec'd.
- **server** (18): full MCP protocol roundtrip — initialize, tools/list (carries inputSchema verbatim, tags capabilities in description), tools/call (echo, unknown tool, fail), policy denial (fs_write refused with helpful env-var hint), session refused even with full policy, unknown method / notification, ping, serve_stdio loop end-to-end.

Run them with the Korg workspace venv:

```bash
PYTHONPATH=src /path/to/Korg/.venv/bin/python3 -m pytest -q
```

## How this completes the loop

Together with the other pieces shipped today:

```
~/.claude/projects/**/*.jsonl       (Claude Code session files)
        ↓ korg-ingest-claude --tail (continuous capture)
~/.korg/claude-events.jsonl          (vendor-neutral ledger)
        ↓
        ├─ korg-recall-mcp           ← agent queries: "what did I do about X?"
        │   returns events with command_ids matching MCP tool names
        ↓
        └─ korg-introspect-mcp <binary>   ← agent calls those same command_ids
            execs the binary, returns the result
```

An agent can now: recall a prior session, find the moment that solved
the current problem, see exactly which `command_id` ran it, **and call
that command_id as an MCP tool to re-execute it on the current branch**.
Closed loop.

## License

MIT.
