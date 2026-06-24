"""korgcert@v1 — the public, independently-verifiable certificate layer.

A Certificate is a single self-contained JSON object that wraps a verified
korg-ledger@v1 event chain together with a human-legible *summary* and an
Ed25519 *seal* signed by an issuer. Anyone can re-verify it offline, with zero
trust in the tool that produced it:

  1. the embedded events hash-chain intact and form a well-formed causal DAG;
  2. the recorded ``tip`` matches the chain head;
  3. the human summary is **re-derived from the events** and must match
     byte-for-byte — so the summary literally cannot lie about what happened;
  4. the ``seal`` signature covers the canonical header (claim + issuer +
     tip + event_count + summary + anchors), so neither the events nor the
     summary nor the claim nor the anchor set can be altered independently of
     the issuer's key.

This module is **stdlib-only**: it does derivation + the structural checks
(1-3). The Ed25519 seal signature (4) lives in :mod:`korg_ledger.signing`
(the optional ``[signing]`` extra), mirroring how per-event signing is kept
off the stdlib core. ``korgcert@v1`` is a strict superset of
``korgex-receipt@v1`` — a Certificate still verifies as a receipt under an older
receipt-only verifier (chain + DAG + tip), which simply does not see the
stronger seal/summary guarantees.

Cross-language conformant with the Rust ``korg-verify`` and JS ``verify.mjs``
korgcert codepaths: derivation and the canonical header are byte-identical.
"""
from __future__ import annotations

from ._hash import canonicalize, verify_anchors, verify_chain

SCHEMA = "korgcert@v1"
SPEC = "korg-ledger@v1"

#: Envelope keys that are NOT part of the signed header. ``events`` is excluded
#: (large, and already bound via ``tip`` + the verified chain); ``seal`` is the
#: signature itself. ``anchors`` ARE in the signed header — the seal commits to
#: the exact anchor set, so an anchor cannot be stripped, added, or altered
#: without breaking the seal (it remains structurally bound to the chain too).
_NON_HEADER_KEYS = ("events", "seal")


def _event_view(event: dict) -> tuple:
    """Normalize either event shape to ``(source_agent, tool_name, args)``.

    Capture ledgers nest the payload under ``event`` (the LedgerWriter
    JournalEvent shape); receipts/flat captures put these at the top level.
    Both are valid korg-ledger@v1 records and must derive the same summary.
    Non-object events contribute nothing (the verifier never crashes on them).
    """
    if not isinstance(event, dict):
        return None, None, None
    inner = event.get("event")
    if isinstance(inner, dict):
        return inner.get("source_agent"), inner.get("tool_name"), inner.get("args")
    return event.get("source_agent"), event.get("tool_name"), event.get("args")


def derive_summary(events: list) -> dict:
    """Deterministically derive the human summary from the event chain.

    Every field here is a pure function of ``events`` so a verifier can
    re-derive it and reject any tampered summary. It emits only integers (counts,
    seq ids), strings, arrays and objects-of-those — never floats — so the derived
    object canonicalizes byte-identically across Rust/Python/JS.
    """
    by_tool: dict[str, int] = {}
    files: set[str] = set()
    agents: set[str] = set()
    safe = events if isinstance(events, list) else []
    for e in safe:
        agent, tool, args = _event_view(e)
        if isinstance(tool, str):
            by_tool[tool] = by_tool.get(tool, 0) + 1
        if isinstance(agent, str):
            agents.add(agent)
        if isinstance(args, dict):
            for key in ("file_path", "path"):
                val = args.get(key)
                if isinstance(val, str):
                    files.add(val)
    seqs = [
        e.get("seq_id")
        for e in safe
        if isinstance(e, dict) and isinstance(e.get("seq_id"), int)
    ]
    return {
        "agents": sorted(agents),
        "by_tool": dict(sorted(by_tool.items())),
        "files": sorted(files),
        "seq_first": min(seqs) if seqs else 0,
        "seq_last": max(seqs) if seqs else 0,
    }


def seal_header(envelope: dict) -> dict:
    """The signed portion of a Certificate: the envelope minus ``events`` and
    ``seal`` (so it includes ``anchors`` when present).

    This is the exact object whose canonicalization is the seal signature
    preimage. Identical at mint time and verify time, so the signature is
    reproducible.
    """
    if not isinstance(envelope, dict):
        return {}
    return {k: v for k, v in envelope.items() if k not in _NON_HEADER_KEYS}


def build_envelope(
    *,
    events: list,
    claim: str,
    issuer_agent: str,
    issued_at: int,
    anchors: list | None = None,
) -> dict:
    """Assemble the *unsigned* Certificate envelope (header + events [+ anchors]).

    The caller signs ``seal_header(envelope)`` and attaches the ``seal``. Kept
    separate from signing so the stdlib core can construct the bound object
    without ``cryptography``.
    """
    if not events:
        raise ValueError("cannot seal an empty event chain")
    tip = events[-1].get("entry_hash")
    if not isinstance(tip, str):
        raise ValueError("final event has no entry_hash; chain is not sealed")
    envelope: dict = {
        "schema": SCHEMA,
        "spec": SPEC,
        "claim": claim,
        "issued_at": int(issued_at),
        "issuer": {"agent": issuer_agent},
        "event_count": len(events),
        "tip": tip,
        "summary": derive_summary(events),
    }
    envelope["events"] = events
    if anchors:
        envelope["anchors"] = anchors
    return envelope


def verify_structure(envelope: dict) -> list:
    """Run the hermetic, crypto-free half of Certificate verification.

    Returns a list of human-readable errors; empty iff the chain, DAG, tip,
    event_count and the **re-derived summary** all check out. The Ed25519 seal
    signature is checked separately by :func:`korg_ledger.signing.verify_seal`.
    """
    errors: list[str] = []
    if not isinstance(envelope, dict):
        return ["envelope is not a JSON object"]
    if envelope.get("schema") != SCHEMA:
        errors.append(f"schema is {envelope.get('schema')!r}, expected {SCHEMA!r}")

    events = envelope.get("events")
    if not isinstance(events, list) or not events:
        errors.append("envelope has no embedded events")
        return errors

    from ._hash import verify_dag  # local import: keeps the public surface in __init__

    errors.extend(verify_chain(events))
    errors.extend(verify_dag(events))

    claimed_tip = envelope.get("tip")
    last = events[-1]
    head = last.get("entry_hash") if isinstance(last, dict) else None
    if claimed_tip != head:
        errors.append("recorded tip does not match the chain head")

    claimed_count = envelope.get("event_count")
    # Type-strict: a JSON float (5.0) must not satisfy the count (Rust's as_u64
    # rejects it) — keep the three verdicts aligned.
    if (
        not isinstance(claimed_count, int)
        or isinstance(claimed_count, bool)
        or claimed_count != len(events)
    ):
        errors.append(
            f"event_count {claimed_count!r} does not match the {len(events)} embedded events"
        )

    claimed_summary = envelope.get("summary")
    derived = derive_summary(events)
    try:
        summary_match = canonicalize(claimed_summary) == canonicalize(derived)
    except ValueError:
        summary_match = False  # out-of-domain number in the claimed summary
    if not summary_match:
        errors.append("summary does not match the events (re-derivation mismatch)")

    anchors = envelope.get("anchors")
    if isinstance(anchors, list) and anchors:
        errors.extend(verify_anchors(events, anchors))

    return errors
