import json
import sys
from pathlib import Path

# korg-ledger-py is a sibling under adapters/
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "korg-ledger-py" / "src"))

from claude_code_adapter.hook import run_hook
from korg_ledger import verify_chain

SESSION = [
    {"type": "user", "message": {"content": "fix the bug"}},
    {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 10, "output_tokens": 5},
        "content": [{"type": "tool_use", "id": "tu1", "name": "Read", "input": {"file": "a.py"}}]}},
    {"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "tu1", "content": "body", "is_error": False}]}},
]


def _write_transcript(path, records):
    path.write_text("".join(json.dumps(r) + "\n" for r in records))


def _ledger(korg_home, sid):
    p = korg_home / "sessions" / f"{sid}.jsonl"
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def test_hook_captures_a_verifiable_session(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    events = _ledger(korg_home, "sess-abc")
    assert [e["event"]["tool_name"] for e in events] == ["user_prompt", "llm_inference", "Read"]
    assert events[2]["event"]["result"] == {"output": "body"}
    assert verify_chain(events) == []


def test_second_firing_with_no_new_content_is_idempotent(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    run_hook(payload, korg_home=korg_home)  # same transcript, no growth
    events = _ledger(korg_home, "sess-abc")
    assert [e["seq_id"] for e in events] == [1, 2, 3]  # no duplicates


def test_incremental_firing_appends_only_new_events(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    # a follow-up turn lands, then the hook fires again
    _write_transcript(transcript, SESSION + [
        {"type": "user", "message": {"content": "now add a test"}},
        {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 3, "output_tokens": 2},
            "content": [{"type": "text", "text": "ok"}]}},
    ])
    run_hook(payload, korg_home=korg_home)
    events = _ledger(korg_home, "sess-abc")
    names = [e["event"]["tool_name"] for e in events]
    assert names == ["user_prompt", "llm_inference", "Read", "user_prompt", "llm_inference"]
    # follow-up user_prompt chains to the prior llm_inference (spec §2a)
    assert events[3]["metadata"]["triggered_by"] == 2
    assert events[4]["metadata"]["triggered_by"] == 2
    assert verify_chain(events) == []


def test_run_hook_never_raises_on_bad_input(tmp_path):
    # missing transcript_path / unreadable file must not raise (hook must exit 0)
    run_hook({"session_id": "x", "hook_event_name": "Stop"}, korg_home=tmp_path / ".korg")
    run_hook({"session_id": "y", "transcript_path": str(tmp_path / "nope.jsonl")},
             korg_home=tmp_path / ".korg")
