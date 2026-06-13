import json

import pytest

from korg_ledger import (
    CausalityError,
    LedgerWriter,
    agent_tool_call_event,
    verify_chain,
)


def _evt(tool, n):
    return agent_tool_call_event(
        source_agent="agent:claude-code@0.2.29",
        tool_name=tool,
        args={"n": n},
        result={"ok": True},
        success=True,
        duration_ms=n,
    )


def _lines(path):
    return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]


def test_append_produces_verifiable_chain(tmp_path):
    led = tmp_path / "ledger.jsonl"
    w = LedgerWriter(led)
    s1 = w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    s2 = w.append(
        event=_evt("Read", 2), actor_id="korg:claude-hook", triggered_by=s1
    )
    assert (s1, s2) == (1, 2)
    events = _lines(led)
    assert len(events) == 2
    assert events[0]["prev_hash"] == "0" * 64
    assert events[1]["prev_hash"] == events[0]["entry_hash"]
    assert events[1]["metadata"]["triggered_by"] == 1
    assert events[0]["event"]["event_type"] == "AgentToolCall"
    assert verify_chain(events) == []


def test_root_event_id_self_references_first_event(tmp_path):
    w = LedgerWriter(tmp_path / "l.jsonl")
    w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    e = _lines(tmp_path / "l.jsonl")[0]
    assert e["metadata"]["root_event_id"] == e["metadata"]["event_id"]
    assert e["metadata"]["causation_id"] is None


def test_rejects_non_earlier_triggered_by(tmp_path):
    w = LedgerWriter(tmp_path / "l.jsonl")
    w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    with pytest.raises(CausalityError):
        # next seq is 2; triggered_by must be strictly earlier (< 2)
        w.append(event=_evt("Read", 2), actor_id="korg:claude-hook", triggered_by=2)


def test_resume_continues_seq_and_chain(tmp_path):
    led = tmp_path / "l.jsonl"
    a = LedgerWriter(led)
    a.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    a.append(event=_evt("Read", 2), actor_id="korg:claude-hook")
    # fresh writer on the same file resumes from the tip
    b = LedgerWriter(led)
    assert b.tip()[0] == 2
    s3 = b.append(event=_evt("Edit", 3), actor_id="korg:claude-hook")
    assert s3 == 3
    events = _lines(led)
    assert len(events) == 3
    assert events[2]["prev_hash"] == events[1]["entry_hash"]
    assert verify_chain(events) == []


def test_hmac_mode_requires_key_to_verify(tmp_path):
    led = tmp_path / "l.jsonl"
    key = b"korg-conformance-key"
    w = LedgerWriter(led, hmac_key=key)
    w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    events = _lines(led)
    assert verify_chain(events, key) == []          # correct key verifies
    assert verify_chain(events, None) != []          # missing key fails
