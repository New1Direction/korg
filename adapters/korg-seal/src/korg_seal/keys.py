"""Issuer Ed25519 key management for korg-seal.

The issuer key is the identity behind a Certificate — relying parties pin its
public half (``korg-seal verify --pin``). The 32-byte raw seed lives at
``~/.korg/issuer.ed25519`` with ``0600`` perms, generated on first use.
"""
from __future__ import annotations

import os
from pathlib import Path

SEED_LEN = 32
DEFAULT_KEY_PATH = Path(os.path.expanduser("~/.korg/issuer.ed25519"))


def load_or_create_seed(path: Path | None = None) -> bytes:
    """Return the 32-byte issuer seed at ``path`` (default ``~/.korg/issuer.ed25519``),
    generating a fresh random one with ``0600`` perms if absent. Atomic write."""
    path = Path(path) if path is not None else DEFAULT_KEY_PATH
    if path.exists():
        seed = path.read_bytes()
        if len(seed) != SEED_LEN:
            raise ValueError(
                f"issuer key at {path} is {len(seed)} bytes, expected {SEED_LEN}"
            )
        return seed

    path.parent.mkdir(parents=True, exist_ok=True)
    seed = os.urandom(SEED_LEN)
    tmp = path.with_name(path.name + ".tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        os.write(fd, seed)
    finally:
        os.close(fd)
    os.replace(tmp, path)
    os.chmod(path, 0o600)
    return seed


def public_key_hex(seed: bytes) -> str:
    """Lowercase-hex raw Ed25519 public key for a 32-byte issuer seed."""
    from korg_ledger.signing import public_key_hex as _pk

    return _pk(seed)
