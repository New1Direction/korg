# korg-ledger@v1 — Tamper-Evident Agent Flight Recorder

**Status:** AUTHORITATIVE · FROZEN hash rules · **Version:** `korg-ledger@v1`, revision **1.1** · **Reference oracle:** [`conformance.py`](./conformance.py) · **Conformance:** [`conformance.json`](./conformance.json) · **Vector generator:** [`tools/generate_vectors.py`](./tools/generate_vectors.py)

This document is the normative definition of korg as an **agent flight
recorder**: the tamper-evident event ledger (§1–§8), how verifiers read both
journal shapes and their causal links (§9), redaction at persistence (§10,
[`REDACTION.md`](./REDACTION.md)), and import envelopes for evidence korg did not
observe itself (§11, [`IMPORT.md`](./IMPORT.md)). It is intentionally small and
language-agnostic. Any implementation — the Rust core (`korg-ledger`,
`korg-registry`, `korg-verify`), the Python producer (`korg_ledger`), the JS
verifier — is **conformant** iff it reproduces the [conformance
vectors](./vectors/) listed in [`conformance.json`](./conformance.json): frozen
tips, exact causal-DAG errors, and the shared redaction cases.

### Authority

**This directory is the canonical contract.** When any other document — the
repo-root `agent_event_spec.md` (the HTTP/MCP application profile), a README, a
code comment — or any implementation disagrees with this spec, this spec and its
vectors win and the other artifact is the bug. The vectors are the tie-breaker
between this prose and the reference oracle. There is exactly one copy of the
vectors and manifest (`vectors/`, `conformance.json`); every implementation reads
them from here, and `tools/generate_vectors.py --check` (CI) fails on any drift.
Migration notes for each revision: [`MIGRATION.md`](./MIGRATION.md).

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
(See the `canon non-ascii` / `canon astral surrogate-pair` assertions in
[`js/conformance.mjs`](./js/conformance.mjs) and the `nonbmp-intact.jsonl`
vector for the exact byte-level oracle.)

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
integer `seq_id`s are unique, and every event's causal parent (§9 — top-level
`triggered_by` on a flat record, `metadata.triggered_by` on a nested
JournalEvent) is a well-formed integer that references an existing, **strictly
earlier** `seq_id`. The strictly-earlier rule makes rewind-by-truncation sound
(cutting at seq N never orphans a survivor). DAG error strings are normative
(§9.4) so the three verifiers can be compared byte-for-byte.

## 6. Conformance

[`conformance.json`](./conformance.json) lists vectors in [`vectors/`](./vectors/):

- **intact** vectors MUST `verify_chain == []` **and** the last event's
  `entry_hash` MUST equal the frozen `tip_entry_hash`. This is the cross-impl
  oracle — reproduce the tip or you are not conformant.
- **tampered** vectors MUST produce a non-empty error containing the named
  `seq`.
- the HMAC vector uses key `"korg-conformance-key"`; verifying it with no key
  MUST fail.
- every vector declares `dag`: `"intact"` vectors MUST `verify_dag == []`;
  `"broken"` vectors MUST produce **exactly** the listed `dag_errors`, in order
  (§9.4).
- vectors that declare `redaction` (`"clean"` / `"unclean"`) pin the
  korg-redact@v1 lint: an implementation that ships the lint MUST produce
  exactly the listed `redaction_errors`. The shared redaction cases
  (`vectors/redaction/korg-redact-v1.json`) pin `redact()` itself.
- the manifest MUST list every `vectors/*.jsonl` file (no unlisted vectors).

Run a conformance harness (exit 0 = conformant): `python3 spec/korg-ledger-v1/conformance.py`
(Python), `node spec/korg-ledger-v1/js/conformance.mjs` (JavaScript), or
`cargo test -p korg-verify` (Rust). Regenerate or drift-check vectors:
`python3 spec/korg-ledger-v1/tools/generate_vectors.py [--write]` — the v1.0
vectors are frozen and the generator asserts it reproduces their tips.

## 7. v1 scope / non-goals

- v1 defines integrity (chain) and causal well-formedness (DAG). It does **not**
  define event semantics, signatures over the chain *tip* (an Ed25519 signature
  over the final `entry_hash` is a v1.1 candidate), or transport.
- **Number domain.** Canonicalizable numbers are integers within ±(2^53−1) — the
  range that round-trips through a JS `Number` (IEEE-754 double). All three
  reference verifiers **reject** integers beyond that range so they never silently
  disagree. NaN/Infinity are rejected (they are not valid JSON). Finite **floats**
  are out of scope (korg events don't emit them in hashed fields): the Python and
  Rust verifiers will hash a finite float, but the JS verifier reports such an event
  unverifiable — producers MUST emit integers. JCS number rules are a future-version
  candidate.

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

## 9. Journal shapes & causal parent (revision 1.1)

### 9.1 Two shapes, one chain

A journal is a sequence of JSON objects. Each object is one of two shapes; a
journal MAY mix them (verifiers decide per event):

| Shape | Written by | Where the causal parent lives |
|---|---|---|
| **flat** — payload fields at the top level (`seq_id`, `tool_name`, `args`, …) | conformance vectors, `korg-runtime` run-once, receipts/Certificates minted from flat events, legacy adapters | top-level `triggered_by` |
| **nested** JournalEvent — `{schema_version, seq_id, metadata:{…}, event:{…}}` | `korg_ledger.LedgerWriter`, the Rust `CapabilityJournal` (korg-server, korg-bridge), the Claude Code hook | `metadata.triggered_by` |

The hash chain (§2–§5) is shape-agnostic: the preimage is the whole top-level
object minus `HASH_FIELDS`, so `metadata` and `event` are hashed in full.
`seq_id`, `prev_hash`, `entry_hash`, and `event_sig` are always top-level.

### 9.2 Ledger integers

A **ledger integer** is a JSON number whose value is integral and within
±(2^53−1). JSON `true`/`false` are **not** integers (Python's `bool` subclasses
`int`; verifiers MUST NOT follow it). An integral number written with a fraction
or exponent (`2.0`, `2e0`) **is** the integer 2: a JS verifier cannot tell `2.0`
from `2` after parsing, so all verifiers accept it in order to agree. Producers
MUST still write plain integers (`2`).

### 9.3 Resolving the causal parent

`causal_parent(event)`:

1. Collect the non-null values of top-level `triggered_by` and, when `metadata`
   is an object, `metadata.triggered_by`. `null` and absence both mean "no
   parent".
2. None collected → the event is a **root**.
3. Any collected value that is not a ledger integer → error
   `triggered_by is not an integer` (a string `"3"`, `true`, `1.5`, `[1]`, an
   integer beyond 2^53−1 — previously these were silently skipped, which made a
   malformed link indistinguishable from a root).
4. Both collected and different → error
   `conflicting triggered_by (top-level A vs metadata B)`.
5. Otherwise the parent is that integer.

Then, as in §5, the parent MUST be the `seq_id` of an event in the journal and
MUST be strictly less than the event's own `seq_id`.

### 9.4 Normative DAG errors

`verify_dag` reports, in journal order (Rust, Python, JS, and the oracle emit
these byte-for-byte; `S` is the event's integer `seq_id` or `?`):

```
duplicate seq_id present                                        (once, first)
seq S: triggered_by is not an integer
seq S: conflicting triggered_by (top-level A vs metadata B)
seq S: triggered_by T does not exist
seq S: triggered_by T is not strictly earlier
seq ?: triggered_by T cannot be ordered (seq_id is not an integer)
```

### 9.5 Write side

Producers MUST validate the causal parent before appending: a ledger integer
naming an existing, strictly-earlier `seq_id`, or null. `LedgerWriter` raises
`CausalityError`; korg-bridge raises `CausalityError`; `POST
/api/agent/tool-call` answers **422** and appends nothing.

## 10. Redaction at persistence (revision 1.1)

A chained event cannot be edited after it is hashed — that is exactly what the
chain detects — so secrets MUST be removed **before** canonicalization, hashing,
signing, and the first byte written to disk. The normative policy
**`korg-redact@v1`** (sensitive-key and value-pattern rules, marker format,
idempotence, fail-closed gate, and the verifier lint) is specified in
[`REDACTION.md`](./REDACTION.md) and pinned by
`vectors/redaction/korg-redact-v1.json`. A record produced under the policy
declares it: `metadata.tags["korg.redaction"] = "korg-redact@v1"` (nested) or
top-level `"redaction": "korg-redact@v1"` (flat). Redaction never changes the
hash rules — a redacted journal is an ordinary v1 chain over redacted bytes.

## 11. Import envelopes (revision 1.1)

Evidence korg did not observe itself (Okto proxy diagnostics, ningen-shikkaku
lifecycle events) enters a ledger only through a **`korg-import@v1`** envelope
with a registered profile — `okto.diagnostics@v1` or
`ningen-shikkaku.lifecycle@v1`. Profiles are strict allowlists with no free-text
fields; secret-bearing source fields are named and rejected; validation is
atomic. Imported events carry `source_agent = "import:<source>@<version>"`, hang
off one `korg.import` batch event that records the envelope's SHA-256, and pass
through redaction like any other event. Normative text:
[`IMPORT.md`](./IMPORT.md); examples: [`imports/`](./imports/).

## 12. Revision history

| Revision | Change | Hash-compatible | Verdict-compatible |
|---|---|---|---|
| 1.0 | chain, HMAC, DAG over top-level `triggered_by` | — | — |
| 1.0 + §8 | `event_sig`, `anchors.jsonl` (additive, outside the preimage) | yes | yes |
| **1.1** | §9 causal parent read from both shapes + integer domain; §10 redaction at persistence; §11 import envelopes; one canonical vector set | **yes** — every v1.0 tip reproduces | **stricter**: a nested journal with a dangling/forward/malformed `metadata.triggered_by` that 1.0 verifiers silently passed now fails. See [`MIGRATION.md`](./MIGRATION.md). |
