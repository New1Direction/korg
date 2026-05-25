"""
Bun Harness — Stable Event Envelope + NDJSON Emitter

This module defines the canonical event format used across the entire
cli-anything-bun system (Python harness, future Rust frontend, ACP, TUI, etc.).

Design principles:
- One stable envelope for all events
- Explicit schema_version for evolution
- Clear separation of event stream vs final results
- First-class support for raw stdout/stderr passthrough
- Minimal dependencies (stdlib only)
"""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass, asdict, field
from datetime import datetime, timezone
from enum import Enum
from typing import Any, Optional, TextIO


# =============================================================================
# Constants
# =============================================================================

SCHEMA_VERSION: str = "1"


class EventLevel(str, Enum):
    """Log level for events. Used for filtering and routing."""
    DEBUG = "debug"
    INFO = "info"
    WARN = "warn"
    ERROR = "error"


# =============================================================================
# Core Event Envelope
# =============================================================================

@dataclass
class BunEvent:
    """
    Stable envelope for every event emitted by the Bun harness.

    All events (semantic + raw) go through this structure.
    This envelope is the contract between the harness and all consumers
    (REPL, ACP, TUI, Rust frontend, etc.).
    """

    # Required semantic fields
    event: str
    """Semantic event type, e.g. 'package.install.started', 'session.started'"""

    # Optional correlation fields
    session_id: Optional[str] = None
    """Links the event to a persistent session (e.g. a dev server)."""
    op_id: Optional[str] = None
    """Correlates events that belong to the same logical operation."""

    # Payload
    data: dict[str, Any] = field(default_factory=dict)
    """Typed payload. Keep it small and structured."""

    # Metadata
    level: EventLevel = EventLevel.INFO
    schema_version: str = SCHEMA_VERSION
    ts: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())

    def to_dict(self) -> dict[str, Any]:
        """Convert to a JSON-serializable dict with clean serialization rules."""
        d = asdict(self)

        # Always present
        d["level"] = self.level.value
        d["schema_version"] = self.schema_version

        # Drop null correlation fields for compactness (still valid)
        if self.session_id is None:
            d.pop("session_id", None)
        if self.op_id is None:
            d.pop("op_id", None)

        # Always include data (even if empty) for schema stability
        if "data" not in d:
            d["data"] = {}

        return d

    def to_json(self) -> str:
        """Compact single-line JSON suitable for NDJSON."""
        return json.dumps(self.to_dict(), separators=(",", ":"))

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> BunEvent:
        """Reconstruct from a dict (useful for testing and adapters)."""
        level = EventLevel(data.get("level", EventLevel.INFO))
        return cls(
            event=data["event"],
            session_id=data.get("session_id"),
            op_id=data.get("op_id"),
            data=data.get("data", {}),
            level=level,
            schema_version=data.get("schema_version", SCHEMA_VERSION),
            ts=data.get("ts", ""),
        )


# =============================================================================
# Emitter
# =============================================================================

def emit(
    event: str,
    data: dict[str, Any] | None = None,
    *,
    level: EventLevel = EventLevel.INFO,
    session_id: str | None = None,
    op_id: str | None = None,
    stream: TextIO | None = None,
) -> BunEvent:
    """
    Create and immediately emit a BunEvent as a single NDJSON line.

    This is the primary way most code will emit events.
    """
    if data is None:
        data = {}
    if stream is None:
        stream = sys.stdout

    ev = BunEvent(
        event=event,
        data=data,
        level=level,
        session_id=session_id,
        op_id=op_id,
    )
    print(ev.to_json(), file=stream, flush=True)
    return ev


def emit_event(event: BunEvent, stream: TextIO | None = None) -> None:
    """Emit an already-constructed BunEvent."""
    if stream is None:
        stream = sys.stdout
    print(event.to_json(), file=stream, flush=True)


# =============================================================================
# Convenience Helpers
# =============================================================================

def info(event: str, data: dict[str, Any] | None = None, **kwargs) -> BunEvent:
    return emit(event, data, level=EventLevel.INFO, **kwargs)


def warn(event: str, data: dict[str, Any] | None = None, **kwargs) -> BunEvent:
    return emit(event, data, level=EventLevel.WARN, **kwargs)


def error(event: str, data: dict[str, Any] | None = None, **kwargs) -> BunEvent:
    return emit(event, data, level=EventLevel.ERROR, **kwargs)


def debug(event: str, data: dict[str, Any] | None = None, **kwargs) -> BunEvent:
    return emit(event, data, level=EventLevel.DEBUG, **kwargs)


# =============================================================================
# Raw Output Events (Critical for debugging and future extraction)
# =============================================================================

def raw_stdout(
    line: str,
    *,
    session_id: str | None = None,
    op_id: str | None = None,
    stream: TextIO | None = None,
) -> BunEvent:
    """Emit a raw stdout line from a Bun process."""
    return emit(
        "process.stdout",
        {"stream": "stdout", "raw": line},
        level=EventLevel.DEBUG,
        session_id=session_id,
        op_id=op_id,
        stream=stream,
    )


def raw_stderr(
    line: str,
    *,
    session_id: str | None = None,
    op_id: str | None = None,
    stream: TextIO | None = None,
) -> BunEvent:
    """Emit a raw stderr line from a Bun process."""
    return emit(
        "process.stderr",
        {"stream": "stderr", "raw": line},
        level=EventLevel.WARN,
        session_id=session_id,
        op_id=op_id,
        stream=stream,
    )


# =============================================================================
# Result Emission (separate from event stream)
# =============================================================================

def emit_result(
    result: dict[str, Any],
    *,
    stream: TextIO | None = None,
) -> None:
    """
    Emit the final structured result of an operation.

    This is distinct from the event stream. Results are emitted once
    per operation and represent the complete outcome.
    """
    if stream is None:
        stream = sys.stdout
    print(json.dumps(result, separators=(",", ":")), file=stream, flush=True)


# =============================================================================
# Utilities
# =============================================================================

def event_from_json(line: str) -> BunEvent:
    """Parse a single NDJSON line back into a BunEvent (mainly for tests/adapters)."""
    data = json.loads(line)
    return BunEvent.from_dict(data)
