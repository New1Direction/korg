import json
import sys
from pathlib import Path

# make korg-ledger-py importable without an install step (sibling under adapters/)
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "korg-ledger-py" / "src"))

from claude_code_adapter.adapter import ClaudeCodeAdapter
from claude_code_adapter.canonical_emit import make_canonical_emit
from korg_ledger import verify_chain


def _lines(p):
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def test_emit_produces_verifiable_chain_with_causal_spine(tmp_path):
    led = tmp_path / "s.jsonl"
    emit = make_canonical_emit(led, actor_id="korg:claude-hook")
    adapter = ClaudeCodeAdapter(emit=emit, source_agent="agent:claude-code#sess1")
    # user prompt → assistant(llm + Read tool) → tool_result
    lines = [
        {"type": "user", "message": {"content": "fix the bug"}},
        {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 10, "output_tokens": 5},
            "content": [{"type": "text", "text": "reading"},
                        {"type": "tool_use", "id": "tu1", "name": "Read", "input": {"file": "a.py"}}]}},
        {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "tu1", "content": "file body", "is_error": False}]}},
    ]
    adapter.ingest(lines)
    events = _lines(led)
    # user_prompt(seq1) → llm_inference(seq2, tb=1) → Read(seq3, tb=2)
    assert [e["seq_id"] for e in events] == [1, 2, 3]
    assert events[0]["event"]["tool_name"] == "user_prompt"
    assert events[1]["event"]["tool_name"] == "llm_inference"
    assert events[1]["metadata"]["triggered_by"] == 1
    assert events[2]["event"]["tool_name"] == "Read"
    assert events[2]["metadata"]["triggered_by"] == 2
    assert events[2]["event"]["result"] == {"output": "file body"}   # result captured
    # all share the session root (event 1's event_id)
    root = events[0]["metadata"]["event_id"]
    assert all(e["metadata"]["root_event_id"] == root for e in events)
    assert verify_chain(events) == []


def test_emit_returns_none_on_causality_violation(tmp_path):
    led = tmp_path / "s.jsonl"
    emit = make_canonical_emit(led, actor_id="korg:claude-hook")
    # a body that claims to be triggered by a non-existent earlier seq
    seq = emit({"source_agent": "agent:claude-code#x", "tool_name": "Read",
                "args": {}, "result": {}, "success": True, "duration_ms": 0,
                "triggered_by": 99})
    assert seq is None
    assert not led.read_text().strip()  # nothing written
