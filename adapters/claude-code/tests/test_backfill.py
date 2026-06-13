import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "korg-ledger-py" / "src"))

from claude_code_adapter.backfill import backfill_all
from korg_ledger import verify_chain

SESSION_A = [
    {"type": "user", "message": {"content": "fix bug"}},
    {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 5, "output_tokens": 2},
        "content": [{"type": "tool_use", "id": "t1", "name": "Read", "input": {"f": "a.py"}}]}},
    {"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": "body", "is_error": False}]}},
]
SESSION_B = [
    {"type": "user", "message": {"content": "write a test"}},
    {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 3, "output_tokens": 1},
        "content": [{"type": "text", "text": "done"}]}},
]


def _make_projects(root, sessions: dict):
    proj = root / "projects" / "my-repo"
    proj.mkdir(parents=True)
    for sid, records in sessions.items():
        (proj / f"{sid}.jsonl").write_text("".join(json.dumps(r) + "\n" for r in records))
    return root / "projects"


def _ledger(korg_home, sid):
    return [json.loads(l) for l in (korg_home / "sessions" / f"{sid}.jsonl").read_text().splitlines() if l.strip()]


def test_backfill_produces_verifiable_per_session_ledgers(tmp_path):
    projects = _make_projects(tmp_path, {"sess-a": SESSION_A, "sess-b": SESSION_B})
    korg_home = tmp_path / ".korg"
    report = backfill_all(projects_dir=projects, korg_home=korg_home)
    assert report.sessions == 2
    assert report.events_written == 3 + 2
    a = _ledger(korg_home, "sess-a")
    assert [e["event"]["tool_name"] for e in a] == ["user_prompt", "llm_inference", "Read"]
    assert verify_chain(a) == []
    assert verify_chain(_ledger(korg_home, "sess-b")) == []


def test_backfill_is_idempotent(tmp_path):
    projects = _make_projects(tmp_path, {"sess-a": SESSION_A})
    korg_home = tmp_path / ".korg"
    backfill_all(projects_dir=projects, korg_home=korg_home)
    report2 = backfill_all(projects_dir=projects, korg_home=korg_home)  # second pass
    assert report2.events_written == 0  # nothing new
    assert len(_ledger(korg_home, "sess-a")) == 3  # no duplicates


def test_backfill_missing_projects_dir_is_safe(tmp_path):
    report = backfill_all(projects_dir=tmp_path / "nope", korg_home=tmp_path / ".korg")
    assert report.sessions == 0
    assert report.events_written == 0
