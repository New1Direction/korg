# korg-import@v1 — Import Envelopes

**Status:** NORMATIVE (korg-ledger@v1 revision 1.1, [SPEC.md §11](./SPEC.md#11-import-envelopes-revision-11)) ·
**Implementation:** [`korg_ledger.imports`](../../adapters/korg-ledger-py/src/korg_ledger/imports.py) · **Examples:** [`imports/`](./imports/)

A flight recorder sometimes needs evidence it did not observe: what the proxy in
front of the agent saw, or what the secret-holding sidecar did. That evidence
comes from tools with their own formats and their own secrets. An **import
envelope** is the only door: a strict, versioned, allowlisted container that
carries *metadata about* those systems and never their secret values.

## 1. Envelope

```json
{
  "schema": "korg-import@v1",
  "profile": "okto.diagnostics@v1",
  "source": {"name": "okto", "version": "0.1.0"},
  "exported_at_ms": 1781395260000,
  "dropped": 0,
  "records": [ {"kind": "session", "...": "..."} ]
}
```

| Field | Required | Rule |
|---|---|---|
| `schema` | yes | exactly `"korg-import@v1"` |
| `profile` | yes | a registered profile id (§3) |
| `source` | yes | exactly `{name, version}`; `name` MUST equal the profile's source; `version` matches `[0-9A-Za-z][0-9A-Za-z.+-]{0,31}` |
| `exported_at_ms` | no | non-negative ledger integer |
| `dropped` | no | non-negative integer: source items the exporter could not project and left out |
| `records` | yes | 1–10 000 objects, each with a `kind` registered by the profile |

No other envelope field is allowed.

## 2. Validation (importer obligations)

An importer MUST reject the **whole envelope**, appending nothing, if any of
these holds (all violations are reported, values are never echoed in errors):

1. an unknown envelope field, schema, profile, or source mismatch;
2. a record whose `kind` the profile does not register;
3. a record field the kind does not allow — **allowlist, not denylist**. Fields
   the profile names as *forbidden* get a specific error ("may carry secrets or
   raw traffic"), but any unlisted field is rejected the same way;
4. a missing required field, or a value of the wrong type. Types are: ledger
   integer (non-negative unless a range is given), boolean, enum, **id**
   (`[A-Za-z0-9][A-Za-z0-9._:-]{0,127}`), **slug** (`[a-z0-9][a-z0-9_.-]{0,63}`),
   and a slug→integer map (≤16 entries). **There is no free-text type**;
5. any record that is not redaction-clean under
   [korg-redact@v1](./REDACTION.md) (a second gate behind the allowlist).

Floats are never allowed (ledger numbers are integers): a fractional source
value is converted by the exporter (`duration_ms` rounded, Okto `confidence`
0–1 → `confidence_bp` 0–10000).

## 3. Mapping into the ledger

A valid envelope becomes `1 + len(records)` events, appended in order:

- **batch event** — `tool_name: "korg.import"`, `args: {profile, record_count,
  envelope_sha256, [exported_at_ms], [dropped]}`, `triggered_by` = the caller's
  parent (or root). `envelope_sha256` is SHA-256 over the canonical (§2)
  envelope bytes, so anyone holding the sanitized envelope can prove it is
  exactly what was imported.
- **one event per record** — `tool_name: "<source>.<kind>"` (e.g.
  `okto.anomaly`, `ningen-shikkaku.trigger`), `args` = the record minus `kind`,
  `result: {import_index: i}`, `triggered_by` = the batch event's `seq_id`.

Every imported event has `source_agent = "import:<source>@<version>"` — a fifth
actor prefix alongside `agent:`, `human:`, `korg:`, `mcp:` — and recorder
`actor_id = "korg:import"`. Imported events are **attested by the importer, not
observed by korg**: the chain proves they were not altered after import, not
that the source reported truthfully.

## 4. Profile `okto.diagnostics@v1`

Source: [Okto](https://github.com/New1Direction/okto), the self-healing local
proxy for agent traffic (`source.name = "okto"`). Okto's own captures hold
request/response headers (including `authorization`, `x-api-key`, `cookie`),
full prompt bodies, raw stream bytes, and anomaly descriptions quoting payload
bytes — none of which may cross.

| kind | required | optional |
|---|---|---|
| `session` | `session_id` id, `client` enum¹ | `at_ms`, `tls_established` bool, `synthetic` bool, `provider_vendor` slug, `provider_product` slug, `turn_count`, `anomaly_count`, `mutation_count` |
| `turn` | `session_id`, `turn_index` | `at_ms`, `status_code` 100–599, `duration_ms`, `chunk_count`, `anomaly_count`, `mutations_applied` (count), `token_usage` slug→int map |
| `anomaly` | `session_id`, `turn_index`, `chunk_index`, `anomaly_type` enum² | `at_ms`, `repaired` bool |
| `mutation` | `mutation_id` id, `scope` enum³, `patch_type` enum⁴ | `at_ms`, `session_id`, `client`, `confidence_bp` 0–10000, `reinforcement_count` |
| `replay` | `session_id`, `resolved` bool | `at_ms`, `total_anomalies`, `total_mutations_applied` |

¹ `claude gemini codex agy kimi kimi-code cursor opencode pi hermes unknown` ·
² `bare_json_without_data_prefix malformed_json_in_sse_frame empty_response_body line_ending_mismatch protocol_anomaly other` ·
³ `request_header request_body response_header response_body stream_chunk` ·
⁴ `literal regex_sub json_patch header_set`

**Forbidden (named):** `request_headers response_headers headers request_body
response_body body raw_hex expected_hex patched_hex pattern_hex patch_hex
description parsed chunks stream_chunks path url connect_target hostname
node_id`. `connect_target`/`hostname`/`path` are excluded because they can name
internal infrastructure or carry query-string tokens.

**Exporters** (`korg_ledger.imports`, projection only — they never read a
forbidden field): `okto_session_to_envelope(Session)` for
`~/.okto/sessions/<id>.jsonl`, `okto_mutation_record(Mutation)` for
`~/.okto/genome.jsonl`. CLI: `python -m korg_ledger envelope okto-session FILE
--source-version 0.1.0`.

## 5. Profile `ningen-shikkaku.lifecycle@v1`

Source: [ningen-shikkaku](https://github.com/New1Direction/ningen-shikkaku) —
the `dazai` session-bound dead-man's switch and `motokano` one-shot MCP secret
server (`source.name = "ningen-shikkaku"`). Its secrets live in locked memory and
are returned by `motokano` tool calls; its tool specs (`--tool …value=…`) and
error lines can echo them. Only lifecycle **metadata** may cross.

| kind | required | optional |
|---|---|---|
| `daemon_start` | `mode` (`armed` \| `dry_run`) | `at_ms`, `pid`, `grace_ms`, `ping_timeout_ms`, `seccomp` bool |
| `buffers` | `allocated` | `at_ms`, `locked` |
| `status` | `alive` bool, `armed` bool | `at_ms`, `grace_seconds`, `registered_pids` |
| `armed` | `via` (`flag` \| `control` \| `mcp`) | `at_ms` |
| `trigger` | `reason`⁵, `mode` (`armed` \| `dry_run`) | `at_ms`, `severity` (`graceful` \| `hard`) |
| `cancelled` | `reason` (`client_reconnected` \| `client_cancel` \| `other`) | `at_ms` |
| `panic` | `reason`⁵ | `at_ms` |
| `wiped` | `buffers` | `at_ms` |
| `pid_registered` | `pid` | `at_ms`, `total` |
| `pid_unregistered` | `pid` | `at_ms` |
| `pid_killed` | `pid`, `outcome` (`sent` \| `already_gone`) | `at_ms` |
| `shutdown` | `how` (`quit` \| `signal`) | `at_ms` |
| `exit_condition_fired` | `counter_mode` (`calls` \| `session` \| `either`) | `at_ms`, `calls` |

⁵ `heartbeat_dropped ping_deadline_missed sigusr1 sigusr2 grace_expired other`
(mapped from the daemon's literal reason strings, e.g. `"heartbeat connection
dropped"` → `heartbeat_dropped`).

**Forbidden (named):** `value secret secret_value plaintext data bytes cmd
command spec tool_spec stdout stderr output argv args env message reply
buffer_address address`.

**Exporter:** `dazai_log_to_envelope(lines)` recognises only the fixed `[dazai]`
lifecycle lines (Rust and Python-reference formats), extracts their numeric and
enum parts, and **drops every other line** — including `error: …` and
`motokano: parsing --tool …` lines that can echo a secret — counting them in
`dropped`. CLI: `python -m korg_ledger envelope dazai-log FILE --source-version
0.1.0`.

## 6. Profile `ningen-shikkaku.lifecycle@v2`

Source: the same project, but its **structured** lifecycle sink instead of its
log. ningen-shikkaku's `dazai daemon` and `motokano` accept
`--korg-sink PATH` (or `/dev/fd/N`) and append one
[`ningen-korg-lifecycle@1`](https://github.com/New1Direction/ningen-shikkaku/blob/main/docs/src/reference/korg.md)
JSON line per lifecycle step. The producer builds those lines from closed enums
and counters only. v2 imports them without scraping log text, covers `motokano`
reads, and drops what v1 still carries: PIDs. Prefer v2. v1 remains for
daemons without the flag and for the Python reference.

Every record carries `component` (`dazai` \| `motokano`) and `session` (id: the
producer's random per-run session id, 32 lowercase hex). It may also carry
`at_ms` (the producer's wall clock; korg's HLC still orders the ledger).

| kind | required (besides `component`, `session`) |
|---|---|
| `session_start` | `armed` bool |
| `arm` | `via` (`startup` \| `control`) |
| `read` | `tool` (0-based index into motokano's `--tool` list, ≤ 2³²−1), `tool_kind` (`static` \| `exec`), `served` bool |
| `wipe` | `reason`⁶, `buffers` (a count, ≤ 2³²−1) |
| `kill` | `reason`⁶, `target` (`registered` \| `self`), `count` (≤ 2³²−1), `executed` bool (`false` = dry-run "would kill") |
| `session_end` | `reason`⁶ |

⁶ `heartbeat_lost ping_timeout panic_signal hard_panic_signal shutdown
client_quit calls_exhausted client_disconnect daemon_lost`

**Forbidden (named):** v1's list plus `pid pids name tool_name path socket
length len size hash sha256 digest`. A tool name can describe the secret it
serves, and a length or digest of a secret is a guessing oracle.

**Exporter:** `ningen_sink_to_envelope(lines)` reads each line only through a
fixed field map (`args.component`, `args.session`, `args.observed_unix_ms` →
`at_ms`, plus the step's own enum and count fields and `result.served` or
`result.executed`). Any other field on the line is never copied. Lines that are
not JSON objects with `args.contract = "ningen-korg-lifecycle@1"` and a known
`tool_name` are dropped and counted in `dropped`, for example a line torn by a
crash. Values are not coerced: a value outside the vocabulary rejects the whole
envelope. `source.version` comes from the records' `args.producer`
(`ningen-shikkaku@<version>`). CLI: `python -m korg_ledger envelope ningen-sink
lifecycle.jsonl`. Envelope a sink once its processes have exited, and give each
run its own file (or rotate files), so a later import doesn't repeat records.
[`imports/ningen-shikkaku-sink.example.jsonl`](./imports/ningen-shikkaku-sink.example.jsonl)
is real 0.1.0 output: an armed dazai hard panic, then a motokano run with a
refused re-read. It projects exactly to
[`imports/ningen-shikkaku-lifecycle-v2.example.json`](./imports/ningen-shikkaku-lifecycle-v2.example.json).

## 7. Adding a profile

A new profile is a spec change: add its table here, its allowlist to
`korg_ledger.imports.PROFILES`, a valid and a rejected example under
[`imports/`](./imports/), and tests. Profiles are versioned independently
(`<source>.<name>@vN`); widening an existing profile's allowlist requires a new
version.
