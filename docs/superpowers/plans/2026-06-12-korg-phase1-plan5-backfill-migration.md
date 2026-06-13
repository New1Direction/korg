# Phase 1 — Plan 5: Retroactive Backfill + Legacy Flat-Ledger Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give existing users verifiable history with no behavior change — `korg-backfill` re-derives canonical per-session ledgers from every historical Claude Code transcript, and a guarded migrator converts a legacy flat `~/.korg/claude-events.jsonl` into the same per-session format.

**Architecture:** `backfill.py` discovers `~/.claude/projects/**/*.jsonl` and runs the Plan-3 `run_hook` capture once per transcript (idempotent via `emitted_count` state) → canonical `~/.korg/sessions/<id>.jsonl`. `migrate.py` reads the legacy flat JSONL, groups events by `source_agent` (session), and replays each group through a `LedgerWriter`, remapping the flat global `seq`→per-session `seq` for `triggered_by`. A `korg-backfill` console script drives both.

**Tech Stack:** Python 3.9+, stdlib only; `korg-ledger-py` (Plan 1); `claude_code_adapter` hook/parser (Plan 3); `pytest`.

---

## Design decision (flagged)

Backfill from the **source transcripts** is preferred over flat-file migration: the transcripts persist (Claude Code keeps them), carry richer data (tokens, cache, real result text), and produce output **identical to live capture**. The flat-file migrator is a guarded fallback for users whose transcripts were deleted — it **skips any session that already has a canonical ledger** (so it never duplicates what backfill produced) and is lossy (synthesized timestamps/UUIDs, `actor_id="korg:migrate"`).

---

## File Structure

```
adapters/claude-code/src/claude_code_adapter/
├── backfill.py     # CREATE: backfill_all / backfill_one (reuses run_hook)
└── migrate.py      # CREATE: migrate_flat_file (flat → per-session canonical, guarded)
adapters/claude-code/tests/
├── test_backfill.py    # CREATE
└── test_migrate.py     # CREATE
adapters/claude-code/pyproject.toml   # MODIFY: korg-backfill console script
```

---

### Task 1: `backfill.py` — retroactive capture from source transcripts

**Files:**
- Create: `adapters/claude-code/src/claude_code_adapter/backfill.py`
- Test: `adapters/claude-code/tests/test_backfill.py`

- [ ] **Step 1: Write the failing tests**

```python
# adapters/claude-code/tests/test_backfill.py
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
```

- [ ] **Step 2: Run them and watch them fail**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_backfill.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'claude_code_adapter.backfill'`.

- [ ] **Step 3: Write `backfill.py`**

```python
# adapters/claude-code/src/claude_code_adapter/backfill.py
"""korg-backfill — retroactively capture historical Claude Code sessions.

Discovers ~/.claude/projects/**/*.jsonl and runs the same capture path the
live korg-hook uses (full-reparse → canonical per-session ledger). Idempotent:
the per-session emitted_count state means re-running adds nothing new.
"""
from __future__ import annotations

import json
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
```

- [ ] **Step 4: Run them and watch them pass**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_backfill.py -v`
Expected: PASS (3 passed).

- [ ] **Step 5: Commit**

```bash
git add adapters/claude-code/src/claude_code_adapter/backfill.py adapters/claude-code/tests/test_backfill.py
git commit -m "feat(claude-code): korg-backfill — retroactive verifiable capture from transcripts"
```

---

### Task 2: `migrate.py` — legacy flat ledger → per-session canonical

**Files:**
- Create: `adapters/claude-code/src/claude_code_adapter/migrate.py`
- Test: `adapters/claude-code/tests/test_migrate.py`

- [ ] **Step 1: Write the failing tests**

```python
# adapters/claude-code/tests/test_migrate.py
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
```

- [ ] **Step 2: Run them and watch them fail**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_migrate.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'claude_code_adapter.migrate'`.

- [ ] **Step 3: Write `migrate.py`**

```python
# adapters/claude-code/src/claude_code_adapter/migrate.py
"""Migrate a legacy flat ~/.korg/claude-events.jsonl into per-session canonical ledgers.

Guarded fallback for users whose source transcripts are gone (backfill is
preferred — see backfill.py). Groups flat events by source_agent (session),
replays each group through a LedgerWriter, and remaps the flat global `seq`
to the new per-session `seq` for `triggered_by`. Skips any session that
already has a canonical ledger (so it never duplicates backfill output).
Lossy: timestamps/UUIDs are synthesized; actor_id is "korg:migrate".
"""
from __future__ import annotations

import json
import re
from collections import OrderedDict
from dataclasses import dataclass, field
from pathlib import Path

from korg_ledger import LedgerWriter, agent_tool_call_event


@dataclass
class MigrateReport:
    sessions: int = 0
    events_written: int = 0
    skipped: list = field(default_factory=list)


def _session_id(source_agent: str) -> str:
    if "#" in source_agent:
        return source_agent.split("#", 1)[1]
    return re.sub(r"[^A-Za-z0-9_.-]", "_", source_agent) or "unknown-session"


def migrate_flat_file(
    flat_path: Path,
    korg_home: Path | None = None,
    *,
    archive: bool = True,
) -> MigrateReport:
    home = korg_home or (Path.home() / ".korg")
    report = MigrateReport()
    if not flat_path.exists():
        return report

    events = [json.loads(ln) for ln in flat_path.read_text().splitlines() if ln.strip()]
    by_session: "OrderedDict[str, list]" = OrderedDict()
    for e in events:
        by_session.setdefault(e.get("source_agent", "unknown"), []).append(e)

    for source_agent, evs in by_session.items():
        sid = _session_id(source_agent)
        ledger = home / "sessions" / f"{sid}.jsonl"
        if ledger.exists():
            report.skipped.append(sid)
            continue
        writer = LedgerWriter(ledger)
        flat_to_new: dict[int, int] = {}
        for e in evs:
            tb_flat = e.get("triggered_by")
            tb_new = flat_to_new.get(tb_flat) if tb_flat is not None else None
            event = agent_tool_call_event(
                source_agent=source_agent,
                tool_name=e["tool_name"],
                args=e.get("args", {}),
                result=e.get("result", {}),
                success=e.get("success", True),
                duration_ms=e.get("duration_ms", 0),
            )
            new_seq = writer.append(event=event, actor_id="korg:migrate", triggered_by=tb_new)
            flat_to_new[e["seq"]] = new_seq
            report.events_written += 1
        report.sessions += 1

    if archive:
        flat_path.rename(Path(str(flat_path) + ".migrated"))
    return report
```

- [ ] **Step 4: Run them and watch them pass**

Run: `PYTHONPATH="adapters/claude-code/src:adapters/korg-ledger-py/src" python3 -m pytest adapters/claude-code/tests/test_migrate.py -v`
Expected: PASS (3 passed).

- [ ] **Step 5: Commit**

```bash
git add adapters/claude-code/src/claude_code_adapter/migrate.py adapters/claude-code/tests/test_migrate.py
git commit -m "feat(claude-code): migrate legacy flat ledger → per-session canonical (guarded)"
```

---

### Task 3: `korg-backfill` console entry point

**Files:**
- Create: `adapters/claude-code/src/claude_code_adapter/backfill_cli.py`
- Modify: `adapters/claude-code/pyproject.toml`

- [ ] **Step 1: Write the CLI**

```python
# adapters/claude-code/src/claude_code_adapter/backfill_cli.py
"""CLI for korg-backfill: retroactively capture history, optionally migrate a flat ledger."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from claude_code_adapter.backfill import DEFAULT_PROJECTS_DIR, backfill_all
from claude_code_adapter.migrate import migrate_flat_file


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="korg-backfill",
        description="Retroactively capture historical Claude Code sessions into verifiable per-session ledgers.",
    )
    parser.add_argument("--projects-dir", type=Path, default=DEFAULT_PROJECTS_DIR,
                        help=f"Claude Code projects dir (default {DEFAULT_PROJECTS_DIR})")
    parser.add_argument("--korg-home", type=Path, default=Path.home() / ".korg",
                        help="korg home (default ~/.korg)")
    parser.add_argument("--migrate-flat", type=Path, default=None,
                        help="also migrate this legacy flat claude-events.jsonl (guarded; backfill is preferred)")
    args = parser.parse_args(argv)

    report = backfill_all(projects_dir=args.projects_dir, korg_home=args.korg_home)
    print(f"backfill: {report.events_written} events across {report.sessions} session(s) "
          f"→ {args.korg_home}/sessions/", file=sys.stderr)

    if args.migrate_flat is not None:
        m = migrate_flat_file(args.migrate_flat, korg_home=args.korg_home)
        print(f"migrate: {m.events_written} events across {m.sessions} session(s); "
              f"skipped {len(m.skipped)} already-present", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Register the console script**

In `adapters/claude-code/pyproject.toml`, under `[project.scripts]` add:

```toml
korg-backfill      = "claude_code_adapter.backfill_cli:main"
```

- [ ] **Step 3: Smoke-test + full adapter suite**

Run:
```bash
PYTHONPATH="adapters/claude-code/src:adapters/korg-ledger-py/src" python3 -m claude_code_adapter.backfill_cli --projects-dir /tmp/nope --korg-home /tmp/kb-home
PYTHONPATH="adapters/claude-code/src:adapters/korg-ledger-py/src" python3 -m pytest adapters/claude-code/tests -q
```
Expected: the CLI prints "backfill: 0 events across 0 session(s)" and exits 0; the full adapter suite passes (canonical_emit + hook + backfill + migrate + pre-existing).

- [ ] **Step 4: Commit**

```bash
git add adapters/claude-code/src/claude_code_adapter/backfill_cli.py adapters/claude-code/pyproject.toml
git commit -m "feat(claude-code): korg-backfill console entry point"
```

---

## Self-Review

**1. Spec coverage (§4.7):** one-time migration of the legacy flat ledger ✓ (Task 2, with the field mapping + `triggered_by` remap + archive); optional backfill over all historical transcripts ✓ (Task 1); both produce canonical, `verify_chain`-clean per-session ledgers ✓ (tests). The spec's single-`ledger.jsonl` framing is superseded by the per-session model (Plan 3); documented at the top with rationale (backfill from source is richer and preferred; migration is guarded to never duplicate backfill).

**2. Placeholder scan:** No TBD/TODO; complete code in every code step; exact commands + expected output.

**3. Type/name consistency:** `backfill_all`/`backfill_one`/`BackfillReport(sessions, events_written, per_session)`, `migrate_flat_file`/`MigrateReport(sessions, events_written, skipped)`, `run_hook`, `LedgerWriter.append(event=, actor_id=, triggered_by=)`, `agent_tool_call_event` used identically across modules, tests, and the CLI. `_session_id` extraction matches the `agent:claude-code#<uuid>` convention written by the flat tail emitter and the per-session ledger filenames used by Plan 3 (`~/.korg/sessions/<id>.jsonl`). Idempotency relies on Plan 3's `emitted_count` state (Task 1) and the existing-ledger skip (Task 2). No gaps found.
