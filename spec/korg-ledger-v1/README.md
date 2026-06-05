# korg-ledger@v1 — an open standard for tamper-evident agent ledgers

**Status:** FROZEN · **Canonical home:** this directory (`korg/spec/korg-ledger-v1/`) is the normative source; all implementations conform to it.

A korg-ledger is a hash-chained log of agent events. Each event carries
`prev_hash` (the previous event's `entry_hash`, or a genesis anchor) and
`entry_hash` (the hash of its own canonical preimage). Given a trusted tip, any
edit, deletion, insertion, or reorder is **detectable and localized to a
`seq_id`**. With an HMAC key the chain is tamper-**proof** (unforgeable without
the key), not merely tamper-evident.

> An agent's session log stops being a file you have to trust and becomes a
> record you can **check**. This is the substrate for verifiable cognition: the
> ledger is the audit answer.

## Files

| File | Role |
|---|---|
| [`SPEC.md`](./SPEC.md) | the normative specification (canonicalization, preimage, chaining, HMAC, verify + DAG algorithms) |
| [`vectors/`](./vectors/) + [`conformance.json`](./conformance.json) | the golden conformance vectors with **frozen tip hashes** — the cross-language oracle |
| [`conformance.py`](./conformance.py) | a dependency-free Python reference verifier (the executable oracle) |
| [`js/`](./js/) | a dependency-free JavaScript verifier (`verify.mjs`) + its conformance harness, for Node and the browser |

## Conformance

An implementation in any language is **conformant** iff, given the vectors in
`conformance.json`, it:

1. reports the intact chains as intact, **and** reproduces each frozen
   `tip_entry_hash` byte-for-byte (basic = `7418b910…`, HMAC = `1b371102…`);
2. flags each tampered vector at the named `seq`;
3. fails an HMAC vector verified without the key.

Run a reference — each is one command, exit 0 = conformant:

```sh
python3 conformance.py        # Python
node js/conformance.mjs        # JavaScript
cargo test -p korg-verify      # Rust (from the korg workspace root)
```

## Conformant implementations

**Three genuinely independent implementations** — three languages, three separate
codepaths written from this spec — reproduce the frozen tips. That is what makes
the spec real and multi-language rather than one app's internal detail:

| Implementation | Language | Where | Conformance |
|---|---|---|---|
| **Python reference** | Python | [`conformance.py`](./conformance.py) — dependency-free, stdlib only | `python3 conformance.py` |
| **JavaScript** | JavaScript | [`js/verify.mjs`](./js/verify.mjs) — dependency-free, Web Crypto, Node + browser | `node js/conformance.mjs` |
| **Rust** | Rust | [`korg-verify`](../../crates/korg-verify) (`cargo install korg-verify`), built on the publishable [`korg-ledger`](../../crates/korg-ledger) crate | `cargo test -p korg-verify` |

Each reproduces every intact vector's frozen `tip_entry_hash` and flags every
tampered vector — so a green check on one is corroborated by two independent
others. The same Ed25519-signed receipt verifies under all three: Python mints
the signature, Rust and JavaScript re-verify it.

The repo's writers conform by construction: the Rust core
(`crates/korg-registry`, `crates/korg-ledger`) chains every `CapabilityJournal`
event on append, and the PyO3 `korg-bridge` writes through it, so any Python
caller (korgex, KorgChat) inherits a conformant journal for free.

## Implementing it (5 steps)

1. **canonicalize** a JSON value: sorted keys, compact separators (`,` / `:`),
   non-ASCII `\uXXXX`-escaped (ASCII output). See SPEC.md §2.
2. **chain_hash(event)** = `sha256(canonicalize(event without entry_hash))`
   (or `hmac_sha256(key, …)`), lowercase hex. `prev_hash` stays in the preimage.
3. on **append**: set `prev_hash` to the prior `entry_hash` (genesis for the
   first), then compute `entry_hash`.
4. **verify_chain**: recompute each event; flag a broken `prev_hash` link or an
   `entry_hash` mismatch, localized to its `seq_id`.
5. run the **conformance vectors** and reproduce the frozen tips. You're conformant.

## Versioning

`korg-ledger@v1` is frozen. Additive, backward-compatible changes ship as a new
minor reference; breaking changes bump to `@v2` with its own vectors. v1 scope:
integrity (chain) + causal well-formedness (DAG). Out of scope for v1:
signatures over the chain tip (a v1.1 candidate), float canonicalization.
