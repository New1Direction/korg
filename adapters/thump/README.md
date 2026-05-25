# thump (Thumper Bun harness)

**Semantic harness for Bun** — a clean, observable, agent-native layer over the Bun JavaScript runtime.

Part of the Thumper project (`thump` binary). The Rust side now provides the preferred native execution path.

This Python package provides a structured, event-driven interface for running Bun commands (`bun run`, `bun add`, `bun install`, `bun remove`, etc.) while emitting a stable NDJSON event stream.

It is designed to be used from:
- Command line
- TUI / IDE integrations
- AI agents (via ACP / stdio)

## Features

- **Stable event envelope** — every operation emits structured events (`*.started`, `process.stdout`, `process.exited`, etc.) with `op_id` / `session_id` correlation.
- **Raw output preservation** — stdout/stderr from Bun is never lost.
- **Operation abstraction** — clean success/error paths with normalized error model.
- **Live process streaming** — subprocess execution with real-time event emission.
- **First-class commands**:
  - `script run`
  - `package add` / `install` / `remove`
- Works great from Rust, Python, or directly on the CLI.

## Installation (development)

```bash
git clone https://github.com/redmicro/api-anything.git
cd api-anything/thump   # or pip install -e . from the thump/ directory
pip install -e .
```

Or run directly:

```bash
python -m thump --help
```

## Usage

### From Python

```python
from thump import script_run, package_add

result = script_run("dev", cwd="my-project")
result = package_add(["hono", "zod"], cwd="my-project", dev=True)
```

### From CLI

```bash
python -m thump script run dev
python -m thump package add hono --dev
python -m thump package install
```

### Streaming events

All commands emit NDJSON to stdout. Each line is either:

- A structured event (with `event`, `op_id`, `data`, etc.)
- A final result object (`{"ok": true, "operation": "..."}`)

This makes it trivial to consume from Rust, Go, another Python process, or an agent.

## Architecture

```
Bun CLI
   ↓
thump (Python semantic layer)
   ↓
Stable NDJSON event stream + final results
   ↓
Consumers: CLI • TUI Jobs • ACP agents • Rust adapter
```

## Related Projects

- [thump](https://github.com/redmicro/api-anything) — The Rust TUI/CLI (Thumper) — primary binary with native Bun execution.

## Status

This is the Python-first implementation (Phase 1) as described in the design document.  
A native Rust implementation of the same protocol is planned for later.

## License

MIT
