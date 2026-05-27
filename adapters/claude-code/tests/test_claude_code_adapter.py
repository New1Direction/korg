"""Tests for the Claude Code session JSONL → korg event adapter.

The fixture `simple_session.jsonl` exercises:
  - The root user prompt
  - One assistant turn with text + tool_use(Read)
  - A user message with only a tool_result (no new prompt emitted)
  - Spec §2a: subsequent llm_inference chains to prior llm_inference
  - Multi-turn: a follow-up user_prompt chains to the prior llm
  - An assistant turn with tool_use only (no text)
  - Session metadata events (system, attachment, ai-title) that must be skipped
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from claude_code_adapter import (
    ClaudeCodeAdapter,
    NormalizedEvent,
    parse_session,
)


FIXTURE = Path(__file__).parent / "fixtures" / "simple_session.jsonl"


def _load_fixture() -> list[str]:
    with FIXTURE.open() as f:
        return [line for line in f if line.strip()]


class _Recorder:
    """Records emit() bodies and assigns sequential seq_ids — a stand-in
    for korg's HTTP /api/agent/tool-call endpoint."""

    def __init__(self) -> None:
        self.bodies: list[dict] = []
        self.next_seq = 1

    def __call__(self, body: dict) -> int:
        self.bodies.append(dict(body))
        seq = self.next_seq
        self.next_seq += 1
        return seq


# ── Parser ────────────────────────────────────────────────────────────


def test_parser_skips_metadata_event_types():
    events = parse_session(_load_fixture())
    # 2 user_prompts (u1 root, u3 followup) + 4 llm_inferences + 2 tool_calls
    assert len(events) == 8


def test_parser_first_user_event_is_root():
    events = parse_session(_load_fixture())
    first = events[0]
    assert first.causal_role == "root"
    assert first.tool_name == "user_prompt"
    assert first.args["prompt"] == "read /tmp/foo and tell me what's in it"


def test_parser_extracts_assistant_text_into_result():
    events = parse_session(_load_fixture())
    llm_rounds = [e for e in events if e.causal_role == "llm_round"]
    assert len(llm_rounds) == 4
    assert llm_rounds[0].result.get("text") == "Let me check that file."
    assert llm_rounds[1].result.get("text") == "The file contains: hello world"


def test_parser_omits_text_field_when_assistant_has_no_text():
    events = parse_session(_load_fixture())
    llm_rounds = [e for e in events if e.causal_role == "llm_round"]
    # a3 is the assistant with tool_use only (no text)
    assert "text" not in llm_rounds[2].result


def test_parser_emits_tool_use_with_call_id():
    events = parse_session(_load_fixture())
    tool_calls = [e for e in events if e.causal_role == "tool_in_round"]
    assert len(tool_calls) == 2
    read_call, bash_call = tool_calls
    assert read_call.tool_name == "Read"
    assert read_call.args == {"file_path": "/tmp/foo"}
    assert read_call.call_id == "toolu_01"
    assert bash_call.tool_name == "Bash"
    assert bash_call.args["command"] == "ls /tmp"
    assert bash_call.call_id == "toolu_02"


def test_parser_attaches_tool_result_to_originating_call():
    events = parse_session(_load_fixture())
    tool_calls = [e for e in events if e.causal_role == "tool_in_round"]
    read_call, bash_call = tool_calls
    assert "hello world" in read_call.result["output"]
    assert read_call.success is True
    assert "foo\nbar\nbaz" in bash_call.result["output"]
    assert bash_call.success is True


def test_parser_distinguishes_root_and_followup_user_prompts():
    events = parse_session(_load_fixture())
    user_events = [e for e in events if e.tool_name == "user_prompt"]
    assert [e.causal_role for e in user_events] == ["root", "user_followup"]
    assert user_events[1].args["prompt"] == "thanks. now run ls in /tmp"


def test_parser_marks_failed_tool_results():
    fixture = [
        {"type": "user", "message": {"role": "user", "content": "fail it"}},
        {
            "type": "assistant",
            "message": {
                "role": "assistant",
                "model": "claude-opus-4-7",
                "content": [
                    {"type": "tool_use", "id": "tu1", "name": "Bash", "input": {"command": "false"}}
                ],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        },
        {
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "tu1", "content": "exit 1", "is_error": True}
                ],
            },
        },
    ]
    events = parse_session(fixture)
    tool_calls = [e for e in events if e.causal_role == "tool_in_round"]
    assert len(tool_calls) == 1
    assert tool_calls[0].success is False
    assert tool_calls[0].result["output"] == "exit 1"


def test_parser_truncates_oversize_tool_results():
    big = "x" * 10000
    fixture = [
        {"type": "user", "message": {"role": "user", "content": "go"}},
        {
            "type": "assistant",
            "message": {
                "role": "assistant",
                "model": "m",
                "content": [{"type": "tool_use", "id": "tu1", "name": "Read", "input": {}}],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        },
        {
            "type": "user",
            "message": {
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "tu1", "content": big}],
            },
        },
    ]
    events = parse_session(fixture)
    tool_call = [e for e in events if e.causal_role == "tool_in_round"][0]
    assert "[truncated]" in tool_call.result["output"]
    assert len(tool_call.result["output"]) < len(big)


def test_parser_handles_mixed_string_and_dict_input():
    events = parse_session([
        json.dumps({"type": "user", "message": {"role": "user", "content": "string-line"}}),
        {"type": "user", "message": {"role": "user", "content": "dict-line"}},
    ])
    assert len(events) == 2
    assert events[0].args["prompt"] == "string-line"
    assert events[1].args["prompt"] == "dict-line"


def test_parser_skips_malformed_lines():
    events = parse_session([
        "not json",
        "",
        "   ",
        json.dumps({"type": "user", "message": {"role": "user", "content": "ok"}}),
    ])
    assert len(events) == 1
    assert events[0].args["prompt"] == "ok"


def test_parser_handles_tool_result_with_list_content():
    """tool_result.content can be a list of {type: text, text: ...} blocks."""
    fixture = [
        {"type": "user", "message": {"role": "user", "content": "go"}},
        {
            "type": "assistant",
            "message": {
                "role": "assistant",
                "model": "m",
                "content": [{"type": "tool_use", "id": "tu1", "name": "Read", "input": {}}],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        },
        {
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "tu1",
                        "content": [{"type": "text", "text": "file contents here"}],
                    }
                ],
            },
        },
    ]
    events = parse_session(fixture)
    tc = [e for e in events if e.causal_role == "tool_in_round"][0]
    assert tc.result["output"] == "file contents here"


# ── Adapter (chain integrity) ─────────────────────────────────────────


def test_adapter_first_body_is_root_with_no_triggered_by():
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    assert "triggered_by" not in rec.bodies[0]
    assert rec.bodies[0]["tool_name"] == "user_prompt"


def test_adapter_first_llm_chains_to_root_user_prompt():
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    # bodies[0] = user_prompt seq=1, bodies[1] = llm_inference triggered_by=1
    assert rec.bodies[1]["tool_name"] == "llm_inference"
    assert rec.bodies[1]["triggered_by"] == 1


def test_adapter_tool_call_chains_to_owning_llm_round():
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    # bodies[2] = Read tool_call, triggered_by = llm_inference at seq=2
    assert rec.bodies[2]["tool_name"] == "Read"
    assert rec.bodies[2]["triggered_by"] == 2


def test_adapter_second_llm_chains_to_prior_llm_not_tool_call():
    """Spec §2a: llm_inference's triggered_by always points at the prior
    llm_inference, never at intervening tool_call events."""
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    assert rec.bodies[3]["tool_name"] == "llm_inference"
    # prior llm at seq=2, NOT the Read tool_call at seq=3
    assert rec.bodies[3]["triggered_by"] == 2


def test_adapter_user_followup_chains_to_prior_llm():
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    # bodies[4] = follow-up user_prompt
    assert rec.bodies[4]["tool_name"] == "user_prompt"
    assert rec.bodies[4]["args"]["prompt"] == "thanks. now run ls in /tmp"
    # chains to prior llm at seq=4
    assert rec.bodies[4]["triggered_by"] == 4


def test_adapter_llm_after_followup_chains_to_prior_llm_not_user():
    """Per spec §2a, even after a user_followup the next llm_inference
    chains to the prior llm_inference, NOT the just-recorded user_prompt."""
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    # bodies[5] = llm_inference after the user_followup
    assert rec.bodies[5]["tool_name"] == "llm_inference"
    # prior llm at seq=4, NOT the user_prompt at seq=5
    assert rec.bodies[5]["triggered_by"] == 4


def test_adapter_stats_tally_correctly():
    rec = _Recorder()
    stats = ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    assert stats.user_prompts == 2
    assert stats.llm_rounds == 4
    assert stats.tool_calls == 2
    assert stats.dropped == 0


def test_adapter_records_dropped_when_emit_returns_none():
    state = {"calls": 0, "next_seq": 1}

    def flaky(body: dict) -> int | None:
        state["calls"] += 1
        if state["calls"] > 3:
            return None
        seq = state["next_seq"]
        state["next_seq"] += 1
        return seq

    stats = ClaudeCodeAdapter(emit=flaky).ingest(_load_fixture())
    # 3 acked, the rest (5 more) dropped
    assert stats.dropped == 5
    assert stats.user_prompts + stats.llm_rounds + stats.tool_calls == 3


def test_adapter_passes_source_agent_on_every_body():
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec, source_agent="agent:claude-code@2.1.150").ingest(_load_fixture())
    assert all(b["source_agent"] == "agent:claude-code@2.1.150" for b in rec.bodies)


def test_adapter_passes_required_body_fields():
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    for body in rec.bodies:
        assert "source_agent" in body
        assert "tool_name" in body
        assert "args" in body
        assert "result" in body
        assert "success" in body
        assert "duration_ms" in body


def test_adapter_default_source_agent():
    rec = _Recorder()
    ClaudeCodeAdapter(emit=rec).ingest(_load_fixture())
    assert rec.bodies[0]["source_agent"].startswith("agent:claude-code@")
