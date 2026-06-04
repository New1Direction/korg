# korg-verify

A standalone, dependency-light **verifier** for korg receipts and journals — no network, no Python.

```
korg-verify <receipt.json | journal.jsonl> [--key <str>] [--pubkey <hex>] [--json]
```

Exit code: `0` valid · `1` invalid/tampered · `2` usage/parse error.

## What it checks

- **Hash chain** — every event's `entry_hash` recomputes and links unbroken from genesis (tamper-evident). Reuses `korg-registry`'s conformance-tested `verify_chain`.
- **Causal DAG** — `triggered_by` links are well-formed (`verify_dag`).
- **Tip** — a receipt's recorded `tip` matches the chain head.
- **Signature** — if the receipt is signed, the Ed25519 signature over the tip is valid. `--pubkey <hex>` *pins* the expected signer and rejects any other key (so a green check proves authorship against a key you trust, not merely against the one the receipt carries).

## Why it exists

It is the third independent implementation of **korg-ledger@v1** — Python (`korgex receipt verify`), JavaScript (the self-verifying HTML report), and now Rust — all checked against the same frozen conformance vectors. That makes "verify a sealed deliverable without trusting the tool that produced it" provable rather than asserted: a single small binary anyone can run, in CI or by hand.

## Examples

```sh
korg-verify deliverable.korgreceipt.json
#   ✓ receipt VALID — 5 events, hash-chain + DAG intact · signed by d04ab232…

korg-verify deliverable.korgreceipt.json --pubkey d04ab232…   # require this exact signer
korg-verify run.jsonl --key "$HMAC_KEY"                        # keyed (HMAC) chain
korg-verify deliverable.korgreceipt.json --json               # machine-readable verdict
```

## Build

```sh
cargo build --release -p korg-verify   # → target/release/korg-verify
```

## Tests

`cargo test -p korg-verify` runs against the shared `crates/korg-registry/tests/conformance` vectors (intact, HMAC-keyed, non-BMP unicode, and tampered cases) plus a real receipt minted by `korgex receipt --sign` — cross-implementation proof that Rust re-derives the chain and verifies the Python-produced Ed25519 signature.
