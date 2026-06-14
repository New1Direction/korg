#!/usr/bin/env python3
"""Write a small deterministic korg-ledger@v1 session for the Gold Seal CI demo.

Usage: python3 make_demo_session.py [out.jsonl]
Requires korg-ledger-py on PYTHONPATH (adapters/korg-ledger-py/src).
"""
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "adapters" / "korg-ledger-py" / "src"))

from korg_ledger import chain_hash  # noqa: E402

STEPS = [
    ("user_prompt", {"prompt": "add a /healthz endpoint and a regression test"}, None),
    ("Read", {"file_path": "src/app.py"}, 1),
    ("Edit", {"file_path": "src/app.py"}, 2),
    ("Write", {"file_path": "tests/test_health.py"}, 3),
    ("Bash", {"command": "pytest -q tests/test_health.py"}, 4),
]


def main() -> None:
    events, prev = [], "0" * 64
    for i, (tool, args, triggered_by) in enumerate(STEPS, start=1):
        rec = {
            "schema_version": "1.0",
            "seq_id": i,
            "source_agent": "agent:korgex@0.14.1",
            "tool_name": tool,
            "args": args,
            "result": {},
            "payload_refs": [],
            "success": True,
            "duration_ms": 0,
            "prev_hash": prev,
        }
        if triggered_by is not None:
            rec["triggered_by"] = triggered_by
        rec["entry_hash"] = chain_hash(rec)
        prev = rec["entry_hash"]
        events.append(rec)

    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("demo-session.jsonl")
    out.write_text("\n".join(json.dumps(e) for e in events) + "\n", encoding="utf-8")
    print(f"wrote {len(events)}-event session → {out}")


if __name__ == "__main__":
    main()
