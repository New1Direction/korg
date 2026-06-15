"""Optional Ed25519 signing for korg-ledger@v1 (requires `cryptography`).

NOT imported by korg_ledger/__init__.py — import it lazily so the stdlib-only
core never depends on `cryptography`. Signs the same canonical preimage as
`entry_hash` (event minus HASH_FIELDS, canonicalized), encoded as lowercase
hex — byte-identical to the Rust `sign_event`/`verify_event_sig`.
"""
from __future__ import annotations

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

from ._hash import HASH_FIELDS, canonicalize


def _preimage(event: dict) -> bytes:
    return canonicalize({k: v for k, v in event.items() if k not in HASH_FIELDS})


def sign_event(private_seed: bytes, event: dict) -> str:
    """Ed25519-sign an event's canonical preimage. `private_seed` is the raw
    32-byte seed. Returns the 64-byte signature as lowercase hex."""
    key = Ed25519PrivateKey.from_private_bytes(private_seed)
    return key.sign(_preimage(event)).hex()


def verify_event_sig(public_bytes: bytes, event: dict, sig_hex: str) -> bool:
    """Verify an event's signature against a raw 32-byte Ed25519 public key.
    Returns False on any error (never raises)."""
    try:
        Ed25519PublicKey.from_public_bytes(public_bytes).verify(
            bytes.fromhex(sig_hex), _preimage(event)
        )
        return True
    except Exception:
        return False


# ── goldseal@v1 — seal-level signing ────────────────────────────────────────
# The seal signs the canonical *header* (the Gold Seal envelope minus
# events/seal/anchors), binding claim + issuer + tip + event_count + summary
# together under one Ed25519 key. Same primitive as per-event signing:
# Ed25519 over the canonical preimage, lowercase hex.


def public_key_hex(private_seed: bytes) -> str:
    """Lowercase-hex raw Ed25519 public key for a 32-byte private seed."""
    return (
        Ed25519PrivateKey.from_private_bytes(private_seed)
        .public_key()
        .public_bytes_raw()
        .hex()
    )


def sign_seal(private_seed: bytes, header: dict) -> str:
    """Ed25519-sign a Gold Seal header's canonical bytes. Lowercase-hex sig."""
    from ._hash import canonicalize

    key = Ed25519PrivateKey.from_private_bytes(private_seed)
    return key.sign(canonicalize(header)).hex()


def verify_seal_sig(public_bytes: bytes, header: dict, sig_hex: str) -> bool:
    """Verify a seal signature over a header's canonical bytes. False on any
    error (never raises)."""
    from ._hash import canonicalize

    try:
        Ed25519PublicKey.from_public_bytes(public_bytes).verify(
            bytes.fromhex(sig_hex), canonicalize(header)
        )
        return True
    except Exception:
        return False


def mint_seal(
    *,
    events: list,
    claim: str,
    issuer_agent: str,
    issued_at: int,
    private_seed: bytes,
    anchors: list | None = None,
) -> dict:
    """Mint a signed goldseal@v1 envelope from a verified event chain.

    Builds the bound header (deriving the summary from ``events``), signs its
    canonical bytes, and attaches the ``seal``. The returned object verifies
    under :func:`verify_seal` and — being a receipt superset — under any
    receipt-only verifier (chain + DAG + tip).
    """
    from . import goldseal

    envelope = goldseal.build_envelope(
        events=events,
        claim=claim,
        issuer_agent=issuer_agent,
        issued_at=issued_at,
        anchors=anchors,
    )
    header = goldseal.seal_header(envelope)
    envelope["seal"] = {
        "alg": "ed25519",
        "pubkey": public_key_hex(private_seed),
        "sig": sign_seal(private_seed, header),
    }
    return envelope


def verify_seal(envelope: dict, pin_pubkey: str | None = None) -> list:
    """Fully verify a goldseal@v1 envelope: structure (chain + DAG + tip +
    re-derived summary) plus the Ed25519 seal signature. Returns a list of
    errors; empty iff valid.

    ``pin_pubkey``: require the issuer key to equal a key the relying party
    already trusts — closing the self-referential hole where a bare check only
    proves the seal matches the *returned* key.
    """
    from . import goldseal

    if not isinstance(envelope, dict):
        return ["envelope is not a JSON object"]

    errors = goldseal.verify_structure(envelope)

    seal = envelope.get("seal")
    if isinstance(seal, dict):
        pubkey = seal.get("pubkey") or ""
        sig = seal.get("sig") or ""
        header = goldseal.seal_header(envelope)
        ok = False
        try:
            ok = verify_seal_sig(bytes.fromhex(pubkey), header, sig)
        except ValueError:
            ok = False
        if not ok:
            errors.append("seal signature does not verify for the header")
        if pin_pubkey is not None and pin_pubkey != pubkey:
            errors.append(f"issuer {pubkey} does not match the pinned key {pin_pubkey}")
    elif pin_pubkey is not None:
        errors.append(f"seal is absent but signer {pin_pubkey} was required")
    else:
        errors.append("seal is absent (unsigned Gold Seal)")

    return errors
