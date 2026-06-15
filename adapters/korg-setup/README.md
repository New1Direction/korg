# korg-setup

**One command to wire up the entire korg + Claude Code loop.**

Before: five steps and a config-file edit.
After: `pip install …; korg-setup`.

```bash
# One-time install of the three adapters (claude-code + recall-mcp + this):
pip install -e adapters/claude-code 'adapters/recall-mcp[semantic]' adapters/korg-setup

# One command sets everything up:
korg-setup
```

That's it. Restart Claude Code and you're done — every session is now
captured into a vendor-neutral ledger AND every prompt can call
`recall(query)` to find relevant moments from your entire AI history.

## What it does

`korg-setup` is an idempotent orchestrator that performs the five
manual steps you'd otherwise do yourself:

1. **Verifies** `korg-ingest-claude` and `korg-recall-mcp` are on `PATH`.
2. **Creates** `~/.korg/` (ledger + state directory).
3. **Registers** the `korg-recall` MCP server in `~/.claude.json` — atomic
   write with a `.korg-backup` copy of your prior config saved alongside.
4. **Installs** the `com.korg.ingest-claude` launchd agent at
   `~/Library/LaunchAgents/`, with stdout/stderr logged under
   `~/Library/Logs/`.
5. **Starts** the agent via `launchctl load -w`, so it runs at every
   login from here on out.

Every step is idempotent — re-running `korg-setup` after the first time
checks what's already done and only touches what's missing.

## Subcommands

```bash
korg-setup                  # run the full setup (prompts before changes)
korg-setup --yes            # non-interactive: skip the confirmation prompt
korg-setup --dry-run        # show exactly what would change, write nothing
korg-setup --no-daemon      # set up the MCP config but skip launchd

korg-setup status           # report what's installed and running
korg-setup uninstall        # stop launchd, remove ~/.claude.json entry
                            # (does NOT delete the ledger — that's yours)
```

`status` is a great first run too — read-only, tells you exactly where
each piece lives.

## Status report example

```
$ korg-setup status

Binaries:
  ✓ korg-ingest-claude     /Users/you/.venv/bin/korg-ingest-claude
  ✓ korg-recall-mcp        /Users/you/.venv/bin/korg-recall-mcp

Claude Code MCP registration:
  ✓ korg-recall registered with command /Users/you/.venv/bin/korg-recall-mcp

Ledger:
  ✓ /Users/you/.korg/claude-events.jsonl
      14,827 event(s), 6.4 MiB

Tail capture service:
  ✓ launchd agent com.korg.ingest-claude is RUNNING
```

## Configuration flags

| Flag | Default | Purpose |
|---|---|---|
| `--ledger-dir` | `~/.korg` | Where ledger + state files live. |
| `--ledger-file` | `~/.korg/claude-events.jsonl` | Path to the ledger JSONL. |
| `--claude-config` | `~/.claude.json` | Path to Claude Code's MCP config. |
| `--mcp-name` | `korg-recall` | Server name registered under `mcpServers`. |
| `--no-daemon` | `false` | Skip the launchd agent install. |
| `--yes` | `false` | Non-interactive. |
| `--dry-run` | `false` | Show what would change, write nothing. |

## Safety

This tool edits `~/.claude.json` — a file that carries your Claude Code
oauth state, project list, growthbook caches, and onboarding flags.
Three precautions apply:

- **Atomic writes via tmp-rename.** The config is never partially written.
- **Automatic backup.** Each edit copies the prior file to
  `~/.claude.json.korg-backup` first. Restore with `mv` if anything
  goes wrong.
- **Idempotent re-runs.** If the `korg-recall` entry is already
  present with the same spec, the file isn't touched at all.

`--dry-run` exercises the full code path without writing anything, so
you can preview every action before committing.

## Platform support

- **macOS:** full support, including launchd agent for auto-start.
- **Linux:** MCP config edit works; daemon install prints a one-liner
  for tmux / screen / systemd-user. (Native systemd-user support is a
  follow-up.)
- **Windows:** not currently supported.

## Tests

A test suite covers the `~/.claude.json` editor, launchd integration,
setup orchestrator, status reporter, discovery, bridge registration, and
Claude settings.

Run them with the Korg workspace venv:

```bash
cd adapters/korg-setup
PYTHONPATH=src /path/to/Korg/.venv/bin/python3 -m pytest -q
```

## Uninstall safety

`korg-setup uninstall`:

- Removes the `korg-recall` entry from `~/.claude.json` (with backup).
- Stops + deletes the launchd plist.
- **Does NOT touch your ledger** (`~/.korg/claude-events.jsonl`) — that
  represents real work and is yours to keep or delete.

If you really want to start over from scratch:

```bash
korg-setup uninstall --yes
rm -rf ~/.korg
```

## License

MIT.
