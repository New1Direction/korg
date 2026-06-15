"""Tests for the EventIndex — file reading, incremental load, multi-source."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from korg_recall_mcp.index import EventIndex


def _write_event(path: Path, seq: int, tool_name: str, args: dict, **extra) -> None:
    record = {
        "seq": seq,
        "source_agent": extra.get("source_agent", "agent:claude-code#x"),
        "tool_name": tool_name,
        "args": args,
        "result": extra.get("result", {}),
        "success": True,
        "duration_ms": 0,
    }
    if "triggered_by" in extra:
        record["triggered_by"] = extra["triggered_by"]
    with path.open("a") as f:
        f.write(json.dumps(record) + "\n")


def test_index_reads_basic_jsonl(tmp_path):
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "hi"})
    _write_event(f, 2, "llm_inference", {}, result={"text": "hello"})

    idx = EventIndex.from_paths(f)
    added = idx.refresh()
    assert added == 2
    assert len(idx) == 2
    assert idx.events[0].tool_name == "user_prompt"
    assert idx.events[0].embed_text == "hi"


def test_index_incremental_refresh(tmp_path):
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "first"})

    idx = EventIndex.from_paths(f)
    assert idx.refresh() == 1
    assert idx.refresh() == 0  # no new lines → no new events

    _write_event(f, 2, "user_prompt", {"prompt": "second"})
    assert idx.refresh() == 1
    assert len(idx) == 2


def test_index_skips_malformed_lines(tmp_path):
    f = tmp_path / "ledger.jsonl"
    with f.open("w") as fh:
        fh.write(json.dumps({"seq": 1, "tool_name": "user_prompt", "args": {"prompt": "ok"}}) + "\n")
        fh.write("not json\n")
        fh.write("\n")
        fh.write(json.dumps({"seq": 2, "tool_name": "user_prompt", "args": {"prompt": "fine"}}) + "\n")

    idx = EventIndex.from_paths(f)
    idx.refresh()
    assert len(idx) == 2
    assert idx.events[0].args["prompt"] == "ok"
    assert idx.events[1].args["prompt"] == "fine"


def test_index_skips_events_with_no_embed_text(tmp_path):
    f = tmp_path / "ledger.jsonl"
    # llm_inference with no text → empty embed_text → skipped
    _write_event(f, 1, "llm_inference", {}, result={"completion_tokens": 1})
    # user_prompt with empty prompt → empty embed_text → skipped
    _write_event(f, 2, "user_prompt", {"prompt": ""})
    # user_prompt with a real prompt → kept
    _write_event(f, 3, "user_prompt", {"prompt": "real one"})

    idx = EventIndex.from_paths(f)
    idx.refresh()
    assert len(idx) == 1
    assert idx.events[0].args["prompt"] == "real one"


def test_index_partial_line_held_back(tmp_path):
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "complete"})
    # Append a partial line with no trailing \n
    with f.open("a") as fh:
        fh.write('{"seq":2,"tool_name":"user_prompt","args":{"prompt":"partial')

    idx = EventIndex.from_paths(f)
    n = idx.refresh()
    assert n == 1  # only the complete line

    # Finish the partial line
    with f.open("a") as fh:
        fh.write('"}}\n')

    n2 = idx.refresh()
    assert n2 == 1
    assert len(idx) == 2


def test_index_reads_from_directory(tmp_path):
    f1 = tmp_path / "session-a.jsonl"
    f2 = tmp_path / "session-b.jsonl"
    _write_event(f1, 1, "user_prompt", {"prompt": "from a"})
    _write_event(f2, 1, "user_prompt", {"prompt": "from b"})

    idx = EventIndex.from_dir(tmp_path)
    idx.refresh()
    prompts = sorted(e.args["prompt"] for e in idx.events)
    assert prompts == ["from a", "from b"]


def test_index_picks_up_new_file_in_directory(tmp_path):
    f1 = tmp_path / "session-a.jsonl"
    _write_event(f1, 1, "user_prompt", {"prompt": "from a"})

    # Point the index at the *directory* so it re-globs each refresh.
    idx = EventIndex(ledger_paths=[tmp_path])
    idx.refresh()
    assert len(idx) == 1

    f2 = tmp_path / "session-b.jsonl"
    _write_event(f2, 1, "user_prompt", {"prompt": "from b"})
    n = idx.refresh()
    assert n == 1
    assert len(idx) == 2


def test_index_handles_missing_paths(tmp_path):
    idx = EventIndex.from_paths(tmp_path / "does-not-exist.jsonl")
    # Should not crash
    assert idx.refresh() == 0
    assert len(idx) == 0


def test_index_preserves_triggered_by_and_success(tmp_path):
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "go"})
    _write_event(
        f, 2, "Bash", {"command": "false"},
        result={"output": "exit 1"},
        triggered_by=1,
    )
    idx = EventIndex.from_paths(f)
    idx.refresh()
    bash_event = [e for e in idx.events if e.tool_name == "Bash"][0]
    assert bash_event.triggered_by == 1


def test_reads_canonical_journalevent_records(tmp_path):
    from korg_recall_mcp.index import EventIndex
    led = tmp_path / "sess.jsonl"
    # one canonical korg-ledger@v1 JournalEvent line (nested event/metadata)
    led.write_text(json.dumps({
        "schema_version": "1.0", "seq_id": 7,
        "metadata": {"triggered_by": 6, "actor_id": "korg:claude-hook"},
        "event": {"event_type": "AgentToolCall", "source_agent": "agent:claude-code#s1",
                  "tool_name": "Read", "args": {"file_path": "rate_limiter.py"},
                  "result": {"output": "TODO: token bucket"}, "success": True, "duration_ms": 5},
        "prev_hash": "0" * 64, "entry_hash": "abc",
    }) + "\n")
    idx = EventIndex.from_paths(led)
    assert idx.refresh() == 1
    e = idx.events[0]
    assert e.seq == 7
    assert e.source_agent == "agent:claude-code#s1"
    assert e.tool_name == "Read"
    assert e.args == {"file_path": "rate_limiter.py"}
    assert e.triggered_by == 6
    assert "rate_limiter.py" in e.embed_text  # searchable via the nested args


def test_poison_record_does_not_freeze_the_index(tmp_path):
    """A record with a non-dict args (list), invalid JSON, or invalid UTF-8 between
    good events must be skipped — the offset still advances and good events index."""
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "first"})
    # poison: truthy non-dict args (escapes `or {}`), a garbage line, and a bad byte
    with f.open("ab") as fh:
        fh.write(json.dumps({"seq": 2, "tool_name": "Bad", "args": ["x", "y"], "result": [1, 2],
                             "source_agent": "a", "success": True}).encode() + b"\n")
        fh.write(b"{not valid json}\n")
        fh.write(b'{"seq": 3, "tool_name": "Edit", "args": {"file_path": "\xff bad utf8"}, '
                 b'"result": {}, "source_agent": "a", "success": true}\n')
    _write_event(f, 4, "user_prompt", {"prompt": "last"})

    idx = EventIndex.from_paths(f)
    added = idx.refresh()
    tools = sorted(e.tool_name for e in idx.events)
    assert "user_prompt" in tools and added >= 2, "good events must index despite poison lines"
    assert idx.refresh() == 0, "offset advanced past the poison; idempotent re-refresh"
