"""Tests for the event → embedding-text flattener."""

from __future__ import annotations

import pytest

from korg_recall_mcp.text import text_for_event


def test_user_prompt_extracts_prompt_arg():
    event = {
        "tool_name": "user_prompt",
        "args": {"prompt": "what is the rust borrow checker"},
        "result": {},
    }
    assert text_for_event(event) == "what is the rust borrow checker"


def test_llm_inference_extracts_result_text():
    event = {
        "tool_name": "llm_inference",
        "args": {"model": "claude-opus-4-7", "prompt_tokens": 5},
        "result": {"completion_tokens": 8, "text": "It tracks ownership at compile time."},
    }
    assert text_for_event(event) == "It tracks ownership at compile time."


def test_llm_inference_no_text_is_empty():
    event = {
        "tool_name": "llm_inference",
        "args": {},
        "result": {"completion_tokens": 2},
    }
    assert text_for_event(event) == ""


def test_tool_call_includes_name_and_key_args():
    event = {
        "tool_name": "Read",
        "args": {"file_path": "/src/main.rs", "offset": 0},
        "result": {"output": "fn main() { println!(\"hello\"); }"},
    }
    out = text_for_event(event)
    assert "Read" in out
    assert "/src/main.rs" in out
    assert "println" in out  # snippet from result


def test_tool_call_with_bash_command_is_findable():
    event = {
        "tool_name": "Bash",
        "args": {"command": "pytest tests/test_auth.py", "description": "run auth tests"},
        "result": {"output": "5 passed in 0.42s"},
    }
    out = text_for_event(event)
    assert "Bash" in out
    assert "pytest" in out
    assert "5 passed" in out


def test_tool_call_with_no_known_args_falls_back_to_json():
    event = {
        "tool_name": "MysteryTool",
        "args": {"weird_field": "important value here"},
        "result": {},
    }
    out = text_for_event(event)
    assert "MysteryTool" in out
    assert "important value here" in out


def test_long_text_is_trimmed():
    event = {
        "tool_name": "user_prompt",
        "args": {"prompt": "x" * 5000},
        "result": {},
    }
    out = text_for_event(event)
    assert len(out) < 5000
    assert out.endswith("…")


def test_empty_event_returns_empty_string():
    assert text_for_event({}) == ""


def test_missing_args_and_result_keys():
    event = {"tool_name": "user_prompt"}
    # No "args" or "result" — should not crash, returns empty
    assert text_for_event(event) == ""
