"""
CodexAdapter — walks NormalizedEvents and emits one korg AgentToolCall per event.

The adapter is transport-agnostic: it takes an `emit` callable that posts an
event body and returns the assigned seq_id (or None if korg is unreachable).
This makes the adapter trivially testable with an in-memory fake.

Causal chain produced (mirrors korgex's own agent loop in src/agent.py):
    user_prompt (triggered_by=None)
      └─ llm_inference (triggered_by=user_prompt_seq)
          ├─ tool_call A (triggered_by=llm_seq, sibling of B)
          └─ tool_call B (triggered_by=llm_seq, sibling of A)
      └─ llm_inference (triggered_by=prior llm_seq)
          └─ tool_call C ...
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Iterable, Optional

from codex_adapter.parser import NormalizedEvent, parse_session


# Module-level type alias — evaluated at import time, so PEP 604 `int | None`
# doesn't work on Python 3.9. Use typing.Optional to stay compatible.
EmitFn = Callable[[dict], Optional[int]]


@dataclass
class IngestStats:
    user_prompts: int = 0
    llm_rounds: int = 0
    tool_calls: int = 0
    dropped: int = 0  # events korg refused or that emit() couldn't acknowledge


class CodexAdapter:
    """Translate a Codex WS session into ordered korg AgentToolCall events."""

    def __init__(
        self,
        emit: EmitFn,
        source_agent: str = "agent:codex@gpt-5.4",
    ) -> None:
        self.emit = emit
        self.source_agent = source_agent

    def ingest(self, frames: Iterable[dict]) -> IngestStats:
        events = parse_session(frames)
        return self.ingest_events(events)

    def ingest_events(self, events: list[NormalizedEvent]) -> IngestStats:
        stats = IngestStats()
        prompt_seq: int | None = None
        llm_seq: int | None = None

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
                    prompt_seq = seq
                    stats.user_prompts += 1

            elif ev.causal_role == "llm_round":
                if prompt_seq is not None:
                    body["triggered_by"] = prompt_seq
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                else:
                    llm_seq = seq
                    # Advance prompt_seq to this llm_seq so the next round
                    # chains here (mirrors korgex/src/agent.py:440).
                    prompt_seq = seq
                    stats.llm_rounds += 1

            elif ev.causal_role == "tool_in_round":
                if llm_seq is not None:
                    body["triggered_by"] = llm_seq
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                else:
                    stats.tool_calls += 1

        return stats
