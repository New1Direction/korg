# Phase 1 — Plan 3: Canonical Capture Emit + `korg-hook` Driver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Claude Code sessions land in a verifiable `korg-ledger@v1` chain automatically, via a cross-platform `PostToolUse`/`Stop`/`SubagentStop` hook that reuses the existing session parser and writes through the Plan-1 `LedgerWriter` — no daemon, no running server.

**Architecture:** A new `EmitFn` (`make_canonical_emit`) maps the adapter's `body` dict into a `LedgerWriter.append`, so the existing `ClaudeCodeAdapter` + `parser.py` are reused unchanged. A short-lived `korg-hook` entrypoint runs on each hook firing: it locates the session transcript, full-reparses it (cheap, and — because `PostToolUse` fires after a tool's result is written — this captures tool results that incremental tail mode drops), seeds a fresh adapter from persisted per-session chain pointers, emits only the new tail `events[emitted_count:]` into a per-session ledger `~/.korg/sessions/<session_id>.jsonl`, then persists `{emitted_count, prompt_seq, llm_seq, root_eid}`. The hook always exits 0 and never blocks the agent.

**Tech Stack:** Python 3.9+, stdlib only; `korg-ledger-py` (Plan 1); the existing `claude_code_adapter` package; `pytest`.

---

## Design decisions (refinements to spec §3/§4.5, flagged)

1. **Per-session ledger files** `~/.korg/sessions/<session_id>.jsonl` rather than one interleaved `~/.korg/ledger.jsonl`. Rationale: one verifiable hash-chain per causal session (seq starts at 1, root at seq 1); eliminates cross-session append races; and a per-session file *is* the shareable "verify this session in a browser" receipt that Phase 3 publishes. Still one **format** (the unification goal). Readers glob `~/.korg/sessions/*.jsonl` (wired in Plan 6).
2. **Full-reparse + `emitted_count`** instead of byte-offset incremental. Rationale: the parser fills tool results by mutating the buffered event when the `tool_result` arrives (`parser.py:176-182`); incremental polls emit the tool event before its result and lose it. `PostToolUse` fires *after* the result is in the transcript, so a full reparse per firing has the result. Cost is O(transcript) per firing (milliseconds for normal sessions); noted as a future optimization.
3. **Concurrency-safe append**: `LedgerWriter.append` re-reads the chain tip under its exclusive lock so overlapping hook processes for the same session can never fork the chain or duplicate a `seq_id`. (Enhancement to the Plan-1 writer — Task 1.)
4. **`causation_id` left `null` in Phase 1**: `triggered_by` (seq) is the authoritative causal link checked by `verify_dag`; the UUID mirror is backfilled in a later phase. `root_event_id` *is* set correctly (persisted per session).

---

## File Structure

```
adapters/korg-ledger-py/src/korg_ledger/writer.py     # MODIFY: lock-safe tip re-read on append
adapters/korg-ledger-py/tests/test_writer.py          # MODIFY: concurrent-append test
adapters/claude-code/src/claude_code_adapter/
├── canonical_emit.py                                  # CREATE: make_canonical_emit EmitFn
└── hook.py                                            # CREATE: korg-hook driver + per-session state
adapters/claude-code/tests/
├── test_canonical_emit.py                             # CREATE
└── test_hook.py                                       # CREATE
adapters/claude-code/pyproject.toml                    # MODIFY: console_script + dep on korg-ledger-py
```

---

### Task 1: Make `LedgerWriter.append` concurrency-safe (lock + tip re-read)

**Files:**
- Modify: `adapters/korg-ledger-py/src/korg_ledger/writer.py`
- Test: `adapters/korg-ledger-py/tests/test_writer.py`

- [ ] **Step 1: Write the failing test** (append to `test_writer.py`)

```python
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
```

- [ ] **Step 2: Run it and watch it fail**

Run: `PYTHONPATH=adapters/korg-ledger-py/src python3 -m pytest adapters/korg-ledger-py/tests/test_writer.py::test_concurrent_writers_do_not_fork_the_chain -v`
Expected: FAIL — `b` uses its stale cached tip (seq 0), writes `seq_id=1` again with `prev_hash=GENESIS`, so `verify_chain` reports a broken chain / duplicate seq.

- [ ] **Step 3: Rewrite `append` + `_append_line` to re-read the tip under the lock**

Replace the `append` method's seq/prev computation and the `_append_line` helper with a single locked critical section. Add a `_read_tip` helper. The full new `append` and helpers:

```python
    def append(
        self,
        *,
        event: dict,
        actor_id: str,
        triggered_by: int | None = None,
        causation_id: str | None = None,
        root_event_id: str | None = None,
        event_id: str | None = None,
    ) -> int:
        eid = event_id or str(uuid.uuid4())
        with self.path.open("a+") as f:
            if fcntl is not None:
                fcntl.flock(f.fileno(), fcntl.LOCK_EX)
            try:
                last_seq, last_hash, last_hlc = self._read_tip(f)
                seq_id = last_seq + 1
                if triggered_by is not None and (triggered_by < 1 or triggered_by >= seq_id):
                    raise CausalityError(
                        f"triggered_by {triggered_by} is not a strictly-earlier seq_id (< {seq_id})"
                    )
                hlc = last_hlc.tick(int(time.time() * 1000))
                metadata = {
                    "event_id": eid,
                    "correlation_id": NIL_UUID,
                    "causation_id": causation_id,
                    "root_event_id": root_event_id or eid,
                    "actor_id": actor_id,
                    "campaign_id": NIL_UUID,
                    "emitted_at": hlc.as_dict(),
                    "branch_id": None,
                    "speculative": False,
                    "retry_count": 0,
                    "tier": "Telemetry",
                    "span_id": None,
                    "tags": {},
                    "triggered_by": triggered_by,
                }
                record = {
                    "schema_version": SCHEMA_VERSION,
                    "seq_id": seq_id,
                    "metadata": metadata,
                    "event": event,
                    "prev_hash": last_hash,
                }
                record["entry_hash"] = chain_hash(record, self._key)
                f.write(json.dumps(record, separators=(",", ":")) + "\n")
                f.flush()
                os.fsync(f.fileno())
                self._last_seq, self._last_hash, self._last_hlc = seq_id, record["entry_hash"], hlc
                return seq_id
            finally:
                if fcntl is not None:
                    fcntl.flock(f.fileno(), fcntl.LOCK_UN)

    def _read_tip(self, f) -> tuple[int, str, Hlc]:
        """Authoritative tip read from disk (caller holds the lock)."""
        last_seq, last_hash = 0, GENESIS
        last_hlc = Hlc(0, 0, self._hlc_actor_id)
        f.seek(0)
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                break
            last_seq = e["seq_id"]
            last_hash = e["entry_hash"]
            hlc = e["metadata"]["emitted_at"]
            last_hlc = Hlc(hlc["physical"], hlc["logical"], hlc.get("actor_id", self._hlc_actor_id))
        return last_seq, last_hash, last_hlc
```

In `__init__`, store the HLC actor id and seed the cache via the same path:

```python
    def __init__(self, path, hmac_key: bytes | None = None, hlc_actor_id: int = 1) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.touch(exist_ok=True)
        self._key = hmac_key
        self._hlc_actor_id = hlc_actor_id
        with self.path.open("r") as f:
            self._last_seq, self._last_hash, self._last_hlc = self._read_tip(f)
```

Delete the now-unused standalone `_resume` and `_append_line` methods (their logic now lives in `_read_tip` / `append`).

- [ ] **Step 4: Run the whole writer suite**

Run: `PYTHONPATH=adapters/korg-ledger-py/src python3 -m pytest adapters/korg-ledger-py/tests -v`
Expected: PASS — all prior tests plus the new concurrency test (the existing resume/torn-line/causality/HMAC tests still pass because `_read_tip` reuses the same parsing).

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-ledger-py/src/korg_ledger/writer.py adapters/korg-ledger-py/tests/test_writer.py
git commit -m "feat(korg-ledger-py): concurrency-safe append (re-read tip under lock)"
```

---

### Task 2: `make_canonical_emit` — the verifiable `EmitFn`

**Files:**
- Create: `adapters/claude-code/src/claude_code_adapter/canonical_emit.py`
- Test: `adapters/claude-code/tests/test_canonical_emit.py`

- [ ] **Step 1: Write the failing test**

```python
# adapters/claude-code/tests/test_canonical_emit.py
import json
import sys
from pathlib import Path

# make korg-ledger-py importable without an install step
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "korg-ledger-py" / "src"))

from claude_code_adapter.adapter import ClaudeCodeAdapter
from claude_code_adapter.canonical_emit import make_canonical_emit
from korg_ledger import verify_chain


def _lines(p):
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def test_emit_produces_verifiable_chain_with_causal_spine(tmp_path):
    led = tmp_path / "s.jsonl"
    emit = make_canonical_emit(led, actor_id="korg:claude-hook")
    adapter = ClaudeCodeAdapter(emit=emit, source_agent="agent:claude-code#sess1")
    # user prompt → assistant(llm + Read tool) → tool_result
    lines = [
        {"type": "user", "message": {"content": "fix the bug"}},
        {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 10, "output_tokens": 5},
            "content": [{"type": "text", "text": "reading"},
                        {"type": "tool_use", "id": "tu1", "name": "Read", "input": {"file": "a.py"}}]}},
        {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "tu1", "content": "file body", "is_error": False}]}},
    ]
    adapter.ingest(lines)
    events = _lines(led)
    # user_prompt(seq1) → llm_inference(seq2, tb=1) → Read(seq3, tb=2)
    assert [e["seq_id"] for e in events] == [1, 2, 3]
    assert events[0]["event"]["tool_name"] == "user_prompt"
    assert events[1]["event"]["tool_name"] == "llm_inference"
    assert events[1]["metadata"]["triggered_by"] == 1
    assert events[2]["event"]["tool_name"] == "Read"
    assert events[2]["metadata"]["triggered_by"] == 2
    assert events[2]["event"]["result"] == {"output": "file body"}   # result captured
    # all share the session root (event 1's event_id)
    root = events[0]["metadata"]["event_id"]
    assert all(e["metadata"]["root_event_id"] == root for e in events)
    assert verify_chain(events) == []


def test_emit_returns_none_on_causality_violation(tmp_path):
    led = tmp_path / "s.jsonl"
    emit = make_canonical_emit(led, actor_id="korg:claude-hook")
    # a body that claims to be triggered by a non-existent earlier seq
    seq = emit({"source_agent": "agent:claude-code#x", "tool_name": "Read",
                "args": {}, "result": {}, "success": True, "duration_ms": 0,
                "triggered_by": 99})
    assert seq is None
    assert not led.read_text().strip()  # nothing written
```

- [ ] **Step 2: Run it and watch it fail**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_canonical_emit.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'claude_code_adapter.canonical_emit'`.

- [ ] **Step 3: Write `canonical_emit.py`**

```python
# adapters/claude-code/src/claude_code_adapter/canonical_emit.py
"""make_canonical_emit — an EmitFn that writes verifiable korg-ledger@v1 events.

Drop-in replacement for make_jsonl_emit (tail.py): translates the adapter's
`body` dict into a LedgerWriter.append, returning the assigned global seq_id
so the adapter's triggered_by chaining works unchanged.
"""
from __future__ import annotations

import uuid
from pathlib import Path
from typing import Any, Callable, Optional

from korg_ledger import CausalityError, LedgerWriter, agent_tool_call_event

EmitFn = Callable[[dict], Optional[int]]


def make_canonical_emit(
    ledger_path: Path,
    *,
    actor_id: str = "korg:claude-hook",
    hmac_key: bytes | None = None,
    root_event_id: str | None = None,
) -> EmitFn:
    """Build an EmitFn appending to one per-session ledger file.

    `root_event_id` seeds the session root across short-lived hook firings;
    when None, the first emitted event becomes the root.
    """
    writer = LedgerWriter(ledger_path, hmac_key=hmac_key)
    state: dict[str, Any] = {"root": root_event_id}

    def emit(body: dict) -> Optional[int]:
        event_id = str(uuid.uuid4())
        root = state["root"] or event_id
        event = agent_tool_call_event(
            source_agent=body["source_agent"],
            tool_name=body["tool_name"],
            args=body.get("args", {}),
            result=body.get("result", {}),
            success=body.get("success", True),
            duration_ms=body.get("duration_ms", 0),
        )
        try:
            seq = writer.append(
                event=event,
                actor_id=actor_id,
                triggered_by=body.get("triggered_by"),
                root_event_id=root,
                event_id=event_id,
            )
        except CausalityError:
            return None
        if state["root"] is None:
            state["root"] = event_id
        return seq

    # expose the (possibly newly-set) root so the hook can persist it
    emit.root_event_id = lambda: state["root"]  # type: ignore[attr-defined]
    return emit
```

- [ ] **Step 4: Run it and watch it pass**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_canonical_emit.py -v`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add adapters/claude-code/src/claude_code_adapter/canonical_emit.py adapters/claude-code/tests/test_canonical_emit.py
git commit -m "feat(claude-code): make_canonical_emit — verifiable EmitFn over LedgerWriter"
```

---

### Task 3: `korg-hook` driver + per-session state

**Files:**
- Create: `adapters/claude-code/src/claude_code_adapter/hook.py`
- Test: `adapters/claude-code/tests/test_hook.py`

- [ ] **Step 1: Write the failing tests**

```python
# adapters/claude-code/tests/test_hook.py
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "korg-ledger-py" / "src"))

from claude_code_adapter.hook import run_hook
from korg_ledger import verify_chain

SESSION = [
    {"type": "user", "message": {"content": "fix the bug"}},
    {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 10, "output_tokens": 5},
        "content": [{"type": "tool_use", "id": "tu1", "name": "Read", "input": {"file": "a.py"}}]}},
    {"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "tu1", "content": "body", "is_error": False}]}},
]


def _write_transcript(path, records):
    path.write_text("".join(json.dumps(r) + "\n" for r in records))


def _ledger(korg_home, sid):
    p = korg_home / "sessions" / f"{sid}.jsonl"
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def test_hook_captures_a_verifiable_session(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    events = _ledger(korg_home, "sess-abc")
    assert [e["event"]["tool_name"] for e in events] == ["user_prompt", "llm_inference", "Read"]
    assert events[2]["event"]["result"] == {"output": "body"}
    assert verify_chain(events) == []


def test_second_firing_with_no_new_content_is_idempotent(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    run_hook(payload, korg_home=korg_home)  # same transcript, no growth
    events = _ledger(korg_home, "sess-abc")
    assert [e["seq_id"] for e in events] == [1, 2, 3]  # no duplicates


def test_incremental_firing_appends_only_new_events(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    # a follow-up turn lands, then the hook fires again
    _write_transcript(transcript, SESSION + [
        {"type": "user", "message": {"content": "now add a test"}},
        {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 3, "output_tokens": 2},
            "content": [{"type": "text", "text": "ok"}]}},
    ])
    run_hook(payload, korg_home=korg_home)
    events = _ledger(korg_home, "sess-abc")
    names = [e["event"]["tool_name"] for e in events]
    assert names == ["user_prompt", "llm_inference", "Read", "user_prompt", "llm_inference"]
    # follow-up user_prompt chains to the prior llm_inference (spec §2a)
    assert events[3]["metadata"]["triggered_by"] == 2
    assert events[4]["metadata"]["triggered_by"] == 2
    assert verify_chain(events) == []


def test_run_hook_never_raises_on_bad_input(tmp_path):
    # missing transcript_path / unreadable file must not raise (hook must exit 0)
    run_hook({"session_id": "x", "hook_event_name": "Stop"}, korg_home=tmp_path / ".korg")
    run_hook({"session_id": "y", "transcript_path": str(tmp_path / "nope.jsonl")},
             korg_home=tmp_path / ".korg")
```

- [ ] **Step 2: Run them and watch them fail**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_hook.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'claude_code_adapter.hook'`.

- [ ] **Step 3: Write `hook.py`**

```python
# adapters/claude-code/src/claude_code_adapter/hook.py
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
from typing import Any

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
        new_events = events[already:]
        if not new_events:
            return
        adapter.ingest_events(new_events)

        state["emitted_count"] = len(events)
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
```

- [ ] **Step 4: Add `parse_all` to the adapter** (a thin single-shot full parse with a *fresh* parser state, so re-parsing the whole transcript each firing is correct and independent of incremental state)

Modify `adapters/claude-code/src/claude_code_adapter/adapter.py` — add this method to `ClaudeCodeAdapter`:

```python
    def parse_all(self, lines):
        """Full single-shot parse of an entire transcript (fresh parser state).

        Used by the short-lived hook driver: re-parsing the whole file each
        firing captures tool results (which the buffered parser fills in by
        mutation) without persisting parser internals across processes.
        """
        from claude_code_adapter.parser import SessionState, parse_session
        return parse_session(lines, state=SessionState())
```

- [ ] **Step 5: Run the hook tests and watch them pass**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_hook.py -v`
Expected: PASS (4 passed) — capture verifiable, idempotent re-firing, incremental tail-only append with correct §2a chaining, and bad input never raises.

- [ ] **Step 6: Commit**

```bash
git add adapters/claude-code/src/claude_code_adapter/hook.py adapters/claude-code/src/claude_code_adapter/adapter.py adapters/claude-code/tests/test_hook.py
git commit -m "feat(claude-code): korg-hook driver — zero-config verifiable session capture"
```

---

### Task 4: Console entry point + cross-impl E2E

**Files:**
- Modify: `adapters/claude-code/pyproject.toml`
- Test: `adapters/claude-code/tests/test_hook.py` (add the oracle E2E)

- [ ] **Step 1: Register the `korg-hook` console script + declare the dependency**

In `adapters/claude-code/pyproject.toml`, under `[project.scripts]` add:

```toml
[project.scripts]
korg-hook = "claude_code_adapter.hook:main"
```

and add `"korg-ledger-py"` to `[project].dependencies` (path/editable install in dev).

- [ ] **Step 2: Write the failing cross-impl E2E test** (append to `test_hook.py`)

This proves the captured ledger verifies under the spec's INDEPENDENT Python oracle, not just our own `verify_chain`.

```python
def test_captured_ledger_verifies_under_spec_oracle(tmp_path):
    sys.path.insert(0, str(Path(__file__).resolve().parents[3] / ".." / "spec" / "korg-ledger-v1"))
    import conformance as oracle  # spec's independent reference verifier

    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-z.jsonl"
    _write_transcript(transcript, SESSION)
    run_hook({"session_id": "sess-z", "transcript_path": str(transcript),
              "hook_event_name": "PostToolUse"}, korg_home=korg_home)
    events = _ledger(korg_home, "sess-z")
    assert oracle.verify_chain(events, None) == []
    assert oracle.chain_hash(events[-1]) == events[-1]["entry_hash"]
```

- [ ] **Step 3: Run it and watch it pass**

Run: `PYTHONPATH=adapters/claude-code/src python3 -m pytest adapters/claude-code/tests/test_hook.py::test_captured_ledger_verifies_under_spec_oracle -v`
Expected: PASS — the hook-captured per-session ledger is verified clean by the spec's independent oracle.

- [ ] **Step 4: Full adapter suite + commit**

Run: `PYTHONPATH="adapters/claude-code/src:adapters/korg-ledger-py/src" python3 -m pytest adapters/claude-code/tests -v`
Expected: PASS (all canonical_emit + hook tests, plus the pre-existing adapter tests untouched).

```bash
git add adapters/claude-code/pyproject.toml adapters/claude-code/tests/test_hook.py
git commit -m "feat(claude-code): register korg-hook console script + spec-oracle E2E"
```

---

## Self-Review

**1. Spec coverage (§4.4, §4.5):** canonical emit replacing the flat backend ✓ (Task 2); reuses the parser + adapter unchanged ✓ (Tasks 2–3 import them, only *add* `parse_all`); hook driver reading `session_id`/`transcript_path`/`hook_event_name`, full-reparse, tail-only emit, persisted per-session state, exit-0 robustness ✓ (Task 3); cross-platform (a `settings.json` command, no daemon) ✓ (entry point Task 4; registration itself is Plan 4); verifiable output cross-checked by the spec oracle ✓ (Task 4). Deviations from §3/§4.5 (per-session files; full-reparse vs byte-offset; concurrency-safe append; `causation_id` null) are documented at the top with rationale.

**2. Placeholder scan:** No TBD/TODO; every code step shows complete code; every run step has an exact command + expected outcome.

**3. Type/name consistency:** `make_canonical_emit`, `EmitFn`, `run_hook`, `main`, `parse_all`, `ClaudeCodeAdapter(emit=, source_agent=)`, `ingest`/`ingest_events`, `_prompt_seq`/`_llm_seq`, `LedgerWriter.append(event=, actor_id=, triggered_by=, root_event_id=, event_id=)`, `agent_tool_call_event`, `verify_chain` are used identically across tasks and match the real signatures in `adapter.py`, `parser.py`, and `korg_ledger`. The `body` dict keys (`source_agent/tool_name/args/result/success/duration_ms/triggered_by`) match `adapter.py:85-92`. The captured event shape matches `examples/claude_code_session_ledger.json`.

**Note:** `make_jsonl_emit` (flat) is left in place but is superseded by `make_canonical_emit`; Plan 4 switches `korg-setup`/daemon defaults to canonical and Plan 6 removes the flat reader. No gaps found.
