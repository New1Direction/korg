"""``python -m korg_ledger`` — import envelopes and lint journals.

    python -m korg_ledger envelope okto-session SESSION.json --source-version 0.1.0 > env.json
    python -m korg_ledger envelope okto-genome genome.jsonl --source-version 0.1.0 > env.json
    python -m korg_ledger envelope dazai-log daemon.log --source-version 0.1.0 > env.json
    python -m korg_ledger envelope ningen-sink lifecycle.jsonl > env.json   # dazai/motokano --korg-sink
    python -m korg_ledger import env.json --ledger .korg/ledger.jsonl [--triggered-by N]
    python -m korg_ledger lint .korg/ledger.jsonl        # chain + causal DAG + redaction
    python -m korg_ledger rechain-redacted old.jsonl new.jsonl   # migration: redacted copy, new chain

Exit codes: 0 ok · 1 rejected / lint failure · 2 usage or parse error.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from ._hash import verify_chain, verify_dag
from .imports import (
    OKTO_PROFILE,
    SCHEMA,
    ImportRejected,
    dazai_log_to_envelope,
    import_envelope,
    ningen_sink_to_envelope,
    okto_mutation_record,
    okto_session_to_envelope,
)
from .redaction import rechain_redacted, verify_redaction
from .writer import LedgerWriter


def _json_objects(text: str) -> list:
    text = text.strip()
    if not text:
        return []
    if text.startswith("["):
        return json.loads(text)
    try:
        return [json.loads(text)]  # a single (possibly pretty-printed) object
    except json.JSONDecodeError:
        return [json.loads(line) for line in text.splitlines() if line.strip()]


def _cmd_envelope(a) -> int:
    text = Path(a.file).read_text(encoding="utf-8")
    if a.source_version is None and a.source != "ningen-sink":
        print(f"--source-version is required for {a.source}", file=sys.stderr)
        return 2
    if a.source == "ningen-sink":
        try:
            env = ningen_sink_to_envelope(text.splitlines(), version=a.source_version)
        except ValueError as ex:
            print(f"error: {ex} (--source-version)", file=sys.stderr)
            return 2
    elif a.source == "okto-session":
        objs = _json_objects(text)
        if len(objs) != 1 or not isinstance(objs[0], dict):
            print("expected exactly one Okto Session object", file=sys.stderr)
            return 2
        env = okto_session_to_envelope(objs[0], okto_version=a.source_version)
    elif a.source == "okto-genome":
        env = {
            "schema": SCHEMA,
            "profile": OKTO_PROFILE,
            "source": {"name": "okto", "version": a.source_version},
            "records": [okto_mutation_record(m) for m in _json_objects(text) if isinstance(m, dict)],
        }
    elif a.source == "dazai-log":
        env = dazai_log_to_envelope(text.splitlines(), version=a.source_version)
    json.dump(env, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


def _cmd_import(a) -> int:
    env = json.loads(Path(a.envelope).read_text(encoding="utf-8"))
    try:
        seqs = import_envelope(LedgerWriter(a.ledger), env, triggered_by=a.triggered_by)
    except ImportRejected as e:
        print(f"import rejected ({len(e.errors)} problem(s)); nothing was written:", file=sys.stderr)
        for err in e.errors[:50]:
            print(f"  - {err}", file=sys.stderr)
        return 1
    print(f"imported {len(seqs) - 1} record(s) as seq {seqs[0]}..{seqs[-1]} (batch seq {seqs[0]})")
    return 0


def _cmd_lint(a) -> int:
    events = _json_objects(Path(a.journal).read_text(encoding="utf-8"))
    key = a.key.encode() if a.key else None
    errors = verify_chain(events, key) + verify_dag(events) + verify_redaction(events)
    if errors:
        print(f"✗ {len(errors)} problem(s):")
        for e in errors[:50]:
            print(f"  - {e}")
        return 1
    print(f"✓ {len(events)} events: chain intact, causal DAG well-formed, redaction-clean (korg-redact@v1)")
    return 0


def _cmd_rechain(a) -> int:
    events = _json_objects(Path(a.journal).read_text(encoding="utf-8"))
    key = a.key.encode() if a.key else None
    errors = verify_chain(events, key)
    if errors and not a.force:
        print("refusing to re-chain a journal that does not verify (pass --force to copy it anyway):")
        for e in errors[:20]:
            print(f"  - {e}")
        return 1
    out = rechain_redacted(events, key)
    if Path(a.out).exists() and Path(a.out).stat().st_size:
        print(f"refusing to overwrite non-empty {a.out}", file=sys.stderr)
        return 2
    Path(a.out).write_text("".join(json.dumps(e, separators=(",", ":")) + "\n" for e in out), encoding="utf-8")
    old_tip = events[-1].get("entry_hash") if events else None
    new_tip = out[-1]["entry_hash"] if out else None
    print(f"wrote {len(out)} redacted events to {a.out}")
    print(f"  old tip {old_tip}\n  new tip {new_tip}\n  record both tips (and anchor them) to keep the copy auditable")
    return 0


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="python -m korg_ledger", description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)

    e = sub.add_parser("envelope", help="build a korg-import@v1 envelope from a sibling tool's native output")
    e.add_argument("source", choices=["okto-session", "okto-genome", "dazai-log", "ningen-sink"])
    e.add_argument("file")
    e.add_argument(
        "--source-version",
        help="version of the tool that produced FILE (required, except ningen-sink records carry it)",
    )
    e.set_defaults(fn=_cmd_envelope)

    i = sub.add_parser("import", help="validate an envelope and append it to a ledger (atomic on rejection)")
    i.add_argument("envelope")
    i.add_argument("--ledger", required=True)
    i.add_argument("--triggered-by", type=int, default=None)
    i.set_defaults(fn=_cmd_import)

    lint = sub.add_parser("lint", help="verify chain + causal DAG + redaction cleanliness of a journal")
    lint.add_argument("journal")
    lint.add_argument("--key", default=None, help="HMAC key for keyed chains")
    lint.set_defaults(fn=_cmd_lint)

    r = sub.add_parser("rechain-redacted", help="write a redacted, re-chained copy of a journal (migration)")
    r.add_argument("journal")
    r.add_argument("out")
    r.add_argument("--key", default=None, help="HMAC key: verifies the input and keys the new chain")
    r.add_argument("--force", action="store_true", help="copy even if the input chain does not verify")
    r.set_defaults(fn=_cmd_rechain)

    a = p.parse_args(argv)
    try:
        return a.fn(a)
    except (OSError, json.JSONDecodeError) as ex:
        print(f"error: {ex}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
