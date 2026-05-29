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
| [`conformance.py`](./conformance.py) | a dependency-free reference verifier (the executable oracle) |

## Conformance

An implementation in any language is **conformant** iff, given the vectors in
`conformance.json`, it:

1. reports the intact chains as intact, **and** reproduces each frozen
   `tip_entry_hash` byte-for-byte (basic = `7418b910…`, HMAC = `1b371102…`);
2. flags each tampered vector at the named `seq`;
3. fails an HMAC vector verified without the key.

Run the reference: `python3 conformance.py` (exit 0 = conformant).

## Conformant implementations

Four independent implementations reproduce the frozen tips — the spec is real
and multi-language, not one app's detail:

| Implementation | Language | Where |
|---|---|---|
| **korg-registry** | Rust | `korg/crates/korg-registry/src/ledger_chain.rs` — chains every `CapabilityJournal` event on append |
| **korgex** | Python | `korgex/src/ledger_spec.py` — `korgex verify` over agent journals |
| **thumper** | Rust | `thumper/src/ledger/chain.rs` — chains every self-heal recovery session |
| **Ledger Explorer** | JavaScript | the launch site — recomputes the chain in-browser; tamper a journal and watch it break |

The PyO3 `korg-bridge` writes through the chained `korg-registry` journal, so
any Python caller (korgex, KorgChat) inherits a conformant journal for free.

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
