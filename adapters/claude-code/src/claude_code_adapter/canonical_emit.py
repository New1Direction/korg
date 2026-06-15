"""make_canonical_emit — an EmitFn that writes verifiable korg-ledger@v1 events.

Drop-in replacement for make_jsonl_emit (tail.py): translates the adapter's
`body` dict into a LedgerWriter.append, returning the assigned global seq_id
so the adapter's triggered_by chaining works unchanged.
"""
from __future__ import annotations

import math
import uuid
from pathlib import Path
from typing import Any, Callable, Optional

from korg_ledger import CausalityError, LedgerWriter, agent_tool_call_event

EmitFn = Callable[[dict], Optional[int]]

_SAFE_INT_MAX = 2**53 - 1


def _canon_safe(value: Any) -> Any:
    """Coerce out-of-domain numbers so a tool arg can never crash the append.

    Integers beyond ±(2^53-1) (e.g. nanosecond timestamps, Snowflake IDs that an
    LLM may pass to a tool) are stringified — recorded faithfully and canon-safe,
    rather than aborting the whole hook firing. Finite floats are left as-is."""
    if isinstance(value, bool):
        return value
    if isinstance(value, int) and abs(value) > _SAFE_INT_MAX:
        return str(value)
    # NaN/Infinity (json.loads accepts these literals by default) would make the
    # canonical encoder raise — stringify so the event is recorded, not dropped.
    if isinstance(value, float) and not math.isfinite(value):
        return str(value)
    if isinstance(value, dict):
        return {k: _canon_safe(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_canon_safe(v) for v in value]
    return value


def make_canonical_emit(
    ledger_path: Path,
    *,
    actor_id: str = "korg:claude-hook",
    hmac_key: bytes | None = None,
    root_event_id: str | None = None,
) -> EmitFn:
    """Build an EmitFn appending to one per-session ledger file.

    `root_event_id` seeds the session root across short-lived hook firings;
    when None, the first emitted event becomes the root.
    """
    writer = LedgerWriter(ledger_path, hmac_key=hmac_key)
    state: dict[str, Any] = {"root": root_event_id}

    def emit(body: dict) -> Optional[int]:
        event_id = str(uuid.uuid4())
        root = state["root"] or event_id
        event = agent_tool_call_event(
            source_agent=body["source_agent"],
            tool_name=body["tool_name"],
            args=_canon_safe(body.get("args", {})),
            result=_canon_safe(body.get("result", {})),
            success=body.get("success", True),
            duration_ms=body.get("duration_ms", 0),
        )
        try:
            seq = writer.append(
                event=event,
                actor_id=actor_id,
                triggered_by=body.get("triggered_by"),
                root_event_id=root,
                event_id=event_id,
            )
        # CausalityError → bad triggered_by; ValueError → a value the canonical
        # encoder rejects. Drop the single event rather than aborting the firing
        # (which would leave the watermark unsaved and re-emit everything next time).
        except (CausalityError, ValueError):
            return None
        if state["root"] is None:
            state["root"] = event_id
        return seq

    # expose the (possibly newly-set) root so the hook can persist it
    emit.root_event_id = lambda: state["root"]  # type: ignore[attr-defined]
    return emit
