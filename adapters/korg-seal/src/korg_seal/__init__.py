"""korg-seal — mint and verify korgcert@v1 certificates.

The producer-side counterpart to the zero-trust verifiers (Rust ``korg-verify``,
JS ``verify.mjs``, the in-browser page): it turns a captured korg-ledger@v1
session into a portable, signed **Certificate** that anyone can re-verify offline.

  capture (the hook)  →  ~/.korg/sessions/<id>.jsonl  →  korg-seal mint  →  korgcert.json  →  anyone verifies

Signing is intrinsic, so this package depends on ``cryptography`` directly and
reuses the conformant ``korg_ledger.korgcert`` / ``korg_ledger.signing`` cores.
"""
from .minter import load_ledger, mint

__all__ = ["mint", "load_ledger"]
