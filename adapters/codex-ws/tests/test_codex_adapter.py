"""
codex-adapter acceptance tests.

The fixture mirrors a real Codex session (per WS_PROTOCOL.md):
  Turn 1: user prompt → 2 parallel exec_command tool calls → response.completed
  Turn 2: tool results arrive in next response.create → apply_patch (custom_tool_call)
          → response.completed.

The adapter must produce a causal tree like:
  user_prompt (seq=1, triggered_by=None)
    └─ llm_inference (seq=2, triggered_by=1)
        ├─ exec_command (seq=3, triggered_by=2)
        └─ exec_command (seq=4, triggered_by=2)
    └─ llm_inference (seq=5, triggered_by=2)  ← advances per agent.py:440
        └─ apply_patch (seq=6, triggered_by=5)
"""

import json
import os
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from codex_adapter import CodexAdapter, parse_session  # noqa: E402


FIXTURE = Path(__file__).parent / "fixtures" / "simple_session.json"


@pytest.fixture
def frames():
    with FIXTURE.open() as f:
        return json.load(f)


class FakeLedger:
    """In-memory emit fake. Captures every event with an assigned seq_id."""

    def __init__(self):
        self.events: list[dict] = []
        self._next_seq = 0

    def emit(self, body: dict) -> int:
        self._next_seq += 1
        body = dict(body)
        body["seq_id"] = self._next_seq
        self.events.append(body)
        return self._next_seq


# ─ Parser ────────────────────────────────────────────────────────────────


def test_parser_emits_one_root(frames):
    events = parse_session(frames)
    roots = [e for e in events if e.causal_role == "root"]
    assert len(roots) == 1
    assert roots[0].tool_name == "user_prompt"
    assert roots[0].args["prompt"] == "list the markdown files in codex-re/"


def test_parser_emits_two_llm_rounds(frames):
    events = parse_session(frames)
    rounds = [e for e in events if e.causal_role == "llm_round"]
    assert len(rounds) == 2
    assert rounds[0].args["model"] == "gpt-5.4"
    assert rounds[0].args["prompt_tokens"] == 20151
    assert rounds[0].result["completion_tokens"] == 295
    assert rounds[0].result["cached_tokens"] == 3456


def test_parser_emits_three_tool_calls(frames):
    events = parse_session(frames)
    tools = [e for e in events if e.causal_role == "tool_in_round"]
    assert len(tools) == 3
    assert [t.tool_name for t in tools] == ["exec_command", "exec_command", "apply_patch"]


def test_parser_parses_function_call_args_as_json(frames):
    events = parse_session(frames)
    first_exec = next(e for e in events if e.tool_name == "exec_command")
    assert isinstance(first_exec.args, dict)
    assert "rg --files" in first_exec.args["cmd"]
    assert first_exec.args["workdir"] == "/Users/alex"


def test_parser_preserves_custom_tool_call_input_as_text(frames):
    events = parse_session(frames)
    patch = next(e for e in events if e.tool_name == "apply_patch")
    assert "input" in patch.args
    assert patch.args["input"].startswith("*** Begin Patch")
    assert "*** End Patch" in patch.args["input"]


def test_parser_attaches_tool_results_from_next_turn(frames):
    events = parse_session(frames)
    exec_events = [e for e in events if e.tool_name == "exec_command"]
    # Results from turn 2's response.create.input get back-attached to turn 1's calls
    assert exec_events[0].result["output"].startswith("Exit code: 0")
    assert "FINDINGS.md" in exec_events[0].result["output"]
    assert exec_events[1].result["output"].endswith("8\n")
    assert exec_events[0].success is True


# ─ Adapter (causal chain) ────────────────────────────────────────────────


def test_adapter_emits_events_in_causal_order(frames):
    fake = FakeLedger()
    adapter = CodexAdapter(fake.emit, source_agent="agent:codex@gpt-5.4")
    stats = adapter.ingest(frames)

    assert stats.user_prompts == 1
    assert stats.llm_rounds == 2
    assert stats.tool_calls == 3
    assert stats.dropped == 0
    assert len(fake.events) == 6


def test_adapter_chains_triggered_by_correctly(frames):
    fake = FakeLedger()
    CodexAdapter(fake.emit).ingest(frames)

    by_seq = {e["seq_id"]: e for e in fake.events}

    # seq 1: root
    assert by_seq[1]["tool_name"] == "user_prompt"
    assert "triggered_by" not in by_seq[1]

    # seq 2: llm_inference, triggered_by=1
    assert by_seq[2]["tool_name"] == "llm_inference"
    assert by_seq[2]["triggered_by"] == 1

    # seq 3+4: parallel exec_command, both triggered_by=2 (siblings)
    assert by_seq[3]["tool_name"] == "exec_command"
    assert by_seq[3]["triggered_by"] == 2
    assert by_seq[4]["tool_name"] == "exec_command"
    assert by_seq[4]["triggered_by"] == 2

    # seq 5: second llm_inference, triggered_by=2 (the prior llm_seq, not 4)
    # This mirrors korgex/src/agent.py:440 where prompt_seq advances to the
    # last llm_seq, not to the most recent tool call.
    assert by_seq[5]["tool_name"] == "llm_inference"
    assert by_seq[5]["triggered_by"] == 2

    # seq 6: apply_patch, triggered_by=5
    assert by_seq[6]["tool_name"] == "apply_patch"
    assert by_seq[6]["triggered_by"] == 5


def test_adapter_uses_configured_source_agent(frames):
    fake = FakeLedger()
    CodexAdapter(fake.emit, source_agent="agent:codex@gpt-5.5").ingest(frames)
    assert all(e["source_agent"] == "agent:codex@gpt-5.5" for e in fake.events)


def test_adapter_handles_emit_returning_none():
    """If korg is unreachable, emit returns None — adapter must not crash and
    must count dropped events instead of chaining bogus triggered_by."""
    def emit(_body):
        return None

    frames = [
        {"direction": "out", "frame": {"type": "response.create", "model": "gpt-5.4",
         "input": [{"type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "hi"}]}]}},
        {"direction": "in", "frame": {"type": "response.completed",
         "response": {"model": "gpt-5.4", "usage": {"input_tokens": 1, "output_tokens": 1}}}},
    ]
    stats = CodexAdapter(emit).ingest(frames)
    assert stats.dropped == 2
    assert stats.user_prompts == 0
    assert stats.llm_rounds == 0


def test_adapter_handles_session_with_no_tools():
    """Pure text response — user prompt, llm round, no tools. Must still chain."""
    fake = FakeLedger()
    frames = [
        {"direction": "out", "frame": {"type": "response.create", "model": "gpt-5.4",
         "input": [{"type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "hello"}]}]}},
        {"direction": "in", "frame": {"type": "response.completed",
         "response": {"model": "gpt-5.4", "usage": {"input_tokens": 5, "output_tokens": 3}}}},
    ]
    CodexAdapter(fake.emit).ingest(frames)
    assert len(fake.events) == 2
    assert fake.events[1]["triggered_by"] == 1
