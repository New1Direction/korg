# korg-seal

Mint and verify **`goldseal@v1`** certificates — turn a captured AI-agent session
into a portable, signed **Gold Seal** that anyone can re-verify offline, with zero
trust in the tool that produced it.

```
  capture (the hook)        korg-seal mint            anyone, anywhere
  ~/.korg/sessions/x.jsonl ───────────────▶ seal.json ───────────────▶ ✓ / ✗
                              (you sign)              korg-verify · verify.mjs · the browser
```

`korg-seal` is the **producer** side. Verification is deliberately separate and
dependency-light: the Rust `korg-verify` binary, the JS `verify.mjs`, and the
in-browser page all check a seal independently — see
[`spec/korg-ledger-v1/GOLDSEAL.md`](../../spec/korg-ledger-v1/GOLDSEAL.md).

## Install

```sh
pipx install ./adapters/korg-seal        # provides the `korg-seal` command
# needs the korg-ledger reference package on PYTHONPATH / installed alongside
```

## Use

```sh
# mint a Gold Seal from a captured session, attaching a human claim
korg-seal mint ~/.korg/sessions/<id>.jsonl \
  --claim "Refactored the auth layer to JWTs; tests pass" \
  -o auth-refactor.goldseal.json

# print your issuer public key — publish/pin this so others can trust your seals
korg-seal key

# verify (the Rust korg-verify binary is the canonical, zero-trust verifier)
korg-seal verify auth-refactor.goldseal.json --pin <issuer-pubkey-hex>
```

What `mint` does:

1. Loads the ledger (JSONL or JSON array, flat or nested event shape).
2. **Refuses to seal a chain that doesn't verify** — you never put a Gold Seal on
   a tampered history (override with `--allow-unverified`, not recommended).
3. Derives the human summary *from the events* (so it cannot lie), builds the
   `goldseal@v1` envelope, and signs the canonical header with your issuer key.

## The issuer key

A 32-byte Ed25519 seed at `~/.korg/issuer.ed25519` (`0600`, generated on first
use). Its public half is your issuer identity — a relying party pins it with
`--pin`. Keep the seed private; back it up if your seals need to stay attributable
to you. Use `--key <path>` to point at a different key file.

## What a Gold Seal proves (and doesn't)

A green verdict proves the events are hash-chain-intact, the summary was re-derived
from them, and the issuer signed the whole thing (claim + summary + tip + anchors).
It does **not** prove *when* it happened (needs an external anchor resolved over the
network) or that the issuer key maps to a real-world identity (the relying party
pins that). Full threat model: `GOLDSEAL.md` §1.

## Tests

```sh
PYTHONPATH=adapters/korg-ledger-py/src:adapters/korg-seal/src \
  python3 -m pytest adapters/korg-seal/tests   # needs `cryptography`
```
