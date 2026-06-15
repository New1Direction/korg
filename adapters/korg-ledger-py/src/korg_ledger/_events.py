"""Builders for korg-ledger CapabilityEvent payloads (the `event` object)."""
from __future__ import annotations

from datetime import datetime, timezone

#: serde nil UUID, used for correlation_id / campaign_id on external events.
NIL_UUID = "00000000-0000-0000-0000-000000000000"


def _now_iso() -> str:
    # ISO-8601 UTC with a trailing Z, matching chrono's DateTime<Utc> output.
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def agent_tool_call_event(
    *,
    source_agent: str,
    tool_name: str,
    args: dict,
    result: dict,
    success: bool,
    duration_ms: int,
    timestamp: str | None = None,
) -> dict:
    """Build the `event` object for a CapabilityEvent::AgentToolCall record."""
    return {
        "event_type": "AgentToolCall",
        "source_agent": source_agent,
        "tool_name": tool_name,
        "args": args,
        "result": result,
        "payload_refs": [],
        "success": success,
        "duration_ms": duration_ms,
        "timestamp": timestamp or _now_iso(),
    }
