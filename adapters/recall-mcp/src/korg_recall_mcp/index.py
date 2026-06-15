"""Read korg ledger events from one or more .jsonl files and keep a
materialized index in memory.

Designed for the layout produced by `korg-ingest-claude --tail`:
- One or more `*.jsonl` files, each line is a `{seq, source_agent, ...}` event.
- New events are appended; the file grows.
- We never want to re-read what we've already loaded.

EventIndex tracks per-file byte offsets so successive `refresh()` calls
only load new lines, mirroring the tail adapter's design.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable


def _normalize_record(obj: dict) -> dict:
    """Return a flat {seq, source_agent, tool_name, args, result, triggered_by, success}
    from EITHER a legacy flat record OR a canonical korg-ledger@v1 JournalEvent."""
    ev = obj.get("event")
    if isinstance(ev, dict) and "tool_name" in ev:  # canonical JournalEvent
        meta = obj.get("metadata") or {}
        return {
            "seq": obj.get("seq_id", 0),
            "source_agent": ev.get("source_agent", ""),
            "tool_name": ev.get("tool_name", ""),
            "args": ev.get("args") or {},
            "result": ev.get("result") or {},
            "triggered_by": meta.get("triggered_by"),
            "success": ev.get("success", True),
        }
    return {  # legacy flat
        "seq": obj.get("seq", 0),
        "source_agent": obj.get("source_agent", ""),
        "tool_name": obj.get("tool_name", ""),
        "args": obj.get("args") or {},
        "result": obj.get("result") or {},
        "triggered_by": obj.get("triggered_by"),
        "success": obj.get("success", True),
    }


@dataclass
class IndexedEvent:
    """One event with the metadata needed for ranking and display.

    `embed_text` is the flattened text the search engine will compare against
    a query; it's computed once at load time and cached on the instance.
    `embedding` is filled in lazily by the search engine when it needs vectors.
    """

    source_file: str          # absolute path of the originating .jsonl
    seq: int                  # the seq id assigned by the emit() backend
    source_agent: str
    tool_name: str
    args: dict[str, Any]
    result: dict[str, Any]
    embed_text: str
    embedding: list[float] | None = None
    # Optional fields preserved for filtering/display
    triggered_by: int | None = None
    success: bool = True


@dataclass
class EventIndex:
    """In-memory index over one or more ledger .jsonl files."""

    ledger_paths: list[Path]
    events: list[IndexedEvent] = field(default_factory=list)
    # Per-file byte offset of "what we've already loaded".
    _offsets: dict[str, int] = field(default_factory=dict)

    @classmethod
    def from_paths(cls, *paths: Path) -> "EventIndex":
        # Accept absolute or expandable paths; existence is checked at refresh.
        return cls(ledger_paths=[Path(p).expanduser() for p in paths])

    @classmethod
    def from_dir(cls, directory: Path, glob: str = "*.jsonl") -> "EventIndex":
        directory = Path(directory).expanduser()
        return cls(ledger_paths=list(directory.glob(glob)) if directory.exists() else [])

    def refresh(self) -> int:
        """Load any new events appended since the last refresh.

        Returns the number of new events added. Idempotent.
        """
        from korg_recall_mcp.text import text_for_event

        added = 0
        # Re-discover the file list each refresh — new files appearing in the
        # dir between refreshes get picked up automatically.
        candidate_paths: list[Path] = []
        for p in self.ledger_paths:
            if p.is_dir():
                candidate_paths.extend(sorted(p.glob("*.jsonl")))
            elif p.exists():
                candidate_paths.append(p)
        # Stable order for deterministic ranking ties
        candidate_paths = sorted(set(candidate_paths))

        for path in candidate_paths:
            key = str(path)
            try:
                size = path.stat().st_size
            except OSError:
                continue
            offset = self._offsets.get(key, 0)
            if size <= offset:
                continue
            try:
                with path.open("r", encoding="utf-8", errors="replace") as f:
                    f.seek(offset)
                    chunk = f.read(size - offset)
            except OSError:
                continue
            if "\n" not in chunk:
                continue
            complete, _partial = chunk.rsplit("\n", 1)
            consumed = len(complete.encode("utf-8")) + 1
            for line in complete.split("\n"):
                line = line.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if not isinstance(obj, dict):
                    continue
                rec = _normalize_record(obj)
                embed_text = text_for_event(rec)
                if not embed_text:
                    continue
                self.events.append(
                    IndexedEvent(
                        source_file=key,
                        seq=int(rec["seq"] or 0),
                        source_agent=str(rec["source_agent"]),
                        tool_name=str(rec["tool_name"]),
                        args=dict(rec["args"]),
                        result=dict(rec["result"]),
                        embed_text=embed_text,
                        triggered_by=rec["triggered_by"],
                        success=bool(rec["success"]),
                    )
                )
                added += 1
            self._offsets[key] = offset + consumed
        return added

    # ── Convenience accessors ─────────────────────────────────────────

    def __len__(self) -> int:
        return len(self.events)

    def by_tool(self, tool_name: str) -> Iterable[IndexedEvent]:
        return (e for e in self.events if e.tool_name == tool_name)

    def by_file(self, file_path: str | Path) -> Iterable[IndexedEvent]:
        target = str(file_path)
        return (e for e in self.events if e.source_file == target)
