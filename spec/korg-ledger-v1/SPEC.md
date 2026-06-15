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

Run a conformance harness (exit 0 = conformant): `python3 spec/korg-ledger-v1/conformance.py`
(Python), `node spec/korg-ledger-v1/js/conformance.mjs` (JavaScript), or
`cargo test -p korg-verify` (Rust). Regenerate vectors:
`python3 spec/korg-ledger-v1/_generate_vectors.py`.

## 7. v1 scope / non-goals

- v1 defines integrity (chain) and causal well-formedness (DAG). It does **not**
  define event semantics, signatures over the chain *tip* (an Ed25519 signature
  over the final `entry_hash` is a v1.1 candidate), or transport.
- Floats are out of scope for v1 canonicalization (korg events don't emit them);
  add them under JCS number rules in a future version if needed.

## 8. Phase-2 extensions: per-event signatures & external anchors

These are **additive** and backward-compatible: both fields are excluded from
the hash preimage (`HASH_FIELDS = ["entry_hash", "event_sig"]` in all impls), so
an unsigned, un-anchored journal is byte-identical to v1 and every frozen vector
still reproduces.

### 8.1 Per-event signature (`event_sig`)

- Optional field on an event. Value: lowercase hex of an Ed25519 signature
  (64 bytes → 128 hex chars) over the **canonical preimage** — exactly the bytes
  `chain_hash` hashes (`event` minus `HASH_FIELDS`, canonicalized per §2). The
  signature is over the **raw preimage bytes**, not their SHA-256 (RFC 8032 pure
  Ed25519).
- Verify: `verify_event_sig(pubkey_hex, event, sig_hex)`. Implemented and
  cross-checked in Rust (`korg-ledger`/`korg-verify`, `signing` feature), Python
  (`korg_ledger.signing`, `cryptography` extra), and JS (`verify.mjs`, Web Crypto).
  The frozen fixture `crates/korg-verify/tests/fixtures/signed-events.jsonl`
  (signed by Python, seed `[42; 32]`) is verified by all three — the cross-impl
  signature oracle.
- What it adds: per-event attributability — the holder of the named key attests
  to that exact event, independent of the chain. What it does **not** add: a
  binding of the key to a real-world identity (the relying party pins the pubkey).

### 8.2 External anchors (`anchors.jsonl`)

A sidecar file (never in the preimage). One record per line:

```json
{"seq_id": 3, "entry_hash": "<hex>", "anchor_kind": "git-tip",
 "anchor_proof": {"repo": "<url>", "commit": "<sha>"}, "anchored_at": "<ISO-8601>"}
```

- **Structural verification** (`verify_anchors(chain, anchors)`, implemented in
  all three impls, always hermetic): every anchor's `entry_hash` must match the
  chain event at its `seq_id`. Empty result = structurally sound.
- **External verification** (`git-tip` kind, Rust verifier, network, off by
  default): the named public commit must witness the `entry_hash`. A public git
  commit is immutable once pushed and mirrored; an owner who rewrites the chain
  must also rewrite/force-push the public commit, which any third party who has
  fetched the repo will detect. **This** is what closes the
  owner-rewrites-history-undetectably gap.
- **Honest limits:** the structural check alone is necessary but not sufficient
  (an owner controlling both the chain and `anchors.jsonl` can produce a
  consistent local forgery — the external witness is the defense). `anchored_at`
  is local wall-clock, **not** a trusted time source: a git-tip anchor proves the
  chain was published *before any third party fetched it*, not before a specific
  clock time. Anchors for seq_ids later removed by a rewind are stale and should
  be dropped.
