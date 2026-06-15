import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "korg-ledger-py" / "src"))

from claude_code_adapter.migrate import migrate_flat_file
from korg_ledger import verify_chain

# legacy flat format: one global seq counter, source_agent encodes session, triggered_by = global seq
FLAT = [
    {"seq": 1, "source_agent": "agent:claude-code#sess-a", "tool_name": "user_prompt",
     "args": {"prompt": "x"}, "result": {}, "success": True, "duration_ms": 0},
    {"seq": 2, "source_agent": "agent:claude-code#sess-a", "tool_name": "llm_inference",
     "args": {}, "result": {}, "success": True, "duration_ms": 0, "triggered_by": 1},
    {"seq": 3, "source_agent": "agent:claude-code#sess-b", "tool_name": "user_prompt",
     "args": {"prompt": "y"}, "result": {}, "success": True, "duration_ms": 0},
    {"seq": 4, "source_agent": "agent:claude-code#sess-a", "tool_name": "Read",
     "args": {"f": "a"}, "result": {"output": "body"}, "success": True, "duration_ms": 5, "triggered_by": 2},
]


def _write_flat(path, events):
    path.write_text("".join(json.dumps(e) + "\n" for e in events))


def _ledger(korg_home, sid):
    return [json.loads(l) for l in (korg_home / "sessions" / f"{sid}.jsonl").read_text().splitlines() if l.strip()]


def test_migrate_splits_by_session_and_remaps_causality(tmp_path):
    flat = tmp_path / "claude-events.jsonl"
    _write_flat(flat, FLAT)
    korg_home = tmp_path / ".korg"
    report = migrate_flat_file(flat, korg_home=korg_home)
    assert report.sessions == 2
    a = _ledger(korg_home, "sess-a")
    # sess-a: 3 events with per-session seq 1,2,3; triggered_by remapped to per-session seqs
    assert [e["seq_id"] for e in a] == [1, 2, 3]
    assert [e["event"]["tool_name"] for e in a] == ["user_prompt", "llm_inference", "Read"]
    assert a[1]["metadata"]["triggered_by"] == 1   # was flat seq 1 → per-session seq 1
    assert a[2]["metadata"]["triggered_by"] == 2   # was flat seq 2 → per-session seq 2
    assert a[2]["event"]["result"] == {"output": "body"}
    assert verify_chain(a) == []
    assert verify_chain(_ledger(korg_home, "sess-b")) == []


def test_migrate_archives_the_flat_file(tmp_path):
    flat = tmp_path / "claude-events.jsonl"
    _write_flat(flat, FLAT)
    migrate_flat_file(flat, korg_home=tmp_path / ".korg")
    assert not flat.exists()
    assert (tmp_path / "claude-events.jsonl.migrated").exists()


def test_migrate_skips_sessions_with_existing_ledger(tmp_path):
    flat = tmp_path / "claude-events.jsonl"
    _write_flat(flat, FLAT)
    korg_home = tmp_path / ".korg"
    # pretend sess-a was already backfilled
    (korg_home / "sessions").mkdir(parents=True)
    (korg_home / "sessions" / "sess-a.jsonl").write_text("")
    report = migrate_flat_file(flat, korg_home=korg_home)
    assert "sess-a" in report.skipped
    assert report.sessions == 1  # only sess-b migrated
