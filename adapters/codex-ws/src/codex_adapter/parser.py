"""
Codex WS frame parser → ordered NormalizedEvent stream.

Input shape: an iterable of frames, each a dict with:
  - "direction": "out" (client→server, i.e. response.create)
                 or "in" (server→client, streaming events)
  - "frame": the raw WS frame payload

The parser walks frames in arrival order and produces events with explicit
causal roles ("root", "llm_round", "tool_in_round"). The adapter consumes
these and chains triggered_by based on role.

Two Codex-specific subtleties:
  - apply_patch uses `custom_tool_call` with raw text in `input`, not JSON
    in `arguments` (see WS_PROTOCOL.md §3c).
  - Tool results arrive in the *next* response.create's `input` array as
    `function_call_output` / `custom_tool_call_output` keyed by call_id.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, Iterable, Literal


CausalRole = Literal["root", "llm_round", "tool_in_round"]


@dataclass
class NormalizedEvent:
    """One event ready for emission to korg's /api/agent/tool-call endpoint."""

    causal_role: CausalRole
    tool_name: str
    args: dict[str, Any] = field(default_factory=dict)
    result: dict[str, Any] = field(default_factory=dict)
    success: bool = True
    duration_ms: int = 0
    # Only set for tool_in_round events — used to match results from the
    # next response.create's input array.
    call_id: str | None = None


def _extract_new_user_message(input_array: list[dict]) -> str | None:
    """A response.create.input contains prior tool outputs + the new user message.
    Return the new user message text, or None if this is a tool-only continuation."""
    for item in input_array:
        if item.get("type") == "message" and item.get("role") == "user":
            for part in item.get("content", []):
                if part.get("type") == "input_text":
                    return part.get("text", "")
    return None


def _extract_tool_results(input_array: list[dict]) -> dict[str, dict]:
    """Pull tool results out of a response.create.input. Keyed by call_id."""
    results: dict[str, dict] = {}
    for item in input_array:
        t = item.get("type")
        if t in ("function_call_output", "custom_tool_call_output"):
            call_id = item.get("call_id")
            if call_id:
                results[call_id] = {"output": item.get("output", "")}
    return results


def _parse_tool_call(item: dict) -> NormalizedEvent | None:
    """Convert a completed response.output_item (function_call or custom_tool_call)
    into a NormalizedEvent. Returns None for non-tool items (messages, reasoning)."""
    t = item.get("type")
    if t == "function_call":
        raw_args = item.get("arguments", "") or "{}"
        try:
            args = json.loads(raw_args)
        except json.JSONDecodeError:
            args = {"_raw": raw_args}
        return NormalizedEvent(
            causal_role="tool_in_round",
            tool_name=item.get("name", "unknown"),
            args=args,
            call_id=item.get("call_id"),
        )
    if t == "custom_tool_call":
        # apply_patch and similar: freeform text in `input`, not JSON args
        return NormalizedEvent(
            causal_role="tool_in_round",
            tool_name=item.get("name", "unknown"),
            args={"input": item.get("input", "")},
            call_id=item.get("call_id"),
        )
    return None


def parse_session(frames: Iterable[dict]) -> list[NormalizedEvent]:
    """Walk Codex WS frames in arrival order, return events in causal emit order.

    Per turn, frames arrive in this order:
      response.create (client)
      response.output_item.done (one per tool call)
      response.completed (closes the turn)

    But the causal order for the ledger must be:
      llm_inference → tool calls (as siblings)

    So tool events are buffered until response.completed, then flushed after
    the llm_inference event. The first turn additionally emits a root
    user_prompt event from the response.create's new user message.
    """
    events: list[NormalizedEvent] = []
    # Tool events seen this turn — flushed after the turn's llm_inference
    current_turn_tools: list[NormalizedEvent] = []
    # Tool events keyed by call_id for back-attachment of results that arrive
    # in the *next* response.create's input array. Spans turns by design.
    pending_tool_calls: dict[str, NormalizedEvent] = {}
    seen_root = False

    for frame_wrapper in frames:
        direction = frame_wrapper.get("direction")
        frame = frame_wrapper.get("frame", {})

        if direction == "out" and frame.get("type") == "response.create":
            inp = frame.get("input", []) or []

            # Attach results from the prior turn's tool calls (matched by call_id).
            # The events themselves were already emitted; we mutate the result
            # field on the same dataclass instance via the pending map.
            for call_id, result in _extract_tool_results(inp).items():
                ev = pending_tool_calls.pop(call_id, None)
                if ev is not None:
                    ev.result = result
                    out = str(result.get("output", ""))
                    # Codex reports tool failures as "Exit code: N" where N != 0.
                    ev.success = (not out.startswith("Exit code: ")) or out.startswith(
                        "Exit code: 0"
                    )

            # First turn only: extract the user prompt and emit the root event.
            if not seen_root:
                prompt = _extract_new_user_message(inp)
                if prompt is not None:
                    events.append(
                        NormalizedEvent(
                            causal_role="root",
                            tool_name="user_prompt",
                            args={"prompt": prompt},
                        )
                    )
                    seen_root = True

        elif direction == "in" and frame.get("type") == "response.output_item.done":
            item = frame.get("item", {}) or {}
            ev = _parse_tool_call(item)
            if ev is not None:
                current_turn_tools.append(ev)
                if ev.call_id:
                    pending_tool_calls[ev.call_id] = ev

        elif direction == "in" and frame.get("type") == "response.completed":
            usage = frame.get("response", {}).get("usage", {}) or {}
            model = frame.get("response", {}).get("model", "unknown")
            events.append(
                NormalizedEvent(
                    causal_role="llm_round",
                    tool_name="llm_inference",
                    args={
                        "model": model,
                        "prompt_tokens": usage.get("input_tokens", 0),
                    },
                    result={
                        "completion_tokens": usage.get("output_tokens", 0),
                        "cached_tokens": (
                            usage.get("input_tokens_details", {}) or {}
                        ).get("cached_tokens", 0),
                    },
                )
            )
            # Flush buffered tool calls for this turn — siblings under the llm_inference
            events.extend(current_turn_tools)
            current_turn_tools = []

    # Anything left in current_turn_tools is from an incomplete final turn
    # (no response.completed seen). Append it so nothing is silently dropped.
    events.extend(current_turn_tools)

    return events
