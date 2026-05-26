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

import base64
import json
import logging
import os
import sys
import threading
import time as _time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable
from urllib.parse import parse_qs, urlparse

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

# v1 ledger ID — only valid value per agent_event_spec.md §8.1.
# Multi-tenancy in v2+ will expand the valid set.
LEDGER_ID = "local"

# Filesystem path to the spec document. Used by korg://local/schema/spec.
# Resolves relative to this file's location so the server works from any cwd.
SPEC_PATH = Path(__file__).parent / "agent_event_spec.md"

# Maximum blob size served through MCP JSON-RPC (§8.4.2).
# Larger blobs return blob_too_large with the HTTP URL as the escape hatch.
BLOB_MAX_BYTES = 10 * 1024 * 1024  # 10MB

# Background poller interval (§8.5.1). Subscriptions fire within ~POLL_INTERVAL seconds.
POLL_INTERVAL: float = float(os.environ.get("KORG_MCP_POLL_INTERVAL", "1.0"))

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
            "- Internal tool composition is not ledgered — only decision-boundary calls.\n"
            "- For LLM rounds, see §2a: round-N's llm_inference chains to round-(N-1)'s\n"
            "  llm_inference, NOT to the most recent tool call.\n\n"
            "Actor identity convention (§1.1):\n"
            "  agent:<name>@<version>  (e.g. agent:claude-code@1.0)\n"
            "  human:<identifier>      (e.g. human:dusk)\n"
            "  korg:<component>        (korg internal events only)\n"
            "  mcp:<server-name>       (MCP server clients)"
        ),
        "annotations": {
            "title": "Append AgentToolCall event",
            "readOnlyHint": False,
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
        },
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
            "For large ledgers this is O(n) — avoid in tight loops "
            "(agent_event_spec.md §6.6)."
        ),
        "annotations": {
            "title": "Query Korg ledger events",
            "readOnlyHint": True,
            "openWorldHint": False,
        },
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


def _korg_blob(sha256: str) -> tuple[bytes, str]:
    """Fetch a blob from korg's blob endpoint. Returns (raw_bytes, content_type)."""
    resp = requests.get(
        f"{KORG_URL}/api/blob/{sha256}",
        timeout=HTTP_TIMEOUT,
    )
    resp.raise_for_status()
    content_type = resp.headers.get("content-type", "application/octet-stream")
    return resp.content, content_type


# ---------------------------------------------------------------------------
# Session walk helper
# ---------------------------------------------------------------------------

def _session_event_ids(all_events: list[dict], root_seq: int) -> set[int]:
    """BFS from root_seq through triggered_by edges. Returns all seq_ids in the session."""
    children: dict[int, list[int]] = {}
    for e in all_events:
        tb = e.get("metadata", {}).get("triggered_by")
        if tb is not None:
            children.setdefault(tb, []).append(e["seq_id"])

    # Check visited *before* enqueuing so duplicate children entries (which
    # can appear when the same event is observed in multiple polls during a
    # cascading root re-assignment) don't balloon the queue. The post-dequeue
    # check is kept as defense in depth.
    visited: set[int] = {root_seq}
    queue = [root_seq]
    while queue:
        seq = queue.pop(0)
        for child in children.get(seq, []):
            if child in visited:
                continue
            visited.add(child)
            queue.append(child)
    return visited


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
                # Resources capability per agent_event_spec.md §8.
                # subscribe: true as of Phase D — poller thread + subscription registry.
                # listChanged: false because the fixed resource list is static in v1.
                "resources": {"subscribe": True, "listChanged": False},
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
# Resources — agent_event_spec.md §8
# ---------------------------------------------------------------------------
#
# Phase A: 5 fixed resources, no subscriptions, no session/event/agent/blob
# templates (those land in Phase B/C). Resource URIs follow §8.1:
#     korg://{ledger}/<path>
# where {ledger} == LEDGER_ID ("local") in v1.

# Fixed resources advertised via resources/list. URIs are concrete, no
# template substitution required. (Templates land in Phase B.)
RESOURCES = [
    {
        "uri": f"korg://{LEDGER_ID}/ledger/recent",
        "name": "Recent ledger events",
        "description": "Most recent N events across all agents, newest-first. Paginated via ?cursor=<seq_id>&limit=<n>.",
        "mimeType": "application/json",
    },
    {
        "uri": f"korg://{LEDGER_ID}/ledger/heads",
        "name": "Session roots (active and recent)",
        "description": "List of root user_prompt seq_ids — entry points for session reads.",
        "mimeType": "application/json",
    },
    {
        "uri": f"korg://{LEDGER_ID}/schema/event",
        "name": "AgentToolCall JSON Schema",
        "description": "Machine-readable schema for one ledger event (agent_event_spec.md §1).",
        "mimeType": "application/json",
    },
    {
        "uri": f"korg://{LEDGER_ID}/schema/spec",
        "name": "Agent event spec",
        "description": "Full agent_event_spec.md — schema, causal rules, rewind, dogfood checklist.",
        "mimeType": "text/markdown",
    },
    {
        "uri": f"korg://{LEDGER_ID}/stats/integrity",
        "name": "Ledger integrity snapshot",
        "description": "Quick health check: event counts, actor convention violations, schema_version distribution. Run korg_dogfood.py for the full §6 checklist.",
        "mimeType": "application/json",
    },
]

# Resource templates for URI patterns where the path includes parameters.
# Phase B: session, event, agent. Phase C adds the blob template.
# The list is the published advertisement of which URI shapes Korg supports.
RESOURCE_TEMPLATES: list[dict] = [
    {
        "uriTemplate": f"korg://{LEDGER_ID}/session/{{root_seq}}",
        "name": "Session metadata",
        "description": (
            "Bounded metadata for one session: root event, event count, agents, "
            "first/last seq_id. §8.3."
        ),
        "mimeType": "application/json",
    },
    {
        "uriTemplate": f"korg://{LEDGER_ID}/session/{{root_seq}}/summary",
        "name": "Session structural skeleton",
        "description": (
            "Paginated lightweight summary of session events, oldest→newest (§8.3). "
            "?cursor=<seq_id>&limit=<n>&source_agent=<id>."
        ),
        "mimeType": "application/json",
    },
    {
        "uriTemplate": f"korg://{LEDGER_ID}/session/{{root_seq}}/events",
        "name": "Session full events",
        "description": (
            "Paginated full event bodies for a session, oldest→newest (§8.3). "
            "?cursor=<seq_id>&limit=<n>&source_agent=<id>."
        ),
        "mimeType": "application/json",
    },
    {
        "uriTemplate": f"korg://{LEDGER_ID}/event/{{seq_id}}",
        "name": "Single event",
        "description": "Full journal envelope for one event by seq_id.",
        "mimeType": "application/json",
    },
    {
        "uriTemplate": f"korg://{LEDGER_ID}/agent/{{source_agent}}/recent",
        "name": "Agent recent events",
        "description": (
            "Recent events from one agent across all sessions, newest→oldest (§8.3.1). "
            "?cursor=<seq_id>&limit=<n>."
        ),
        "mimeType": "application/json",
    },
    {
        "uriTemplate": f"korg://{LEDGER_ID}/blob/{{sha256}}",
        "name": "Content-addressed blob",
        "description": (
            "One blob by sha256. Max 10MB over JSON-RPC; larger blobs return "
            "blob_too_large error with http_url escape hatch (§8.4)."
        ),
        "mimeType": "application/octet-stream",
    },
]


# The AgentToolCall JSON Schema, served at korg://local/schema/event.
# Derived from agent_event_spec.md §1.
EVENT_JSON_SCHEMA = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": f"korg://{LEDGER_ID}/schema/event",
    "title": "AgentToolCall",
    "description": "One event in the korg ledger. See agent_event_spec.md §1.",
    "type": "object",
    "properties": {
        "source_agent": {
            "type": "string",
            "description": "Actor identity per §1.1: agent:<name>@<ver>, human:<id>, korg:<comp>, mcp:<server>.",
            "pattern": "^(agent|human|korg|mcp):",
        },
        "tool_name": {
            "type": "string",
            "description": "Verbatim tool name; no normalization across agents.",
        },
        "args": {
            "type": "object",
            "description": "Tool arguments. Values >1KB content-referenced per §7.3.",
        },
        "result": {
            "type": "object",
            "description": "Tool output. Values >1KB content-referenced per §7.3.",
        },
        "success": {"type": "boolean"},
        "duration_ms": {"type": "integer", "minimum": 0},
        "triggered_by": {
            "type": ["integer", "null"],
            "description": "seq_id of parent event; null for roots (§2).",
        },
        "payload_refs": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                    "size_bytes": {"type": "integer", "minimum": 0},
                    "label": {"type": "string"},
                },
                "required": ["sha256", "size_bytes"],
            },
        },
    },
    "required": ["source_agent", "tool_name", "args", "result", "success", "duration_ms"],
    "additionalProperties": False,
}


# ---------------------------------------------------------------------------
# URI parser — §8.1
# ---------------------------------------------------------------------------

class _UriError(Exception):
    """Raised for malformed or unsupported korg:// URIs.

    reason  — stable string reason code placed in the JSON-RPC error data dict.
    message — human-readable description for the error message field.
    code    — JSON-RPC error code (default -32602 invalid params; -32603 for
              transport-limit errors like blob_too_large).
    extra   — additional key/value pairs merged into the error data dict
              (e.g. sha256, size_bytes, http_url for blob_too_large).
    """

    def __init__(
        self,
        reason: str,
        message: str,
        code: int = -32602,
        extra: dict | None = None,
    ) -> None:
        super().__init__(message)
        self.reason = reason
        self.message = message
        self.code = code
        self.extra: dict = extra or {}


def _parse_korg_uri(uri: str) -> tuple[list[str], dict[str, str]]:
    """Parse a korg:// URI. Returns (path_parts, query_dict).

    Raises _UriError on:
      - Wrong scheme (not korg://)
      - Missing or unsupported ledger ID (v1: only "local")
      - Empty path
    """
    parsed = urlparse(uri)
    if parsed.scheme != "korg":
        raise _UriError("bad_scheme", f"URI scheme must be korg://, got {parsed.scheme!r}")
    if not parsed.netloc:
        raise _UriError("missing_ledger", "URI must include a ledger ID: korg://{ledger}/...")
    if parsed.netloc != LEDGER_ID:
        raise _UriError(
            "unknown_ledger",
            f"Unknown ledger {parsed.netloc!r}; v1 supports only {LEDGER_ID!r} (§8.1).",
        )

    path = parsed.path.strip("/")
    if not path:
        raise _UriError("empty_path", "URI must include a resource path after the ledger ID")
    parts = path.split("/")

    query = {k: v[0] for k, v in parse_qs(parsed.query).items() if v}
    return parts, query


# ---------------------------------------------------------------------------
# Fixed resource handlers
# ---------------------------------------------------------------------------

def _resource_ledger_recent(query: dict[str, str]) -> dict:
    """korg://local/ledger/recent — paginated recent events."""
    limit = max(1, min(int(query.get("limit", "50")), 500))
    cursor = query.get("cursor")
    events = _korg_journal(limit * 4)  # over-fetch for cursor filtering

    # Cursor: return events with seq_id < cursor (i.e., older than cursor).
    if cursor is not None:
        cursor_seq = int(cursor)
        events = [e for e in events if e.get("seq_id", 0) < cursor_seq]

    # Already newest-first from _korg_journal; trim to limit
    events = events[:limit]

    next_cursor = events[-1]["seq_id"] if len(events) == limit else None
    return {
        "events": events,
        "next_cursor": next_cursor,
        "has_more": next_cursor is not None,
    }


def _resource_ledger_heads(query: dict[str, str]) -> dict:
    """korg://local/ledger/heads — list of root user_prompt seq_ids."""
    limit = max(1, min(int(query.get("limit", "50")), 200))
    # Over-fetch enough to find root events
    raw = _korg_journal(500)
    roots = [
        {
            "seq_id": e["seq_id"],
            "source_agent": e.get("event", {}).get("source_agent"),
            "prompt_preview": str(
                e.get("event", {}).get("args", {}).get("prompt", "")
            )[:120],
        }
        for e in raw
        if e.get("event", {}).get("event_type") == "AgentToolCall"
        and e.get("event", {}).get("tool_name") == "user_prompt"
        and e.get("metadata", {}).get("triggered_by") is None
    ][:limit]
    return {"heads": roots, "count": len(roots)}


def _resource_schema_event(_query: dict[str, str]) -> dict:
    """korg://local/schema/event — the AgentToolCall JSON Schema."""
    return EVENT_JSON_SCHEMA


def _resource_schema_spec(_query: dict[str, str]) -> str:
    """korg://local/schema/spec — the spec document. Returns text, not JSON."""
    if not SPEC_PATH.exists():
        raise _UriError(
            "spec_not_found",
            f"Spec file missing at {SPEC_PATH}. Ensure agent_event_spec.md is co-located with mcp_server.py.",
        )
    return SPEC_PATH.read_text(encoding="utf-8")


def _resource_stats_integrity(_query: dict[str, str]) -> dict:
    """korg://local/stats/integrity — quick health snapshot.

    Phase A: counts + actor convention check. Full §6 dogfood checks are
    heavy (causal chain walks, blob existence) and live in korg_dogfood.py.
    """
    raw = _korg_journal(500)  # bounded — don't scan the whole world
    agent_events = [
        e for e in raw
        if e.get("event", {}).get("event_type") == "AgentToolCall"
    ]

    # §1.1 actor convention prefixes
    valid_prefixes = ("agent:", "human:", "korg:", "mcp:")
    actor_counts: dict[str, int] = {}
    violations: list[str] = []
    schema_versions: dict[str, int] = {}
    roots = 0

    for e in agent_events:
        ev = e.get("event", {})
        src = ev.get("source_agent", "")
        actor_counts[src] = actor_counts.get(src, 0) + 1
        if not any(src.startswith(p) for p in valid_prefixes) and src not in violations:
            violations.append(src)

        sv = e.get("schema_version", "MISSING")
        schema_versions[sv] = schema_versions.get(sv, 0) + 1

        if (
            ev.get("tool_name") == "user_prompt"
            and e.get("metadata", {}).get("triggered_by") is None
        ):
            roots += 1

    return {
        "sample_size": len(agent_events),
        "sample_note": "Stats computed over the last 500 ledger events.",
        "total_agents_seen": len(actor_counts),
        "source_agent_counts": actor_counts,
        "schema_version_distribution": schema_versions,
        "root_sessions_in_sample": roots,
        "actor_convention_violations": violations,
        "actor_convention_ok": len(violations) == 0,
        "note": "For the full §6 dogfood checklist, run scripts/korg_dogfood.py.",
    }


# ---------------------------------------------------------------------------
# Phase B resource handlers — variable-segment URIs (§8.3)
# ---------------------------------------------------------------------------

def _resource_session_meta(root_seq: int, query: dict[str, str]) -> dict:
    """korg://local/session/{root_seq} — bounded session metadata."""
    all_events = _korg_journal(10000)
    by_seq = {e["seq_id"]: e for e in all_events}

    if root_seq not in by_seq:
        raise _UriError("not_found", f"No event at seq_id={root_seq}")

    session_seqs = _session_event_ids(all_events, root_seq)
    session_events = sorted(
        [by_seq[s] for s in session_seqs if s in by_seq],
        key=lambda e: e["seq_id"],
    )

    agents = sorted({e.get("event", {}).get("source_agent") for e in session_events} - {None})
    last_event = session_events[-1] if session_events else by_seq[root_seq]

    return {
        "root_seq": root_seq,
        "root_event": by_seq[root_seq],
        "total_events": len(session_events),
        "agent_count": len(agents),
        "agents": agents,
        "first_seq": session_events[0]["seq_id"] if session_events else root_seq,
        "last_seq": last_event["seq_id"],
        "last_event_at": last_event.get("metadata", {}).get("recorded_at"),
        "last_event_seq": last_event["seq_id"],
        "schema_version": by_seq[root_seq].get("schema_version", "1.0"),
    }


def _resource_session_summary(root_seq: int, query: dict[str, str]) -> dict:
    """korg://local/session/{root_seq}/summary — paginated skeleton, oldest→newest."""
    limit = max(1, min(int(query.get("limit", "100")), 1000))
    cursor = int(query.get("cursor", "0"))
    source_agent_filter = query.get("source_agent")

    all_events = _korg_journal(10000)
    by_seq = {e["seq_id"]: e for e in all_events}

    if root_seq not in by_seq:
        raise _UriError("not_found", f"No event at seq_id={root_seq}")

    session_seqs = _session_event_ids(all_events, root_seq)
    session_events = sorted(
        [by_seq[s] for s in session_seqs if s in by_seq],
        key=lambda e: e["seq_id"],
    )

    # head surface: cursor=N → return events with seq_id > N (oldest→newest)
    if cursor > 0:
        session_events = [e for e in session_events if e["seq_id"] > cursor]

    if source_agent_filter:
        session_events = [
            e for e in session_events
            if e.get("event", {}).get("source_agent") == source_agent_filter
        ]

    page = session_events[:limit]
    has_more = len(session_events) > limit
    next_cursor = page[-1]["seq_id"] if has_more else None

    skeleton = [
        {
            "seq_id": e["seq_id"],
            "source_agent": e.get("event", {}).get("source_agent"),
            "tool_name": e.get("event", {}).get("tool_name"),
            "triggered_by": e.get("metadata", {}).get("triggered_by"),
            "success": e.get("event", {}).get("success"),
            "duration_ms": e.get("event", {}).get("duration_ms"),
            "has_payload_refs": bool(e.get("metadata", {}).get("payload_refs")),
        }
        for e in page
    ]

    return {"events": skeleton, "next_cursor": next_cursor, "has_more": has_more}


def _resource_session_events(root_seq: int, query: dict[str, str]) -> dict:
    """korg://local/session/{root_seq}/events — paginated full bodies, oldest→newest."""
    limit = max(1, min(int(query.get("limit", "50")), 500))
    cursor = int(query.get("cursor", "0"))
    source_agent_filter = query.get("source_agent")

    all_events = _korg_journal(10000)
    by_seq = {e["seq_id"]: e for e in all_events}

    if root_seq not in by_seq:
        raise _UriError("not_found", f"No event at seq_id={root_seq}")

    session_seqs = _session_event_ids(all_events, root_seq)
    session_events = sorted(
        [by_seq[s] for s in session_seqs if s in by_seq],
        key=lambda e: e["seq_id"],
    )

    if cursor > 0:
        session_events = [e for e in session_events if e["seq_id"] > cursor]

    if source_agent_filter:
        session_events = [
            e for e in session_events
            if e.get("event", {}).get("source_agent") == source_agent_filter
        ]

    page = session_events[:limit]
    has_more = len(session_events) > limit
    next_cursor = page[-1]["seq_id"] if has_more else None

    return {"events": page, "next_cursor": next_cursor, "has_more": has_more}


def _resource_event_read(seq_id: int, query: dict[str, str]) -> dict:
    """korg://local/event/{seq_id} — single event full body."""
    all_events = _korg_journal(10000)
    by_seq = {e["seq_id"]: e for e in all_events}

    if seq_id not in by_seq:
        raise _UriError("not_found", f"No event at seq_id={seq_id}")
    return by_seq[seq_id]


def _resource_agent_recent(source_agent: str, query: dict[str, str]) -> dict:
    """korg://local/agent/{source_agent}/recent — newest→oldest, cursor paginated."""
    limit = max(1, min(int(query.get("limit", "50")), 500))
    cursor = query.get("cursor")

    all_events = _korg_journal(limit * 8)  # over-fetch before agent filter
    filtered = [
        e for e in all_events
        if e.get("event", {}).get("source_agent") == source_agent
    ]

    # tail surface: cursor=N → return events with seq_id < N (newest→oldest)
    if cursor is not None:
        cursor_seq = int(cursor)
        filtered = [e for e in filtered if e.get("seq_id", 0) < cursor_seq]

    page = filtered[:limit]
    has_more = len(filtered) > limit
    next_cursor = page[-1]["seq_id"] if has_more else None

    return {
        "source_agent": source_agent,
        "events": page,
        "next_cursor": next_cursor,
        "has_more": has_more,
    }


# ---------------------------------------------------------------------------
# Phase D — Subscription engine (§8.5)
# ---------------------------------------------------------------------------

@dataclass
class Subscription:
    uri: str
    predicate: Callable[[dict], bool]


# Registry: uri → list[Subscription]. Guarded by _subscription_lock.
_subscriptions: dict[str, list[Subscription]] = {}
_subscription_lock = threading.Lock()

# seq_id → root_seq_id lookup (§8.5.2). Built at startup + extended per tick.
# Guarded by _seq_to_root_lock.
_seq_to_root: dict[int, int] = {}
# Events whose parent (triggered_by) was outside the bootstrap window when we
# saw them. parent_seq -> [child_seq, ...]. When the parent's root is finally
# resolved (or the parent itself arrives), walk these children to fill in their
# roots. Without this, the previous fallback `_seq_to_root.get(tb, seq)` would
# silently route orphans into the wrong session.
_pending_root_resolve: dict[int, list[int]] = {}
_seq_to_root_lock = threading.Lock()

# Highest seq_id the poller has processed. Events with seq > this are "new".
# Written only by the poller thread (no lock needed for the int assignment itself,
# but reads in the predicate table use it only for initialization, not hot-path).
_last_seen_seq: int = 0

# All stdout writes — responses (main thread) and notifications (poller) —
# acquire this lock to prevent interleaved JSON-RPC frames (§8.5.1).
_stdout_lock = threading.Lock()


def _send_notification(msg: dict) -> None:
    """Write a notification to stdout under the stdout lock."""
    out = json.dumps(msg, separators=(",", ":"))
    with _stdout_lock:
        sys.stdout.write(out + "\n")
        sys.stdout.flush()


def _update_seq_to_root(events: list[dict]) -> None:
    """Extend the seq_id→root_seq_id table. Events must be processed oldest-first.

    Events whose parent isn't yet in the table (e.g. parent fell outside the
    bootstrap fetch window) get parked in _pending_root_resolve. When the
    parent's root is later resolved we walk the pending list and assign the
    same root to every descendant. Critically, we never write a wrong root —
    the old fallback `_seq_to_root.get(tb, seq)` made orphans into self-roots,
    which silently misrouted them into their own faux-session.
    """
    sorted_evs = sorted(events, key=lambda e: e.get("seq_id", 0))
    with _seq_to_root_lock:
        for e in sorted_evs:
            seq = e.get("seq_id")
            if seq is None:
                continue
            tb = e.get("metadata", {}).get("triggered_by")
            if tb is None:
                _seq_to_root[seq] = seq
                _resolve_pending_children(seq, seq)
            else:
                root = _seq_to_root.get(tb)
                if root is not None:
                    _seq_to_root[seq] = root
                    _resolve_pending_children(seq, root)
                else:
                    # Parent unknown — defer until it shows up.
                    _pending_root_resolve.setdefault(tb, []).append(seq)


def _resolve_pending_children(parent_seq: int, root_seq: int) -> None:
    """Caller must hold _seq_to_root_lock. Drains _pending_root_resolve[parent_seq]
    and recursively resolves grandchildren too."""
    stack = [(parent_seq, root_seq)]
    while stack:
        anc, root = stack.pop()
        children = _pending_root_resolve.pop(anc, [])
        for child in children:
            _seq_to_root[child] = root
            # Grandchildren may have been parked waiting on this child.
            stack.append((child, root))


def _compile_predicate(uri: str) -> Callable[[dict], bool]:
    """Compile a O(1) subscription predicate for `uri`. Raises _UriError if
    the URI is valid but not subscribable."""
    parsed = urlparse(uri)
    path_parts = parsed.path.strip("/").split("/")

    if path_parts == ["ledger", "recent"]:
        return lambda _e: True

    if path_parts == ["ledger", "heads"]:
        return lambda e: (
            e.get("event", {}).get("tool_name") == "user_prompt"
            and e.get("metadata", {}).get("triggered_by") is None
        )

    if (
        len(path_parts) == 3
        and path_parts[0] == "session"
        and path_parts[2] == "summary"
    ):
        try:
            root_seq = int(path_parts[1])
        except ValueError:
            raise _UriError("not_subscribable", f"Invalid session seq_id in {uri!r}")

        def _session_pred(event: dict, r: int = root_seq) -> bool:
            seq = event.get("seq_id")
            if seq is None:
                return False
            with _seq_to_root_lock:
                return _seq_to_root.get(seq) == r

        return _session_pred

    if (
        len(path_parts) == 3
        and path_parts[0] == "agent"
        and path_parts[2] == "recent"
    ):
        agent = path_parts[1]
        return lambda e, a=agent: e.get("event", {}).get("source_agent") == a

    raise _UriError(
        "not_subscribable",
        f"{uri!r} does not support subscriptions. "
        f"Subscribable: ledger/recent, ledger/heads, session/{{root}}/summary, "
        f"agent/{{id}}/recent (§8.5.2).",
    )


def _dispatch_notifications(new_events: list[dict]) -> None:
    """Fire notifications for every subscription whose predicate matches any new event.
    At most one notification per subscribed URI per call (deduplication across events).
    Predicate exceptions are caught, logged, and skipped — they never kill the loop."""
    if not new_events:
        return

    with _subscription_lock:
        subs_snapshot = {uri: list(subs) for uri, subs in _subscriptions.items()}

    for uri, subs in subs_snapshot.items():
        fired = False
        for event in new_events:
            if fired:
                break
            for sub in subs:
                try:
                    if sub.predicate(event):
                        fired = True
                        break
                except Exception:
                    logger.exception(
                        "predicate exception for uri=%s seq=%s — skipping",
                        uri, event.get("seq_id", "?"),
                    )
        if fired:
            _send_notification({
                "jsonrpc": "2.0",
                "method": "notifications/resources/updated",
                "params": {"uri": uri},
            })


def _poller_loop() -> None:
    """Background daemon: polls the journal, maintains _seq_to_root, dispatches
    notifications. See §8.5.1 for failure-mode handling."""
    global _last_seen_seq

    # Startup: populate seq_to_root and set last_seen_seq from the current ledger.
    try:
        bootstrap = _korg_journal(10000)
        _update_seq_to_root(bootstrap)
        if bootstrap:
            _last_seen_seq = max(e["seq_id"] for e in bootstrap)
        logger.info("poller initialized, last_seen_seq=%d", _last_seen_seq)
    except Exception:
        logger.warning("poller: failed to initialize from journal, starting from seq=0")

    while True:
        _time.sleep(POLL_INTERVAL)

        # — Failure mode 1: Korg unreachable —
        try:
            events = _korg_journal(10000)
        except requests.ConnectionError:
            logger.debug("poller: korg unreachable, retrying")
            continue
        # — Failure mode 2: malformed response —
        except json.JSONDecodeError:
            logger.warning("poller: korg returned malformed JSON")
            continue
        except Exception:
            logger.exception("poller: unexpected error fetching journal")
            continue

        # Skip the seq_to_root update on empty / all-stale batches. The
        # function is idempotent on already-known events, but reprocessing
        # them on every tick is wasted work and obscures the warn-once log
        # we get on real changes.
        if events:
            _update_seq_to_root(events)

        new_events = [e for e in events if e.get("seq_id", 0) > _last_seen_seq]
        if not new_events:
            continue

        _last_seen_seq = max(e["seq_id"] for e in new_events)

        # — Failure mode 3: predicate exceptions — handled inside _dispatch_notifications
        _dispatch_notifications(new_events)


def _start_poller() -> None:
    """Launch the background poller daemon thread (started once from main())."""
    t = threading.Thread(target=_poller_loop, name="korg-poller", daemon=True)
    t.start()
    logger.info("background poller started (interval=%.1fs)", POLL_INTERVAL)


def _resource_blob_read(sha256: str, query: dict[str, str]) -> tuple[Any, str]:
    """korg://local/blob/{sha256} — one content-addressed blob (§8.4).

    Returns (data, mime_type) where data is str for text blobs, bytes for binary.
    Raises _UriError("blob_too_large", ..., code=-32603) when over BLOB_MAX_BYTES.
    """
    if len(sha256) != 64 or not all(c in "0123456789abcdefABCDEF" for c in sha256):
        raise _UriError("bad_sha256", f"sha256 must be 64 hex characters, got {sha256!r}")

    try:
        raw_bytes, content_type = _korg_blob(sha256)
    except requests.HTTPError as e:
        if e.response.status_code == 404:
            raise _UriError("not_found", f"Blob {sha256} not found in korg")
        raise

    size_bytes = len(raw_bytes)
    if size_bytes > BLOB_MAX_BYTES:
        raise _UriError(
            "blob_too_large",
            f"Blob {sha256} is {size_bytes} bytes — exceeds {BLOB_MAX_BYTES // (1024 * 1024)}MB "
            f"JSON-RPC cap. Fetch directly from korg HTTP endpoint.",
            code=-32603,
            extra={
                "sha256": sha256,
                "size_bytes": size_bytes,
                "http_url": f"/api/blob/{sha256}",
            },
        )

    # §8.4.1: wrap per content type. Trust an explicit text/* or
    # application/json from the upstream. For octet-stream or missing types
    # (often "I don't know"), attempt JSON parse — but NOT a bare UTF-8
    # decode, since random binary frequently decodes as valid UTF-8 (e.g. a
    # base64 payload) and would otherwise be misclassified as text.
    ct_lower = content_type.lower() if isinstance(content_type, str) else ""
    if ct_lower.startswith("application/json"):
        try:
            return raw_bytes.decode("utf-8"), "application/json"
        except UnicodeDecodeError:
            return raw_bytes, content_type
    if ct_lower.startswith("text/"):
        try:
            return raw_bytes.decode("utf-8"), content_type
        except UnicodeDecodeError:
            return raw_bytes, content_type

    # Unknown / octet-stream upstream — sniff JSON specifically. We do NOT
    # fall back to "valid UTF-8 = text" because that's the misclassification
    # the audit flagged.
    if ct_lower in ("", "application/octet-stream"):
        try:
            text = raw_bytes.decode("utf-8")
            json.loads(text)
            return text, "application/json"
        except (UnicodeDecodeError, json.JSONDecodeError):
            pass

    # Binary — return raw bytes; handle_resources_read packs as MCP blob content.
    return raw_bytes, content_type or "application/octet-stream"


def _dispatch_variable_resource(
    parts: list[str], query: dict[str, str]
) -> tuple[Any, str]:
    """Route and execute a Phase B/C variable-segment resource. Returns (data, mime_type).

    Raises _UriError("no_such_resource", ...) if no pattern matches.
    """
    if parts[0] == "session" and len(parts) >= 2:
        try:
            root_seq = int(parts[1])
        except ValueError:
            raise _UriError(
                "bad_seq_id", f"session seq_id must be an integer, got {parts[1]!r}"
            )
        if len(parts) == 2:
            return _resource_session_meta(root_seq, query), "application/json"
        if len(parts) == 3 and parts[2] == "summary":
            return _resource_session_summary(root_seq, query), "application/json"
        if len(parts) == 3 and parts[2] == "events":
            return _resource_session_events(root_seq, query), "application/json"
        raise _UriError(
            "no_such_resource",
            f"Unknown session sub-resource {parts[2]!r}; valid: summary, events",
        )

    if parts[0] == "event" and len(parts) == 2:
        try:
            seq_id = int(parts[1])
        except ValueError:
            raise _UriError(
                "bad_seq_id", f"event seq_id must be an integer, got {parts[1]!r}"
            )
        return _resource_event_read(seq_id, query), "application/json"

    if parts[0] == "agent" and len(parts) == 3 and parts[2] == "recent":
        return _resource_agent_recent(parts[1], query), "application/json"

    if parts[0] == "blob" and len(parts) == 2:
        return _resource_blob_read(parts[1], query)

    raise _UriError(
        "no_such_resource",
        f"No resource matches korg://{LEDGER_ID}/{'/'.join(parts)}",
    )


# Routing table: path-parts tuple → (handler, mime_type)
# Phase A only routes the 5 fixed resources. Phase B adds session/event/agent.
_RESOURCE_ROUTES: dict[tuple[str, ...], tuple[Any, str]] = {
    ("ledger", "recent"): (_resource_ledger_recent, "application/json"),
    ("ledger", "heads"): (_resource_ledger_heads, "application/json"),
    ("schema", "event"): (_resource_schema_event, "application/json"),
    ("schema", "spec"): (_resource_schema_spec, "text/markdown"),
    ("stats", "integrity"): (_resource_stats_integrity, "application/json"),
}


# ---------------------------------------------------------------------------
# resources/* method handlers
# ---------------------------------------------------------------------------

def handle_resources_list(req_id: Any) -> dict:
    return {"jsonrpc": "2.0", "id": req_id, "result": {"resources": RESOURCES}}


def handle_resources_templates_list(req_id: Any) -> dict:
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {"resourceTemplates": RESOURCE_TEMPLATES},
    }


def handle_resources_read(params: dict, req_id: Any) -> dict:
    uri = params.get("uri", "")
    logger.debug("resources/read: %s", uri)

    try:
        parts, query = _parse_korg_uri(uri)
    except _UriError as e:
        return _error_with_data(req_id, -32602, e.message, {"reason": e.reason, "uri": uri})

    route = _RESOURCE_ROUTES.get(tuple(parts))

    try:
        if route is not None:
            handler, mime_type = route
            result = handler(query)
        else:
            # Variable-segment routes (Phase B): session/*, event/*, agent/*/recent.
            # _dispatch_variable_resource raises _UriError("no_such_resource", ...)
            # when nothing matches, so the error path below handles unknown URIs.
            result, mime_type = _dispatch_variable_resource(parts, query)
    except _UriError as e:
        data = {"reason": e.reason, "uri": uri, **e.extra}
        return _error_with_data(req_id, e.code, e.message, data)
    except requests.ConnectionError:
        return _error_with_data(
            req_id,
            -32603,
            f"korg unreachable at {KORG_URL}",
            {"reason": "korg_unreachable", "uri": uri},
        )
    except Exception as exc:
        logger.exception("Unexpected error in resources/read for %s", uri)
        return _error_with_data(
            req_id,
            -32603,
            f"Internal error: {type(exc).__name__}: {exc}",
            {"reason": "internal_error", "uri": uri},
        )

    # Pack the result into MCP's resource content shape.
    # Binary blobs (bytes) → MCP "blob" content with base64 data.
    # Text/JSON → MCP "text" content.
    if isinstance(result, bytes):
        content_item = {
            "uri": uri,
            "mimeType": mime_type,
            "blob": base64.b64encode(result).decode("ascii"),
        }
    else:
        text = result if isinstance(result, str) else json.dumps(result, indent=2)
        content_item = {
            "uri": uri,
            "mimeType": mime_type,
            "text": text,
        }
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "result": {"contents": [content_item]},
    }


def handle_resources_subscribe(params: dict, req_id: Any) -> dict:
    uri = params.get("uri", "")
    try:
        _parse_korg_uri(uri)
        predicate = _compile_predicate(uri)
    except _UriError as e:
        data = {"reason": e.reason, "uri": uri, **e.extra}
        return _error_with_data(req_id, e.code, e.message, data)

    with _subscription_lock:
        subs = _subscriptions.setdefault(uri, [])
        # Idempotent per §8.5.3 — no-op if already subscribed to this URI.
        if not subs:
            subs.append(Subscription(uri=uri, predicate=predicate))

    return {"jsonrpc": "2.0", "id": req_id, "result": {}}


def handle_resources_unsubscribe(params: dict, req_id: Any) -> dict:
    uri = params.get("uri", "")
    with _subscription_lock:
        _subscriptions.pop(uri, None)
    return {"jsonrpc": "2.0", "id": req_id, "result": {}}


def _error_with_data(req_id: Any, code: int, message: str, data: dict) -> dict:
    """JSON-RPC error response with a structured `data` field. Used by
    resources/read to carry a stable `reason` code per §8.6."""
    return {
        "jsonrpc": "2.0",
        "id": req_id,
        "error": {"code": code, "message": message, "data": data},
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
    elif method == "resources/list":
        return handle_resources_list(req_id)
    elif method == "resources/templates/list":
        return handle_resources_templates_list(req_id)
    elif method == "resources/read":
        return handle_resources_read(params, req_id)
    elif method == "resources/subscribe":
        return handle_resources_subscribe(params, req_id)
    elif method == "resources/unsubscribe":
        return handle_resources_unsubscribe(params, req_id)
    elif method == "ping":
        return handle_ping(req_id)
    else:
        return _error(req_id, -32601, f"Method not found: {method!r}")


def main() -> None:
    logger.info("korg MCP server v%s starting (korg at %s)", SERVER_VERSION, KORG_URL)
    _start_poller()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        logger.debug("← %s", line[:200])

        response = process_message(line)
        if response is not None:
            out = json.dumps(response, separators=(",", ":"))
            logger.debug("→ %s", out[:200])
            with _stdout_lock:
                sys.stdout.write(out + "\n")
                sys.stdout.flush()


if __name__ == "__main__":
    main()
