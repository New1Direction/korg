"""
GrokHeavyAdapter — emit korg AgentToolCall events for a 16-agent session.

Causal model:

  user_prompt (root, triggered_by=None, source_agent=agent:grok-heavy@orchestrator)
    │
    ├─ llm_inference   (source=agent:grok-heavy-grok@4.20,   triggered_by=root)
    │   └─ tool calls from Grok                                triggered_by=llm
    ├─ llm_inference   (source=agent:grok-heavy-agent-1@4.20, triggered_by=root)
    │   └─ tool calls from Agent 1                             triggered_by=llm
    ├─ ... (one llm_inference per rolloutId)
    └─ llm_inference   (source=agent:grok-heavy-agent-15@4.20, triggered_by=root)
        └─ tool calls from Agent 15                            triggered_by=llm

Each of the 16 agents is a sibling under the root. Tool calls are children of
that agent's llm_inference, not children of root.

This is the "shared parent, distinct identity" pattern. It's the natural fit
for korg's single-parent triggered_by field. Cross-agent dependencies via
chatroom_send are visible in args (the "to" field names the recipient) but
not modeled as graph edges in v1 — that's intentional. (See README for why.)
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Iterable, Optional

from grok_heavy_adapter.parser import ParsedSession, parse_ndjson_stream


# Module-level type alias — evaluated at import time, so PEP 604 `int | None`
# doesn't work on Python 3.9. Use typing.Optional to stay compatible.
EmitFn = Callable[[dict], Optional[int]]

# Default orchestrator identity for the user_prompt root event.
DEFAULT_ORCHESTRATOR = "agent:grok-heavy@orchestrator"
# Grok Heavy uses grok-4-heavy under the hood; the public model ID is "grok-3"
# per telemetry (FINDINGS.md §Subscription). Version slot uses the public name.
DEFAULT_VERSION = "4-heavy"


@dataclass
class IngestStats:
    user_prompts: int = 0
    agents_spawned: int = 0
    tool_calls: int = 0
    dropped: int = 0


def _agent_identity(rollout_id: str, version: str) -> str:
    """Map "Grok" / "Agent 1" → "agent:grok-heavy-grok@4-heavy" / "...agent-1@..."."""
    slug = rollout_id.strip().lower().replace(" ", "-")
    return f"agent:grok-heavy-{slug}@{version}"


class GrokHeavyAdapter:
    """Translate a Grok Heavy NDJSON session into korg AgentToolCall events."""

    def __init__(
        self,
        emit: EmitFn,
        orchestrator: str = DEFAULT_ORCHESTRATOR,
        version: str = DEFAULT_VERSION,
    ) -> None:
        self.emit = emit
        self.orchestrator = orchestrator
        self.version = version

    def ingest(self, lines: Iterable[str]) -> IngestStats:
        return self.ingest_session(parse_ndjson_stream(lines))

    def ingest_session(self, session: ParsedSession) -> IngestStats:
        stats = IngestStats()

        # 1. Root user_prompt event
        if session.user_prompt is None:
            return stats  # malformed session; nothing to do
        root_seq = self.emit(
            {
                "source_agent": self.orchestrator,
                "tool_name": "user_prompt",
                "args": {"prompt": session.user_prompt},
                "result": {},
                "success": True,
                "duration_ms": 0,
            }
        )
        if root_seq is None:
            stats.dropped += 1
            return stats
        stats.user_prompts = 1

        # 2. Per-agent llm_inference. We use the rolloutIds list from uiLayout
        #    when available (canonical order), else fall back to whichever
        #    rolloutIds actually produced tokens.
        candidate_ids = session.rollout_ids or sorted(session.agent_steps.keys())
        agent_llm_seq: dict[str, int] = {}
        for rid in candidate_ids:
            if rid not in session.agent_steps:
                continue  # no tokens from this agent — don't fabricate an inference
            identity = _agent_identity(rid, self.version)
            seq = self.emit(
                {
                    "source_agent": identity,
                    "tool_name": "llm_inference",
                    "args": {"model": f"grok-{self.version}", "agent_role": rid},
                    "result": {"message_steps": sorted(session.agent_steps[rid])},
                    "success": True,
                    "duration_ms": 0,
                    "triggered_by": root_seq,
                }
            )
            if seq is None:
                stats.dropped += 1
            else:
                agent_llm_seq[rid] = seq
                stats.agents_spawned += 1

        # 3. Tool calls — child of the originating agent's llm_inference
        for tc in session.tool_calls:
            parent = agent_llm_seq.get(tc.rollout_id)
            if parent is None:
                # Tool call from an agent we never saw produce tokens — skip
                # rather than chain to root (would lie about causality).
                stats.dropped += 1
                continue
            identity = _agent_identity(tc.rollout_id, self.version)
            seq = self.emit(
                {
                    "source_agent": identity,
                    "tool_name": tc.tool_name,
                    "args": tc.tool_args,
                    "result": {},
                    "success": True,
                    "duration_ms": 0,
                    "triggered_by": parent,
                }
            )
            if seq is None:
                stats.dropped += 1
            else:
                stats.tool_calls += 1

        return stats
