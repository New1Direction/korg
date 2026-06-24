#!/usr/bin/env python3
"""Differential fuzzer — prove the three independent korgcert@v1 verifiers AGREE.

For each adversarially-mutated Certificate, run the Python, Rust, and JS verifiers
and assert they return the same validity. Three independent codepaths written
from one spec must never disagree on a verdict; a divergence is a conformance bug.

All candidates keep ``schema: korgcert@v1`` so every verifier routes to its
korgcert path (the per-language fuzz suites already cover off-schema / non-object
input). Needs the built ``korg-verify`` binary + node + cryptography.

    PYTHONPATH=adapters/korg-ledger-py/src python3 spec/korg-ledger-v1/tools/diff_fuzz.py
"""
from __future__ import annotations

import copy
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "adapters" / "korg-ledger-py" / "src"))

from korg_ledger.signing import verify_seal  # noqa: E402

FIXTURE = REPO / "crates" / "korg-verify" / "tests" / "fixtures" / "korgcert-v1.json"
VERIFY_MJS = REPO / "spec" / "korg-ledger-v1" / "js" / "verify.mjs"
RUST_BIN = next(
    (p for p in [REPO / "target/debug/korg-verify", REPO / "target/release/korg-verify"] if p.exists()),
    None,
)


def _flip(s: str, i: int) -> str:
    chars = list(s)
    chars[i] = "f" if chars[i] != "f" else "0"
    return "".join(chars)


def _cli_valid(cmd: list[str]) -> object:
    out = subprocess.run(cmd, capture_output=True, text=True)
    try:
        return json.loads(out.stdout).get("valid")
    except Exception:
        return f"ERR({out.returncode}):{(out.stdout or out.stderr)[:60]!r}"


def mutations(base: dict):
    yield "base", base

    def mut(label, fn):
        c = copy.deepcopy(base)
        fn(c)
        return label, c

    yield mut("tip-flip", lambda c: c.update(tip=_flip(c["tip"], 0)))
    yield mut("sig-flip", lambda c: c["seal"].update(sig=_flip(c["seal"]["sig"], 0)))
    yield mut("pubkey-flip", lambda c: c["seal"].update(pubkey=_flip(c["seal"]["pubkey"], 0)))
    yield mut("claim", lambda c: c.update(claim=c["claim"] + "!"))
    yield mut("issued_at", lambda c: c.update(issued_at=c["issued_at"] + 1))
    yield mut("event_count", lambda c: c.update(event_count=c["event_count"] + 1))
    yield mut("event-hash", lambda c: c["events"][2].update(entry_hash=_flip(c["events"][2]["entry_hash"], 0)))
    yield mut("event-arg", lambda c: c["events"][2]["args"].update(file_path="src/evil.py"))
    yield mut("drop-last-event", lambda c: c["events"].pop())
    yield mut("reorder-events", lambda c: c.__setitem__("events", [c["events"][1], c["events"][0]] + c["events"][2:]))
    yield mut("event-null", lambda c: c["events"].__setitem__(2, None))
    yield mut("event-empty", lambda c: c["events"].__setitem__(2, {}))
    yield mut("event-not-list", lambda c: c.update(events="nope"))
    yield mut("summary-files", lambda c: c["summary"].update(files=[]))
    yield mut("summary-bytool", lambda c: c["summary"]["by_tool"].update(Bash=99))
    yield mut("drop-summary", lambda c: c.pop("summary"))
    yield mut("drop-tip", lambda c: c.pop("tip"))
    yield mut("drop-seal", lambda c: c.pop("seal"))
    yield mut("drop-events", lambda c: c.pop("events"))
    yield mut("junk-field", lambda c: c.update(zzz_junk={"a": [1, 2, 3]}))
    yield mut("anchor-commit", lambda c: c["anchors"][0]["anchor_proof"].update(commit="f" * 40))
    yield mut("anchor-hash", lambda c: c["anchors"][0].update(entry_hash="deadbeef"))
    yield mut("drop-anchors", lambda c: c.pop("anchors"))
    # out-of-domain numbers must be rejected by all three (was a silent divergence)
    yield mut("bigint-arg", lambda c: c["events"][1]["args"].update(big=2**60))
    yield mut("bigint-issued", lambda c: c.update(issued_at=2**60))


def main() -> None:
    if RUST_BIN is None:
        print("korg-verify binary not built (cargo build -p korg-verify)", file=sys.stderr)
        sys.exit(2)

    base = json.loads(FIXTURE.read_text())
    diverged = 0
    total = 0
    for label, candidate in mutations(base):
        total += 1
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
            json.dump(candidate, f)
            path = f.name
        try:
            py = verify_seal(candidate) == []
            rust = _cli_valid([str(RUST_BIN), path, "--json"])
            js = _cli_valid(["node", str(VERIFY_MJS), path, "--json"])
        finally:
            os.unlink(path)
        agree = py == rust == js
        if not agree:
            diverged += 1
        print(f"  [{'ok' if agree else 'DIVERGE':>7}] {label:16} py={py} rust={rust} js={js}")

    # Positive cross-impl check: a VALID seal full of canonicalization edge cases —
    # astral-plane (non-BMP) object keys, the max-safe integer, and non-ASCII paths —
    # must verify VALID in all three. This is what catches a key-sort / number /
    # escaping divergence on LEGITIMATE data (the opposite of the mutation cases).
    from korg_ledger import chain_hash
    from korg_ledger.signing import mint_seal

    edge_events = []
    prev = "0" * 64
    edge_steps = [
        ("user_prompt", {"prompt": "café — naïve 𝄞", "￿": 1, "\U0001d11e": 2}),
        ("Edit", {"file_path": "src/café/𝄞.py", "n": 9007199254740991}),
    ]
    for i, (tool, args) in enumerate(edge_steps, start=1):
        rec = {
            "schema_version": "1.0", "seq_id": i, "source_agent": "a", "tool_name": tool,
            "args": args, "result": {}, "payload_refs": [], "success": True,
            "duration_ms": 0, "prev_hash": prev,
        }
        rec["entry_hash"] = chain_hash(rec)
        prev = rec["entry_hash"]
        edge_events.append(rec)
    edge_seal = mint_seal(
        events=edge_events, claim="café 𝄞", issuer_agent="a", issued_at=1, private_seed=bytes([42]) * 32
    )
    total += 1
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as f:
        json.dump(edge_seal, f, ensure_ascii=False)
        path = f.name
    try:
        py = verify_seal(edge_seal) == []
        rust = _cli_valid([str(RUST_BIN), path, "--json"])
        js = _cli_valid(["node", str(VERIFY_MJS), path, "--json"])
    finally:
        os.unlink(path)
    agree = py is True and rust is True and js is True
    if not agree:
        diverged += 1
    print(f"  [{'ok' if agree else 'DIVERGE':>7}] {'edge-valid (astral/maxint/unicode)':16} py={py} rust={rust} js={js}")

    # Negative-seq bound anchor: seq_id is a SIGNED integer, so a valid seal can
    # carry a bound anchor at a negative seq_id. This is the exact artifact the old
    # Rust as_u64 anchor matcher split on (Rust REJECT vs Py/JS ACCEPT) — mint it
    # valid and assert all three still agree it is VALID.
    neg_events = []
    prev = "0" * 64
    for sid in (-2, -1):
        rec = {
            "schema_version": "1.0", "seq_id": sid, "source_agent": "a", "tool_name": "Edit",
            "args": {"file_path": "src/x.py"}, "result": {}, "payload_refs": [], "success": True,
            "duration_ms": 0, "prev_hash": prev,
        }
        rec["entry_hash"] = chain_hash(rec)
        prev = rec["entry_hash"]
        neg_events.append(rec)
    neg_anchor = [{
        "seq_id": -1, "entry_hash": neg_events[-1]["entry_hash"], "anchor_kind": "git-tip",
        "anchor_proof": {"repo": "https://github.com/New1Direction/korg", "commit": "0" * 40},
        "anchored_at": "2026-06-14T00:00:00Z",
    }]
    neg_seal = mint_seal(
        events=neg_events, claim="neg-seq", issuer_agent="a", issued_at=1,
        private_seed=bytes([7]) * 32, anchors=neg_anchor,
    )
    total += 1
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as f:
        json.dump(neg_seal, f, ensure_ascii=False)
        path = f.name
    try:
        py = verify_seal(neg_seal) == []
        rust = _cli_valid([str(RUST_BIN), path, "--json"])
        js = _cli_valid(["node", str(VERIFY_MJS), path, "--json"])
    finally:
        os.unlink(path)
    agree = py is True and rust is True and js is True
    if not agree:
        diverged += 1
    print(f"  [{'ok' if agree else 'DIVERGE':>7}] {'neg-seq bound anchor':16} py={py} rust={rust} js={js}")

    print(f"\ndifferential fuzz: {total} candidates · {diverged} divergence(s)")
    sys.exit(1 if diverged else 0)


if __name__ == "__main__":
    main()
