#!/usr/bin/env python3
"""Differential fuzzer — prove the three independent goldseal@v1 verifiers AGREE.

For each adversarially-mutated Gold Seal, run the Python, Rust, and JS verifiers
and assert they return the same validity. Three independent codepaths written
from one spec must never disagree on a verdict; a divergence is a conformance bug.

All candidates keep ``schema: goldseal@v1`` so every verifier routes to its
goldseal path (the per-language fuzz suites already cover off-schema / non-object
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

FIXTURE = REPO / "crates" / "korg-verify" / "tests" / "fixtures" / "goldseal-v1.json"
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

    print(f"\ndifferential fuzz: {total} candidates · {diverged} divergence(s)")
    sys.exit(1 if diverged else 0)


if __name__ == "__main__":
    main()
