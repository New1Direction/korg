#!/usr/bin/env python3
"""Mint the frozen cross-implementation goldseal@v1 fixture.

Deterministic: a fixed event chain + the fixed Ed25519 seed ``[42]*32`` +
a fixed ``issued_at`` produce a byte-stable seal. Re-running overwrites the
fixture identically. The Python-minted seal is then verified — unchanged —
by the Rust ``korg-verify`` and the JS ``verify.mjs`` goldseal codepaths;
that three-way agreement is the conformance proof.

Run:  python3 spec/korg-ledger-v1/tools/mint_goldseal_fixture.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "adapters" / "korg-ledger-py" / "src"))

from korg_ledger import chain_hash  # noqa: E402
from korg_ledger.signing import mint_seal  # noqa: E402

SEED = bytes([42]) * 32
ISSUED_AT = 1760000000  # fixed so the fixture is reproducible (no wall clock)
AGENT = "agent:korgex@0.14.1"

# A small but realistic session, in the flat korg-ledger@v1 event shape (the
# same shape as signed-receipt.json). triggered_by links make the causal DAG
# real, not just a set of unique seq_ids.
_STEPS = [
    ("user_prompt", {"prompt": "Add a /healthz endpoint and a regression test"}, None),
    ("Read", {"file_path": "src/app.py"}, 1),
    ("Edit", {"file_path": "src/app.py"}, 2),
    ("Write", {"file_path": "tests/test_health.py"}, 3),
    ("Bash", {"command": "pytest -q tests/test_health.py"}, 4),
]


def build_chain() -> list[dict]:
    events: list[dict] = []
    prev = "0" * 64
    for i, (tool, args, triggered_by) in enumerate(_STEPS, start=1):
        record = {
            "schema_version": "1.0",
            "seq_id": i,
            "source_agent": AGENT,
            "tool_name": tool,
            "args": args,
            "result": {},
            "payload_refs": [],
            "success": True,
            "duration_ms": 0,
            "prev_hash": prev,
        }
        if triggered_by is not None:
            record["triggered_by"] = triggered_by
        record["entry_hash"] = chain_hash(record)
        prev = record["entry_hash"]
        events.append(record)
    return events


def main() -> None:
    events = build_chain()
    tip = events[-1]["entry_hash"]
    # A git-tip anchor over the chain head. It is bound BOTH structurally
    # (entry_hash must match the chain) AND cryptographically (the seal signs the
    # anchor set). The external/network proof of the commit is a separate operator
    # step; here the values are fixed so the fixture stays deterministic.
    anchors = [
        {
            "seq_id": events[-1]["seq_id"],
            "entry_hash": tip,
            "anchor_kind": "git-tip",
            "anchor_proof": {
                "repo": "https://github.com/New1Direction/korg",
                "commit": "0" * 40,
            },
            "anchored_at": "2026-06-14T00:00:00Z",
        }
    ]
    seal = mint_seal(
        events=events,
        claim="Added a /healthz endpoint to src/app.py with a passing regression test",
        issuer_agent=AGENT,
        issued_at=ISSUED_AT,
        private_seed=SEED,
        anchors=anchors,
    )

    out_dir = REPO / "crates" / "korg-verify" / "tests" / "fixtures"
    (out_dir / "goldseal-v1.json").write_text(
        json.dumps(seal, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    (out_dir / "goldseal-v1.pubkey").write_text(seal["seal"]["pubkey"] + "\n", encoding="utf-8")

    # Also drop a copy next to the web verifier samples for the seal.html demo.
    web_sample = REPO / "spec" / "korg-ledger-v1" / "web" / "samples" / "goldseal.json"
    web_sample.write_text(
        json.dumps(seal, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    print("minted goldseal@v1 fixture")
    print(f"  pubkey  {seal['seal']['pubkey']}")
    print(f"  tip     {seal['tip']}")
    print(f"  events  {seal['event_count']}")
    print(f"  files   {seal['summary']['files']}")


if __name__ == "__main__":
    main()
