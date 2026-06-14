"""
ClaudeCodeAdapter — walks NormalizedEvents and emits one korg AgentToolCall per event.

The adapter is transport-agnostic: it takes an `emit` callable that posts an
event body and returns the assigned seq_id (or None if korg is unreachable).
This makes the adapter trivially testable with an in-memory fake.

The adapter is also **stateful** across `ingest()` calls — `prompt_seq`,
`llm_seq`, and the parser's `pending_tool_calls` persist between calls so
that tail-mode polling (multiple incremental ingests against the same
session) produces the same causal chain as a single-shot ingest. For a
fresh ingest of a different session, construct a new adapter instance.

Causal chain produced (matches spec §2a — llm_inference points at PRIOR
llm_inference, not at intervening tool calls or user prompts):

    user_prompt (triggered_by=None)                 ← root
      └─ llm_inference (triggered_by=user_prompt)
          ├─ tool_use Read (triggered_by=llm_inference)
          └─ tool_use Bash (triggered_by=llm_inference, sibling)
      └─ llm_inference (triggered_by=prior llm_inference)  ← spec §2a
    user_prompt (triggered_by=prior llm_inference)  ← follow-up turn
      └─ llm_inference (triggered_by=prior llm_inference)  ← chains to LLM, not the user
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Iterable, Optional

from claude_code_adapter.parser import (
    NormalizedEvent,
    SessionState,
    parse_session,
)


EmitFn = Callable[[dict], Optional[int]]


@dataclass
class IngestStats:
    user_prompts: int = 0
    llm_rounds: int = 0
    tool_calls: int = 0
    dropped: int = 0  # events korg refused or that emit() couldn't acknowledge

    def merge(self, other: "IngestStats") -> None:
        self.user_prompts += other.user_prompts
        self.llm_rounds += other.llm_rounds
        self.tool_calls += other.tool_calls
        self.dropped += other.dropped


class ClaudeCodeAdapter:
    """Translate a Claude Code session JSONL into ordered korg AgentToolCall events.

    A single adapter instance represents a single logical session. Call
    `ingest()` once for one-shot replay, or repeatedly for tail-mode
    incremental ingest — the chain state persists across calls.
    """

    def __init__(
        self,
        emit: EmitFn,
        source_agent: str = "agent:claude-code@2.1.0",
    ) -> None:
        self.emit = emit
        self.source_agent = source_agent
        # Chain state — persists across ingest() calls so tail mode works.
        self._prompt_seq: int | None = None
        self._llm_seq: int | None = None
        # True when the current round's llm_inference emit was dropped, so its
        # sibling tool events must not chain to a stale (earlier) _llm_seq.
        self._round_dropped: bool = False
        # Parser state — pending_tool_calls and seen_first_user persist
        # between incremental parses.
        self._parser_state = SessionState()

    def ingest(self, lines: Iterable[Any]) -> IngestStats:
        events = parse_session(lines, state=self._parser_state)
        return self.ingest_events(events)

    def parse_all(self, lines: Iterable[Any]) -> list[NormalizedEvent]:
        """Full single-shot parse of an entire transcript (fresh parser state).

        Used by the short-lived korg-hook driver: re-parsing the whole file
        each firing captures tool results (which the buffered parser fills in
        by mutation) without persisting parser internals across processes.
        """
        return parse_session(lines, state=SessionState())

    def ingest_events(self, events: list[NormalizedEvent]) -> IngestStats:
        stats = IngestStats()

        for ev in events:
            body: dict[str, Any] = {
                "source_agent": self.source_agent,
                "tool_name": ev.tool_name,
                "args": ev.args,
                "result": ev.result,
                "success": ev.success,
                "duration_ms": ev.duration_ms,
            }

            if ev.causal_role == "root":
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                else:
                    self._prompt_seq = seq
                    stats.user_prompts += 1

            elif ev.causal_role == "user_followup":
                # Follow-up user prompts chain to the prior llm_inference
                # (matches KorgChat's multi-turn behavior).
                if self._llm_seq is not None:
                    body["triggered_by"] = self._llm_seq
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                else:
                    # We DON'T update prompt_seq here — per spec §2a, the next
                    # llm_inference still chains to the prior llm_inference,
                    # not at this just-recorded user_prompt.
                    stats.user_prompts += 1

            elif ev.causal_role == "llm_round":
                if self._llm_seq is not None:
                    # Spec §2a: chain to the prior llm_inference.
                    body["triggered_by"] = self._llm_seq
                elif self._prompt_seq is not None:
                    # First llm_inference of the session — chain to the root.
                    body["triggered_by"] = self._prompt_seq
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                    self._round_dropped = True  # this round's parent is absent
                else:
                    self._llm_seq = seq
                    self._round_dropped = False
                    stats.llm_rounds += 1

            elif ev.causal_role == "tool_in_round":
                # Don't chain to a stale _llm_seq if this round's llm_inference was
                # dropped — an honest unparented event beats a wrong (earlier) parent.
                if self._llm_seq is not None and not getattr(self, "_round_dropped", False):
                    body["triggered_by"] = self._llm_seq
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                else:
                    stats.tool_calls += 1

        return stats
