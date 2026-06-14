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


def anchor(
    *,
    seal: dict,
    repo: str,
    commit: str,
    seed: bytes,
    seq_id: int | None = None,
    anchored_at: str | None = None,
) -> dict:
    """Re-mint an existing Gold Seal with a git-tip anchor bound into the seal.

    Anchoring is post-hoc: you mint a seal, publish/commit it to a public repo,
    then anchor it to that commit. Because the anchor is *bound* (signed), this
    re-signs the seal — a deliberate, stronger attestation that now includes the
    time anchor. The events/tip are unchanged, so the publishing commit still
    witnesses the tip. Preserves the original claim, issuer, and issued_at.
    """
    from korg_ledger.signing import mint_seal

    events = seal.get("events") or []
    if not events:
        raise ValueError("seal has no embedded events to anchor")
    if seq_id is None:
        target = events[-1]
    else:
        target = next((e for e in events if e.get("seq_id") == seq_id), None)
        if target is None:
            raise ValueError(f"no event with seq_id {seq_id} in the seal")

    record: dict = {
        "seq_id": target.get("seq_id"),
        "entry_hash": target.get("entry_hash"),
        "anchor_kind": "git-tip",
        "anchor_proof": {"repo": repo, "commit": commit},
    }
    if anchored_at:
        record["anchored_at"] = anchored_at

    issuer = (seal.get("issuer") or {}).get("agent") or "agent:korg-seal"
    return mint_seal(
        events=events,
        claim=seal.get("claim", ""),
        issuer_agent=issuer,
        issued_at=int(seal.get("issued_at", 0)),
        private_seed=seed,
        anchors=[record],
    )
