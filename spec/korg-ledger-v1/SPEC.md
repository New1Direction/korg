# korg-ledger@v1 — Tamper-Evident Cognition Ledger

**Status:** FROZEN · **Version:** `korg-ledger@v1` · **Reference:** [`src/ledger_spec.py`](../../src/ledger_spec.py) · **Conformance:** [`conformance.json`](./conformance.json)

This document is the normative definition of korg's tamper-evident event ledger.
It is intentionally small and language-agnostic. Any implementation — the Rust
core (`korg-registry`), the Python reference (korgex), a JS verifier — is
**conformant** iff it reproduces the frozen [conformance vectors](./vectors/)
byte-for-byte. The reference module and this document MUST agree; the vectors
are the tie-breaker.

> The guarantee in one line: a journal is a hash-chain of events; any edit,
> deletion, insertion, or reorder is detectable and localized to a `seq_id`.
> With an HMAC key it is tamper-**proof** (unforgeable without the key), not
> merely tamper-**evident**.

## 1. Event

An event is a JSON object. The chain is defined over two reserved fields; all
other fields are opaque application payload (korgex uses `seq_id`, `tool_name`,
`args`, `result`, `success`, `duration_ms`, `triggered_by`, `source_agent`,
`schema_version`, …).

| Field | Type | Meaning |
|---|---|---|
| `prev_hash` | hex string (64) | the previous event's `entry_hash`, or the genesis anchor for the first event |
| `entry_hash` | hex string (64) | this event's hash (see §3) |

`GENESIS_HASH` = `"0" * 64` (64 zero characters).

## 2. Canonicalization

To hash an event, first canonicalize a JSON value to bytes. The rules (these are
what make the hash reproducible across languages):

1. Serialize as JSON.
2. **Object keys sorted** ascending by Unicode code point.
3. **No insignificant whitespace** — item separator `,`, key/value separator `:`.
4. **Non-ASCII escaped** as `\uXXXX`; output is pure ASCII (so there is no UTF-8
   encoding ambiguity). Encode the resulting ASCII string to bytes.

Reference: `json.dumps(value, sort_keys=True, separators=(",", ":")).encode("ascii")`.
Equivalent to RFC 8785 (JCS) for the JSON subset korg emits (objects, arrays,
strings, integers, booleans, null — no floats).

```
canonicalize({"z": [3, 2], "a": {"y": 1, "x": 2}})  ==  b'{"a":{"x":2,"y":1},"z":[3,2]}'
```

Non-ASCII is escaped to its `\uXXXX` form so the output is pure ASCII — e.g. a
value of `"é"` serializes to the six-character escape `"é"`, never raw UTF-8.
(See the conformance test `test_canonicalize_is_sorted_compact_ascii` for the
exact byte-level assertion.)

## 3. `entry_hash`

The **preimage** is the canonicalization of the event with its `entry_hash`
field removed (`prev_hash` IS included — that is what links the chain):

```
preimage = canonicalize({ k: v for k, v in event if k != "entry_hash" })
entry_hash = sha256(preimage).hexdigest()                 # tamper-EVIDENT
entry_hash = hmac_sha256(key, preimage).hexdigest()       # tamper-PROOF (key present)
```

Hex is lowercase.

## 4. Chaining

For events in journal order `e₁, e₂, … eₙ`:

- `e₁.prev_hash == GENESIS_HASH`
- `eᵢ.prev_hash == eᵢ₋₁.entry_hash`  for `i > 1`
- each `eᵢ.entry_hash == chain_hash(eᵢ)` per §3

## 5. Verification

`verify_chain(events, key=None) -> errors[]` ( `[]` ⇔ intact ). Walk events in
order, tracking `expected_prev` (starts at `GENESIS_HASH`):

- if `entry_hash` is absent → error "not chained";
- if `prev_hash != expected_prev` → error "broken link" (insert/delete/reorder);
- if `chain_hash(event, key) != entry_hash` → error "content tampered";
- set `expected_prev = entry_hash`.

Each error names the offending `seq_id`. A verifier given the wrong key (or no
key for a keyed chain) MUST report tampering.

`verify_dag(events) -> errors[]` additionally checks the causal structure:
`seq_id`s are unique, and every `triggered_by` references an existing,
**strictly earlier** `seq_id`. The strictly-earlier rule makes rewind-by-
truncation sound (cutting at seq N never orphans a survivor).

## 6. Conformance

[`conformance.json`](./conformance.json) lists vectors in [`vectors/`](./vectors/):

- **intact** vectors MUST `verify_chain == []` **and** the last event's
  `entry_hash` MUST equal the frozen `tip_entry_hash`. This is the cross-impl
  oracle — reproduce the tip or you are not conformant.
- **tampered** vectors MUST produce a non-empty error containing the named
  `seq`.
- the HMAC vector uses key `"korg-conformance-key"`; verifying it with no key
  MUST fail.

Run the reference harness: `python3 spec/korg-ledger-v1/conformance.py`
(exit 0 = conformant). Regenerate vectors: `python3 spec/korg-ledger-v1/_generate_vectors.py`.

## 7. v1 scope / non-goals

- v1 defines integrity (chain) and causal well-formedness (DAG). It does **not**
  define event semantics, signatures over the chain *tip* (an Ed25519 signature
  over the final `entry_hash` is a v1.1 candidate), or transport.
- Floats are out of scope for v1 canonicalization (korg events don't emit them);
  add them under JCS number rules in a future version if needed.
