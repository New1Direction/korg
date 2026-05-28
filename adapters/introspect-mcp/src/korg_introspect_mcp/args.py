"""Map MCP tool-call arguments to a CLI argv array.

Convention (default; matches clap / argparse with kebab-case long flags
across the korg ecosystem):

  Property type        →  CLI argv
  ─────────────────────────────────────────────────────────────
  string  "query"="x"  →  --query x
  number  "top_n"=5    →  --top-n 5         (underscore → hyphen)
  bool    "quiet"=True →  --quiet           (flag-only when true)
  bool    "quiet"=False→  (omitted)
  array   "tools"=[a,b]→  --tools a --tools b

Subcommand path:
  A `command_id` of "thump.bun.script.run" is split on `.` after the
  first segment (the binary name). The remaining segments become a
  subcommand path: ["bun", "script", "run"]. So the full argv is:
      ["thump", "bun", "script", "run", "--name", "x", ...]

The naked binary path (command_id == binary_name with no segments) gets
no subcommand:
      ["thump", "--query", "x"]

This convention is intentional: ALL korg-ecosystem binaries follow it
(clap + argparse with kebab-case long flags). If a binary diverges,
the fix is on the binary side, not here — keep this mapper boring.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any


def kebab(name: str) -> str:
    """`top_n` → `top-n`. Idempotent for already-kebab names."""
    return name.replace("_", "-")


def value_to_argv(value: Any) -> list[str]:
    """Convert a single scalar arg value to its CLI string form."""
    if isinstance(value, bool):
        # Bools are handled at the property level (flag-on-true) — they
        # shouldn't end up here. If they do, error.
        raise TypeError("bool should not reach value_to_argv; handle at property level")
    if isinstance(value, (int, float)):
        return [str(value)]
    if isinstance(value, str):
        return [value]
    if isinstance(value, Path):
        return [str(value)]
    # Fallback for objects: JSON-serialize. Rare but safer than crashing.
    import json as _json
    return [_json.dumps(value, default=str)]


def build_argv(
    binary_path: Path,
    command_id: str,
    binary_name: str,
    arguments: dict[str, Any],
) -> list[str]:
    """Construct the full argv for invoking `command_id` with `arguments`.

    `binary_path` is the absolute path to the executable.
    `binary_name` is the introspect document's declared binary name
    (e.g. "thump"). This is the first segment of every command_id.
    `command_id` is the callable's stable ID (e.g. "thump.generate").
    `arguments` is the MCP-supplied dict, already validated against the schema.
    """
    argv: list[str] = [str(binary_path)]

    # Subcommand path: split command_id after the binary segment.
    segments = command_id.split(".")
    if segments and segments[0] == binary_name:
        argv.extend(segments[1:])
    else:
        # command_id doesn't start with binary_name — treat the whole thing
        # as a subcommand path. Unusual but allowed.
        argv.extend(segments)

    for key, value in arguments.items():
        flag = "--" + kebab(key)
        if isinstance(value, bool):
            if value:
                argv.append(flag)
            # False booleans are omitted
            continue
        if isinstance(value, list):
            for item in value:
                argv.append(flag)
                argv.extend(value_to_argv(item))
            continue
        if value is None:
            continue
        argv.append(flag)
        argv.extend(value_to_argv(value))

    return argv
