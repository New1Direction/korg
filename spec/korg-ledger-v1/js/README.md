# @korgg/ledger-verify

The **JavaScript** implementation of **korg-ledger@v1** — one of three independent
verifiers (alongside the Python reference and the Rust `korg-verify` crate), each
written from [the spec](../SPEC.md) and checked against the same frozen
[conformance vectors](../vectors/). Tamper one byte and all three reject it.

- **Zero dependencies.** Uses only the Web Crypto standard (`crypto.subtle`).
- **Isomorphic.** The same `verify.mjs` runs in Node (≥18) and the browser.
- **No network, no trust in the producing tool.** A receipt verifies (or doesn't)
  from its bytes alone.

## CLI

```sh
npx @korgg/ledger-verify <receipt.json | journal.jsonl> [--key <str>] [--pubkey <hex>] [--json]
# or, from a checkout:
node verify.mjs deliverable.korgreceipt.json
```

Exit code: `0` valid · `1` invalid/tampered · `2` usage/parse error.

```
✓ receipt VALID — 6 events, hash-chain + DAG intact · signed by b251a84c2d23d318…
```

`--pubkey <hex>` *pins* the expected signer and rejects any other key, so a green
check proves authorship against a key you already trust — not merely against the
one the receipt happens to carry.

## Library

```js
import { verifyText, verifyChain, canonicalize } from "@korgg/ledger-verify";

const verdict = await verifyText(receiptText, { pinPubkey: "b251a84c…" });
verdict.valid; // boolean
```

In the browser, import the same module and pass the receipt text — Web Crypto does
the SHA-256 / HMAC / Ed25519. (Ed25519 in `crypto.subtle` requires a recent
runtime: Node ≥18.4 and current Chrome/Safari/Firefox; the chain + DAG checks work
everywhere.)

## What a green verdict proves

The recorded events hash-chain intact and link in a well-formed causal DAG
(tamper-evident), the receipt's tip matches the chain head, and — if signed — the
holder of the named key attests to that exact tip. It does **not** prove *when* it
happened (needs an external time anchor) or that the key maps to a real-world
identity (the relying party pins that with `--pubkey`).

## Conformance

```sh
npm test        # node conformance.mjs — reproduces the frozen tip hashes; exit 0 = conformant
```

This is the executable oracle: an intact vector must reproduce its frozen
`tip_entry_hash`, a tampered vector must be flagged at the named `seq`. The same
manifest ([`../conformance.json`](../conformance.json)) drives the Python
([`../conformance.py`](../conformance.py)) and Rust
([`../../../crates/korg-verify`](../../../crates/korg-verify)) implementations.
