"""claude-code-adapter — Claude Code session JSONL → korg AgentToolCall events."""

from claude_code_adapter.adapter import (
    ClaudeCodeAdapter,
    EmitFn,
    IngestStats,
)
from claude_code_adapter.parser import (
    NormalizedEvent,
    SessionState,
    parse_session,
)
from claude_code_adapter.tail import (
    DEFAULT_POLL_INTERVAL_S,
    DEFAULT_PROJECTS_DIR,
    DEFAULT_STATE_PATH,
    PollStats,
    TailIngester,
    TailState,
    make_jsonl_emit,
    make_stub_emit,
)

__all__ = [
    # Adapter
    "ClaudeCodeAdapter",
    "EmitFn",
    "IngestStats",
    # Parser
    "NormalizedEvent",
    "SessionState",
    "parse_session",
    # Tail (v0.2.0)
    "DEFAULT_POLL_INTERVAL_S",
    "DEFAULT_PROJECTS_DIR",
    "DEFAULT_STATE_PATH",
    "PollStats",
    "TailIngester",
    "TailState",
    "make_jsonl_emit",
    "make_stub_emit",
]
