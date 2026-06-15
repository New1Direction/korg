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
#: "event_sig" is the reserved Phase-2 per-event signature slot — excluded in
#: lockstep across the Rust, Python (this + the spec oracle), and JS impls.
HASH_FIELDS = ("entry_hash", "event_sig")


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
