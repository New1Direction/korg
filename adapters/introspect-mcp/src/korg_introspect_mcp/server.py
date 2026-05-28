"""Minimal MCP JSON-RPC server bridging `--introspect`-aware binaries.

Same wire format as korg-recall-mcp (stdio JSON-RPC, one message per
line). On startup we run `<binary> --introspect`, validate, and
register one MCP tool per discovered callable. Tool names use the
binary's `command_id` directly so cross-session recall finds them by
the same identifier.

Methods handled:
  - initialize
  - notifications/initialized
  - tools/list
  - tools/call
  - ping

Tool naming policy: the MCP tool name is the `command_id` itself
(e.g. `thump.generate`, `korg.rewind`). This makes the bridge
deterministic — agents that recall a prior session event with
`tool_name: thump.generate` can invoke the matching MCP tool by the
same name.
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass
from typing import Any

from korg_introspect_mcp.discovery import DiscoveredBinary
from korg_introspect_mcp.invoker import invoke
from korg_introspect_mcp.safety import Policy


PROTOCOL_VERSION = "2024-11-05"
SERVER_NAME = "korg-introspect-mcp"
SERVER_VERSION = "0.1.0"


def _ok(msg_id: Any, result: dict[str, Any]) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": msg_id, "result": result}


def _err(msg_id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}}


@dataclass
class Server:
    discovery: DiscoveredBinary
    policy: Policy

    # ── MCP tool descriptors ──────────────────────────────────────────

    def tools_list(self) -> list[dict[str, Any]]:
        """Emit one tools/list entry per discovered callable.

        Tool name = command_id. Description includes the side-effects and
        output-mode tags so the agent can reason about it before invoking.
        """
        tools: list[dict[str, Any]] = []
        for c in self.discovery.callables:
            cap = c.capabilities
            tags = (
                f"[side_effects: {cap.get('side_effects', 'unknown')}, "
                f"output_mode: {cap.get('output_mode', 'unknown')}, "
                f"long_running: {cap.get('long_running', False)}]"
            )
            description = (c.description or c.name) + " " + tags
            tools.append({
                "name": c.command_id,
                "description": description,
                "inputSchema": c.input_schema,
            })
        return tools

    # ── Dispatch ──────────────────────────────────────────────────────

    def handle(self, message: dict[str, Any]) -> dict[str, Any] | None:
        method = message.get("method")
        msg_id = message.get("id")
        params = message.get("params") or {}
        is_notification = "id" not in message

        if method == "initialize":
            return _ok(msg_id, {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": f"{SERVER_NAME}({self.discovery.binary_name})",
                    "version": SERVER_VERSION,
                },
            })

        if method == "notifications/initialized":
            return None

        if method == "tools/list":
            return _ok(msg_id, {"tools": self.tools_list()})

        if method == "tools/call":
            name = params.get("name")
            args = params.get("arguments") or {}
            callable_def = self.discovery.by_command_id(str(name))
            if callable_def is None:
                return _err(msg_id, -32602, f"unknown tool: {name}")

            side_effects = callable_def.capabilities.get("side_effects", "none")
            if not self.policy.allows(side_effects):
                return _ok(msg_id, {
                    "content": [{"type": "text", "text": self.policy.explain_denial(side_effects)}],
                    "isError": True,
                })

            try:
                result = invoke(
                    callable_def,
                    args,
                    binary_path=self.discovery.binary_path,
                    binary_name=self.discovery.binary_name,
                )
            except Exception as e:
                return _err(msg_id, -32603, f"invocation failed: {e}")

            return _ok(msg_id, {
                "content": [{"type": "text", "text": result.text}],
                "isError": result.is_error,
            })

        if method == "ping":
            return _ok(msg_id, {})

        if is_notification:
            return None
        return _err(msg_id, -32601, f"method not found: {method}")


def serve_stdio(
    discovery: DiscoveredBinary,
    policy: Policy,
    stdin=None,
    stdout=None,
    stderr=None,
) -> int:
    """Run the JSON-RPC loop until stdin closes."""
    stdin = stdin or sys.stdin
    stdout = stdout or sys.stdout
    stderr = stderr or sys.stderr
    server = Server(discovery=discovery, policy=policy)

    for raw in stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError as e:
            print(f"[korg-introspect-mcp] bad json: {e}", file=stderr, flush=True)
            continue
        try:
            response = server.handle(msg)
        except Exception as e:
            response = _err(msg.get("id"), -32603, str(e))
        if response is not None:
            print(json.dumps(response), file=stdout, flush=True)
    return 0
