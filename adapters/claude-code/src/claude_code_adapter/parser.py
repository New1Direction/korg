"""
Claude Code session JSONL parser → ordered NormalizedEvent stream.

Input shape: an iterable of strings or pre-parsed dicts. Each line is one
event from a `~/.claude/projects/<dir>/<uuid>.jsonl` file. The two event
types that matter for causality are:

  - "user"        — a turn-message (plain text OR tool_result blocks)
  - "assistant"   — a model response (text + thinking + tool_use blocks)

Other types (system, attachment, file-history-snapshot, last-prompt,
permission-mode, ai-title, queue-operation) are metadata and are skipped.

The parser walks events in file order and produces a NormalizedEvent
stream with explicit causal roles:

  - "root"           — the very first user_prompt of the session
  - "user_followup"  — a subsequent user_prompt (plain-text user message
                       after a previous assistant turn)
  - "llm_round"      — one assistant message (one llm_inference event)
  - "tool_in_round"  — one tool_use block, sibling under its llm_round

Tool results attach back to the originating tool_use by `tool_use_id`,
mirroring the codex-ws adapter's call_id back-attachment pattern.

Known limitations:
  - Sidechains (`isSidechain: true`, Claude Code's Task tool sub-agents)
    are processed inline by file order. korg's single-parent
    `triggered_by` model can't preserve true sub-agent fan-out; the
    sidechain events become siblings of their invoking thread.
  - Thinking blocks are not emitted as separate events. They're
    metadata of an llm_inference.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, Iterable, Literal


CausalRole = Literal["root", "user_followup", "llm_round", "tool_in_round"]


@dataclass
class NormalizedEvent:
    """One event ready for emission to korg's /api/agent/tool-call endpoint."""

    causal_role: CausalRole
    tool_name: str
    args: dict[str, Any] = field(default_factory=dict)
    result: dict[str, Any] = field(default_factory=dict)
    success: bool = True
    duration_ms: int = 0
    # Only set for tool_in_round events — used to match tool_result blocks
    # that arrive in the next user message.
    call_id: str | None = None


@dataclass
class SessionState:
    """Persistent parser state across incremental parse_session() calls.

    Needed for tail mode: a tool_use may land in one batch and its
    tool_result in the next. Carrying pending_tool_calls across calls
    lets the result attach back to its tool_use even across polls.

    Single-shot use doesn't need this — just let parse_session() default
    a fresh instance per call.
    """

    pending_tool_calls: dict[str, NormalizedEvent] = field(default_factory=dict)
    seen_first_user: bool = False


# Soft cap on tool_result content stored inline. Bigger payloads should
# arrive as payload_refs (sha256, size_bytes) — but the adapter doesn't
# do content addressing for v1.
_MAX_RESULT_INLINE_CHARS = 8000


def _extract_text(content: Any) -> str:
    """Pull a single text string out of an assistant message's content list.

    Claude returns content as a list of blocks (text, thinking, tool_use, ...).
    Multiple text blocks within one message get joined by newlines.
    """
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    parts: list[str] = []
    for block in content:
        if isinstance(block, dict) and block.get("type") == "text":
            parts.append(block.get("text", ""))
    return "\n".join(parts)


def _stringify_tool_result(content: Any) -> str:
    """tool_result.content can be a string OR a list of {type, text} blocks."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for block in content:
            if isinstance(block, dict):
                if block.get("type") == "text":
                    parts.append(block.get("text", ""))
                elif "content" in block:
                    parts.append(str(block.get("content", "")))
            else:
                parts.append(str(block))
        return "\n".join(parts)
    return str(content)


def parse_session(
    lines: Iterable[Any],
    state: SessionState | None = None,
) -> list[NormalizedEvent]:
    """Walk Claude Code session events in file order, return ordered NormalizedEvents.

    `lines` may be raw JSONL strings or already-decoded dicts — mixed iterables
    work too. Malformed lines are skipped silently (they're typically blank
    lines at the end of a file).

    `state` is optional; pass a persistent `SessionState` instance when
    parsing the same logical session in multiple calls (tail mode). Without
    it, each call is independent.
    """
    if state is None:
        state = SessionState()
    events: list[NormalizedEvent] = []
    pending_tool_calls = state.pending_tool_calls

    for line in lines:
        if isinstance(line, dict):
            obj: dict[str, Any] = line
        else:
            s = line.strip() if isinstance(line, str) else ""
            if not s:
                continue
            try:
                obj = json.loads(s)
            except json.JSONDecodeError:
                continue

        t = obj.get("type")

        if t == "user":
            msg = obj.get("message") or {}
            content = msg.get("content")

            # Plain-string user message → a fresh user_prompt.
            if isinstance(content, str) and content.strip():
                events.append(
                    NormalizedEvent(
                        causal_role="root" if not state.seen_first_user else "user_followup",
                        tool_name="user_prompt",
                        args={"prompt": content},
                    )
                )
                state.seen_first_user = True
                continue

            # List-form content: may contain tool_results, text blocks, or both.
            if isinstance(content, list):
                text_buf: list[str] = []
                for block in content:
                    if not isinstance(block, dict):
                        continue
                    btype = block.get("type")
                    if btype == "tool_result":
                        # Attach back to the originating tool_use by tool_use_id.
                        tool_use_id = block.get("tool_use_id")
                        ev = pending_tool_calls.pop(tool_use_id, None)
                        if ev is not None:
                            output = _stringify_tool_result(block.get("content", ""))
                            if len(output) > _MAX_RESULT_INLINE_CHARS:
                                output = output[:_MAX_RESULT_INLINE_CHARS] + "…[truncated]"
                            ev.result = {"output": output}
                            ev.success = not block.get("is_error", False)
                    elif btype == "text":
                        text_buf.append(block.get("text", ""))
                # If the user message also carried plain text (alongside tool
                # results or alone), emit it as a follow-up prompt.
                combined = "\n".join(t for t in text_buf if t).strip()
                if combined:
                    events.append(
                        NormalizedEvent(
                            causal_role="root" if not state.seen_first_user else "user_followup",
                            tool_name="user_prompt",
                            args={"prompt": combined},
                        )
                    )
                    state.seen_first_user = True

        elif t == "assistant":
            msg = obj.get("message") or {}
            content = msg.get("content") or []
            usage = msg.get("usage") or {}
            model = msg.get("model", "unknown")

            text_part = _extract_text(content)
            tool_calls: list[NormalizedEvent] = []
            if isinstance(content, list):
                for block in content:
                    if not isinstance(block, dict):
                        continue
                    if block.get("type") == "tool_use":
                        tool_use_id = block.get("id")
                        tc = NormalizedEvent(
                            causal_role="tool_in_round",
                            tool_name=block.get("name", "unknown"),
                            args=block.get("input", {}) or {},
                            call_id=tool_use_id,
                        )
                        tool_calls.append(tc)
                        if tool_use_id:
                            pending_tool_calls[tool_use_id] = tc

            result: dict[str, Any] = {
                "completion_tokens": usage.get("output_tokens", 0),
            }
            if text_part:
                result["text"] = text_part
            cache_read = usage.get("cache_read_input_tokens")
            if cache_read is not None:
                result["cached_tokens"] = cache_read

            events.append(
                NormalizedEvent(
                    causal_role="llm_round",
                    tool_name="llm_inference",
                    args={
                        "model": model,
                        "prompt_tokens": usage.get("input_tokens", 0),
                    },
                    result=result,
                )
            )
            # Tool calls are siblings under this llm_round; emit them after
            # so the ledger sees llm_inference first.
            events.extend(tool_calls)

        # All other event types (system, attachment, file-history-snapshot,
        # last-prompt, permission-mode, ai-title, queue-operation) are
        # metadata and don't participate in the causal chain.

    return events
