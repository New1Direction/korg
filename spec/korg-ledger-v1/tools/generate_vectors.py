#!/usr/bin/env python3
"""Regenerate (or check) every korg-ledger@v1 conformance vector + the manifest.

The single source for spec/korg-ledger-v1/vectors/*.jsonl and conformance.json.
Deterministic: no clocks, no random UUIDs. Hashes come from the stdlib oracle
(../conformance.py), never from a product implementation.

    python3 spec/korg-ledger-v1/tools/generate_vectors.py           # --check (default): exit 1 on drift
    python3 spec/korg-ledger-v1/tools/generate_vectors.py --write   # rewrite vectors + manifest

The v1.0 vectors are FROZEN: this script asserts it reproduces their original
tip hashes, so a regeneration can never silently move the oracle.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

SPEC = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SPEC))

from conformance import GENESIS, chain_hash  # noqa: E402

VECTORS = SPEC / "vectors"
MANIFEST = SPEC / "conformance.json"
HMAC_KEY = "korg-conformance-key"

# Tips frozen at korg-ledger@v1.0. Never edit these.
FROZEN_V1_0 = {
    "basic-intact.jsonl": "7418b9105a664e21078fde881fbd8a5295c49bb384aa62a48d5b544292d910af",
    "hmac-intact.jsonl": "1b371102038082d3fac965fe21ba357129c61f57fcd93f1a0ffa5e6eaa454e78",
    "nonbmp-intact.jsonl": "045d2d865e89c54340408b309c81d6b0f1367ebf0aa4504f1af66f7f1e4cb590",
}


def chain(records, key=None):
    """Assign prev_hash/entry_hash in order (records are mutated and returned)."""
    prev = GENESIS
    for r in records:
        r["prev_hash"] = prev
        r["entry_hash"] = chain_hash(r, key)
        prev = r["entry_hash"]
    return records


def flat(seq, tool, args, result, duration, triggered_by=None, agent="agent:conformance"):
    r = {
        "schema_version": "1.0",
        "seq_id": seq,
        "source_agent": agent,
        "tool_name": tool,
        "args": args,
        "result": result,
        "success": True,
        "duration_ms": duration,
    }
    if triggered_by is not None:
        r["triggered_by"] = triggered_by
    return r


def nested(seq, tool, args, result, triggered_by, *, tags=None, physical=1781395200000):
    """A JournalEvent exactly as LedgerWriter / the Rust registry serialize it,
    with fixed ids and clock so the bytes are reproducible."""
    eid = f"00000000-0000-4000-8000-{seq:012d}"
    causation = f"00000000-0000-4000-8000-{triggered_by:012d}" if isinstance(triggered_by, int) else None
    return {
        "schema_version": "1.0",
        "seq_id": seq,
        "metadata": {
            "event_id": eid,
            "correlation_id": "00000000-0000-0000-0000-000000000000",
            "causation_id": causation,
            "root_event_id": "00000000-0000-4000-8000-000000000001",
            "actor_id": "korg:conformance",
            "campaign_id": "00000000-0000-0000-0000-000000000000",
            "emitted_at": {"physical": physical, "logical": seq - 1, "actor_id": 1},
            "branch_id": None,
            "speculative": False,
            "retry_count": 0,
            "tier": "Telemetry",
            "span_id": None,
            "tags": dict(tags or {}),
            "triggered_by": triggered_by,
        },
        "event": {
            "event_type": "AgentToolCall",
            "source_agent": "agent:conformance",
            "tool_name": tool,
            "args": args,
            "result": result,
            "payload_refs": [],
            "success": True,
            "duration_ms": 0,
            "timestamp": "2026-06-14T00:00:00Z",
        },
    }


def legacy_basic():
    return [
        flat(1, "user_prompt", {"prompt": "add a function"}, {}, 0),
        flat(2, "llm_inference", {"model": "m", "prompt_tokens": 10}, {"completion_tokens": 4}, 12, 1),
        flat(3, "Write", {"path": "mathx.py"}, {"ok": True}, 3, 2),
    ]


def build():
    """→ list of (filename, records, ensure_ascii, compact, manifest_entry)."""
    out = []

    # ── v1.0 (frozen) ────────────────────────────────────────────────────────
    basic = chain(legacy_basic())
    out.append(("basic-intact.jsonl", basic, True, False,
                {"key": None, "verify": "intact", "dag": "intact"}))

    out.append(("hmac-intact.jsonl", chain(legacy_basic(), HMAC_KEY.encode()), True, False,
                {"key": HMAC_KEY, "verify": "intact", "dag": "intact"}))

    nonbmp = chain([
        flat(1, "user_prompt", {"prompt": "make it \U0001F600 in 中文 \U00010000"}, {}, 0),
        flat(2, "llm_inference", {"model": "m中", "prompt_tokens": 7},
             {"completion_tokens": 3, "note": "ok \U0001F4A9"}, 9, 1),
        flat(3, "Write", {"path": "中文.py", "snippet": "# \U0001F600 \U00010000"}, {"ok": True}, 4, 2),
    ])
    out.append(("nonbmp-intact.jsonl", nonbmp, False, False,
                {"key": None, "verify": "intact", "dag": "intact"}))

    tampered = json.loads(json.dumps(basic))
    tampered[1]["args"]["model"] = "EVIL"
    out.append(("tampered-content.jsonl", tampered, True, False,
                {"key": None, "verify": "tampered", "error_contains": "seq 2", "dag": "intact"}))

    deletion = [json.loads(json.dumps(basic[0])), json.loads(json.dumps(basic[2]))]
    out.append(("tampered-deletion.jsonl", deletion, True, False,
                {"key": None, "verify": "tampered", "error_contains": "seq 3",
                 "dag": "broken", "dag_errors": ["seq 3: triggered_by 2 does not exist"]}))

    # ── v1.1: nested JournalEvent shape + causal-parent resolution (§9) ─────
    out.append(("nested-intact.jsonl", chain([
        nested(1, "user_prompt", {"prompt": "fix the bug"}, {}, None),
        nested(2, "llm_inference", {"model": "m"}, {"text": "reading the file"}, 1),
        nested(3, "Edit", {"file_path": "math_utils.py"}, {"ok": True}, 2),
        nested(4, "Bash", {"command": "pytest -q"}, {"exit_code": 0}, 2),
    ]), True, True, {"key": None, "verify": "intact", "dag": "intact"}))

    out.append(("nested-dag-broken.jsonl", chain([
        nested(1, "user_prompt", {"prompt": "fix the bug"}, {}, None),
        nested(2, "llm_inference", {"model": "m"}, {}, 3),
        nested(3, "Edit", {"file_path": "math_utils.py"}, {"ok": True}, 9),
    ]), True, True, {
        "key": None, "verify": "intact", "dag": "broken",
        "dag_errors": [
            "seq 2: triggered_by 3 is not strictly earlier",
            "seq 3: triggered_by 9 does not exist",
        ],
    }))

    malformed = [
        flat(1, "user_prompt", {"prompt": "p"}, {}, 0),
        flat(2, "llm_inference", {}, {}, 0, "1"),
        flat(3, "Read", {}, {}, 0, True),
        {**flat(4, "Edit", {}, {}, 0, 2), "metadata": {"triggered_by": 1}},
        flat(5, "Bash", {}, {}, 0, 5),
        {**flat(6, "Bash", {}, {}, 0), "metadata": {"triggered_by": [1]}},
    ]
    out.append(("dag-malformed.jsonl", chain(malformed), True, False, {
        "key": None, "verify": "intact", "dag": "broken",
        "dag_errors": [
            "seq 2: triggered_by is not an integer",
            "seq 3: triggered_by is not an integer",
            "seq 4: conflicting triggered_by (top-level 2 vs metadata 1)",
            "seq 5: triggered_by 5 is not strictly earlier",
            "seq 6: triggered_by is not an integer",
        ],
    }))

    mixed = [
        flat(1, "user_prompt", {"prompt": "p"}, {}, 0),
        nested(2, "llm_inference", {"model": "m"}, {}, 1),
        {**flat(3, "Edit", {"file_path": "a.py"}, {}, 0, 2), "metadata": {"triggered_by": 2}},
        {**flat(4, "Read", {"file_path": "a.py"}, {}, 0), "metadata": {"triggered_by": None}},
    ]
    out.append(("mixed-shape-intact.jsonl", chain(mixed), True, True,
                {"key": None, "verify": "intact", "dag": "intact"}))

    # ── v1.1: redaction at persistence (REDACTION.md) ────────────────────────
    tags = {"korg.redaction": "korg-redact@v1"}
    out.append(("redacted-intact.jsonl", chain([
        nested(1, "user_prompt", {"prompt": "deploy with my key"}, {}, None, tags=tags),
        nested(2, "Bash", {"command": "export OPENAI_API_KEY=[REDACTED:env_assignment] && ./deploy"},
               {"exit_code": 0}, 1, tags=tags),
        nested(3, "WebFetch", {"url": "https://[REDACTED:url_credentials]@example.com/",
                               "headers": {"Authorization": "[REDACTED:sensitive_key]"}},
               {"status": 200}, 2, tags=tags),
    ]), True, True, {"key": None, "verify": "intact", "dag": "intact", "redaction": "clean"}))

    out.append(("unredacted-intact.jsonl", chain([
        nested(1, "user_prompt", {"prompt": "deploy with my key"}, {}, None),
        nested(2, "Bash", {"command": "export OPENAI_API_KEY=sk-conformanceAAAAAAAAAAAAAAAAAAAA && ./deploy"},
               {"exit_code": 0}, 1),
        nested(3, "WebFetch", {"url": "https://bob:hunter2@example.com/",
                               "headers": {"Authorization": "Bearer conformance0000000000"}},
               {"status": 200}, 2),
    ]), True, True, {
        "key": None, "verify": "intact", "dag": "intact", "redaction": "unclean",
        "redaction_errors": [
            "seq 2: unredacted provider_api_key at /event/args/command",
            "seq 2: unredacted env_assignment at /event/args/command",
            "seq 3: unredacted sensitive_key at /event/args/headers/Authorization",
            "seq 3: unredacted url_credentials at /event/args/url",
        ],
    }))
    return out


def render(records, ensure_ascii, compact):
    seps = (",", ":") if compact else (", ", ": ")
    return "".join(json.dumps(r, ensure_ascii=ensure_ascii, separators=seps) + "\n" for r in records)


def manifest_for(built):
    vectors = []
    for name, records, _a, _c, entry in built:
        v = {"file": name, **entry}
        if entry["verify"] == "intact":
            key = entry["key"].encode() if entry["key"] else None
            v["tip_entry_hash"] = chain_hash(records[-1], key)
        vectors.append(v)
    return {
        "spec_version": "korg-ledger@v1",
        "revision": "1.1",
        "canonicalization": "JSON, keys sorted by code point, separators (',',':'), non-ASCII \\uXXXX-escaped; preimage = event minus entry_hash and event_sig",
        "hmac": "HMAC-SHA256 over the same preimage when a key is present",
        "causal_parent": "top-level triggered_by (flat) or metadata.triggered_by (nested); null/absent = root; both present must agree; value must be an integer (SPEC.md §9)",
        "redaction_cases": "redaction/korg-redact-v1.json",
        "vectors": vectors,
    }


def main(argv) -> int:
    write = "--write" in argv
    built = build()
    for name, records, *_ in built:
        if name in FROZEN_V1_0:
            assert chain_hash(records[-1], HMAC_KEY.encode() if name.startswith("hmac") else None) == FROZEN_V1_0[name], (
                f"{name}: regenerated tip no longer matches the FROZEN v1.0 tip")
    files = {name: render(records, a, c) for name, records, a, c, _ in built}
    manifest = json.dumps(manifest_for(built), indent=2, ensure_ascii=False) + "\n"

    drift = []
    for name, text in files.items():
        path = VECTORS / name
        if write:
            path.write_text(text, encoding="utf-8")
        elif not path.exists() or path.read_text(encoding="utf-8") != text:
            drift.append(str(path.relative_to(SPEC)))
    known = set(files) | {"redaction"}
    stray = sorted(p.name for p in VECTORS.iterdir() if p.name not in known)
    if write:
        MANIFEST.write_text(manifest, encoding="utf-8")
    elif MANIFEST.read_text(encoding="utf-8") != manifest:
        drift.append("conformance.json")

    if stray:
        print(f"vectors/ has files the generator does not own: {stray}")
    if drift:
        print("conformance vectors drifted from the generator:\n  " + "\n  ".join(drift))
        print("run: python3 spec/korg-ledger-v1/tools/generate_vectors.py --write")
    if write:
        print(f"wrote {len(files)} vectors + conformance.json")
    return 1 if (drift or stray) and not write else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
