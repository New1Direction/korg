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
