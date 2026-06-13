"""make_canonical_emit — an EmitFn that writes verifiable korg-ledger@v1 events.

Drop-in replacement for make_jsonl_emit (tail.py): translates the adapter's
`body` dict into a LedgerWriter.append, returning the assigned global seq_id
so the adapter's triggered_by chaining works unchanged.
"""
from __future__ import annotations

import uuid
from pathlib import Path
from typing import Any, Callable, Optional

from korg_ledger import CausalityError, LedgerWriter, agent_tool_call_event

EmitFn = Callable[[dict], Optional[int]]


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
            args=body.get("args", {}),
            result=body.get("result", {}),
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
        except CausalityError:
            return None
        if state["root"] is None:
            state["root"] = event_id
        return seq

    # expose the (possibly newly-set) root so the hook can persist it
    emit.root_event_id = lambda: state["root"]  # type: ignore[attr-defined]
    return emit
