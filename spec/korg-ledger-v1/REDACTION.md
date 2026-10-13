# korg-redact@v1 — Redaction at Persistence

**Status:** NORMATIVE (korg-ledger@v1 revision 1.1, [SPEC.md §10](./SPEC.md#10-redaction-at-persistence-revision-11)) ·
**Cases:** [`vectors/redaction/korg-redact-v1.json`](./vectors/redaction/korg-redact-v1.json) ·
**Implementations:** Python [`korg_ledger.redaction`](../../adapters/korg-ledger-py/src/korg_ledger/redaction.py), Rust [`korg_ledger::redact`](../../crates/korg-ledger/src/redact.rs) (feature `redact`)

The key words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119.

## 1. Why "at persistence"

A korg ledger is a hash chain. Removing a secret from an event after it is hashed
changes the event's `entry_hash`, which every verifier reports as tampering — and
re-hashing the tail to hide that is exactly the forgery the chain exists to
catch. So there is one safe place to redact: **before the event exists**.

## 2. Producer obligations

**R1 — Order.** A producer MUST apply the policy to the record body, then
canonicalize, hash (§3), sign (§8.1), and write. No byte of an unredacted record
may reach disk, a log, a blob store, or a network sink owned by the ledger — no
write-then-rewrite.

**R2 — Scope.** The policy applies to the whole record except the structural
fields `prev_hash`, `entry_hash`, `event_sig` (hex digests by construction).
For a nested JournalEvent that includes `metadata` and `event`; for a flat
record, every payload field.

**R3 — Declaration.** A record produced under the policy MUST declare it:
`metadata.tags["korg.redaction"] = "korg-redact@v1"` (nested) or top-level
`"redaction": "korg-redact@v1"` (flat). The declaration is inside the preimage,
so it cannot be added or stripped later without breaking the chain. A record
without it makes no redaction claim.

**R4 — Fail closed.** After redaction the producer MUST re-scan the record; if
it is not clean (see §5) the producer MUST NOT persist it and MUST surface an
error (`RedactionError` in Python). Under a correct implementation this is
unreachable because the policy is idempotent; it is the gate that makes the
guarantee unconditional.

**R5 — No oracle.** A marker MUST NOT carry any function of the secret — no
hash, prefix, suffix, length, or entropy hint. A ledger is meant to be shared;
a hash of a low-entropy secret is a dictionary-attack oracle.

**R6 — No retroactive redaction in place.** A persisted chained event MUST NOT
be edited to remove a secret. Remediation is re-chaining a redacted copy as a new
journal (see [`MIGRATION.md`](./MIGRATION.md#redacting-an-existing-journal)).

**R7 — Opt-out is explicit and visible.** A producer MAY offer an explicit
opt-out (e.g. byte-exact fixture replay — `LedgerWriter(redaction=None)`); such
records carry no declaration (R3) and fail the lint if they hold a secret.

## 3. The policy

`redact(value) → (value', findings)` walks a JSON value. Object **keys are never
rewritten** (they are structure). Numbers, booleans, and null pass through.

### 3.1 Sensitive keys

Normalize an object key: insert `_` before an ASCII uppercase letter that follows
an ASCII lowercase letter or digit; ASCII-lowercase; map `-` and space to `_`
(`accessToken` → `access_token`, `X-Api-Key` → `x_api_key`, `APIKey` →
`apikey`). The key is **sensitive** if the normalized form is one of

```
authorization proxy_authorization cookie set_cookie api_key apikey password
passwd passphrase secret token private_key secret_key aws_secret_access_key
credential credentials
```

or ends with one of

```
_api_key _apikey _secret _secret_key _private_key _password _passwd _passphrase _token
```

(`prompt_tokens`, `max_tokens`, `token_count` are **not** sensitive.) The value
of a sensitive key is replaced — whole, whatever its type — by
`[REDACTED:sensitive_key]`, **except** `null`, booleans, `""`, and a string that
is already exactly a marker; those carry no secret and are kept.

### 3.2 Value patterns

Every other string is rewritten by these rules, **in this order**, each applied
to the output of the previous one. `B` is `(^|[^A-Za-z0-9_])` — an ASCII
boundary that is captured and re-emitted (no look-behind, no `\b`); in the
table `B sk-…` means `B` immediately followed by `sk-…` (the space is only for
readability).

| Rule id | Pattern (regex) | Replacement |
|---|---|---|
| `private_key_block` | `-----BEGIN[A-Z ]*PRIVATE KEY-----(?:[\s\S]*?-----END[A-Z ]*PRIVATE KEY-----\|[\s\S]*)` | marker |
| `url_credentials` | `([A-Za-z][A-Za-z0-9+.\-]*://)[^ \t\r\n/:@\[\]]+:[^ \t\r\n/@\[\]]+@` | `\1` marker `@` |
| `bearer_token` | `((?:Bearer\|bearer\|BEARER)[ \t]+)[A-Za-z0-9._~+/\-]{16,}=*` | `\1` marker |
| `provider_api_key` | `B sk-[A-Za-z0-9_\-]{20,}` | `\1` marker |
| `github_token` | `B (?:gh[pousr]_[A-Za-z0-9]{36,}\|github_pat_[A-Za-z0-9_]{22,})` | `\1` marker |
| `slack_token` | `B xox[abposr]-[A-Za-z0-9\-]{10,}` | `\1` marker |
| `aws_access_key_id` | `B (?:AKIA\|ASIA)[0-9A-Z]{16}` | `\1` marker |
| `google_api_key` | `B AIza[0-9A-Za-z_\-]{35}` | `\1` marker |
| `jwt` | `B eyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}` | `\1` marker |
| `env_assignment` | `B ([A-Z][A-Z0-9_]*(?:API_KEY\|APIKEY\|SECRET\|SECRET_KEY\|PASSWORD\|PASSWD\|TOKEN\|PRIVATE_KEY\|ACCESS_KEY))([ \t]*[=:][ \t]*)(?:"[^"\n]*"\|'[^'\n]*'\|[^ \t\r\n"',;&]+)` | `\1\2\3` marker |
| `json_secret_field` | `("(?:password\|passwd\|passphrase\|secret\|client_secret\|api_key\|apikey\|access_token\|refresh_token\|auth_token\|id_token\|token\|private_key\|authorization)"[ \t]*:[ \t]*)"(?:[^"\\\n]\|\\.)*"` | `\1"` marker `"` |

The marker for rule `r` is the literal string `[REDACTED:r]`.

**Fixed point.** The rule list is applied repeatedly until a full pass changes
nothing (at most 8 passes). One pass is not enough: a replacement can expose a
new match — `AKIA…AKIA…` back to back, where the first marker's `]` becomes the
boundary the second key needed. Iterating to the fixed point makes the policy
idempotent by construction; a string still changing after 8 passes is left
unclean and the producer's R4 gate refuses it. `userinfo` in `url_credentials`
excludes `[` and `]` (RFC 3986 gen-delims), so a marker is never mistaken for
credentials.

**ASCII only, case-explicit.** The patterns deliberately avoid `\s`, `\w`, `\b`,
and case-insensitive flags: Python `re` and the Rust `regex` crate (and JS)
disagree on their Unicode semantics, and a disagreement would let one
implementation persist what another redacts. `ésk-…` is redacted (é is not an
ASCII word character) in every implementation; the shared cases pin this.

### 3.3 Findings

Each rule application that changed a string, and each sensitive-key
replacement, yields a finding `{path, rule}` where `path` is an RFC 6901 JSON
pointer (`""` for the root). Reported lists are sorted by `path` (code point)
then by the rule order above, with `sensitive_key` first — implementations
iterate object members in different orders, the sorted list is canonical.

## 4. Properties (MUST hold; tested)

- **Deterministic** — same input, same output, in every implementation (the
  shared cases).
- **Idempotent** — `redact(redact(x)) == (redact(x), [])`; property-tested over
  spliced fragments of every rule.
- **Non-expanding claims** — markers never match a rule in a way that changes
  them, so an already-redacted ledger re-scans clean.

## 5. The lint (verifiers)

A record is **redaction-clean** iff `redact(body) == body`, where `body` is the
record minus `prev_hash`/`entry_hash`/`event_sig`. Journal lint errors, one per
finding, in journal order:

```
seq S: unredacted <rule> at <json-pointer>
```

Run it with `korg-verify <journal> --require-redacted` (Rust; any failure makes
the verdict INVALID and sets `redaction_ok: false` in `--json`) or
`python -m korg_ledger lint <journal>`. The lint is a **floor, not a ceiling**:
a clean result means no *known* credential shape survived, not that the ledger
holds no sensitive data. Producers of structured sources SHOULD allowlist fields
instead (as the [import profiles](./IMPORT.md) do).

## 6. Where it runs today

| Persistence path | Redacts before hashing | Declares policy |
|---|---|---|
| `korg_ledger.LedgerWriter` (Python; Claude Code hook, `migrate`, imports) | yes (default) | `metadata.tags` |
| `POST /api/agent/tool-call` (korg-server; MCP agents) | yes — `args` and `result` | `metadata.tags` |
| korg-bridge `append*` (PyO3) | yes — `args` and `result` | `metadata.tags` |
| korg internal events (`CapabilityJournal::append`) | no — korg's own governance events carry no agent payload | — |
| `korg-runtime` run-once ledger (flat) | no — fixed internal pipeline fields | — |

Known gaps (not covered by this policy): content-addressed blobs written by the
stream-json adapter to `.korg/blobs/` before the POST; object keys that are
themselves secrets; secrets with no recognizable shape under a non-sensitive key.
