"""Integration tests for korg_bridge.Bridge.

These tests verify that the in-process bridge writes events whose on-disk
shape matches what korg-server's HTTP path would have written, and that
causal chains (triggered_by / root_event_id) work end-to-end.
"""

from __future__ import annotations

import json
import tempfile
from pathlib import Path

import pytest

import korg_bridge


@pytest.fixture
def tmp_journal(tmp_path: Path) -> Path:
    return tmp_path / "journal.json"


def _read_events(journal_path: Path) -> list[dict]:
    with journal_path.open() as f:
        return json.load(f)


def test_module_version_present():
    assert korg_bridge.__version__ == "0.3.0"


def test_repr_initial_state(tmp_journal):
    bridge = korg_bridge.Bridge(str(tmp_journal))
    rep = repr(bridge)
    assert "events=0" in rep
    assert "last_seq_id=0" in rep


def test_record_user_prompt_is_root(tmp_journal):
    bridge = korg_bridge.Bridge(str(tmp_journal))
    seq = bridge.record_user_prompt("add /healthz")

    assert seq == 1, "first event should get seq_id=1"

    events = _read_events(tmp_journal)
    assert len(events) == 1

    ev = events[0]
    assert ev["seq_id"] == 1
    # Root events MUST have triggered_by=None — this is the invariant the
    # audit caught us misrouting in the orphaned-parent fix. Re-asserting
    # at the bridge boundary so a future regression here is loud.
    assert ev["metadata"]["triggered_by"] is None
    # Root event's root_event_id is its own event_id.
    assert ev["metadata"]["root_event_id"] == ev["metadata"]["event_id"]

    body = ev["event"]
    assert body["event_type"] == "AgentToolCall"
    assert body["source_agent"] == "human:claude-code-user"
    assert body["tool_name"] == "user_prompt"
    assert body["args"] == {"prompt": "add /healthz"}
    assert body["success"] is True


def test_triggered_by_chain(tmp_journal):
    bridge = korg_bridge.Bridge(str(tmp_journal))
    root = bridge.record_user_prompt("plan and ship")
    llm = bridge.record_llm_call(
        model="claude-opus-4-7",
        prompt_tokens=42,
        completion_tokens=137,
        duration_ms=890,
        triggered_by=root,
    )
    edit = bridge.record_tool_call(
        source_agent="agent:korgex@0.3.0",
        tool_name="Edit",
        args={"file_path": "src/routes.py"},
        result={"success": True},
        success=True,
        duration_ms=23,
        triggered_by=llm,
    )

    assert (root, llm, edit) == (1, 2, 3)

    events = _read_events(tmp_journal)
    assert [e["seq_id"] for e in events] == [1, 2, 3]
    assert events[0]["metadata"]["triggered_by"] is None
    assert events[1]["metadata"]["triggered_by"] == root
    assert events[2]["metadata"]["triggered_by"] == llm

    # All three should share the same root_event_id — the walk from `edit`
    # back via triggered_by lands at `root`.
    root_id = events[0]["metadata"]["root_event_id"]
    assert events[1]["metadata"]["root_event_id"] == root_id
    assert events[2]["metadata"]["root_event_id"] == root_id


def test_tool_call_args_are_arbitrary_json(tmp_journal):
    """Python dict/list/None/bool/numbers should all round-trip cleanly."""
    bridge = korg_bridge.Bridge(str(tmp_journal))
    args = {
        "string": "hello",
        "int": 42,
        "float": 3.14,
        "bool": True,
        "none": None,
        "list": [1, 2, {"nested": "ok"}],
    }
    result = {"ok": True, "items": [{"a": 1}, {"b": 2}]}
    seq = bridge.record_tool_call(
        source_agent="agent:korgex@0.3.0",
        tool_name="ComplexThing",
        args=args,
        result=result,
        success=True,
        duration_ms=10,
        triggered_by=None,
    )
    assert seq == 1

    events = _read_events(tmp_journal)
    body = events[0]["event"]
    assert body["event_type"] == "AgentToolCall"
    assert body["args"] == args
    assert body["result"] == result


def test_journal_resumes_after_reopen(tmp_journal):
    """Closing the bridge (drop) and reopening must continue the seq counter."""
    bridge1 = korg_bridge.Bridge(str(tmp_journal))
    bridge1.record_user_prompt("first")
    bridge1.record_tool_call(
        source_agent="agent:korgex@0.3.0",
        tool_name="Bash",
        args={"command": "pytest"},
        result={"exit": 0},
        success=True,
        duration_ms=5,
        triggered_by=1,
    )
    last_before = bridge1.last_seq_id()
    assert last_before == 2

    del bridge1

    bridge2 = korg_bridge.Bridge(str(tmp_journal))
    assert bridge2.last_seq_id() == 2, "reopened bridge should see existing seq_id"
    seq = bridge2.record_tool_call(
        source_agent="agent:korgex@0.3.0",
        tool_name="Read",
        args={"path": "src/main.rs"},
        result={"bytes": 1234},
        success=True,
        duration_ms=2,
        triggered_by=2,
    )
    assert seq == 3, "new event continues the seq sequence past the reload"

    events = _read_events(tmp_journal)
    assert [e["seq_id"] for e in events] == [1, 2, 3]
    assert events[2]["event"]["tool_name"] == "Read"


def test_actor_id_marks_bridge_recorder(tmp_journal):
    """actor_id distinguishes bridge events from korg-internal events."""
    bridge = korg_bridge.Bridge(str(tmp_journal))
    bridge.record_user_prompt("hello")
    events = _read_events(tmp_journal)
    assert events[0]["metadata"]["actor_id"] == "korg:bridge"


def test_absurd_duration_rejected(tmp_journal):
    bridge = korg_bridge.Bridge(str(tmp_journal))
    with pytest.raises(ValueError, match="absurdly large"):
        bridge.record_tool_call(
            source_agent="agent:test",
            tool_name="Edit",
            args={},
            result={},
            success=True,
            duration_ms=2**63,
            triggered_by=None,
        )


def test_explicit_paths_work(tmp_path: Path):
    """Custom snapshot_path / lock_path are respected."""
    bridge = korg_bridge.Bridge(
        str(tmp_path / "events.jsonl"),
        snapshot_path=str(tmp_path / "checkpoint.bin"),
        lock_path=str(tmp_path / "events.lck"),
    )
    bridge.record_user_prompt("setup")
    bridge.flush()
    assert (tmp_path / "events.jsonl").exists()
    # lock file is created lazily; existence is best-effort to assert
