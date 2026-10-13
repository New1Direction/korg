#!/usr/bin/env python3
"""
korg-ledger@v1 — standalone conformance verifier (the citable reference).

Dependency-free (Python stdlib only). Reproduces the spec's canonicalization +
hash-chain from scratch and checks every vector in conformance.json. Any
implementation in any language is conformant iff it reproduces the frozen
tip_entry_hash on each intact vector — this script is the executable oracle.

    python3 conformance.py        # exit 0 = the reference reproduces the vectors
"""
from __future__ import annotations

import hashlib
import hmac
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
GENESIS = "0" * 64


def canonicalize(value) -> bytes:
    # JSON, keys sorted by code point, no whitespace, non-ASCII \uXXXX-escaped.
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("ascii")


def chain_hash(event: dict, key: bytes | None = None) -> str:
    # entry_hash and the reserved Phase-2 event_sig are excluded from the preimage.
    preimage = {k: v for k, v in event.items() if k not in ("entry_hash", "event_sig")}
    data = canonicalize(preimage)
    if key is not None:
        return hmac.new(key, data, hashlib.sha256).hexdigest()
    return hashlib.sha256(data).hexdigest()


def verify_chain(events: list, key: bytes | None = None) -> list:
    errors, expected_prev = [], GENESIS
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


_SAFE_INT_MAX = 2**53 - 1


def json_int(v):
    # §9.2: an integral JSON number within ±(2^53-1); booleans are not integers.
    if isinstance(v, bool):
        return None
    if isinstance(v, int):
        return v if abs(v) <= _SAFE_INT_MAX else None
    if isinstance(v, float) and v.is_integer() and abs(v) <= _SAFE_INT_MAX:
        return int(v)
    return None


def causal_parent(e: dict):
    # §9.3: top-level triggered_by (flat) or metadata.triggered_by (nested);
    # null/absent = root; when both are present they must agree.
    raw = [e.get("triggered_by")]
    md = e.get("metadata")
    raw.append(md.get("triggered_by") if isinstance(md, dict) else None)
    ints = []
    for v in (x for x in raw if x is not None):
        n = json_int(v)
        if n is None:
            return None, "triggered_by is not an integer"
        ints.append(n)
    if len(ints) == 2 and ints[0] != ints[1]:
        return None, f"conflicting triggered_by (top-level {ints[0]} vs metadata {ints[1]})"
    return (ints[0] if ints else None), None


def verify_dag(events: list) -> list:
    errors = []
    dicts = [e for e in events if isinstance(e, dict)]
    seqs = [n for n in (json_int(e.get("seq_id")) for e in dicts) if n is not None]
    if len(set(seqs)) != len(seqs):
        errors.append("duplicate seq_id present")
    for e in dicts:
        sid = json_int(e.get("seq_id"))
        label = "?" if sid is None else str(sid)
        tb, err = causal_parent(e)
        if err:
            errors.append(f"seq {label}: {err}")
        elif tb is None:
            continue
        elif tb not in set(seqs):
            errors.append(f"seq {label}: triggered_by {tb} does not exist")
        elif sid is None:
            errors.append(f"seq ?: triggered_by {tb} cannot be ordered (seq_id is not an integer)")
        elif tb >= sid:
            errors.append(f"seq {label}: triggered_by {tb} is not strictly earlier")
    return errors


def _read(name):
    with open(os.path.join(HERE, "vectors", name), encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def run() -> int:
    with open(os.path.join(HERE, "conformance.json"), encoding="utf-8") as f:
        manifest = json.load(f)
    assert manifest["spec_version"] == "korg-ledger@v1"
    listed = {v["file"] for v in manifest["vectors"]}
    on_disk = {n for n in os.listdir(os.path.join(HERE, "vectors")) if n.endswith(".jsonl")}
    if listed != on_disk:
        print(f"  [FAIL] manifest/vectors out of sync: unlisted={sorted(on_disk - listed)} missing={sorted(listed - on_disk)}")
        return 1
    failures = 0
    for v in manifest["vectors"]:
        events = _read(v["file"])
        key = v["key"].encode() if v.get("key") else None
        errors = verify_chain(events, key)
        ok, detail = True, ""
        if v["verify"] == "intact":
            if errors:
                ok, detail = False, f"expected intact, got {errors}"
            elif chain_hash(events[-1], key) != v["tip_entry_hash"]:
                ok, detail = False, "tip_entry_hash not reproduced"
        else:
            if not errors:
                ok, detail = False, "expected tampered, verified clean"
            elif not any(v["error_contains"] in e for e in errors):
                ok, detail = False, f"errors {errors} missing {v['error_contains']!r}"
        # §9 causal DAG — exact error strings are part of the oracle.
        dag = verify_dag(events)
        want_dag = v.get("dag_errors", []) if v.get("dag") == "broken" else []
        if ok and dag != want_dag:
            ok, detail = False, f"dag errors {dag} != expected {want_dag}"
        print(f"  [{'PASS' if ok else 'FAIL'}] {v['file']:<26} {v['verify']:<8} dag={v.get('dag', '-'):<7} {detail}")
        failures += 0 if ok else 1
    print(f"\nkorg-ledger@v1 conformance: {'PASS' if not failures else f'{failures} FAILURE(S)'}")
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(run())
