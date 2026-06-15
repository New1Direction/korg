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


def _read(name):
    with open(os.path.join(HERE, "vectors", name)) as f:
        return [json.loads(line) for line in f if line.strip()]


def run() -> int:
    manifest = json.load(open(os.path.join(HERE, "conformance.json")))
    assert manifest["spec_version"] == "korg-ledger@v1"
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
        print(f"  [{'PASS' if ok else 'FAIL'}] {v['file']:<26} {v['verify']:<8} {detail}")
        failures += 0 if ok else 1
    print(f"\nkorg-ledger@v1 conformance: {'PASS' if not failures else f'{failures} FAILURE(S)'}")
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(run())
