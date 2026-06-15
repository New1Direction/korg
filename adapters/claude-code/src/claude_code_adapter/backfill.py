"""korg-backfill — retroactively capture historical Claude Code sessions.

Discovers ~/.claude/projects/**/*.jsonl and runs the same capture path the
live korg-hook uses (full-reparse → canonical per-session ledger). Idempotent:
the per-session emitted_count state means re-running adds nothing new.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

from claude_code_adapter.hook import run_hook

DEFAULT_PROJECTS_DIR = Path.home() / ".claude" / "projects"


@dataclass
class BackfillReport:
    sessions: int = 0
    events_written: int = 0
    per_session: dict = field(default_factory=dict)  # session_id → events written


def _ledger_len(korg_home: Path, session_id: str) -> int:
    p = korg_home / "sessions" / f"{session_id}.jsonl"
    if not p.exists():
        return 0
    return sum(1 for ln in p.read_text().splitlines() if ln.strip())


def backfill_one(transcript_path: Path, korg_home: Path) -> int:
    """Capture one transcript; return the number of new events written."""
    session_id = transcript_path.stem
    before = _ledger_len(korg_home, session_id)
    run_hook(
        {"session_id": session_id, "transcript_path": str(transcript_path),
         "hook_event_name": "Backfill"},
        korg_home=korg_home,
    )
    return _ledger_len(korg_home, session_id) - before


def backfill_all(
    projects_dir: Path = DEFAULT_PROJECTS_DIR,
    korg_home: Path | None = None,
) -> BackfillReport:
    """Backfill every transcript under projects_dir. Idempotent."""
    home = korg_home or (Path.home() / ".korg")
    report = BackfillReport()
    if not projects_dir.exists():
        return report
    for path in sorted(projects_dir.glob("**/*.jsonl")):
        written = backfill_one(path, home)
        report.sessions += 1
        report.events_written += written
        report.per_session[path.stem] = written
    return report
