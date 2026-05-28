"""Invoke a discovered callable: build argv, exec, format output for MCP.

Output handling by declared `output_mode`:
  - "envelope"  → parse stdout as JSON, return as pretty-printed text
                  (the agent can parse it back). On parse failure, fall
                  back to raw text + a warning prefix.
  - "stream"    → return stdout as text (for streaming-NDJSON or
                  free-form output). The full output is buffered then
                  returned at once — MCP stdio doesn't support
                  intermediate progress for v1.
  - "session"   → unsupported for v1. Sessions need persistent
                  bidirectional I/O which MCP stdio doesn't fit
                  cleanly into a one-shot tool call.
  - "none"      → return a status string ("ok") + any stderr captured.

Exit code → MCP content:
  - 0 → success, return stdout
  - non-0 → return both stdout and stderr in the error body so the agent
    can debug.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from korg_introspect_mcp.args import build_argv
from korg_introspect_mcp.discovery import DiscoveredCallable


DEFAULT_TIMEOUT_S = 300.0  # 5 minutes. Long enough for most tools, short
                            # enough to fail if something hangs.
SESSION_NOT_SUPPORTED = (
    "[korg-introspect-mcp] this callable declares output_mode=session, "
    "which requires persistent bidirectional I/O. Not supported in v1 — "
    "run the binary directly for session-mode operations."
)


@dataclass
class InvocationResult:
    """The structured result of a single invocation."""

    text: str
    is_error: bool


def invoke(
    callable_def: DiscoveredCallable,
    arguments: dict[str, Any],
    *,
    binary_path: Path,
    binary_name: str,
    timeout_s: float = DEFAULT_TIMEOUT_S,
) -> InvocationResult:
    """Build argv, exec the binary, format stdout per output_mode."""
    output_mode = callable_def.capabilities.get("output_mode", "envelope")

    if output_mode == "session":
        return InvocationResult(text=SESSION_NOT_SUPPORTED, is_error=True)

    argv = build_argv(
        binary_path=binary_path,
        command_id=callable_def.command_id,
        binary_name=binary_name,
        arguments=arguments,
    )

    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=timeout_s,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return InvocationResult(
            text=f"[korg-introspect-mcp] {callable_def.command_id} timed out after {timeout_s}s",
            is_error=True,
        )
    except OSError as e:
        return InvocationResult(
            text=f"[korg-introspect-mcp] could not exec {binary_path}: {e}",
            is_error=True,
        )

    stdout = proc.stdout or ""
    stderr = proc.stderr or ""
    is_error = proc.returncode != 0

    # Format based on declared output mode.
    if output_mode == "envelope":
        # Try to parse + pretty-print; on failure, return raw.
        body = stdout.strip()
        if body:
            try:
                parsed = json.loads(body)
                body = json.dumps(parsed, indent=2)
            except json.JSONDecodeError:
                # Not valid JSON — return raw with a note.
                pass
        text = body
    elif output_mode == "stream":
        text = stdout
    elif output_mode == "none":
        text = stdout.strip() or "ok"
    else:
        # Unknown output_mode — be defensive and return raw stdout.
        text = stdout

    if is_error:
        # Append stderr + exit code so the agent can debug.
        suffix = []
        if stderr.strip():
            suffix.append(f"\n[stderr]\n{stderr.rstrip()}")
        suffix.append(f"\n[exit_code] {proc.returncode}")
        text = (text or "").rstrip() + "".join(suffix)

    return InvocationResult(text=text, is_error=is_error)
