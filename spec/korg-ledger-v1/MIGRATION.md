# Migrating to korg-ledger@v1 revision 1.1

Revision 1.1 makes `spec/korg-ledger-v1/` the single authoritative contract for
korg as an agent flight recorder. **No hash rule changed**: every v1.0 vector
reproduces its frozen tip, and every existing journal hashes exactly as before.
What changed is what verifiers check, what producers write, and where the
vectors live.

## At a glance

| You are… | What changes | Action |
|---|---|---|
| verifying journals | causal links are now read from nested `metadata.triggered_by` too, and malformed links are errors | re-verify; see [§1](#1-verifiers-nested-causal-links-are-now-checked) |
| writing with `korg_ledger.LedgerWriter` | redaction on by default; `metadata.tags` gains `korg.redaction`; non-integer `triggered_by` raises | usually none; see [§2](#2-python-producer-ledgerwriter) |
| posting to `POST /api/agent/tool-call` | `args`/`result` redacted; bad `triggered_by` → **422** | handle 422; see [§3](#3-http--mcp-producers-korg-server-korg-bridge) |
| running `korg-verify` | new `--require-redacted`; JSON verdict gains `redaction_ok` | optional |
| reading the vectors | one copy in `spec/korg-ledger-v1/vectors/`; manifest gains `dag`, `dag_errors`, `redaction*`, `revision` | repoint paths; see [§4](#4-conformance-vectors) |
| importing Okto / ningen-shikkaku data | new `korg-import@v1` envelopes | see [`IMPORT.md`](./IMPORT.md) |

## 1. Verifiers: nested causal links are now checked

Before 1.1, the Rust, Python, and JS `verify_dag` read `triggered_by` only at the
top level. Every production writer stores it under `metadata.triggered_by`, so
for real journals the DAG check was effectively uniqueness-only: a forward or
dangling link passed. They also disagreed on odd values (`true` counted as 1 in
Python only; `1.5` was checked in JS only).

From 1.1 (SPEC §9) all three resolve the parent from either shape, require a
ledger integer, reject top-level/metadata conflicts, and emit identical error
strings. **A journal that verified before can now fail** — only if it carries a
malformed or impossible link, which 1.0 already forbade in prose.

Check an existing journal:

```sh
korg-verify path/to/ledger.jsonl                 # Rust
node spec/korg-ledger-v1/js/verify.mjs path/to/ledger.jsonl
PYTHONPATH=adapters/korg-ledger-py/src python3 -m korg_ledger lint path/to/ledger.jsonl
```

If it now reports `seq N: triggered_by T does not exist` / `is not strictly
earlier`, the producer recorded a bad link (the HTTP endpoint did not validate
links before 1.1). The chain is still intact — the record is authentic, the link
is wrong. Do not edit it (that breaks the chain); keep it as evidence, and fix
the producer. Error-string consumers: Rust previously printed `seq Some(3)` /
`seq None`; all implementations now print `seq 3` / `seq ?`.

## 2. Python producer (`LedgerWriter`)

- **Redaction at persistence is on by default** (`redaction="korg-redact@v1"`).
  Each record is redacted before it is hashed and written, and
  `metadata.tags["korg.redaction"] = "korg-redact@v1"` is added. Payloads with
  credential-shaped values are stored with `[REDACTED:<rule>]` markers; nothing
  else changes. Pass `redaction=None` only for byte-exact fixture replay — those
  records carry no tag and fail `--require-redacted` if they hold a secret.
- `triggered_by` must be an `int` (not `bool`, `str`, or `float`); otherwise
  `CausalityError` (a `ValueError`) — the Claude Code hook already drops such an
  event instead of aborting.
- New: `korg_ledger.redact/scan/verify_redaction`, `causal_parent`, `json_int`,
  `korg_ledger.imports`, and the `python -m korg_ledger` CLI.

## 3. HTTP / MCP producers (korg-server, korg-bridge)

- `POST /api/agent/tool-call` redacts `args` and `result` before the journal
  sees them and tags the event, exactly like `LedgerWriter`.
- A `triggered_by` that does not name an existing, strictly-earlier `seq_id` is
  now refused with **422 Unprocessable Entity** and nothing is appended (it used
  to be persisted silently, producing a journal that 1.1 verifiers reject).
  `mcp_server.py` already surfaces non-2xx responses as tool errors. The
  korg-bridge (`append*`) already raised `CausalityError`; it now also redacts.
- `mcp_server.py`'s actor check accepts the new `import:` prefix.

## 4. Conformance vectors

- The duplicate copy under `crates/korg-registry/tests/conformance/` (an older
  SPEC.md, a manifest with an extra vector) is **removed**. Rust tests read
  `spec/korg-ledger-v1/` directly; `nonbmp-intact.jsonl` moved there and is now
  checked by Python and JS too.
- New vectors: `nested-intact`, `nested-dag-broken`, `dag-malformed`,
  `mixed-shape-intact`, `redacted-intact`, `unredacted-intact`, plus the shared
  redaction cases in `vectors/redaction/korg-redact-v1.json`.
- Manifest additions (old harnesses ignore them): `revision`, per-vector `dag`
  / `dag_errors`, `redaction` / `redaction_errors`, top-level `causal_parent`,
  `redaction_cases`. Existing fields are unchanged.
- `tools/generate_vectors.py` regenerates every vector deterministically and
  asserts the v1.0 tips; CI runs it in `--check` mode, plus a manifest ↔ disk
  completeness check in every harness.

## Redacting an existing journal

A journal that already contains a secret cannot be fixed in place (REDACTION.md
R6). Make a redacted **copy as a new chain**, keep both tips:

```sh
PYTHONPATH=adapters/korg-ledger-py/src \
  python3 -m korg_ledger rechain-redacted old.jsonl redacted.jsonl [--key K]
#  wrote N redacted events to redacted.jsonl
#    old tip <hex>
#    new tip <hex>
```

The copy preserves `seq_id`s and causal links, declares the policy on every
record, drops `event_sig` (it signed the unredacted bytes — re-sign if needed),
and verifies clean under `korg-verify --require-redacted`. Then rotate the
leaked credential: redaction limits future exposure, it does not undo past
exposure. Anchor or publish the old tip next to the new one so the copy's
provenance stays auditable; delete the original only under your retention
policy.
