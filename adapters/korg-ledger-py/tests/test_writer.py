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


def test_verify_chain_flags_event_with_no_entry_hash(tmp_path):
    led = tmp_path / "l.jsonl"
    w = LedgerWriter(led)
    w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    events = _lines(led)
    del events[0]["entry_hash"]  # an unchained event must be detected, not ignored
    errors = verify_chain(events)
    assert any("missing entry_hash" in e for e in errors), errors


def test_concurrent_writers_do_not_fork_the_chain(tmp_path):
    led = tmp_path / "l.jsonl"
    a = LedgerWriter(led)
    b = LedgerWriter(led)  # second instance with an independent (soon-stale) cache
    a.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    # b's in-memory tip is stale (seq 0); a correct append must re-read from disk
    s = b.append(event=_evt("Read", 2), actor_id="korg:claude-hook")
    assert s == 2
    events = _lines(led)
    assert [e["seq_id"] for e in events] == [1, 2]
    assert events[1]["prev_hash"] == events[0]["entry_hash"]
    assert verify_chain(events) == []


def test_resume_tolerates_blank_and_torn_final_line(tmp_path):
    led = tmp_path / "l.jsonl"
    a = LedgerWriter(led)
    a.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    a.append(event=_evt("Read", 2), actor_id="korg:claude-hook")
    # simulate a crash mid-write: a blank line then a torn JSON line at the tail
    with led.open("a") as f:
        f.write("\n")
        f.write('{"seq_id": 3, "metadata": {"emitted')  # truncated, no newline
    b = LedgerWriter(led)  # must recover from the last intact event, not crash
    assert b.tip()[0] == 2
    s3 = b.append(event=_evt("Edit", 3), actor_id="korg:claude-hook")
    assert s3 == 3


def test_torn_final_line_is_tolerated(tmp_path):
    """A crash mid-write leaves a torn LAST line; the writer ignores it and
    continues the chain from the last intact event."""
    led = tmp_path / "ledger.jsonl"
    w = LedgerWriter(led)
    w.append(event=_evt("user_prompt", 1), actor_id="a")
    with led.open("a") as f:
        f.write('{"seq_id": 2, "partial')  # torn final line, no newline
    w2 = LedgerWriter(led)  # re-reads tip; must not choke
    s = w2.append(event=_evt("Read", 2), actor_id="a")
    assert s == 2  # continued from seq 1, not forked


def test_mid_file_corruption_fails_loud(tmp_path):
    """A corrupt line with valid data AFTER it means the log was spliced — the
    writer must refuse to append rather than silently fork the chain."""
    led = tmp_path / "ledger.jsonl"
    w = LedgerWriter(led)
    w.append(event=_evt("user_prompt", 1), actor_id="a")
    w.append(event=_evt("Read", 2), actor_id="a")
    # splice a garbage line into the MIDDLE
    good = led.read_text().splitlines()
    led.write_text(good[0] + "\n" + "{garbage not json}\n" + good[1] + "\n")
    with pytest.raises(ValueError, match="corrupt ledger line"):
        LedgerWriter(led)
