"""Minimal MCP (Model Context Protocol) JSON-RPC server over stdio.

We implement the protocol directly rather than depending on an SDK so the
package has no required dependencies beyond stdlib. The protocol surface
we cover:

  - initialize
  - notifications/initialized (no-op response)
  - tools/list
  - tools/call (one tool: `recall`)

Anything else returns a "method not found" error. That's enough for
Claude Code and any other MCP client that just wants to call recall.

Wire format: one JSON-RPC message per line on stdin, one response per
line on stdout. Diagnostics go to stderr so they don't corrupt the
protocol stream.
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass
from typing import Any, Callable

from korg_recall_mcp.index import EventIndex
from korg_recall_mcp.introspect import get_callables
from korg_recall_mcp.search import (
    DEFAULT_MIN_SCORE,
    DEFAULT_TOP_N,
    Match,
    Mode,
    RecallEngine,
)


PROTOCOL_VERSION = "2024-11-05"
SERVER_NAME = "korg-recall"
SERVER_VERSION = "0.1.0"


# ── Tool schema (sourced from the introspect registry) ────────────────


def _recall_tool_schema() -> dict[str, Any]:
    """The MCP tools/list entry for `recall`.

    Same source of truth as the `--introspect` document — see
    `korg_recall_mcp.introspect.get_callables()`. Changing the schema
    in one place updates both the MCP descriptor and the CLI
    introspection output.
    """
    for c in get_callables():
        if c.name == "recall":
            return c.to_mcp_tool()
    raise RuntimeError("recall callable missing from registry")


# ── Result formatting ─────────────────────────────────────────────────


def format_matches_for_llm(matches: list[Match], mode: str) -> str:
    """Render search results as a single text block for the MCP response."""
    if not matches:
        return f"[recall · {mode}] no relevant matches."
    lines = [f"[recall · {mode}] {len(matches)} match(es):"]
    for m in matches:
        ev = m.event
        agent_short = ev.source_agent.replace("agent:", "")[:40]
        snippet = ev.embed_text.replace("\n", " ")[:200]
        lines.append(
            f"  · seq={ev.seq} score={m.score:.2f} agent={agent_short} "
            f"tool={ev.tool_name} :: {snippet}"
        )
    return "\n".join(lines)


# ── Server ────────────────────────────────────────────────────────────


@dataclass
class Server:
    engine: RecallEngine

    def handle(self, message: dict[str, Any]) -> dict[str, Any] | None:
        """Process one JSON-RPC message. Returns the response, or None if
        the message was a notification (no response expected)."""
        method = message.get("method")
        msg_id = message.get("id")
        params = message.get("params") or {}

        # Notifications have no `id` and get no response.
        is_notification = "id" not in message

        if method == "initialize":
            return _ok(msg_id, {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION,
                },
            })

        if method == "notifications/initialized":
            return None

        if method == "tools/list":
            return _ok(msg_id, {"tools": [_recall_tool_schema()]})

        if method == "tools/call":
            name = params.get("name")
            args = params.get("arguments") or {}
            if name != "recall":
                return _err(msg_id, -32602, f"unknown tool: {name}")
            try:
                content = self._call_recall(args)
                return _ok(msg_id, {"content": [{"type": "text", "text": content}]})
            except Exception as e:
                return _err(msg_id, -32603, f"recall failed: {e}")

        if method == "ping":
            return _ok(msg_id, {})

        # Unknown method
        if is_notification:
            return None
        return _err(msg_id, -32601, f"method not found: {method}")

    def _call_recall(self, args: dict[str, Any]) -> str:
        query = str(args.get("query", "")).strip()
        if not query:
            return "[recall] empty query."
        top_n = int(args.get("top_n", DEFAULT_TOP_N))
        min_score = float(args.get("min_score", DEFAULT_MIN_SCORE))
        mode: Mode = args.get("mode", "auto")  # type: ignore[assignment]
        tool_filter = args.get("tool_filter")
        matches = self.engine.search(
            query,
            mode=mode,
            top_n=top_n,
            min_score=min_score,
            tool_filter=tool_filter,
        )
        return format_matches_for_llm(matches, self.engine.last_mode or mode)


def _ok(msg_id: Any, result: dict[str, Any]) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": msg_id, "result": result}


def _err(msg_id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}}


def serve_stdio(
    engine: RecallEngine,
    stdin=None,
    stdout=None,
    stderr=None,
) -> int:
    """Run the JSON-RPC loop against stdin/stdout. Returns when stdin closes."""
    stdin = stdin or sys.stdin
    stdout = stdout or sys.stdout
    stderr = stderr or sys.stderr
    server = Server(engine=engine)

    for raw in stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError as e:
            print(f"[korg-recall-mcp] bad json: {e}", file=stderr, flush=True)
            continue
        try:
            response = server.handle(msg)
        except Exception as e:
            response = _err(msg.get("id"), -32603, str(e))
        if response is not None:
            print(json.dumps(response), file=stdout, flush=True)
    return 0
