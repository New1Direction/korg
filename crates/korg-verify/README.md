# korg-verify

A **standalone**, dependency-light **verifier** for korg receipts and journals — no network, no Python, and no workspace dependencies (the `korg-ledger@v1` chain primitives are vendored in, so `cargo install korg-verify` pulls in nothing from this repo).

```sh
cargo install korg-verify
```

```
korg-verify <receipt.json | journal.jsonl> [--key <str>] [--pubkey <hex>] [--json]
```

Exit code: `0` valid · `1` invalid/tampered · `2` usage/parse error.

## What it checks

- **Hash chain** — every event's `entry_hash` recomputes and links unbroken from genesis (tamper-evident). Uses its own vendored, conformance-tested `verify_chain` (`src/chain.rs`).
- **Causal DAG** — `triggered_by` links are well-formed (`verify_dag`).
- **Tip** — a receipt's recorded `tip` matches the chain head.
- **Signature** — if the receipt is signed, the Ed25519 signature over the tip is valid. `--pubkey <hex>` *pins* the expected signer and rejects any other key (so a green check proves authorship against a key you trust, not merely against the one the receipt carries).

## Why it exists

It is one of **three genuinely independent implementations** of **korg-ledger@v1**, each written from the spec and checked against the same frozen conformance vectors:

| Language | Implementation | Where |
|---|---|---|
| Python | `conformance.py` (dependency-free reference) | `spec/korg-ledger-v1/conformance.py` |
| JavaScript | `verify.mjs` (Node + browser, Web Crypto) | `spec/korg-ledger-v1/js/verify.mjs` |
| Rust | this crate | `crates/korg-verify` |

Three independent codepaths reproducing the same tip hashes is what makes "verify a sealed deliverable without trusting the tool that produced it" *provable* rather than asserted: tamper one byte and all three reject it.

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

`cargo test -p korg-verify` runs against the vendored frozen `korg-ledger@v1` vectors in `tests/conformance/` (intact, HMAC-keyed, non-BMP unicode, and tampered cases) plus a real receipt minted by `korgex receipt --sign` — cross-implementation proof that Rust re-derives the chain and verifies the Python-produced Ed25519 signature.
