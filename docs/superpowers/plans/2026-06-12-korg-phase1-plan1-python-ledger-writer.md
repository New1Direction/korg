# Phase 1 — Plan 1: Pure-Python Conformant Ledger Writer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `korg-ledger-py`, a stdlib-only Python package that *produces* `korg-ledger@v1`-conformant `JournalEvent` JSONL (hash-chained, HLC-ordered, causality-gated) and is pinned to the frozen spec vectors — the foundation every capture path will write through.

**Architecture:** A small package (`korg_ledger`) with four focused modules: `_hash` (canonicalization + `chain_hash` + `verify_chain`, vendored from `spec/korg-ledger-v1/conformance.py` and guarded by a conformance test against the frozen vectors), `_hlc` (Hybrid Logical Clock tick mirroring the Rust semantics), `_events` (the `AgentToolCall` event builder), and `writer` (`LedgerWriter`: assigns `seq_id`/`prev_hash`/HLC, builds the full `JournalEvent`, enforces strictly-earlier causality, appends one JSON object per line under an exclusive file lock, and resumes from an existing ledger tip). Self-consistency is proven by the package's own `verify_chain`; cross-language equivalence is guaranteed by reproducing the frozen `tip_entry_hash` vectors. (Rust-verifies-Python-output is exercised later in Plan 6.)

**Tech Stack:** Python 3.9+, stdlib only (`hashlib`, `hmac`, `json`, `uuid`, `time`, `fcntl`), `pytest`, `hatchling` build backend.

---

## Plan Set (Phase 1 sequence)

Phase 1 (spec: `docs/superpowers/specs/2026-06-12-korg-phase1-unified-verifiable-capture-design.md`) is delivered as six sequenced sub-plans, each producing working, testable software on its own. **This document is Plan 1.** The rest get their own plan files as we reach them.

| # | Sub-plan | Spec §| Depends on |
|---|---|---|---|
| **1** | **Pure-Python conformant ledger writer (`korg-ledger-py`)** | 4.3 | — (keystone) |
| 2 | Rust format reservations (`event_sig`, `LedgerRewind`) + JSONL on-disk for `CapabilityJournal` | 4.1, 4.2 | coordinates `HASH_FIELDS` with 1 |
| 3 | Canonical capture emit + `korg-hook` driver (reuse parser) | 4.4, 4.5 | 1 |
| 4 | `korg-setup` `settings.json` hook registration + daemon demotion | 4.6 | 3 |
| 5 | Flat→canonical migration + optional backfill | 4.7 | 1 |
| 6 | Readers unification + cross-producer E2E/CI + honest docs | 4.8, 4.9, 4.10 | 1, 2, 3 |

**Scope discipline:** Plan 1 keeps `HASH_FIELDS = ("entry_hash",)` exactly as the *current* frozen spec. The `event_sig` reservation (spec §4.1) is a coordinated Rust + Python + JS change landed in Plan 2 — adding it here alone would break the 3-language conformance.

---

## File Structure

```
adapters/korg-ledger-py/
├── pyproject.toml                      # package metadata, hatchling build
├── README.md                           # what this is + conformance note
├── src/korg_ledger/
│   ├── __init__.py                     # public exports
│   ├── _hash.py                        # canonicalize, chain_hash, verify_chain, GENESIS, HASH_FIELDS
│   ├── _hlc.py                         # Hlc dataclass + tick()
│   ├── _events.py                      # agent_tool_call_event(), NIL_UUID
│   └── writer.py                       # LedgerWriter, CausalityError
└── tests/
    ├── test_conformance.py             # reproduces frozen spec vectors (drift guard)
    ├── test_hlc.py                     # HLC monotonicity
    └── test_writer.py                  # append/chain/causality/resume/HMAC
```

Each module has one responsibility. `_hash` is the only place canonicalization lives; everything else imports it.

---

### Task 1: Scaffold the `korg-ledger-py` package

**Files:**
- Create: `adapters/korg-ledger-py/pyproject.toml`
- Create: `adapters/korg-ledger-py/src/korg_ledger/__init__.py`
- Create: `adapters/korg-ledger-py/README.md`

- [ ] **Step 1: Create `pyproject.toml`**

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "korg-ledger-py"
version = "0.1.0"
description = "Pure-Python conformant producer + verifier for the korg-ledger@v1 tamper-evident ledger format."
readme = "README.md"
requires-python = ">=3.9"
license = { text = "MIT OR Apache-2.0" }
dependencies = []

[project.optional-dependencies]
dev = ["pytest>=7"]

[tool.hatch.build.targets.wheel]
packages = ["src/korg_ledger"]
```

- [ ] **Step 2: Create the package `__init__.py` with public exports**

```python
"""korg-ledger@v1 — pure-Python conformant producer + verifier.

Third independent implementation of the frozen korg-ledger@v1 spec
(alongside the Rust `korg-ledger` crate and the JS `verify.mjs`). Produces
JournalEvent JSONL that the Rust `korg-verify` validates.
"""
from __future__ import annotations

from ._events import NIL_UUID, agent_tool_call_event
from ._hash import GENESIS, HASH_FIELDS, canonicalize, chain_hash, verify_chain
from ._hlc import Hlc
from .writer import CausalityError, LedgerWriter

__all__ = [
    "GENESIS",
    "HASH_FIELDS",
    "NIL_UUID",
    "Hlc",
    "LedgerWriter",
    "CausalityError",
    "canonicalize",
    "chain_hash",
    "verify_chain",
    "agent_tool_call_event",
]
```

- [ ] **Step 3: Create `README.md`**

```markdown
# korg-ledger-py

Pure-Python (stdlib-only) producer and verifier for the **korg-ledger@v1**
tamper-evident ledger format. It is a third independent implementation of the
frozen spec at `spec/korg-ledger-v1/`, pinned to the same conformance vectors
as the Rust and JavaScript references.

`LedgerWriter` produces hash-chained `JournalEvent` JSONL that the Rust
`korg-verify` binary validates byte-for-byte.

    pip install -e adapters/korg-ledger-py
    pytest adapters/korg-ledger-py/tests -v
```

- [ ] **Step 4: Install editable and verify import fails cleanly (modules not written yet)**

Run: `pip install -e adapters/korg-ledger-py && python -c "import korg_ledger"`
Expected: install succeeds; the `import` FAILS with `ModuleNotFoundError: No module named 'korg_ledger._events'` (the submodules don't exist yet — that's expected; Task 2 onward creates them).

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-ledger-py/pyproject.toml adapters/korg-ledger-py/src/korg_ledger/__init__.py adapters/korg-ledger-py/README.md
git commit -m "chore(korg-ledger-py): scaffold pure-Python ledger package"
```

---

### Task 2: `_hash.py` — canonicalization + hash chain (pinned to frozen vectors)

**Files:**
- Create: `adapters/korg-ledger-py/src/korg_ledger/_hash.py`
- Test: `adapters/korg-ledger-py/tests/test_conformance.py`

- [ ] **Step 1: Write the failing conformance test**

This reads the frozen vectors from the repo spec dir and asserts our implementation reproduces the pinned tip hashes and detects tampering — the same oracle the Rust/JS references answer to.

```python
# adapters/korg-ledger-py/tests/test_conformance.py
import json
from pathlib import Path

import pytest

from korg_ledger import chain_hash, verify_chain

# tests/ -> korg-ledger-py -> adapters -> <repo root>
SPEC = Path(__file__).resolve().parents[3] / "spec" / "korg-ledger-v1"


def _vectors():
    manifest = json.loads((SPEC / "conformance.json").read_text())
    assert manifest["spec_version"] == "korg-ledger@v1"
    return manifest["vectors"]


def _read(name):
    text = (SPEC / "vectors" / name).read_text()
    return [json.loads(line) for line in text.splitlines() if line.strip()]


@pytest.mark.skipif(not SPEC.exists(), reason="spec vectors not present")
def test_reproduces_frozen_tip_hashes():
    for v in _vectors():
        if v["verify"] != "intact":
            continue
        events = _read(v["file"])
        key = v["key"].encode() if v.get("key") else None
        assert verify_chain(events, key) == [], f"{v['file']} should verify clean"
        assert chain_hash(events[-1], key) == v["tip_entry_hash"], v["file"]


@pytest.mark.skipif(not SPEC.exists(), reason="spec vectors not present")
def test_detects_tampering():
    for v in _vectors():
        if v["verify"] != "tampered":
            continue
        events = _read(v["file"])
        key = v["key"].encode() if v.get("key") else None
        errors = verify_chain(events, key)
        assert errors, f"{v['file']} should report tampering"
        assert any(v["error_contains"] in e for e in errors), errors
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pytest adapters/korg-ledger-py/tests/test_conformance.py -v`
Expected: FAIL — `ImportError: cannot import name 'chain_hash'` (`_hash.py` not written yet).

- [ ] **Step 3: Write `_hash.py`**

Mirrors `spec/korg-ledger-v1/conformance.py` exactly so it reproduces the frozen vectors. `HASH_FIELDS` is a module constant so the Plan 2 `event_sig` change is a one-line edit.

```python
# adapters/korg-ledger-py/src/korg_ledger/_hash.py
"""korg-ledger@v1 canonicalization + hash chain (stdlib only).

Byte-for-byte equivalent to spec/korg-ledger-v1/conformance.py and the Rust
`korg-ledger` crate. Equivalence is pinned by tests/test_conformance.py.
"""
from __future__ import annotations

import hashlib
import hmac
import json

#: prev_hash of the first event in a journal (64 zero hex chars).
GENESIS = "0" * 64

#: Fields that ARE the hash/signature and so are excluded from the preimage.
#: korg-ledger@v1 ships ("entry_hash",); "event_sig" is added in a coordinated
#: Rust + Python + JS change (Plan 2), never alone.
HASH_FIELDS = ("entry_hash",)


def canonicalize(value) -> bytes:
    """JSON, keys sorted by code point, no whitespace, non-ASCII \\uXXXX-escaped."""
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("ascii")


def chain_hash(event: dict, key: bytes | None = None) -> str:
    """SHA-256 (or HMAC-SHA256 with a key) over the canonical preimage."""
    preimage = {k: v for k, v in event.items() if k not in HASH_FIELDS}
    data = canonicalize(preimage)
    if key is not None:
        return hmac.new(key, data, hashlib.sha256).hexdigest()
    return hashlib.sha256(data).hexdigest()


def verify_chain(events: list, key: bytes | None = None) -> list:
    """Recompute the chain; empty list iff intact. Each error names a seq_id."""
    errors: list[str] = []
    expected_prev: str | None = GENESIS
    for e in events:
        sid = e.get("seq_id")
        stored = e.get("entry_hash")
        if stored is None:
            errors.append(f"seq {sid}: missing entry_hash")
            expected_prev = None
            continue
        if e.get("prev_hash") != expected_prev:
            errors.append(f"seq {sid}: prev_hash breaks the chain")
        if chain_hash(e, key) != stored:
            errors.append(f"seq {sid}: entry_hash mismatch (content tampered)")
        expected_prev = stored
    return errors
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pytest adapters/korg-ledger-py/tests/test_conformance.py -v`
Expected: PASS (2 passed) — both frozen tip hashes reproduced, both tamper vectors detected.

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-ledger-py/src/korg_ledger/_hash.py adapters/korg-ledger-py/tests/test_conformance.py
git commit -m "feat(korg-ledger-py): canonicalization + hash chain pinned to frozen vectors"
```

---

### Task 3: `_hlc.py` — Hybrid Logical Clock tick

**Files:**
- Create: `adapters/korg-ledger-py/src/korg_ledger/_hlc.py`
- Test: `adapters/korg-ledger-py/tests/test_hlc.py`

- [ ] **Step 1: Write the failing test**

Mirrors the Rust `HlcTimestamp::tick` semantics (`crates/korg-registry/src/log.rs:220`): same physical ⇒ logical+1; advancing physical ⇒ logical resets to 0; the clock never moves backward.

```python
# adapters/korg-ledger-py/tests/test_hlc.py
from korg_ledger import Hlc


def test_same_physical_increments_logical():
    clock = Hlc(physical=1000, logical=0, actor_id=1)
    nxt = clock.tick(wall_clock_ms=1000)
    assert (nxt.physical, nxt.logical) == (1000, 1)


def test_advancing_physical_resets_logical():
    clock = Hlc(physical=1000, logical=5, actor_id=1)
    nxt = clock.tick(wall_clock_ms=2000)
    assert (nxt.physical, nxt.logical) == (2000, 0)


def test_clock_never_moves_backward():
    clock = Hlc(physical=5000, logical=3, actor_id=1)
    nxt = clock.tick(wall_clock_ms=1000)  # wall clock behind logical time
    assert nxt.physical == 5000
    assert nxt.logical == 4


def test_as_dict_shape():
    assert Hlc(7, 2, 1).as_dict() == {"physical": 7, "logical": 2, "actor_id": 1}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pytest adapters/korg-ledger-py/tests/test_hlc.py -v`
Expected: FAIL — `ImportError: cannot import name 'Hlc'`.

- [ ] **Step 3: Write `_hlc.py`**

```python
# adapters/korg-ledger-py/src/korg_ledger/_hlc.py
"""Hybrid Logical Clock — mirrors crates/korg-registry/src/log.rs HlcTimestamp."""
from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Hlc:
    physical: int
    logical: int
    actor_id: int = 1

    def tick(self, wall_clock_ms: int) -> "Hlc":
        new_physical = max(wall_clock_ms, self.physical)
        new_logical = self.logical + 1 if new_physical == self.physical else 0
        return Hlc(new_physical, new_logical, self.actor_id)

    def as_dict(self) -> dict:
        return {
            "physical": self.physical,
            "logical": self.logical,
            "actor_id": self.actor_id,
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pytest adapters/korg-ledger-py/tests/test_hlc.py -v`
Expected: PASS (4 passed).

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-ledger-py/src/korg_ledger/_hlc.py adapters/korg-ledger-py/tests/test_hlc.py
git commit -m "feat(korg-ledger-py): HLC tick mirroring Rust semantics"
```

---

### Task 4: `_events.py` — `AgentToolCall` event builder

**Files:**
- Create: `adapters/korg-ledger-py/src/korg_ledger/_events.py`
- Test: covered by `test_writer.py` (Task 5); add a focused unit test here.
- Test: `adapters/korg-ledger-py/tests/test_events.py`

- [ ] **Step 1: Write the failing test**

The builder must produce exactly the `event` object shape of a real canonical record (`examples/claude_code_session_ledger.json`): keys `event_type, source_agent, tool_name, args, result, payload_refs, success, duration_ms, timestamp`.

```python
# adapters/korg-ledger-py/tests/test_events.py
from korg_ledger import agent_tool_call_event


def test_builds_agent_tool_call_event_shape():
    ev = agent_tool_call_event(
        source_agent="agent:claude-code@0.2.29",
        tool_name="Read",
        args={"file_path": "math_utils.py"},
        result={"lines": 7},
        success=True,
        duration_ms=50,
        timestamp="2026-05-25T02:50:37.077539Z",
    )
    assert ev == {
        "event_type": "AgentToolCall",
        "source_agent": "agent:claude-code@0.2.29",
        "tool_name": "Read",
        "args": {"file_path": "math_utils.py"},
        "result": {"lines": 7},
        "payload_refs": [],
        "success": True,
        "duration_ms": 50,
        "timestamp": "2026-05-25T02:50:37.077539Z",
    }


def test_timestamp_defaults_to_utc_z():
    ev = agent_tool_call_event(
        source_agent="a", tool_name="t", args={}, result={},
        success=True, duration_ms=0,
    )
    assert ev["timestamp"].endswith("Z")
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pytest adapters/korg-ledger-py/tests/test_events.py -v`
Expected: FAIL — `ImportError: cannot import name 'agent_tool_call_event'`.

- [ ] **Step 3: Write `_events.py`**

```python
# adapters/korg-ledger-py/src/korg_ledger/_events.py
"""Builders for korg-ledger CapabilityEvent payloads (the `event` object)."""
from __future__ import annotations

from datetime import datetime, timezone

#: serde nil UUID, used for correlation_id / campaign_id on external events.
NIL_UUID = "00000000-0000-0000-0000-000000000000"


def _now_iso() -> str:
    # ISO-8601 UTC with a trailing Z, matching chrono's DateTime<Utc> output.
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def agent_tool_call_event(
    *,
    source_agent: str,
    tool_name: str,
    args: dict,
    result: dict,
    success: bool,
    duration_ms: int,
    timestamp: str | None = None,
) -> dict:
    """Build the `event` object for a CapabilityEvent::AgentToolCall record."""
    return {
        "event_type": "AgentToolCall",
        "source_agent": source_agent,
        "tool_name": tool_name,
        "args": args,
        "result": result,
        "payload_refs": [],
        "success": success,
        "duration_ms": duration_ms,
        "timestamp": timestamp or _now_iso(),
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pytest adapters/korg-ledger-py/tests/test_events.py -v`
Expected: PASS (2 passed).

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-ledger-py/src/korg_ledger/_events.py adapters/korg-ledger-py/tests/test_events.py
git commit -m "feat(korg-ledger-py): AgentToolCall event builder"
```

---

### Task 5: `writer.py` — `LedgerWriter` (chain, causality, resume, HMAC)

**Files:**
- Create: `adapters/korg-ledger-py/src/korg_ledger/writer.py`
- Test: `adapters/korg-ledger-py/tests/test_writer.py`

- [ ] **Step 1: Write the failing tests**

```python
# adapters/korg-ledger-py/tests/test_writer.py
import json

import pytest

from korg_ledger import (
    CausalityError,
    LedgerWriter,
    agent_tool_call_event,
    verify_chain,
)


def _evt(tool, n):
    return agent_tool_call_event(
        source_agent="agent:claude-code@0.2.29",
        tool_name=tool,
        args={"n": n},
        result={"ok": True},
        success=True,
        duration_ms=n,
    )


def _lines(path):
    return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]


def test_append_produces_verifiable_chain(tmp_path):
    led = tmp_path / "ledger.jsonl"
    w = LedgerWriter(led)
    s1 = w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    s2 = w.append(
        event=_evt("Read", 2), actor_id="korg:claude-hook", triggered_by=s1
    )
    assert (s1, s2) == (1, 2)
    events = _lines(led)
    assert len(events) == 2
    assert events[0]["prev_hash"] == "0" * 64
    assert events[1]["prev_hash"] == events[0]["entry_hash"]
    assert events[1]["metadata"]["triggered_by"] == 1
    assert events[0]["event"]["event_type"] == "AgentToolCall"
    assert verify_chain(events) == []


def test_root_event_id_self_references_first_event(tmp_path):
    w = LedgerWriter(tmp_path / "l.jsonl")
    w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    e = _lines(tmp_path / "l.jsonl")[0]
    assert e["metadata"]["root_event_id"] == e["metadata"]["event_id"]
    assert e["metadata"]["causation_id"] is None


def test_rejects_non_earlier_triggered_by(tmp_path):
    w = LedgerWriter(tmp_path / "l.jsonl")
    w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    with pytest.raises(CausalityError):
        # next seq is 2; triggered_by must be strictly earlier (< 2)
        w.append(event=_evt("Read", 2), actor_id="korg:claude-hook", triggered_by=2)


def test_resume_continues_seq_and_chain(tmp_path):
    led = tmp_path / "l.jsonl"
    a = LedgerWriter(led)
    a.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    a.append(event=_evt("Read", 2), actor_id="korg:claude-hook")
    # fresh writer on the same file resumes from the tip
    b = LedgerWriter(led)
    assert b.tip()[0] == 2
    s3 = b.append(event=_evt("Edit", 3), actor_id="korg:claude-hook")
    assert s3 == 3
    events = _lines(led)
    assert len(events) == 3
    assert events[2]["prev_hash"] == events[1]["entry_hash"]
    assert verify_chain(events) == []


def test_hmac_mode_requires_key_to_verify(tmp_path):
    led = tmp_path / "l.jsonl"
    key = b"korg-conformance-key"
    w = LedgerWriter(led, hmac_key=key)
    w.append(event=_evt("user_prompt", 1), actor_id="korg:claude-hook")
    events = _lines(led)
    assert verify_chain(events, key) == []          # correct key verifies
    assert verify_chain(events, None) != []          # missing key fails
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `pytest adapters/korg-ledger-py/tests/test_writer.py -v`
Expected: FAIL — `ImportError: cannot import name 'LedgerWriter'`.

- [ ] **Step 3: Write `writer.py`**

```python
# adapters/korg-ledger-py/src/korg_ledger/writer.py
"""LedgerWriter — produces korg-ledger@v1 JournalEvent JSONL.

Assigns seq_id / prev_hash / HLC, builds the full JournalEvent (matching the
Rust serialization in crates/korg-registry/src/log.rs), enforces strictly-
earlier causality, and appends one JSON object per line under an exclusive
file lock. Resumes from an existing ledger's tip.
"""
from __future__ import annotations

import json
import os
import time
import uuid
from pathlib import Path

try:
    import fcntl  # POSIX
except ImportError:  # pragma: no cover - Windows
    fcntl = None

from ._events import NIL_UUID
from ._hash import GENESIS, chain_hash
from ._hlc import Hlc

SCHEMA_VERSION = "1.0"


class CausalityError(ValueError):
    """Raised when triggered_by does not reference a strictly-earlier seq_id."""


class LedgerWriter:
    def __init__(self, path, hmac_key: bytes | None = None, hlc_actor_id: int = 1) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._key = hmac_key
        self._last_seq, self._last_hash, self._last_hlc = self._resume(hlc_actor_id)

    def _resume(self, hlc_actor_id: int):
        last_seq, last_hash = 0, GENESIS
        last_hlc = Hlc(0, 0, hlc_actor_id)
        if self.path.exists():
            with self.path.open() as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        e = json.loads(line)
                    except json.JSONDecodeError:
                        break  # tolerate a torn final line from a crash mid-write
                    last_seq = e["seq_id"]
                    last_hash = e["entry_hash"]
                    hlc = e["metadata"]["emitted_at"]
                    last_hlc = Hlc(hlc["physical"], hlc["logical"], hlc.get("actor_id", hlc_actor_id))
        return last_seq, last_hash, last_hlc

    def tip(self) -> tuple[int, str]:
        """(seq_id, entry_hash) of the last appended event."""
        return (self._last_seq, self._last_hash)

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
        seq_id = self._last_seq + 1
        if triggered_by is not None and (triggered_by < 1 or triggered_by >= seq_id):
            raise CausalityError(
                f"triggered_by {triggered_by} is not a strictly-earlier seq_id (< {seq_id})"
            )
        eid = event_id or str(uuid.uuid4())
        hlc = self._last_hlc.tick(int(time.time() * 1000))
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
            "prev_hash": self._last_hash,
        }
        record["entry_hash"] = chain_hash(record, self._key)
        self._append_line(json.dumps(record, separators=(",", ":")))
        self._last_seq, self._last_hash, self._last_hlc = seq_id, record["entry_hash"], hlc
        return seq_id

    def _append_line(self, line: str) -> None:
        with self.path.open("a") as f:
            if fcntl is not None:
                fcntl.flock(f.fileno(), fcntl.LOCK_EX)
            try:
                f.write(line + "\n")
                f.flush()
                os.fsync(f.fileno())
            finally:
                if fcntl is not None:
                    fcntl.flock(f.fileno(), fcntl.LOCK_UN)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `pytest adapters/korg-ledger-py/tests/test_writer.py -v`
Expected: PASS (5 passed).

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-ledger-py/src/korg_ledger/writer.py adapters/korg-ledger-py/tests/test_writer.py
git commit -m "feat(korg-ledger-py): LedgerWriter with chain, causality, resume, HMAC"
```

---

### Task 6: Full-suite green + coverage gate

**Files:** none (verification only)

- [ ] **Step 1: Run the whole package test suite**

Run: `pytest adapters/korg-ledger-py/tests -v`
Expected: PASS — conformance (2) + hlc (4) + events (2) + writer (5) = 13 passed, 0 failed.

- [ ] **Step 2: Confirm coverage meets the 80% floor**

Run: `pytest adapters/korg-ledger-py/tests --cov=korg_ledger --cov-report=term-missing`
Expected: total coverage ≥ 80% (the four modules are small and fully exercised). If any line in `_hash`/`_hlc`/`_events`/`writer` is uncovered, add a focused test before proceeding.

- [ ] **Step 3: Commit any added tests**

```bash
git add adapters/korg-ledger-py/tests
git commit -m "test(korg-ledger-py): close coverage gaps to >=80%"
```

---

## Self-Review

**1. Spec coverage (§4.3):** stdlib-only writer ✓ (Task 5, no third-party deps); reuses spec canonicalization ✓ (Task 2 mirrors `conformance.py`, pinned by frozen vectors); `append(...) -> seq_id` ✓; `tip()` for resume ✓ (Task 5); HLC ✓ (Task 3); causality gate ✓ (Task 5); HMAC mode ✓ (Task 5); exclusive-lock append ✓ (`_append_line`); third independent conformant producer ✓ (Task 2 conformance test). Reproduces frozen tip hashes ✓ (Task 2 — the §4.3 "Verification" requirement). Cross-producer verification by Rust `korg-verify` is intentionally deferred to Plan 6 (it needs the built Rust binary) — noted in the header and the plan set table.

**2. Placeholder scan:** No TBD/TODO/"handle errors appropriately"; every code step shows complete code; every run step gives an exact command + expected output.

**3. Type/name consistency:** `LedgerWriter`, `append`, `tip`, `CausalityError`, `agent_tool_call_event`, `Hlc.tick`, `Hlc.as_dict`, `canonicalize`, `chain_hash`, `verify_chain`, `GENESIS`, `HASH_FIELDS`, `NIL_UUID` are used identically across `__init__.py` exports, the modules, and the tests. The `JournalEvent`/metadata field names and the `tier="Telemetry"` value match the real record in `examples/claude_code_session_ledger.json` and the structs in `crates/korg-registry/src/log.rs:274-330`. `triggered_by` strictly-earlier rule matches `verify_dag` in `crates/korg-ledger/src/lib.rs:177`.

No gaps found.
