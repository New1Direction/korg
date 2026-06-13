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
