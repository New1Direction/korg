# Korg Agent Event Spec

**Schema version:** v1.0 (frozen)
**Last updated:** 2026-05-25

## Status

Sections §1, §1.1, §2, §2a, §3, §4, §5, §6, §7.2, §7.3, §7.5, §7.6, §7.7 are
**frozen** at schema v1.0. Changes require a schema version bump.

§1.2 is **PROPOSED**. It documents the append-shape vs journal-shape
distinction and classifies the 14 metadata fields. It will commit once
the Rust `EventMetadata` struct is confirmed stable and no field names
change in a near-term release.

§2b is **PROPOSED**. It will commit (or revise) once a real Claude Code
capture confirms the sub-agent causal model holds. See the §2b heading
for the discipline pact.

## Scope

This document specifies the on-wire and on-ledger contract for one
event type: `AgentToolCall`. Any agent that posts events to Korg —
korgex, the stream-json adapter, the codex-ws adapter, the grok-heavy
adapter, or a third party — MUST conform to this spec. Conformance is
verified by the dogfood checklist (§6).

The spec does not cover Korg's internal storage format, web UI, or
HTTP/MCP transport details. Those are implementation choices below this
contract.

## Canonicality rule

When this spec disagrees with what Korg emits on the wire, **the wire
is canonical** and the spec gets a PR. This applies to string-level
details (e.g., `schema_version` value), field placement (append shape
vs journal shape per §1.2), and any structural shape choice. If you
find a disagreement: file the spec PR, point reviewers at the Rust
source location, and don't change Korg's behavior to match this doc.

---

## §1 — AgentToolCall event structure

One event is appended to the ledger for each completed tool call at the
agent's decision boundary. Internal composition (a tool that calls
another tool inside one handler) is not ledgered — only calls the LLM
itself made (§2).

### Wire shape

```json
{
  "source_agent":  "agent:korgex@0.2.2",
  "tool_name":     "Edit",
  "args":          {"file_path": "...", "old_string": "...", "new_string": "..."},
  "result":        {"success": true},
  "success":       true,
  "duration_ms":   142,
  "triggered_by":  217,
  "payload_refs":  []
}
```

| Field | Type | Required | Notes |
|---|---|---|---|
| `source_agent` | string | yes | Actor identity (§1.1) |
| `tool_name` | string | yes | Verbatim — no normalization across agents |
| `args` | object | yes | Tool arguments; values >1KB content-referenced per §7.3 |
| `result` | object | yes | Tool output; values >1KB content-referenced per §7.3 |
| `success` | boolean | yes | Tool-level success/failure |
| `duration_ms` | integer | yes | Wall-clock duration of the tool call |
| `triggered_by` | integer\|null | optional | seq_id of parent event; omit (or null) for root events (§2) |
| `payload_refs` | array | optional | Content-ref descriptors (§7.3); default `[]` |

The server assigns `seq_id` on append and returns it in the response.
Clients use that seq_id as `triggered_by` on subsequent events.

### §1.2 — Append shape vs journal shape (PROPOSED)

There are two distinct representations of an AgentToolCall event.

#### §1.2.1 — Append shape (POST body)

The flat body sent by a client to `POST /api/agent_tool_call`:

| Field | Type | Required | Notes |
|---|---|---|---|
| `event_type` | string | yes | Always `"AgentToolCall"` |
| `tool_name` | string | yes | |
| `source_agent` | string | yes | Actor identity (§1.1) |
| `args` | object | yes | Tool input; large values content-referenced (§7.3) |
| `result` | object | yes | Tool output; large values content-referenced (§7.3) |
| `success` | boolean | yes | Tool-level success/failure |
| `duration_ms` | integer | yes | Wall-clock duration of the tool call |
| `triggered_by` | integer\|null | optional | seq_id of parent event; omit or null for root events |
| `payload_refs` | array | optional | Content-ref descriptors (§7.3); default `[]` |

#### §1.2.2 — Journal shape (envelope)

The envelope stored in the ledger and returned by `GET /api/journal`:

```json
{
  "schema_version": "1.0",
  "seq_id": 42,
  "metadata": { ... },
  "event": { ... }
}
```

Top-level fields: `schema_version` (string), `seq_id` (integer), `metadata` (object),
`event` (object).

#### §1.2.3 — Metadata fields

The `metadata` object carries 14 server-managed fields, classified below.

**Load-bearing** (used by causation walk, rewind, and integrity checks):

| Field | Type | Notes |
|---|---|---|
| `triggered_by` | integer\|null | seq_id of parent event (preserved from POST body) |
| `causation_id` | string\|null | UUID of parent event's `event_id` (§1.2.5) |
| `event_id` | string | UUID assigned by server on append |
| `actor_id` | string | Recorder identity (§1.2.6); always `"korg:api"` for HTTP appends |
| `seq_id` | integer | Monotonically increasing ledger position |
| `ledger_hash` | string | SHA-256 of the canonical ledger state at this seq_id |
| `parent_hash` | string\|null | `ledger_hash` of seq_id − 1; null for the first event |

**Informational** (available to clients, not used by ledger logic):

| Field | Type | Notes |
|---|---|---|
| `emitted_at` | HLC timestamp | Hybrid logical clock (physical ms + logical counter + actor_id) |
| `recorded_at` | string | ISO 8601 UTC, server wall-clock time of append |
| `session_id` | string\|null | Opaque session identifier; set by client if provided |

**Server-internal** (present on wire, not meaningful to clients):

| Field | Type | Notes |
|---|---|---|
| `hlc_physical` | integer | Physical component of the HLC (ms since epoch) |
| `hlc_logical` | integer | Logical counter component of the HLC |
| `rewind_of` | integer\|null | seq_id this event invalidates; null for normal events (§3) |
| `payload_refs` | array | Content-ref descriptors mirrored from POST body |

#### §1.2.4 — Event fields (AgentToolCall variant)

The `event` object inside the journal envelope:

| Field | Type | Notes |
|---|---|---|
| `event_type` | string | Always `"AgentToolCall"` |
| `tool_name` | string | |
| `source_agent` | string | Actor identity (§1.1) |
| `args` | object | Tool input |
| `result` | object | Tool output |
| `success` | boolean | |
| `duration_ms` | integer | |
| `timestamp` | string | ISO 8601 UTC; server-assigned, same as `metadata.recorded_at` |
| `payload_refs` | array | Mirrors `metadata.payload_refs` |

#### §1.2.5 — `causation_id` vs `triggered_by`

Both fields encode the same parent edge in the causal graph:

| Field | Addressing | Set by |
|---|---|---|
| `triggered_by` | seq_id (integer) | Client (POST body) |
| `causation_id` | UUID of parent's `event_id` | Server (derived on append) |

Use `triggered_by` for all client-side causal chain logic. Use `causation_id`
only when correlating across ledger replicas where seq_ids may differ.

#### §1.2.6 — `actor_id` vs `source_agent`

| Field | Meaning | Example |
|---|---|---|
| `source_agent` | The agent performing the tool call | `agent:korgex@0.2.2` |
| `actor_id` | The recorder that appended the event to Korg | `korg:api` |

For all events posted via `POST /api/agent_tool_call`, `actor_id` is always
`"korg:api"`. The recorder and the actor are always distinct.

#### §1.2.7 — Server-derived fields on append

When a client POSTs the append shape, the server adds:

- `seq_id` — next monotonic integer
- `event_id` — fresh UUID
- `actor_id` — `"korg:api"`
- `causation_id` — UUID of the event at `triggered_by`, or null
- `ledger_hash` — SHA-256 of canonical chain state
- `parent_hash` — `ledger_hash` of seq_id − 1
- `emitted_at` / `hlc_physical` / `hlc_logical` — HLC timestamp
- `recorded_at` — server wall-clock UTC
- `event.timestamp` — same as `recorded_at`

Clients MUST NOT send any of these fields in the POST body; the server
ignores them if present.

---

### §1.1 — Actor identity convention

`source_agent` follows one of four prefix forms:

| Prefix | Meaning | Example |
|---|---|---|
| `agent:<name>@<version>` | Agent runtime | `agent:korgex@0.2.2` |
| `human:<identifier>` | Human override | `human:dusk` |
| `korg:<component>` | Korg internal event | `korg:replay` |
| `mcp:<server-name>` | MCP server client | `mcp:github` |

Sub-agents use distinct identities; see §2b for the convention under
the cross-agent spine.

The dogfood checklist (§6.5) verifies every `source_agent` in a session
conforms to this convention.

---

## §2 — Causal chain (`triggered_by`)

Every non-root event carries `triggered_by` = seq_id of the event that
caused this call. The graph formed by `triggered_by` edges is a tree
rooted at one or more `user_prompt` events with `triggered_by=None`.

### Rules

1. **Root events.** `tool_name="user_prompt"` and `triggered_by=None`.
   A session has exactly one root.
2. **Parallel tool calls** from the same LLM response share `triggered_by`
   — they are siblings, not chained.
3. **Retries** point at the failure event, not at the original call.
4. **Internal tool composition is not ledgered.** If `Edit` internally
   calls a helper, only `Edit` is recorded. The boundary is "what the
   LLM decided to invoke."

### §2a — llm_inference parent rule (single-agent spine)

The triggered_by of round-N's `llm_inference` points at round-(N-1)'s
`llm_inference` seq_id within the same agent. It does NOT point at the
most recent tool call from round-(N-1).

**Rationale.** The cause of round-N's inference is the prior inference's
decision to take another turn. Tool results inform that decision; they
do not cause it. The agent could have stopped after the tool result and
didn't — that choice is made by the LLM, and the LLM event is its
proximate cause.

**Equivalently** (graph-shape phrasing): within a single agent,
`llm_inference` events form a linked list via `triggered_by`. Tool calls
hang off that spine as siblings — never inline in it.

**WRONG** — naive "chain to most recent emitted event":

    seq=1  user_prompt    triggered_by=None
    seq=2  llm_inference  triggered_by=1
    seq=3  Edit           triggered_by=2    ✓
    seq=4  llm_inference  triggered_by=3    ← INVALID

**CORRECT:**

    seq=4  llm_inference  triggered_by=2

**Why it matters.** Rewinding from a tool_call seq backward through
`triggered_by` must arrive at the `llm_inference` that produced it, then
at the prior `llm_inference`, and ultimately at the root `user_prompt`.
Naive chaining produces a topology that walks through tool calls between
inferences, which breaks replay (the tool's result is not the prompt to
the next round; the prior inference is). See §3 for full rewind semantics.

### §2b (PROPOSED) — sub-agent inference parent rule (cross-agent spine)

> **PROPOSED status.** §2b will commit or revise once a real Claude
> Code `--output-format stream-json --verbose` capture confirms the
> sub-agent causal model holds against `parent_tool_use_id`. Until
> then, this section may change. Local mirrors (e.g., the docstring in
> `korgex/src/korg_ledger.py`) do not include §2b — the spec leads
> reality by at most one PROPOSED rule.

§2a's linked list holds within a `source_agent`. Across agents, spines
fork at spawn tool_calls and rejoin nowhere — each sub-agent's spine
terminates independently when its loop exits. The union of all spines
is still a tree rooted at `user_prompt`, but recovering a single
agent's linked list requires filtering by `source_agent`.

When a tool call spawns a sub-agent — i.e., its implementation invokes
a new LLM loop with its own tools and context, rather than executing a
deterministic operation — the sub-agent's first `llm_inference` points
at the spawning tool_call seq_id, NOT at any `llm_inference`.

Each agent has its own §2a spine. Sub-agent spines branch off tool
calls in the parent agent, not off `llm_inference` events. The
`source_agent` field distinguishes which spine an event belongs to.

**Worked example:**

    seq=1  user_prompt              triggered_by=None      source=human:dusk
    seq=2  llm_inference            triggered_by=1         source=agent:main
    seq=3  Agent (spawn)            triggered_by=2         source=agent:main
    seq=4  llm_inference            triggered_by=3         source=agent:sub
    seq=5  Read                     triggered_by=4         source=agent:sub
    seq=6  llm_inference            triggered_by=4         source=agent:sub  (§2a within sub)
    seq=7  llm_inference            triggered_by=2         source=agent:main (§2a within main,
                                                                              skipping the
                                                                              entire sub subtree)

Walking back through main's spine: `7 → 2 → 1`.
Walking back through sub's spine:  `6 → 4 → 3 → 2 → 1`.
The two spines cross at `seq=3` (the spawn tool_call).

**Recursive nesting.** Sub-agents may themselves spawn sub-agents; §2b
applies recursively. The `source_agent` naming convention for nested
spawns (depth 3+) is deferred until a captured session exercises it.

**Mapping to existing protocols.** In Claude Code's stream-json this
is signalled by `parent_tool_use_id` on the spawned events; in korgex
(once the Agent tool is implemented) it will be the seq_id of the
Agent tool's call event.

---

## §3 — Rewind semantics

A rewind to seq=N truncates the ledger to seq ≤ N and invalidates every
event with seq > N that was causally downstream of N. Korg's `autoheal`
(roadmap v0.2) and any replay primitive MUST respect this rule.

### Invalidation definition

Event S is **causally downstream** of N if any of:

1. S's `triggered_by` chain reaches N (direct or transitive parent), OR
2. S is an `llm_inference` whose prompt consumed N's result — i.e., N
   is a sibling tool_call under S's parent `llm_inference` in the same
   agent (§2a), OR
3. S is in a sub-agent spawned by N or one of N's descendants (§2b),
   OR
4. S is an `llm_inference` in the parent agent whose prompt consumed
   the return value of a spawn tool_call whose subtree contains N (§2b).

Rule (2) is the non-obvious one: invalidation extends to events that
aren't `triggered_by` descendants but whose context depended on the
rewound event. Most observability tools don't model this because they
have no rewind primitive. Korg does.

### Worked example (from §2b)

Given the §2b worked example:

    rewinding to seq=5  invalidates {6, 7}
    rewinding to seq=3  invalidates {4, 5, 6, 7}
    rewinding to seq=2  invalidates {3, 4, 5, 6, 7}

Breakdown:
- `rewind(5)` invalidates `seq=6` by rule (2): seq=6's prompt
  consumed seq=5's Read result. Invalidates `seq=7` by rule (4): main's
  next inference consumed the spawn tool_call's return value, which
  depended on the sub-agent's subtree including seq=5.
- `rewind(3)` invalidates the entire sub-agent subtree by rule (3),
  plus seq=7 by rule (4).
- `rewind(2)` invalidates everything by rule (1) — seq=3 chains to
  seq=2, and rules (3)/(4) propagate from there.

### Rewind is content-aware, not just topology-aware

Rules (2) and (4) require the implementation to know that an
`llm_inference`'s prompt depends on its preceding sibling tool_calls.
This is implicit in §2a's spine structure and made explicit here so
implementers don't ship a topology-only rewind that misses rule (2)
invalidations.

---

## §4 — Originator

Korg does not carry a separate `originator` field. The originator of
any event is the root `user_prompt` reached by walking `triggered_by`
back to a null parent.

For multi-agent sessions (§2b), every spine terminates at the same
root — sub-agents inherit the originator of the spawning agent's chain.

This is intentional: the causal chain is the audit answer. A separate
originator field would be a denormalization of information already in
the chain, and could drift.

---

## §5 — Client write semantics

Clients SHOULD post events to Korg via a non-blocking enqueue that
returns immediately and serializes HTTP writes through one background
worker. The agent loop must not be blocked by Korg availability or
latency. The full implementation rule is §7.5.

Synchronous writes are permitted only for events whose seq_id must be
known immediately to chain a child event:

- `user_prompt` (root) — needed to chain the first `llm_inference`.
- `llm_inference` — needed to chain that round's tool calls as siblings.

All other events (tool calls, retries) MUST use async enqueue. The
seq_id is not needed for chaining because tool calls do not have
children at the same agent layer — their effects appear on the next
`llm_inference`'s prompt, not as a triggered_by from another event.

---

## §6 — Dogfood checklist

A session passes the dogfood checklist if and only if all six checks
succeed. Reference implementation: `korgex/korg_dogfood.py`.

### §6.1 — Backward causal chain (leaf → root)

For each `AgentToolCall` event, walking `triggered_by` reaches a root
event with `triggered_by=None` within ≤200 hops. A "dead pointer"
(parent seq_id not present in the ledger) is acceptable only if it
corresponds to a §7.5 drop-oldest event, and MUST be reported, not
silently accepted.

### §6.2 — Forward causal chain (root → leaves)

From any root event, BFS over forward `triggered_by` edges reaches at
least 2 descendant events (root + at least one child). If the ledger
has > 100 events and forward walk requires a full O(n) scan, the
implementation SHOULD warn — production ledgers need a
`triggered_by` index (see §6.6).

### §6.3 — File-touch query

For any event with `args.file_path` (or `args.path`/`args.filepath`),
the file path MUST appear verbatim in the event — NOT inlined as a
content-ref string. No `args` or `result` field value MAY exceed 1KB
inline; values above the threshold MUST be content-referenced per §7.3.

### §6.4 — Blob atomicity

Every `payload_refs[].sha256` MUST correspond to a blob present at
`$KORG_BLOB_DIR/<sha256[:2]>/<sha256>` (default `.korg/blobs/`). Missing
blobs are a ledger integrity failure and MUST abort, not be silently
ignored. See §7.3 for the blob-first atomicity rule.

### §6.5 — Actor identity convention

Every `source_agent` value MUST begin with `agent:`, `human:`, `korg:`,
or `mcp:` per §1.1. Non-conforming identities fail the check.

Additionally, every event MUST carry `schema_version` matching the
current frozen version (`1.0`).

### §6.6 — Forward-walk index readiness

When the ledger exceeds ~500 events, forward walk by `triggered_by`
SHOULD use an index rather than a full journal scan. Below that
threshold, O(n) scan is acceptable. The check warns rather than fails
at the threshold; failure is reserved for absent indices on ledgers
> 5000 events.

---

## §7 — Implementation rules

### §7.2 — Canonical hashing

When a value is large enough to be content-referenced (§7.3), its hash
MUST be SHA-256 of the canonical byte representation:

| Value type | Canonical bytes |
|---|---|
| Structured (dict/list/etc.) | Compact JSON: no whitespace, sorted keys (`json.dumps(value, separators=(",", ":"), sort_keys=True)`) → UTF-8 |
| String | UTF-8 directly |
| Binary | Raw bytes |

Two agents emitting the same logical content MUST produce the same
SHA-256. This makes the blob store globally deduplicated and
verification deterministic across agents.

### §7.3 — Content-ref threshold and blob-first atomicity

**Threshold.** Any field value (in `args` or `result`) whose canonical
byte representation (§7.2) exceeds **1024 bytes** MUST be replaced with
a content-ref sentinel and the original written to the blob store:

```json
{"_ref": "sha256:<digest>", "size_bytes": <int>}
```

A descriptor MUST be appended to the event's `payload_refs`:

```json
{"sha256": "<digest>", "size_bytes": <int>, "label": "<tool>.<field>"}
```

The threshold is applied **uniformly** — no exception for "small"
payloads even if they look semantically simple. Two agents reading the
same byte sequence MUST make the same content-ref decision.

**Blob-first atomicity.** Blobs MUST be written to
`$KORG_BLOB_DIR/<sha256[:2]>/<sha256>` **before** the event is sent to
the server. Missing blobs on replay are a ledger integrity failure —
abort loudly, do not silently fabricate.

(This section was previously numbered §3 in some code comments before
schema v1.0 froze. §3 now belongs to rewind semantics; the content-ref
rule lives here. Both code references to "spec §3" for content-refs
and "spec §3" for rewind are correct in their respective contexts —
the former predates the freeze.)

### §7.5 — Serialized writes / bounded queue

Clients MUST serialize HTTP writes to Korg through exactly one
in-flight request at a time. This preserves the enqueue-order
invariant that `triggered_by` depends on: if two events are enqueued
in causal order, their assigned seq_ids MUST also be in causal order.

**Implementation.**

- One background daemon thread per client.
- Bounded queue, default capacity 256.
- On queue full: drop oldest, log a WARNING, enqueue new event. Do not
  block the agent loop. (Drop-oldest beats drop-newest because newer
  events are likelier to be referenced as `triggered_by` parents by
  events still to come.)
- On client shutdown: SHOULD flush the queue via atexit handler before
  process exit. (Open issue: korgex's `KorgLedgerClient` does not do
  this yet — the daemon thread dies with the process. Tracked.)

The drop-oldest case produces "dead pointer" parents (§6.1). Those
MUST be logged as such, not silently masked.

### §7.6 — Causal backward walk root invariant

Every event in a well-formed session MUST be reachable from exactly one
root `user_prompt` by walking `triggered_by` backward. A session that
does not satisfy this is malformed.

Two known cases produce dead pointers without violating this invariant:

1. The parent was dropped under §7.5 (queue overflow). The walk
   terminates at the dead pointer and reports it; the session is
   marked partial, not malformed.
2. The session began before the ledger was running, and root capture
   was synthesized rather than observed (e.g., stream-json adapter's
   v1.1 synthesized prompt root). The synthesized root MUST be
   labeled as such in `args`, not silently emitted as if observed.

### §7.7 — Trust boundary (v1)

Schema v1 is designed for **local, single-user workspaces**. Korg
trusts `source_agent` as provided — there is no signature, no
authentication, no permission check. A malicious client can post events
with any `source_agent` it wants.

This is acceptable for v1 because the deployment scope is local. It is
**not acceptable** for any networked or multi-tenant deployment.
Cryptographic agent identity (ed25519-signed `source_agent`) is on the
roadmap before any networked deployment ships.

Until then: running the Korg server on an untrusted network exposes
the ledger to forgery. The README's "Trust Boundary & Deployment Scope"
warning is the operational restatement of this rule.

---

---

## §8 — MCP resource URI scheme

> **Status (per subsection).** §8 is the read-side public API for any
> MCP client connecting to Korg.
>
> **VALIDATED — Phase A:** §8.1, the fixed-resource portion of §8.2,
> the tail-surface cursor in §8.3.1 (`ledger/recent`), and the error
> reason codes in §8.6.
>
> **VALIDATED — Phase B:** the template portion of §8.2 (5 URI
> templates); §8.3 session shapes (metadata, summary, events) including
> the `source_agent` spine filter; §8.3.1 head-surface cursor
> (oldest→newest) for `session/*/summary` and `session/*/events`;
> `event/{seq_id}` single-event read; `agent/{source_agent}/recent`.
> Validated end-to-end by a 41-test live JSON-RPC suite including §2b
> main-spine and sub-spine assertions.
>
> **VALIDATED — Phase C:** §8.4 blob handling; `blob_too_large` error
> reason in §8.6. 49-test suite, including base64 round-trip for binary
> blobs and structured `blob_too_large` response with escape hatch.
>
> **VALIDATED — Phase D:** §8.5 (subscriptions). Journal-watching poller,
> subscription registry, predicate filtering, confirmation semantics, and
> ephemeral state model all implemented and end-to-end validated. 64-test
> suite covers all Phase D surfaces. §8.6.1 **DEFERRED** to v0.2 autoheal.
> The event schema (§1–§7) is unaffected by §8 changes.

### §8.1 — URI scheme: `korg://{ledger}/...` (VALIDATED Phase A)

Every Korg resource URI begins `korg://{ledger}/`. The `{ledger}`
segment names which ledger the URI refers to. In schema v1.0, the only
valid ledger ID is **`local`** — v1 servers reject any other.

The ledger slot exists in v1 so clients build correct expectations
for v2+ multi-ledger deployments (hosted Korg, federated audit). The
URI scheme does not change when multi-tenancy ships; only the set of
valid ledger IDs expands.

Bad: `korg://event/42`
Good: `korg://local/event/42`

### §8.2 — Resource catalog (fixed: VALIDATED Phase A; templates: VALIDATED Phase B)

**Templates** (advertised via `resources/templates/list`):

| URI template | Returns |
|---|---|
| `korg://{ledger}/session/{root_seq}` | Session metadata (bounded) |
| `korg://{ledger}/session/{root_seq}/summary` | Paginated structural skeleton |
| `korg://{ledger}/session/{root_seq}/events` | Paginated full event bodies |
| `korg://{ledger}/event/{seq_id}` | One event, full body |
| `korg://{ledger}/agent/{source_agent}/recent` | Paginated recent events from one agent |
| `korg://{ledger}/blob/{sha256}` | One content-addressed blob |

**Fixed resources** (advertised via `resources/list`):

| URI | Returns |
|---|---|
| `korg://{ledger}/ledger/recent` | Paginated recent events across all agents (newest → oldest, see §8.3.1) |
| `korg://{ledger}/ledger/heads` | List of active session root seq_ids |
| `korg://{ledger}/schema/event` | The AgentToolCall JSON Schema |
| `korg://{ledger}/schema/spec` | The agent_event_spec.md content |
| `korg://{ledger}/stats/integrity` | Dogfood checklist status (§6) |

**Not listed:** `korg://{ledger}/blob/{sha256}` URIs are NEVER enumerated
in `resources/list`. The list would grow unboundedly. Clients construct
blob URIs from event `payload_refs[].sha256` and resolve via
`resources/read`. The template advertises the addressability;
enumeration is up to the client.

### §8.3 — Session resource shape (VALIDATED Phase B)

A "session" is the subtree rooted at one `user_prompt` event. For
multi-agent sessions (§2b), the session includes events from all spines
descended from that root — main agent and every sub-agent.

**`korg://{ledger}/session/{root_seq}` — metadata only.** Bounded
small. Returns:

```json
{
  "root_seq": 2390,
  "root_event": {...},
  "total_events": 4127,
  "agent_count": 3,
  "agents": ["agent:korgex@0.2.2", "agent:korgex-sub@0.2.2"],
  "first_seq": 2390,
  "last_seq": 6516,
  "last_event_at": "2026-05-25T14:30:00Z",
  "last_event_seq": 6516,
  "schema_version": "1.0"
}
```

No `status` field. Determining whether a session is "active" or
"complete" requires policy (timeout? explicit close event?) that v1
does not define. `last_event_at` and `last_event_seq` are the
observable facts; callers that need a derived status MUST compute it
themselves.

**`korg://{ledger}/session/{root_seq}/summary` — paginated structural
skeleton.** Query params: `cursor` (default 0), `limit` (default 100,
max 1000), `source_agent` (optional filter). Returns:

```json
{
  "events": [
    {"seq_id": 2390, "source_agent": "agent:korgex@0.2.2", "tool_name": "user_prompt",
     "triggered_by": null, "success": true, "duration_ms": 0, "has_payload_refs": false},
    ...
  ],
  "next_cursor": 2490,
  "has_more": true
}
```

Per-event record is bounded ~150 bytes. 1000 events ≈ 150KB per page.

**`korg://{ledger}/session/{root_seq}/events` — paginated full bodies.**
Same query params as summary. Returns full event objects (with `args`,
`result`, `payload_refs`). Lower default limit (`50`, max `500`) because
event bodies can include content-refs and large fields.

**Event ordering: flat chronological across all spines.** Events are
returned ordered by `seq_id` ascending, mixing main-agent and
sub-agent events together as they occurred (§2b). This matches the
natural reading of a session URI: "everything caused by this user
prompt, in order."

Clients that want a single spine pass `?source_agent=X` to filter.
Clients that want a per-agent view (independent of session boundary)
use `korg://{ledger}/agent/{source_agent}/recent`. The two URIs have
different products: session is "what did this prompt cause"; agent is
"what has this agent done lately."

The same flat-chronological rule applies to
`korg://{ledger}/session/{root_seq}/summary`.

### §8.3.1 — Cursor direction convention (VALIDATED Phase A + Phase B)

Cursors are seq_ids (per §8.3), but the **direction** of pagination
depends on which surface the resource exposes. Tail surfaces ("most
recent N") paginate newest→oldest; head surfaces ("session from its
root") paginate oldest→newest. The convention is per-endpoint because
the natural reading order differs — forcing both to share semantics
either makes one feel wrong or inverts complexity for symmetry's sake.

| Endpoint | Direction | Cursor semantics |
|---|---|---|
| `korg://{ledger}/ledger/recent` | newest → oldest | `?cursor=N` returns events with `seq_id < N` |
| `korg://{ledger}/agent/{source_agent}/recent` | newest → oldest | same as above |
| `korg://{ledger}/session/{root_seq}/summary` | oldest → newest | `?cursor=N` returns events with `seq_id > N` |
| `korg://{ledger}/session/{root_seq}/events` | oldest → newest | same as above |

Worked example — tail surface (newest first):

    Page 1: GET korg://local/ledger/recent?limit=2
            → events [seq=99, seq=98];  next_cursor=98
    Page 2: GET korg://local/ledger/recent?limit=2&cursor=98
            → events [seq=97, seq=96];  next_cursor=96

Worked example — head surface (oldest first):

    Page 1: GET korg://local/session/100/events?limit=2
            → events [seq=100, seq=101]; next_cursor=101
    Page 2: GET korg://local/session/100/events?limit=2&cursor=101
            → events [seq=102, seq=103]; next_cursor=103

The catalog entries in §8.2 and the per-resource sections repeat the
direction inline so clients don't need to cross-reference §8.3.1. Per-
endpoint natural direction beats forced symmetry, *provided* the
direction is visible at the point of use.

**Status:** VALIDATED. Tail-surface validated by Phase A (`ledger/recent`
and `agent/recent`). Head-surface validated by Phase B (`session/*/summary`
and `session/*/events`) with cursor pagination covered by the 41-test suite.

### §8.4 — Blob handling (VALIDATED Phase C)

**`korg://{ledger}/blob/{sha256}`** returns one blob.

#### §8.4.1 — Rust HTTP endpoint shape (Phase C decision)

`GET /api/blob/{sha256}` returns **raw bytes** with a `Content-Type`
header matching the blob's stored MIME type (or `application/octet-stream`
if unknown). This is the standard HTTP idiom — it lets `curl`, browsers,
and HTTP-level tools work without decoding an envelope.

The MCP handler then wraps the raw response:

| Blob content | MCP content type | Encoding |
|---|---|---|
| Parses as valid JSON | `text` with `mimeType: "application/json"` | UTF-8 string |
| Valid UTF-8, not JSON | `text` with `mimeType: "text/plain"` | UTF-8 string |
| Binary / invalid UTF-8 | `blob` with `mimeType: "application/octet-stream"` (or sniffed) | base64 |

The Rust endpoint is idiomatic HTTP; the MCP layer adds the envelope
only where the protocol requires it.

#### §8.4.2 — Size cap and failure mode (Phase C decision)

**Cap:** 10MB raw bytes over JSON-RPC. Larger blobs return a structured
error — **no partial reads**. Content-addressed blobs are atomic; a
partial response would not match the sha256 and would be meaningless
to verify.

Failure shape: JSON-RPC error `-32603` with:
```json
{
  "reason": "blob_too_large",
  "sha256": "...",
  "size_bytes": 15728640,
  "http_url": "/api/blob/{sha256}"
}
```

The `http_url` field carries the escape hatch. Clients that need the
full blob MUST use the HTTP endpoint directly. This keeps the JSON-RPC
path well-behaved while documenting exactly where to go instead.

This is a v1 limitation — JSON-RPC + base64 makes multi-MB transfers
brittle. The HTTP escape hatch keeps `payload_refs` honoring §7.3's
atomicity without forcing every read through the JSON-RPC layer.

### §8.5 — Subscriptions (VALIDATED — Phase D)

Phase D introduces the first stateful MCP surface: Phase A–C are
stateless translators over Korg's HTTP API; Phase D adds a subscription
registry and a background journal-watching task. Three design decisions
are pinned here before implementation.

#### §8.5.1 — Journal-watching architecture (Phase D decision)

**Polling at 1s, Python-only.** The MCP server polls
`GET /api/journal` every 1 second, tracking the highest `seq_id` seen.
Events with `seq_id > last_known_seq` are new; the poller runs their
seq_ids against each subscription's predicate and fires
`notifications/resources/updated` for matches.

No Rust changes required. Polling introduces a maximum notification
latency of ~1 second, which is acceptable for v1 use cases (monitoring
dashboards, audit watchers). If a real use case demands sub-100ms
notification latency, push from Rust (long-poll endpoint, websocket, or
unix socket) becomes worth building. Do not pre-optimize.

The background poller is a daemon thread launched at server start. If
polling fails (Korg unreachable), the poller logs and retries on the
next interval — it does not tear down existing subscriptions.

**Notification write coordination.** The daemon thread writes
notifications using a module-level `_stdout_lock` (`threading.Lock`).
All stdout writes — both request responses (main thread) and
notifications (poller thread) — acquire this lock before writing. This
prevents interleaved JSON-RPC frames, which would corrupt the stdio
transport unrecoverably. All writes go through a single `_send_notification`
function that holds the lock; tests patch that function rather than
stdout directly.

**`last_seen_seq` initialization.** At startup the poller fetches the
full journal, populates the `seq_id → root_seq_id` lookup table, sets
`last_seen_seq` to the current max, then begins polling for new events.
It does NOT notify on events that existed before the server started.
Clients subscribing immediately after startup will miss events that
landed between startup and the subscribe call; they MUST read the
resource directly if they need current state. This is consistent with
§8.5.4 — subscriptions are forward-looking by design.

**Three poll-failure modes** (handled distinctly in the daemon thread):

1. *Korg HTTP unreachable* — `requests.ConnectionError`: log at DEBUG,
   sleep POLL_INTERVAL, retry. Do not clear subscriptions.
2. *Korg returns malformed response* — `json.JSONDecodeError` or
   unexpected structure: log at WARN, retry. If persistent, the
   operator must check Korg's health; the MCP server cannot crash
   without dropping all clients.
3. *Predicate raises exception* — `except Exception` around each
   predicate call: log at ERROR with the event seq_id that triggered
   it, skip that predicate for this event, continue dispatching other
   subscriptions. A broken predicate must not kill the poll loop.

#### §8.5.2 — Subscription predicates (Phase D decision)

Each subscription is `(uri, predicate, client_id)`, not just
`(uri, client_id)`. The predicate is compiled once at subscribe time
and is O(1) per new event.

**Predicate table by URI pattern:**

| Subscribed URI | Predicate | What triggers a notification |
|---|---|---|
| `korg://{ledger}/ledger/recent` | always true | Any new event |
| `korg://{ledger}/ledger/heads` | `tool_name == "user_prompt" and triggered_by is None` | New session root only |
| `korg://{ledger}/session/{root}/summary` | event seq_id is in the session's triggered_by subtree | New event in this session |
| `korg://{ledger}/agent/{agent}/recent` | `source_agent == agent` | New event from this agent |

For `session/{root}` subscriptions, the predicate uses a
**`seq_id → root_seq_id` lookup table** (`_seq_to_root: dict[int, int]`),
maintained by the poller. Lookup is O(1): `_seq_to_root.get(event.seq_id) == root`.

The table is built at startup (full journal scan) and extended
incrementally on each poll tick (oldest-first insertion):

- If `triggered_by is None`: `_seq_to_root[seq] = seq` (it's a root).
- Else: `_seq_to_root[seq] = _seq_to_root.get(triggered_by, seq)` — inherit
  parent's root; fall back to self if parent predates the table.

This removes the O(depth) BFS walk per predicate evaluation. Under heavy
session activity, this is the difference between making the 1s budget
and missing it. The table grows monotonically (one int per event) and is
never compacted in v1 — a background compaction strategy can wait until
the table is measurably large.

**Non-subscribable URIs:** `event/{seq_id}`, `blob/{sha256}`,
`schema/event`, `schema/spec`, `stats/integrity`. These are either
immutable (events and blobs never change after append) or poll-on-demand
(schema/stats). Subscribing to a non-subscribable URI returns a
structured error: `{reason: "not_subscribable", uri}`.

#### §8.5.3 — Confirmation semantics (Phase D decision)

**Empty confirmation.** `resources/subscribe` returns an empty success
response — it does NOT send an initial snapshot. Clients that want
current state MUST read the resource separately after subscribing.

This keeps subscribe and read orthogonal:
- "Subscribe to a session that has no events yet" works cleanly — no
  snapshot to send, no empty-vs-error ambiguity.
- Clients don't have to handle both initial-snapshot and delta-update
  shapes in the same code path.
- Clients that need current state always issue an explicit read; the
  subscription only tells them when to do that read again.

The `notifications/resources/updated` body is the standard MCP shape:
`{uri: "<the subscribed URI>"}`. Korg does NOT embed the new state
in the notification. Clients re-read on update. This keeps notification
traffic small and avoids stale-state-in-flight races.

**Unsubscribe semantics (Phase D decision):**

- `resources/unsubscribe` matches by URI. Removes all subscriptions
  for that URI from the registry. Unsubscribing a URI not currently
  subscribed is a no-op (returns empty success).
- `resources/subscribe` is **idempotent** per URI: subscribing a URI
  already in the registry is a no-op (returns empty success, does not
  add a duplicate). Multiple subscribes to the same URI from the same
  client result in exactly one subscription entry, and one notification
  per matching event. Subscription IDs are not used in v1 — the URI
  is the identifier. If a real use case demands per-subscription granularity
  (e.g., different filter parameters on the same URI from the same client),
  subscription IDs can be added in v2.

#### §8.5.4 — Ephemeral subscription state (Phase D design constraint)

**Subscription state is in-memory and ephemeral by design.** MCP server
restart clears all subscriptions. This is intentional:

- Subscriptions are a derived signal ("tell me when things change"),
  not source-of-truth state. The ledger is the source of truth.
- Clients that cannot tolerate gaps must re-read after reconnect
  regardless of whether the server persisted their subscription —
  network drops, process restarts, and machine reboots all require the
  same reconnection handling.
- Persisting subscriptions would buy nothing the client does not already
  have to handle, at the cost of a persistence layer and restart
  semantics.

**Consequence to document at subscribe time:** The MCP server SHOULD
include a note in its initialization response (or in a future
`serverInfo.capabilities` extension) that subscriptions are not
persisted. Clients MUST implement reconnect-and-resubscribe logic.

The `subscribe` / `listChanged` capability flags in `initialize` are
only set to `true` once Phase D lands. Phase A–C keep both flags
`false` to avoid advertising capabilities the server does not yet
implement.

**Subscribable URIs (summary):**
- `korg://{ledger}/ledger/recent` — any new event
- `korg://{ledger}/ledger/heads` — new session root only
- `korg://{ledger}/session/{root_seq}/summary` — new event in this session
- `korg://{ledger}/agent/{source_agent}/recent` — new event from this agent

### §8.6 — Edge cases (error reasons: VALIDATED Phase A + Phase B + Phase C)

**Non-existent URI:** `resources/read` returns JSON-RPC error `-32602`
with `data: {reason: "not_found"}` for any unresolvable seq_id or blob sha256.
(VALIDATED Phase B: `not_found` returned for missing `event/{seq_id}` and
`session/{root_seq}`.)

**Blob too large (VALIDATED Phase C):** When a blob exceeds the 10MB
JSON-RPC cap, `resources/read` returns error `-32603` with
`data: {reason: "blob_too_large", sha256, size_bytes, http_url}`. The
`http_url` is the direct HTTP path (`/api/blob/{sha256}`) the client
MUST use instead. No partial reads — see §8.4.2.

**Non-subscribable URI (PROPOSED Phase D):** `resources/subscribe` on an
immutable or unsubscribable URI returns `-32602` with
`data: {reason: "not_subscribable", uri}`. See §8.5.2 for the full
non-subscribable list.

**Rewound events:** Pre-rewind events at the same `seq_id` are NOT
addressable after rewind. The ledger is the current authoritative
state; URI resolution always refers to the current ledger. Clients
that cache `seq_id` → content mappings MUST invalidate on rewind
notifications (TBD: a `notifications/ledger/rewound` event will be
defined when v0.2 autoheal ships).

**Multi-agent filtering:** All session resources accept an optional
`source_agent` query parameter to filter to one agent's spine. Without
it, results include all spines (main + sub-agents per §2b).

**Pagination consistency:** Cursors are seq_ids, not opaque tokens. A
client paginating through a session that grows mid-read sees the new
events on the next page — there is no point-in-time snapshot. For
exact replay, use a rewind to a known seq_id and re-read.

### §8.6.1 — Rewound notification (stub; wire shape DEFERRED to v0.2)

When the ledger is rewound (via Korg's `autoheal` loop in v0.2, or by
any future replay/fork primitive), connected MCP clients need to know
the rewind happened so they can invalidate cached state. This stub
documents what's KNOWN about the notification's content even though
the wire shape is deferred until v0.2 autoheal ships. Same discipline
as §2b: defer the commitment, not the thinking.

**Load-bearing fields** (the notification MUST carry these at minimum):

1. **Rewind target seq_id** — the seq we rewound to. Events with
   `seq_id ≤ target` remain authoritative.
2. **Invalidation set per §3** — the set of seq_ids no longer
   addressable (downstream of target per §3's four invalidation rules).
   Shape TBD: full list, range, or predicate; depends on what autoheal
   needs to broadcast efficiently.
3. **New head seq_id** — the new authoritative tail of the ledger.
   For a clean rewind with no replay yet, this equals the target.
4. **Rewound-by actor** — the `source_agent` of the actor that
   initiated the rewind (`korg:autoheal`, `human:<id>`, or similar).
   Audit needs to know who undid what.

**What is deferred:**

- Exact MCP method name. Plausibly `notifications/ledger/rewound`
  under a Korg-specific category, since standard MCP notification
  categories (`resources/updated`, `resources/list_changed`) don't
  carry enough fields for a rewind. This is a Korg extension —
  standard MCP clients will receive and silently ignore.
- Exact object shape of the invalidation set.
- Whether the notification is sent before or after URI resolution
  starts returning invalidated content (race semantics).

These finalize when v0.2 autoheal lands, because the consumer side
shapes the producer side. Designing the wire format before autoheal
exists would produce a notification that's plausible but probably
wrong, and "probably wrong but already shipped" is the worst state
for a public surface.

Until v0.2 ships, clients SHOULD assume the ledger is append-only
and SHOULD NOT cache `seq_id → content` mappings without an
invalidation strategy of their own.

### §8.7 — Forward compatibility commitment

§8 URI templates and resource shapes are stable within schema v1.x.
Additive changes (new URI templates, new optional query params, new
fields in returned objects) are non-breaking. Removing or renaming a
URI template, or changing a field's type, requires a schema version
bump and a deprecation period.

The `{ledger}` slot is the explicit forward-compat surface for
multi-tenant deployments. No other URI form should ever need to change.

---

## Changelog

- **2026-05-25** — Phase A landed. Per-subsection status markers added
  reflecting validation state: §8.1, fixed-resource portion of §8.2,
  tail-surface portion of §8.3.1, and error reasons in §8.6 now
  VALIDATED. Cursor direction convention codified at §8.3.1: per-
  endpoint natural direction (tail = newest→oldest, head =
  oldest→newest) with the direction repeated inline at each endpoint.
- **2026-05-25** — §1.2 added (PROPOSED) — append shape vs journal shape.
  Documents the 9-field POST body (§1.2.1), the 4-field journal envelope
  (§1.2.2), the 14 metadata fields classified as load-bearing /
  informational / server-internal (§1.2.3), the AgentToolCall event
  fields including server-assigned `timestamp` (§1.2.4), `causation_id`
  vs `triggered_by` addressing (§1.2.5), `actor_id` recorder vs
  `source_agent` actor distinction (§1.2.6), and the complete list of
  server-derived fields on append (§1.2.7). §6.5 backtick value fixed
  from `v1.0` to `1.0`.
- **2026-05-25** — Phase C landed. `GET /api/blob/:sha256` added to Korg's
  Axum HTTP server (raw bytes, sha256 validation, 404 on missing, fan-out
  `.korg/blobs/{prefix}/{sha256}` layout). MCP blob handler added:
  `_korg_blob` HTTP helper, `_resource_blob_read` with 10MB cap and
  `blob_too_large` error (reason code, sha256, size_bytes, http_url
  escape hatch per §8.4.2), binary content packed as MCP `blob` type
  (base64), text/JSON packed as `text`. `_UriError` extended with `code`
  and `extra` fields for structured error data. Blob template added to
  RESOURCE_TEMPLATES (6 total). 49-test suite passes.
- **2026-05-25** — Phase D landed. §8.5 subscriptions promoted from PROPOSED
  to VALIDATED. Journal-watching poller (1s interval, daemon thread, retry on
  disconnect), subscription registry, predicate filtering (event_type, agent,
  seq_id_gt), confirmation semantics, and ephemeral state model all implemented.
  `_stdout_lock` write-coordination for interleaved notification frames.
  `last_seen_seq` initialized from live journal at startup. 64-test suite
  validates all Phase A–D surfaces end-to-end.
- **2026-05-25** — Phase B landed. §8.2 templates (5 URI templates),
  §8.3 session shapes (metadata, summary, events), §8.3.1 head-surface
  cursor all promoted to VALIDATED. `status` field removed from session
  metadata in favour of `last_event_at`+`last_event_seq` (no policy
  required). §8.4 Phase C design decisions pinned: raw-bytes Rust HTTP
  endpoint + MCP envelope wrapping (§8.4.1); atomic error on >10MB with
  `blob_too_large` reason code and `http_url` escape hatch, no partial
  reads (§8.4.2). `blob_too_large` error reason added to §8.6. 41-test
  JSON-RPC suite validates all Phase B surfaces including §2b spine
  assertions.
- **2026-05-25** — §8 added (PROPOSED) — MCP resource URI scheme.
  Three load-bearing decisions documented: ledger-id slot in URI scheme
  (§8.1), session metadata/summary/events split with pagination (§8.3),
  blob template-only listing with 10MB JSON-RPC cap (§8.4). Session
  events flat-chronological by default (§8.3); spine-scoped reads via
  `?source_agent=X` filter or `korg://{ledger}/agent/...`. Rewound
  notification stub added (§8.6.1) with load-bearing fields documented;
  wire shape deferred to v0.2 autoheal.
- **2026-05-25** — schema v1.0 frozen. §2a added (llm_inference parent
  rule). §2b added as PROPOSED (sub-agent inference parent rule). §3
  added (rewind semantics, lifted from §2b's example paragraph). §7.3
  noted the §3 → §7.3 renumber for the content-ref rule.
- **2026-05-19** — initial draft (referenced but not committed).
