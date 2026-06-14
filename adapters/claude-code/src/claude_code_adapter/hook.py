"""korg-hook — zero-config Claude Code capture driver.

Registered as PostToolUse / Stop / SubagentStop hooks. On each firing it
full-reparses the session transcript (so tool results, present post-
PostToolUse, are captured), seeds a fresh adapter from persisted per-session
chain pointers, and appends only the new tail events to a per-session
verifiable ledger. It NEVER raises and always exits 0 — capture must never
break or slow a Claude session.
"""
from __future__ import annotations

import json
import os
import sys
import traceback
from pathlib import Path

from claude_code_adapter.adapter import ClaudeCodeAdapter
from claude_code_adapter.canonical_emit import make_canonical_emit


def _korg_home() -> Path:
    return Path(os.environ.get("KORG_HOME", str(Path.home() / ".korg")))


def _state_path(korg_home: Path, session_id: str) -> Path:
    return korg_home / "hook-state" / f"{session_id}.json"


def _load_state(path: Path) -> dict:
    if path.exists():
        try:
            return json.loads(path.read_text())
        except (json.JSONDecodeError, OSError):
            pass
    return {"emitted_count": 0, "prompt_seq": None, "llm_seq": None, "root_eid": None}


def _save_state(path: Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(state))
    os.replace(tmp, path)


def run_hook(payload: dict, korg_home: Path | None = None) -> None:
    """Process one hook firing. Never raises."""
    try:
        home = korg_home or _korg_home()
        session_id = payload.get("session_id") or "unknown-session"
        transcript = payload.get("transcript_path")
        if not transcript:
            return
        tpath = Path(transcript)
        if not tpath.exists():
            return

        records = [ln for ln in tpath.read_text(errors="replace").splitlines() if ln.strip()]

        state_path = _state_path(home, session_id)
        state = _load_state(state_path)
        already = state["emitted_count"]

        ledger = home / "sessions" / f"{session_id}.jsonl"
        emit = make_canonical_emit(
            ledger, actor_id="korg:claude-hook", root_event_id=state["root_eid"]
        )
        adapter = ClaudeCodeAdapter(
            emit=emit, source_agent=f"agent:claude-code#{session_id}"
        )
        # seed the chain pointers so the new tail chains to prior firings
        adapter._prompt_seq = state["prompt_seq"]
        adapter._llm_seq = state["llm_seq"]

        events = adapter.parse_all(records)  # full parse, single-shot semantics
        tail = events[already:]

        # Hold the watermark BEFORE the first tail tool event whose result hasn't
        # landed yet. The ledger is append-only and cannot patch an already-emitted
        # event, so emitting a tool_in_round with result={} now would lose its
        # output forever (parallel/multi-tool rounds land results across firings).
        # A later PostToolUse — or the Stop hook, fired once all results are in —
        # re-parses with the result present and emits it then.
        safe_len = len(tail)
        for i, ev in enumerate(tail):
            if ev.causal_role == "tool_in_round" and not ev.result:
                safe_len = i
                break
        new_events = tail[:safe_len]
        if not new_events:
            return
        adapter.ingest_events(new_events)

        state["emitted_count"] = already + safe_len
        state["prompt_seq"] = adapter._prompt_seq
        state["llm_seq"] = adapter._llm_seq
        state["root_eid"] = emit.root_event_id()  # type: ignore[attr-defined]
        _save_state(state_path, state)
    except Exception:  # never break the agent
        try:
            log = (korg_home or _korg_home()) / "logs" / "korg-hook.log"
            log.parent.mkdir(parents=True, exist_ok=True)
            with log.open("a") as f:
                f.write(traceback.format_exc() + "\n")
        except Exception:
            pass


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return 0
    run_hook(payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
