"""
Grok Heavy NDJSON parser.

Wire format (grok-heavy-re/FINDINGS.md):

  POST https://grok.com/_data/v1/a/t/?verbose=1
  Response body: newline-delimited JSON, each line:
    {"result": {"response": {<one of the union variants>}}}

Variants we care about:
  - userResponse:   {"message": "<text>", "sender": "human", ...}
  - uiLayout:       {"rolloutIds": ["Grok", "Agent 1", ..., "Agent 15"], ...}
  - token:          {"token": "<text>", "messageTag": "summary"|"tool_usage_card",
                     "messageStepId": int, "rolloutId": "Grok"|"Agent N"}

Tool calls are XML embedded in tool_usage_card tokens (concatenated across deltas):
  <xai:tool_usage_card>
    <xai:tool_usage_card_id>uuid</xai:tool_usage_card_id>
    <xai:tool_name>chatroom_send</xai:tool_name>
    <xai:tool_args><![CDATA[{"message":"...","to":"Grok"}]]></xai:tool_args>
  </xai:tool_usage_card>

A single tool_usage_card may be split across many token frames. The parser
accumulates per (rolloutId, messageStepId) until it can parse a complete card.
"""

from __future__ import annotations

import json
import re
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Any, Iterable


# Regex parser is intentionally used instead of a full XML parser:
# the CDATA payload is JSON and may legally contain '>' characters, which
# trips most XML parsers when the surrounding namespace is informal.
_TOOL_NAME_RE = re.compile(r"<xai:tool_name>(.*?)</xai:tool_name>", re.DOTALL)
_TOOL_ARGS_RE = re.compile(
    r"<xai:tool_args>\s*<!\[CDATA\[(.*?)\]\]>\s*</xai:tool_args>",
    re.DOTALL,
)
_TOOL_ID_RE = re.compile(r"<xai:tool_usage_card_id>(.*?)</xai:tool_usage_card_id>", re.DOTALL)
_CARD_END_RE = re.compile(r"</xai:tool_usage_card>")


@dataclass
class AgentToolCall:
    """One tool call extracted from a tool_usage_card."""

    rollout_id: str  # "Grok", "Agent 1", ...
    message_step_id: int
    tool_name: str
    tool_args: dict[str, Any]
    card_id: str | None = None


@dataclass
class ParsedSession:
    user_prompt: str | None = None
    rollout_ids: list[str] = field(default_factory=list)
    # rollout_id → set of message_step_ids that produced tokens
    agent_steps: dict[str, set[int]] = field(default_factory=lambda: defaultdict(set))
    tool_calls: list[AgentToolCall] = field(default_factory=list)


def parse_tool_usage_card(xml_text: str) -> tuple[str, dict[str, Any], str | None] | None:
    """Parse one <xai:tool_usage_card>...</xai:tool_usage_card> blob.

    Returns (tool_name, args_dict, card_id) or None if the blob isn't a
    complete, parseable card.
    """
    if not _CARD_END_RE.search(xml_text):
        return None
    name_m = _TOOL_NAME_RE.search(xml_text)
    args_m = _TOOL_ARGS_RE.search(xml_text)
    if not name_m or not args_m:
        return None
    tool_name = name_m.group(1).strip()
    raw_args = args_m.group(1)
    try:
        args = json.loads(raw_args)
    except json.JSONDecodeError:
        args = {"_raw": raw_args}
    id_m = _TOOL_ID_RE.search(xml_text)
    card_id = id_m.group(1).strip() if id_m else None
    return tool_name, args, card_id


def parse_ndjson_stream(lines: Iterable[str]) -> ParsedSession:
    """Walk NDJSON lines and produce a normalized ParsedSession."""
    session = ParsedSession()
    # Accumulate tool_usage_card text per (rollout_id, message_step_id).
    # When a complete card is parseable, emit it and clear the buffer.
    card_buffers: dict[tuple[str, int], list[str]] = defaultdict(list)

    for raw_line in lines:
        line = raw_line.strip()
        if not line:
            continue
        try:
            envelope = json.loads(line)
        except json.JSONDecodeError:
            continue

        response = (envelope.get("result") or {}).get("response") or {}

        # User prompt
        ur = response.get("userResponse")
        if isinstance(ur, dict) and ur.get("sender") == "human":
            session.user_prompt = ur.get("message")
            continue

        # UI layout — gives us the canonical list of agent rolloutIds upfront
        ui = response.get("uiLayout")
        if isinstance(ui, dict) and "rolloutIds" in ui:
            session.rollout_ids = list(ui["rolloutIds"])
            continue

        # Streaming tokens
        if "token" not in response:
            continue

        rollout_id = response.get("rolloutId")
        step_id = response.get("messageStepId")
        tag = response.get("messageTag")
        token = response.get("token", "")
        if not rollout_id or step_id is None:
            continue

        session.agent_steps[rollout_id].add(step_id)

        if tag == "tool_usage_card":
            key = (rollout_id, step_id)
            card_buffers[key].append(token)
            joined = "".join(card_buffers[key])
            parsed = parse_tool_usage_card(joined)
            if parsed is not None:
                tool_name, tool_args, card_id = parsed
                session.tool_calls.append(
                    AgentToolCall(
                        rollout_id=rollout_id,
                        message_step_id=step_id,
                        tool_name=tool_name,
                        tool_args=tool_args,
                        card_id=card_id,
                    )
                )
                card_buffers.pop(key, None)

    return session
