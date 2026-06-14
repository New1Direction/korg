"""LedgerWriter — produces korg-ledger@v1 JournalEvent JSONL.

Assigns seq_id / prev_hash / HLC, builds the full JournalEvent (matching the
Rust serialization in crates/korg-registry/src/log.rs), enforces strictly-
earlier causality, and appends one JSON object per line. Each append re-reads
the chain tip under an exclusive lock, so overlapping writer processes on the
same file can never fork the chain or duplicate a seq_id.
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
    def __init__(
        self,
        path,
        hmac_key: bytes | None = None,
        hlc_actor_id: int = 1,
        signing_key: bytes | None = None,
    ) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.touch(exist_ok=True)
        self._key = hmac_key
        self._hlc_actor_id = hlc_actor_id
        # Optional raw 32-byte Ed25519 seed. When set, each appended event is
        # signed (requires the `signing` extra / `cryptography`). Default None
        # keeps the writer stdlib-only.
        self._signing_key = signing_key
        with self.path.open("r") as f:
            self._last_seq, self._last_hash, self._last_hlc = self._read_tip(f)

    def _read_tip(self, f) -> tuple[int, str, Hlc]:
        """Authoritative tip read from disk (caller holds the lock for writes).

        A torn FINAL line (crash mid-write) is tolerated. A corrupt line with
        valid data after it means the log was spliced/truncated mid-file — we
        FAIL LOUD rather than silently returning a stale tip, which would let the
        next append reuse a seq_id and fork the chain."""
        last_seq, last_hash = 0, GENESIS
        last_hlc = Hlc(0, 0, self._hlc_actor_id)
        f.seek(0)
        lines = f.readlines()
        for i, raw in enumerate(lines):
            line = raw.strip()
            if not line:
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError as ex:
                if any(later.strip() for later in lines[i + 1 :]):
                    raise ValueError(
                        f"corrupt ledger line {i + 1} with data after it "
                        f"(spliced/truncated log); refusing to append: {ex}"
                    ) from ex
                break  # torn final line from a crash mid-write — safe to ignore
            last_seq = e["seq_id"]
            last_hash = e["entry_hash"]
            hlc = e["metadata"]["emitted_at"]
            last_hlc = Hlc(hlc["physical"], hlc["logical"], hlc.get("actor_id", self._hlc_actor_id))
        return last_seq, last_hash, last_hlc

    def tip(self) -> tuple[int, str]:
        """(seq_id, entry_hash) of the last appended event (cached)."""
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
                if self._signing_key is not None:
                    from . import signing  # lazy: keeps the stdlib path import-clean

                    record["event_sig"] = signing.sign_event(self._signing_key, record)
                f.seek(0, os.SEEK_END)
                f.write(json.dumps(record, separators=(",", ":")) + "\n")
                f.flush()
                os.fsync(f.fileno())
                self._last_seq, self._last_hash, self._last_hlc = seq_id, record["entry_hash"], hlc
                return seq_id
            finally:
                if fcntl is not None:
                    fcntl.flock(f.fileno(), fcntl.LOCK_UN)
