# goldseal@v1 — Public, Independently-Verifiable Certificate

> A **Gold Seal** is a single, self-contained JSON object that attests an AI-agent
> session happened as claimed — and lets *anyone* re-verify it offline, with zero
> trust in the tool that produced it. It is a strict superset of `korgex-receipt@v1`
> built on the [`korg-ledger@v1`](./SPEC.md) chain primitives.

A receipt proves the event chain is intact. A Gold Seal adds the part a **human**
actually reads — *what the agent did* — and binds it cryptographically so it
**cannot lie**. The summary is not asserted; it is a pure function of the events,
re-derived by the verifier and rejected on any mismatch.

This document is normative. An implementation conforms iff it reproduces the
frozen fixture `crates/korg-verify/tests/fixtures/goldseal-v1.json` (minted by the
Python reference, verified byte-identically by the Rust `korg-verify` and the JS
`verify.mjs`).

---

## 1. Threat model — what a Gold Seal does and does not prove

A green verdict proves, with **no trust in the issuer's tooling**:

1. **Integrity** — the embedded events hash-chain intact and form a well-formed
   causal DAG (SPEC §4–5): nothing was inserted, deleted, reordered, or edited.
2. **Completeness of the head** — the recorded `tip` is the chain head and
   `event_count` matches the embedded events.
3. **Truthful summary** — the human-legible `summary` is re-derivable from the
   events byte-for-byte (§4). It is impossible to overstate or hide what happened
   (a dropped file, a hidden tool call, a miscounted step) without detection.
4. **Authorship** — the `seal` is an Ed25519 signature, by the issuer's key, over
   the canonical header (§3) — so the claim, issuer, tip, count, and summary all
   move together or not at all.

It explicitly does **NOT** prove, on its own:

- **When** it happened. Ed25519 carries no time. Attach an external anchor
  (SPEC §8.2) and resolve it out-of-band for a "not-after" bound.
- **That the issuer key maps to a real-world identity.** The relying party pins a
  key it already trusts (`--pubkey`). An unpinned seal only proves *some* holder
  of *some* key signed it.
- **That the `claim` string is true.** `claim` is free-form prose the issuer
  asserts; it is signature-protected (cannot be altered by a third party) but is
  not machine-checkable against the events. Trust it exactly as much as you trust
  the pinned issuer. The machine-checked facts live in `summary`.
- **Semantic correctness of the work.** A Gold Seal proves the session is a
  faithful record — not that the code is good.

---

## 2. Envelope

A Gold Seal is one JSON object:

| Field         | Type           | Bound by | Meaning |
|---------------|----------------|----------|---------|
| `schema`      | `"goldseal@v1"`| seal     | Format identifier. |
| `spec`        | `"korg-ledger@v1"` | seal | Underlying chain spec. |
| `claim`       | string         | seal     | Issuer-asserted one-line description (prose, not machine-checked). |
| `issued_at`   | integer        | seal     | Unix seconds the seal was minted (integer — floats are out of canonicalization scope, SPEC §2). |
| `issuer`      | object         | seal     | `{ "agent": "<identity label>" }`. A label, not a trust anchor (see §1). |
| `event_count` | integer        | seal     | MUST equal `len(events)`. |
| `tip`         | hex string     | seal     | MUST equal the last event's `entry_hash`. |
| `summary`     | object         | seal     | The re-derivable attestation (§4). |
| `events`      | array          | `tip`/`summary` | The full korg-ledger@v1 chain (flat or nested event shape). |
| `anchors`     | array (opt.)   | seal + structural | Anchor records (SPEC §8.2). Bound to the chain structurally **and** signed by the seal (§3) — cannot be stripped, added, or altered. |
| `seal`        | object         | —        | `{ "alg": "ed25519", "pubkey": <hex>, "sig": <hex> }`. |

---

## 3. The header and the seal signature

The **header** is the envelope minus `events` and `seal` (so it *includes*
`anchors` when present — `events` is excluded only because it is large and already
bound via `tip` + the verified chain):

```
header = { schema, spec, claim, issued_at, issuer, event_count, tip, summary[, anchors] }
```

The seal signature is Ed25519 over `canonicalize(header)` (SPEC §2 canonical bytes:
sorted keys, compact, `\uXXXX`-escaped ASCII), encoded as lowercase hex — the exact
primitive and message-shaping as the per-event `event_sig` (SPEC §8.1), only the
preimage is the header object rather than an event.

```
seal.sig = hex( Ed25519_sign( seed, canonicalize(header) ) )
```

Ed25519 is deterministic (RFC 8032), so all three reference implementations sign a
given header to byte-identical hex. The header is reconstructed identically at mint
and verify time (it is a pure key-subset of the envelope), so the signature is
reproducible.

---

## 4. Summary derivation (the anti-spoofing core)

`summary` is a **pure, deterministic function of `events`**. The verifier
re-derives it and compares `canonicalize(derived) == canonicalize(claimed)`. This
is what makes the human-readable part unforgeable.

For each event, take its `(source_agent, tool_name, args)` from the top level, or
from a nested `event` object if present (capture ledgers nest; receipts are flat —
both derive identically). Then:

```
summary = {
  "agents":     sorted, unique source_agent strings,
  "by_tool":    { tool_name: count } over every event with a string tool_name,
  "files":      sorted, unique string values of args.file_path / args.path,
  "seq_first":  min seq_id (integer, 0 if none),
  "seq_last":   max seq_id (integer, 0 if none),
}
```

Every value is an integer, string, array, or object of those — never a float — so
the derived object canonicalizes byte-identically across languages. `files`
captures path-bearing arguments by the fixed key set `{file_path, path}`; this is a
*defined* rule (exact re-derivation), not a best-effort heuristic.

> **Why this matters.** The legacy receipt signature covers only the `tip` hash —
> the summary a human reads is unprotected. A Gold Seal closes that gap twice over:
> the summary is re-derived from events (cannot disagree with them) *and* the seal
> signs it (cannot be swapped for the events independently).

---

## 5. Verification algorithm

A conforming verifier, given an envelope and an optional pinned issuer key, runs in
order and the verdict is **valid** iff every applicable check passes:

1. `schema == "goldseal@v1"`.
2. `verify_chain(events)` is empty — the hash chain is intact (SPEC §5).
3. `verify_dag(events)` is empty — unique `seq_id`s and strictly-earlier
   `triggered_by` links.
4. `tip == events[-1].entry_hash`.
5. `event_count == len(events)`.
6. `canonicalize(derive_summary(events)) == canonicalize(summary)` (§4).
7. **Seal present** — verify `seal.sig` is a valid Ed25519 signature by
   `seal.pubkey` over `canonicalize(header)` (§3). An **absent seal fails**: a
   `goldseal@v1` without a seal is a *downgrade*, not a merely-unsigned artifact.
8. If a key is pinned, `seal.pubkey` MUST equal it.
9. If `anchors` is present and non-empty, each anchor's `entry_hash` MUST match the
   chain event at its `seq_id` (structural; SPEC §8.2). The anchor set is also
   covered by the seal signature (step 7, §3), so it cannot be stripped or altered
   without failing the seal.

### Graceful degradation

A Gold Seal carries an `events` array, so an **older, receipt-only verifier** still
checks it — chain + DAG + tip — and reports it as a valid but *unsigned* receipt (it
does not know about `seal`/`summary`). That is honest: an old tool proves integrity;
a goldseal-aware tool additionally proves the summary and authorship.

---

## 6. Limits & non-goals (v1)

- **Time is proven by an explicit network step, not offline.** The anchor *set* is
  bound into the seal (§3) and structurally to the chain (§5.9), so it cannot be
  stripped, added, or forged. Offline, a green seal proves *which* commit is claimed
  as the witness. Proving *when* — that the named public commit actually introduced
  the `entry_hash` to the immutable git history — is a deliberate network step (SPEC
  §8.2, "external verification"), kept out of the hermetic verifier. The reference
  resolver is `korg-seal resolve` (§8): it fetches the commit and confirms the
  witness, yielding a "the chain existed no later than `<commit date>`" bound. The
  post-hoc flow is mint → publish/commit the seal → `korg-seal anchor` (re-signs the
  seal with the publishing commit bound) → anyone `korg-seal resolve`s it.
- **No revocation.** There is no built-in mechanism to revoke a minted seal; relying
  parties manage issuer-key trust and rotation out-of-band.
- **Identity is pinned, not proven.** See §1. v1 deliberately does not specify a PKI;
  it specifies a *verifiable artifact* and leaves key↔identity binding to the relying
  party (a known key, an `.well-known` publication, a keybase-style proof — all out of
  scope here).
- **Sorting is by code point.** `agents`/`files` are sorted as the reference impls
  sort (Python code-point, Rust byte-wise UTF-8, JS UTF-16). These agree for the
  BMP/ASCII strings korg emits; astral-plane content in those fields is out of scope.

---

## 7. Conformance

The frozen fixture `crates/korg-verify/tests/fixtures/goldseal-v1.json` is minted by
`spec/korg-ledger-v1/tools/mint_goldseal_fixture.py` (Python, seed `[42; 32]`) and is
the cross-implementation oracle:

- **Python** mints it and `korg_ledger.signing.verify_seal` round-trips it.
- **Rust** `korg-verify` verifies it (`crates/korg-verify/tests/goldseal.rs`) and the
  `korg-verify` binary renders its summary.
- **JS** `verify.mjs` verifies it and re-derives the identical summary
  (`spec/korg-ledger-v1/js/conformance.mjs`).

Each suite also pins the security properties: a lying summary, a moved claim, a
tampered event, a stripped seal, and a wrong pinned key all fail.

---

## 8. Reference minter & resolver

Verification is intentionally separate from minting — a relying party should never
need the producer's tooling. The reference **producer** is `korg-seal`
(`adapters/korg-seal/`):

- `korg-seal mint <session.jsonl> --claim "..."` derives the summary, builds the
  envelope, and signs the header with a local issuer key (`~/.korg/issuer.ed25519`).
  It refuses to seal a chain that does not verify. A seal it mints is verified —
  unchanged — by the Rust, JS, and browser verifiers above.
- `korg-seal anchor <seal> --repo <url> --commit <sha>` binds a `git-tip` time
  anchor and re-signs (the post-hoc anchoring flow of §6).
- `korg-seal resolve <seal>` performs the network step: it fetches each git-tip
  anchor's commit and confirms it introduced the anchored `entry_hash`, reporting a
  "the chain existed no later than `<commit date>`" bound. This is the only step
  that touches the network; the *what* and *who* stay provable offline.
