"""Mint a goldseal@v1 certificate from a captured session ledger."""
from __future__ import annotations

import json
import time
from pathlib import Path

from korg_ledger import verify_chain, verify_dag


def load_ledger(path: str | Path) -> list:
    """Load a korg-ledger@v1 chain from a JSONL or JSON-array file."""
    text = Path(path).read_text(encoding="utf-8").strip()
    if not text:
        raise ValueError("ledger is empty")
    if text.startswith("["):
        events = json.loads(text)
    else:
        events = [json.loads(line) for line in text.splitlines() if line.strip()]
    if not isinstance(events, list) or not events:
        raise ValueError("ledger has no events")
    return events


def mint(
    *,
    ledger_path: str | Path,
    claim: str,
    seed: bytes,
    issuer_agent: str | None = None,
    issued_at: int | None = None,
    anchors: list | None = None,
    strict: bool = True,
) -> dict:
    """Mint a signed Gold Seal from the ledger at ``ledger_path``.

    Refuses (in ``strict`` mode) to seal a chain that does not verify — you
    should never put a Gold Seal on a tampered history. ``issuer_agent``
    defaults to a label derived from the issuer key; ``issued_at`` defaults to
    the current Unix time.
    """
    from korg_ledger.signing import mint_seal, public_key_hex

    events = load_ledger(ledger_path)

    problems = verify_chain(events) + verify_dag(events)
    if problems and strict:
        raise ValueError(
            f"refusing to seal a ledger that does not verify: {problems[0]}"
        )

    if issuer_agent is None:
        issuer_agent = f"agent:korg-seal#{public_key_hex(seed)[:16]}"
    if issued_at is None:
        issued_at = int(time.time())

    return mint_seal(
        events=events,
        claim=claim,
        issuer_agent=issuer_agent,
        issued_at=int(issued_at),
        private_seed=seed,
        anchors=anchors,
    )
