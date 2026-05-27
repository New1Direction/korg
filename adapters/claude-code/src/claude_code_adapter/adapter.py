"""
ClaudeCodeAdapter — walks NormalizedEvents and emits one korg AgentToolCall per event.

The adapter is transport-agnostic: it takes an `emit` callable that posts an
event body and returns the assigned seq_id (or None if korg is unreachable).
This makes the adapter trivially testable with an in-memory fake.

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

from claude_code_adapter.parser import NormalizedEvent, parse_session


EmitFn = Callable[[dict], Optional[int]]


@dataclass
class IngestStats:
    user_prompts: int = 0
    llm_rounds: int = 0
    tool_calls: int = 0
    dropped: int = 0  # events korg refused or that emit() couldn't acknowledge


class ClaudeCodeAdapter:
    """Translate a Claude Code session JSONL into ordered korg AgentToolCall events."""

    def __init__(
        self,
        emit: EmitFn,
        source_agent: str = "agent:claude-code@2.1.0",
    ) -> None:
        self.emit = emit
        self.source_agent = source_agent

    def ingest(self, lines: Iterable[Any]) -> IngestStats:
        events = parse_session(lines)
        return self.ingest_events(events)

    def ingest_events(self, events: list[NormalizedEvent]) -> IngestStats:
        stats = IngestStats()
        # Most-recent user_prompt seq — chains the next llm_inference to it
        # only if no prior llm_inference exists (i.e. this is the first turn).
        prompt_seq: int | None = None
        # Most-recent llm_inference seq — the spec §2a anchor for the next
        # llm_inference AND the parent of any subsequent tool_call / user_followup.
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

            elif ev.causal_role == "user_followup":
                # Follow-up user prompts chain to the prior llm_inference
                # (matches KorgChat's multi-turn behavior).
                if llm_seq is not None:
                    body["triggered_by"] = llm_seq
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                else:
                    # We DON'T update prompt_seq here — per spec §2a, the next
                    # llm_inference still chains to the prior llm_inference,
                    # not at this just-recorded user_prompt.
                    stats.user_prompts += 1

            elif ev.causal_role == "llm_round":
                if llm_seq is not None:
                    # Spec §2a: chain to the prior llm_inference.
                    body["triggered_by"] = llm_seq
                elif prompt_seq is not None:
                    # First llm_inference of the session — chain to the root.
                    body["triggered_by"] = prompt_seq
                seq = self.emit(body)
                if seq is None:
                    stats.dropped += 1
                else:
                    llm_seq = seq
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
