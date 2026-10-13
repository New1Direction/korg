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


_SAFE_INT_MAX = 2**53 - 1


def _reject_out_of_domain(value) -> None:
    """Raise ValueError on numbers the three implementations cannot agree on:
    integers beyond ±(2^53-1) (JS Number loses precision there). NaN/Infinity are
    rejected by ``allow_nan=False`` below. Finite floats are out of korg-ledger@v1
    scope (SPEC §2) but left to json.dumps so this never breaks capture of tool
    args that carry a finite float — Python/Rust agree on them; the JS verifier
    reports such an event unverifiable."""
    if isinstance(value, bool):
        return
    if isinstance(value, int):
        if abs(value) > _SAFE_INT_MAX:
            raise ValueError(f"integer out of safe range (|n| > 2^53-1): {value}")
    elif isinstance(value, dict):
        for v in value.values():
            _reject_out_of_domain(v)
    elif isinstance(value, (list, tuple)):
        for v in value:
            _reject_out_of_domain(v)


def canonicalize(value) -> bytes:
    """JSON, keys sorted by code point, no whitespace, non-ASCII \\uXXXX-escaped.
    Rejects NaN/Infinity and out-of-safe-range integers so the three impls agree."""
    _reject_out_of_domain(value)
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode("ascii")


def chain_hash(event: dict, key: bytes | None = None) -> str:
    """SHA-256 (or HMAC-SHA256 with a key) over the canonical preimage."""
    preimage = {k: v for k, v in event.items() if k not in HASH_FIELDS}
    data = canonicalize(preimage)
    if key is not None:
        return hmac.new(key, data, hashlib.sha256).hexdigest()
    return hashlib.sha256(data).hexdigest()


def verify_anchors(chain: list, anchors: list) -> list:
    """Structural check (stdlib only, no network): each anchor's entry_hash must
    match the chain event at its seq_id. The external git-tip proof — the actual
    owner-rewrite defense — is verified by the Rust verifier (see the spec)."""
    errors: list[str] = []
    if not isinstance(chain, list) or not isinstance(anchors, list):
        return ["chain or anchors is not a list"]
    by_seq = {e.get("seq_id"): e for e in chain if isinstance(e, dict)}
    for a in anchors:
        if not isinstance(a, dict):
            errors.append("anchor record is not a JSON object")
            continue
        seq = a.get("seq_id")
        want = a.get("entry_hash")
        if seq is None or want is None:
            errors.append("anchor record missing seq_id or entry_hash")
            continue
        e = by_seq.get(seq)
        if e is None:
            errors.append(f"anchor seq {seq}: no event with that seq_id in the chain")
        elif e.get("entry_hash") != want:
            errors.append(f"anchor seq {seq}: entry_hash does not match the chain")
    return errors


def verify_chain(events: list, key: bytes | None = None) -> list:
    """Recompute the chain; empty list iff intact. Each error names a seq_id.

    Robust to hostile input: a non-list, or any non-object event, is reported as
    an error rather than raising — matching the Rust/JS verifiers, which never
    crash on malformed input."""
    errors: list[str] = []
    if not isinstance(events, list):
        return ["events is not a list"]
    expected_prev: str | None = GENESIS
    for idx, e in enumerate(events):
        if not isinstance(e, dict):
            errors.append(f"event {idx} is not a JSON object")
            expected_prev = None
            continue
        sid = e.get("seq_id")
        stored = e.get("entry_hash")
        if stored is None:
            errors.append(f"seq {sid}: missing entry_hash")
            expected_prev = None
            continue
        if e.get("prev_hash") != expected_prev:
            errors.append(f"seq {sid}: prev_hash breaks the chain")
        # chain_hash → canonicalize raises on out-of-domain numbers; treat as a
        # verification failure rather than crashing.
        try:
            computed = chain_hash(e, key)
        except ValueError as ex:
            errors.append(f"seq {sid}: not canonicalizable ({ex})")
            expected_prev = stored
            continue
        if computed != stored:
            errors.append(f"seq {sid}: entry_hash mismatch (content tampered)")
        expected_prev = stored
    return errors


def json_int(value):
    """The integer a JSON value denotes, or None (korg-ledger@v1 §9.2).

    A ledger integer is a JSON number with an integral value within ±(2^53-1).
    Booleans are NOT integers (Python's ``bool`` subclasses ``int``; Rust and JS
    treat ``true`` as a boolean), and an integral float such as ``2.0`` IS the
    integer 2 (JS cannot tell ``2.0`` from ``2`` after parsing, so all three
    verifiers must accept it to agree)."""
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value if abs(value) <= _SAFE_INT_MAX else None
    if isinstance(value, float) and value.is_integer() and abs(value) <= _SAFE_INT_MAX:
        return int(value)
    return None


def _seq_label(e: dict) -> str:
    sid = json_int(e.get("seq_id"))
    return "?" if sid is None else str(sid)


def causal_parent(event: dict):
    """Resolve an event's causal parent across the flat and nested shapes (§9.3).

    Returns ``(parent, error)``: ``parent`` is the parent seq_id (int) or None
    for a root; ``error`` is a message (without the ``seq N:`` prefix) when the
    link is malformed. A flat record carries ``triggered_by`` at the top level, a
    nested JournalEvent under ``metadata``; ``null`` and absence both mean root.
    When both are present they must name the same parent."""
    raw = []
    if event.get("triggered_by") is not None:
        raw.append(("top-level", event["triggered_by"]))
    md = event.get("metadata")
    if isinstance(md, dict) and md.get("triggered_by") is not None:
        raw.append(("metadata", md["triggered_by"]))
    if not raw:
        return None, None
    parsed = []
    for _where, v in raw:
        n = json_int(v)
        if n is None:
            return None, "triggered_by is not an integer"
        parsed.append(n)
    if len(parsed) == 2 and parsed[0] != parsed[1]:
        return None, f"conflicting triggered_by (top-level {parsed[0]} vs metadata {parsed[1]})"
    return parsed[0], None


def verify_dag(events: list) -> list:
    """Check the causal DAG (korg-ledger@v1 §5 + §9): unique integer seq_ids, and
    every causal parent — top-level ``triggered_by`` (flat) or
    ``metadata.triggered_by`` (nested JournalEvent) — is a well-formed integer
    that references an existing, strictly-earlier seq_id.

    Error strings are byte-identical to the Rust ``verify_dag`` and the JS
    ``verifyDag``; the conformance manifest pins them."""
    errors: list[str] = []
    if not isinstance(events, list):
        return ["events is not a list"]
    dicts = [e for e in events if isinstance(e, dict)]
    seqs = [n for n in (json_int(e.get("seq_id")) for e in dicts) if n is not None]
    seqset = set(seqs)
    if len(seqset) != len(seqs):
        errors.append("duplicate seq_id present")
    for e in dicts:
        label = _seq_label(e)
        tb, err = causal_parent(e)
        if err is not None:
            errors.append(f"seq {label}: {err}")
            continue
        if tb is None:
            continue
        sid = json_int(e.get("seq_id"))
        if tb not in seqset:
            errors.append(f"seq {label}: triggered_by {tb} does not exist")
        elif sid is None:
            errors.append(f"seq ?: triggered_by {tb} cannot be ordered (seq_id is not an integer)")
        elif tb >= sid:
            errors.append(f"seq {label}: triggered_by {tb} is not strictly earlier")
    return errors
