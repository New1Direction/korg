#!/usr/bin/env python3
"""
korg_mcp_server.py — korg MCP server v1

Exposes the korg ledger to any MCP-compatible agent (Claude Code, Codex,
korgex, etc.) over stdio transport using JSON-RPC 2.0.

Transport: stdio (newline-delimited JSON, one message per line).
Protocol:  Model Context Protocol (MCP) 2024-11-05.
Auth:      v1 local-only — trusts source_agent as provided (spec §7.7).

Tools exposed:
  korg_append_event   — write one AgentToolCall event, get back seq_id
  korg_query_events   — read events by tool_name or triggered_by

Design rules:
  - No cleverness. Garbage in → append to ledger → seq_id out.
  - No normalization across agents.
  - No auto-detection of event types.
  - source_agent is trusted as-is. Lying hurts the liar.
  - If korg is unreachable, return a clear error — don't silently swallow.
  - All errors are JSON-RPC error responses, never tracebacks to stdout.

Usage (in Claude Code's MCP config):
  {
    "mcpServers": {
      "korg": {
        "command": "python3",
        "args": ["/path/to/korg_mcp_server.py"],
        "env": {
          "KORG_URL": "http://localhost:8080"
        }
      }
    }
  }

Or run directly for testing:
  echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}' | python3 korg_mcp_server.py
"""

from __future__ import annotations

import json
import logging
import os
import sys
from typing import Any

import requests

# ---------------------------------------------------------------------------
# Logging — stderr only. stdout is the MCP transport.
# ---------------------------------------------------------------------------

logging.basicConfig(
    level=logging.DEBUG if os.environ.get("KORG_MCP_DEBUG") else logging.INFO,
    format="[korg-mcp] %(levelname)s %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

KORG_URL = os.environ.get("KORG_URL", "http://localhost:8080").rstrip("/")
HTTP_TIMEOUT = float(os.environ.get("KORG_MCP_TIMEOUT", "5"))

MCP_PROTOCOL_VERSION = "2024-11-05"
SERVER_NAME = "korg"
SERVER_VERSION = "1.0.0"

# ---------------------------------------------------------------------------
# Tool schemas
# ---------------------------------------------------------------------------

TOOLS = [
    {
        "name": "korg_append_event",
        "description": (
            "Append one AgentToolCall event to the korg audit ledger. "
            "Returns the assigned seq_id. Use seq_id as triggered_by on your next call "
            "to chain events into a causal tree.\n\n"
            "Causal chain rules (agent_event_spec.md §2):\n"
            "- Start each session with tool_name='user_prompt' and no triggered_by.\n"
            "- Set triggered_by to the seq_id of the event that caused this call.\n"
            "- Parallel tool calls from the same LLM response share triggered_by (siblings).\n"
            "- Retry events point at the failure event, not the original call.\n"
            "- Internal tool composition is not ledgered — only decision-boundary calls.\n\n"
            "Actor identity convention:\n"
            "  agent:<name>@<version>  (e.g. agent:claude-code@1.0)\n"
            "  human:<identifier>      (e.g. human:dusk)\n"
            "  korg:<component>        (korg internal events only)\n"
            "  mcp:<server-name>       (MCP server clients)"
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "source_agent": {
                    "type": "string",
                    "description": "Agent identity. Use agent:<name>@<version> convention.",
                },
                "tool_name": {
                    "type": "string",
                    "description": (
                        "Name of the tool or event type. "
                        "Use 'user_prompt' for session roots, 'llm_inference' for LLM calls, "
                        "or the actual tool name (Edit, Read, Bash, etc.)."
                    ),
                },
                "args": {
                    "type": "object",
                    "description": (
                        "Tool arguments as a JSON object. "
                        "Values >1KB should be content-referenced — see payload_refs."
                    ),
                },
                "result": {
                    "type": "object",
                    "description": (
                        "Tool result as a JSON object. "
                        "Values >1KB should be content-referenced — see payload_refs."
                    ),
                },
                "success": {
                    "type": "boolean",
                    "description": "Whether the tool call succeeded.",
                },
                "duration_ms": {
                    "type": "integer",
                    "description": "Wall-clock duration of the tool call in milliseconds.",
                },
                "triggered_by": {
                    "type": "integer",
                    "description": (
                        "seq_id of the parent event in the causal chain. "
                        "Omit for root events (user_prompt). Required for all other events."
                    ),
                },
                "payload_refs": {
                    "type": "array",
                    "description": (
                        "Content-addressed references for large payloads (>1KB). "
                        "Each ref: {sha256, size_bytes, label}. "
                        "Blobs must be written to .korg/blobs/<sha256[:2]>/<sha256> before "
                        "calling append (blob-first atomicity — spec §3)."
                    ),
                    "items": {
                        "type": "object",
                        "properties": {
                            "sha256": {"type": "string"},
                            "size_bytes": {"type": "integer"},
                            "label": {"type": "string"},
                        },
                        "required": ["sha256", "size_bytes"],
                    },
                    "default": [],
                },
            },
            "required": ["source_agent", "tool_name", "args", "result", "success", "duration_ms"],
        },
    },
    {
        "name": "korg_query_events",
        "description": (
            "Query recent AgentToolCall events from the korg ledger. "
            "Use to find a session's root seq_id, walk the causal chain, "
            "or check what events exist before appending.\n\n"
            "Filters are applied client-side over the last N events. "
            "For large ledgers this is O(n) — avoid in tight loops."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of events to return (default 20, max 200).",
                    "default": 20,
                },
                "tool_name": {
                    "type": "string",
                    "description": "Filter: only return events with this tool_name.",
                },
                "triggered_by": {
                    "type": "integer",
                    "description": "Filter: only return events triggered by this seq_id (forward walk).",
                },
                "source_agent": {
                    "type": "string",
                    "description": "Filter: only return events from this source_agent.",
                },
            },
            "required": [],
        },
    },
]


# ---------------------------------------------------------------------------
# korg HTTP client
# ---------------------------------------------------------------------------

def _korg_append(body: dict) -> dict:
    """POST to korg's ingestion endpoint. Returns the response JSON."""
    resp = requests.post(
        f"{KORG_URL}/api/agent/tool-call",
        json=body,
        timeout=HTTP_TIMEOUT,
    )
    resp.raise_for_status()
    return resp.json()


def _korg_journal(limit: int) -> list[dict]:
    """Fetch the last `limit` events from korg's journal endpoint."""
    resp = requests.get(
        f"{KORG_URL}/api/journal",
        timeout=HTTP_TIMEOUT,
    )
    resp.raise_for_status()

    events: list[dict] = []
    for line in resp.text.splitlines():
        line = line.strip()
        if not line or line.startswith("//"):
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue

    # journal returns oldest-first; return the last `limit` in reverse (newest first)
    return list(reversed(events[-limit:]))


# ---------------------------------------------------------------------------
# Tool handlers
# ---------------------------------------------------------------------------

def handle_korg_append_event(arguments: dict) -> dict:
    """
    Translate an MCP tool call into a POST /api/agent/tool-call request.

    No cleverness: forward args exactly as received, return seq_id.
    Validation is done by the JSON schema above; anything that gets here
    should have the required fields.
    """
    # Build the HTTP body. Only include triggered_by if it was provided.
    body: dict[str, Any] = {
        "source_agent": arguments["source_agent"],
        "tool_name": arguments["tool_name"],
        "args": arguments.get("args", {}),
        "result": arguments.get("result", {}),
        "payload_refs": arguments.get("payload_refs", []),
        "success": arguments["success"],
        "duration_ms": arguments["duration_ms"],
    }

    if "triggered_by" in arguments and arguments["triggered_by"] is not None:
        body["triggered_by"] = int(arguments["triggered_by"])

    logger.debug("append_event: tool=%s triggered_by=%s", body["tool_name"], body.get("triggered_by"))

    result = _korg_append(body)
    seq_id: int = result["seq_id"]

    logger.info("appended seq=%d tool=%s agent=%s", seq_id, body["tool_name"], body["source_agent"])

    return {
        "seq_id": seq_id,
        "message": (
            f"Event recorded at seq={seq_id}. "
            f"Use triggered_by={seq_id} on your next event to continue the causal chain."
        ),
    }


def handle_korg_query_events(arguments: dict) -> dict:
    """
    Fetch and filter events from the korg journal.

    Filters are applied client-side. This is intentionally simple for v1.
    """
    raw_limit = arguments.get("limit", 20)
    limit = max(1, min(int(raw_limit), 200))

    # Fetch from korg (returns up to 100 by default; we cap at 200 and filter client-side)
    fetch_limit = min(limit * 4, 200)  # over-fetch to allow for filtering
    all_events = _korg_journal(fetch_limit)

    # Filter to AgentToolCall events only
    agent_events = [
        e for e in all_events
        if e.get("event", {}).get("event_type") == "AgentToolCall"
    ]

    # Apply filters
    tool_name_filter = arguments.get("tool_name")
    triggered_by_filter = arguments.get("triggered_by")
    source_agent_filter = arguments.get("source_agent")

    filtered = agent_events
    if tool_name_filter:
        filtered = [e for e in filtered if e.get("event", {}).get("tool_name") == tool_name_filter]
    if triggered_by_filter is not None:
        tb = int(triggered_by_filter)
        filtered = [e for e in filtered if e.get("metadata", {}).get("triggered_by") == tb]
    if source_agent_filter:
        filtered = [e for e in filtered if e.get("event", {}).get("source_agent") == source_agent_filter]

    # Trim to requested limit
    filtered = filtered[:limit]

    # Return a compact, readable summary
    summary = []
    for e in filtered:
        ev = e.get("event", {})
        md = e.get("metadata", {})
        summary.append({
            "seq_id": e.get("seq_id"),
            "tool_name": ev.get("tool_name"),
            "source_agent": ev.get("source_agent"),
            "success": ev.get("success"),
            "duration_ms": ev.get("duration_ms"),
            "triggered_by": md.get("triggered_by"),
            "schema_version": e.get("schema_version"),
        })

    return {
        "count": len(summary),
        "events": summary,
        "note": (
            "Showing newest-first. Use triggered_by=<seq_id> to find all children "
            "of an event (forward walk). Use tool_name='user_prompt' to find session roots."
        ),
    }


# ---------------------------------------------------------------------------
# MCP protocol dispatch
# ---------------------------------------------------------------------------

def handle_initialize(params: dict, req_id: Any) -> dict:
    client_version = params.get("protocolVersion", "?")
    logger.info("initialize from %s (protocol %s)", params.get("clientInfo", {}).get("name", "?"), client_version)
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {},
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
            },
        },
    }


def handle_tools_list(req_id: Any) -> dict:
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {"tools": TOOLS},
    }


def handle_tools_call(params: dict, req_id: Any) -> dict:
    name = params.get("name")
    arguments = params.get("arguments", {})

    logger.debug("tools/call: %s", name)

    try:
        if name == "korg_append_event":
            data = handle_korg_append_event(arguments)
        elif name == "korg_query_events":
            data = handle_korg_query_events(arguments)
        else:
            return _error(req_id, -32601, f"Unknown tool: {name!r}")

        return {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps(data, indent=2),
                    }
                ],
                "isError": False,
            },
        }

    except requests.ConnectionError:
        return _tool_error(req_id, f"korg is not reachable at {KORG_URL}. Start korg with: cargo run -- --web")
    except requests.HTTPError as e:
        return _tool_error(req_id, f"korg returned HTTP {e.response.status_code}: {e.response.text[:200]}")
    except Exception as e:
        logger.exception("Unexpected error in %s", name)
        return _tool_error(req_id, f"Internal error: {type(e).__name__}: {e}")


def handle_ping(req_id: Any) -> dict:
    return {"jsonrpc": "2.0", "id": req_id, "result": {}}


def _error(req_id: Any, code: int, message: str) -> dict:
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "error": {"code": code, "message": message},
    }


def _tool_error(req_id: Any, message: str) -> dict:
    """Return a tool-level error (isError=True in the result content)."""
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {
            "content": [{"type": "text", "text": f"Error: {message}"}],
            "isError": True,
        },
    }


# ---------------------------------------------------------------------------
# Main stdio loop
# ---------------------------------------------------------------------------

def process_message(raw: str) -> dict | None:
    """Parse and dispatch one JSON-RPC message. Returns response dict or None for notifications."""
    try:
        msg = json.loads(raw)
    except json.JSONDecodeError as e:
        return _error(None, -32700, f"Parse error: {e}")

    method = msg.get("method", "")
    req_id = msg.get("id")  # None for notifications
    params = msg.get("params", {})

    # Notifications (no id) — ack with nothing
    if req_id is None:
        if method == "notifications/initialized":
            logger.info("client initialized")
        elif method == "notifications/cancelled":
            logger.debug("client cancelled request")
        else:
            logger.debug("unhandled notification: %s", method)
        return None

    # Requests
    if method == "initialize":
        return handle_initialize(params, req_id)
    elif method == "tools/list":
        return handle_tools_list(req_id)
    elif method == "tools/call":
        return handle_tools_call(params, req_id)
    elif method == "ping":
        return handle_ping(req_id)
    else:
        return _error(req_id, -32601, f"Method not found: {method!r}")


def main() -> None:
    logger.info("korg MCP server v%s starting (korg at %s)", SERVER_VERSION, KORG_URL)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        logger.debug("← %s", line[:200])

        response = process_message(line)
        if response is not None:
            out = json.dumps(response, separators=(",", ":"))
            logger.debug("→ %s", out[:200])
            sys.stdout.write(out + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
