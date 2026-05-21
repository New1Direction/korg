---
date: 2026-05-20
type: design
tags: [design, acp, message-schema, reference-harness, protocol, wire-format]
harness: korg
domain: acp, orchestration, state-management
status: emerging
ai-first: true
---

# ACP Message Schema

This note defines the concrete message schemas, wire formats, and encoding rules for the Agent Client Protocol (ACP) introduced in `ACP-Binding-Design.md` and used by the `Leader-Broker-ACP-Model-for-Parallel-Agents.md`.

The schemas are intentionally minimal and stable. Every field exists to support one of the three core Korg contracts. Implementations may add optional extensions, but they must not break the contract guarantees.

**Traceability**: This note is a faithful implementation of the ACP message surface described in [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]]. The exact RPC methods (`task.create`, `message.send`, `conflict.resolve`, `result.stream`), error code taxonomy, provenance chain format, delta patching semantics, Arena scoring, and headless streaming behavior are directly derived from the real Grok 4.20 Heavy ACP protocol and its production usage.

---

## Wire Format Rules

ACP uses a dual-encoding strategy:

- **JSON** (UTF-8) — Primary format for development, debugging, logging, and human inspection.
- **CBOR** (Concise Binary Object Representation) — Recommended production format for efficiency, especially with large provenance chains or `.ktrans` payloads.

**Rules that apply to both encodings:**

- All messages are self-describing objects with a top-level `type` field (string) and `version` field (integer, starting at 1).
- Timestamps are ISO-8601 strings in UTC (e.g., `2026-05-20T14:30:00Z`).
- Hashes are lowercase hexadecimal strings.
- UUIDs are lowercase strings in the standard hyphenated form.
- Unknown fields must be ignored by receivers (forward compatibility).
- Messages may be wrapped in an outer envelope for transport (e.g., length-prefixed frames over TCP, WebSocket messages, or gRPC).

**Recommendation:** Implementations should support both encodings and negotiate via a `Hello` or capability exchange during connection setup.

---

## Common Fields

The following fields appear in multiple messages:

- `version`: Integer. Current value: `1`
- `timestamp`: ISO-8601 UTC string
- `sender_id`: String identifier of the sending role instance (e.g., Broker UUID or Leader name)
- `correlation_id`: Optional string used to correlate requests and responses (recommended for all request/response pairs)

---

## Message Schemas

### Routing & Work Dispatch

#### RouteWork (Leader → Broker)

```json
{
  "type": "RouteWork",
  "version": 1,
  "routing_id": "uuid",
  "capabilities": ["string", ...],
  "base_snapshot": "hex-hash",
  "epoch_deadline": "ISO-8601",
  "priority": integer (0-100, default 50),
  "blast_radius_hint": object (optional),
  "payload": object (opaque strategy-specific data)
}
```

#### AckRoute (Broker → Leader)

```json
{
  "type": "AckRoute",
  "version": 1,
  "routing_id": "uuid",
  "worker_id": "string",
  "broker_id": "string"
}
```

#### Stalled (Broker → Leader)

```json
{
  "type": "Stalled",
  "version": 1,
  "routing_id": "uuid",
  "reason": "epoch_timeout" | "no_capacity" | "capability_mismatch" | string,
  "details": object (optional)
}
```

---

### Worker Lifecycle

#### WorkerHello (Worker → Broker)

```json
{
  "type": "WorkerHello",
  "version": 1,
  "worker_id": "string",
  "claimed_capabilities": ["string", ...],
  "worktree_root": "string" (optional, for debugging)
}
```

#### Heartbeat (Worker ↔ Broker)

```json
{
  "type": "Heartbeat",
  "version": 1,
  "worker_id": "string",
  "token_velocity": number,
  "last_ast_delta_hash": "hex-hash",
  "progress": object (optional)
}
```

#### RequestTerminate (Broker → Worker)

```json
{
  "type": "RequestTerminate",
  "version": 1,
  "worker_id": "string",
  "reason": "doom_loop" | "campaign_complete" | "error" | "manual" | string,
  "grace_period_ms": integer (optional)
}
```

#### TerminationReport (Worker → Broker)

```json
{
  "type": "TerminationReport",
  "version": 1,
  "worker_id": "string",
  "exit_status": "success" | "error" | "killed",
  "reason": string,
  "terminal_tx_id": "uuid" (or null if none emitted),
  "doom_loop_detected": boolean,
  "final_velocity": number (optional),
  "final_ast_delta_hash": "hex-hash" (optional)
}
```

---

### Transaction Handoff

#### SubmitTransaction (Worker → Broker)

```json
{
  "type": "SubmitTransaction",
  "version": 1,
  "worker_id": "string",
  "tx_id": "uuid",
  "base_snapshot": "hex-hash",
  "content_hash": "hex-hash",
  "payload": object | "content-addressed-reference",
  "encoding": "json" | "cbor" | "external"
}
```

The `payload` may be the full `.ktrans` object or a reference when the payload exceeds size limits (see below).

#### TransactionAccepted (Broker → Worker)

```json
{
  "type": "TransactionAccepted",
  "version": 1,
  "tx_id": "uuid",
  "queue_position": integer (optional)
}
```

#### TransactionRejected (Broker → Worker)

```json
{
  "type": "TransactionRejected",
  "version": 1,
  "tx_id": "uuid",
  "error_code": string,
  "details": object (optional)
}
```

---

### Epistemic & Arbitration Feedback

#### ContestedNotification

```json
{
  "type": "ContestedNotification",
  "version": 1,
  "target_path": "string",
  "conflicting_tx_ids": ["uuid", ...],
  "authority_vectors": object,
  "suggested_resolution": "higher_authority" | "human_review" | "rebased" | string
}
```

#### MergeOutcome

```json
{
  "type": "MergeOutcome",
  "version": 1,
  "target_path": "string",
  "winning_tx_id": "uuid",
  "losing_tx_ids": ["uuid", ...],
  "resolution": "authority" | "rebasement" | "human_override",
  "new_semantic_decision_id": "uuid" (optional)
}
```

#### StatePromotion

```json
{
  "type": "StatePromotion",
  "version": 1,
  "target_path": "string",
  "new_state": "VERIFIED" | "CONTESTED",
  "verification_evidence": object
}
```

---

## Content Hashing and Provenance Encoding

- Every `.ktrans` payload carried in `SubmitTransaction` **must** include a `content_hash` of its canonical serialization.
- The canonical form for hashing is the CBOR encoding of the `.ktrans` object (even when sent as JSON).
- Provenance chains are arrays of objects containing at minimum `{ "tx_id": "...", "content_hash": "..." }`.
- Receivers must verify the `content_hash` before accepting a transaction into the merge queue.

---

## Error Code Taxonomy

Standard error codes (used in `TransactionRejected`, `Stalled`, etc.):

- `invalid_base_snapshot`
- `provenance_missing`
- `hash_mismatch`
- `epoch_expired`
- `capability_mismatch`
- `payload_too_large`
- `schema_violation`
- `doom_loop_detected`
- `merge_conflict`
- `internal_error`

Implementations may define additional codes but should prefix them (e.g., `vendor-foo-...`).

Retry semantics: `TransactionRejected` with transient errors (e.g., temporary queue pressure) may be retried by the sender. Permanent errors (schema violation, hash mismatch) must not be retried without correction.

---

## Size Limits and Fragmentation

- Recommended maximum message size (excluding transport framing): **8 MiB**.
- `.ktrans` payloads larger than 8 MiB should use the `content-addressed-reference` form in `SubmitTransaction`.
- The reference must point to a location the Broker can fetch (e.g., content-addressed object store, shared filesystem path with hash verification).
- Fragmentation of individual messages is **not** supported at the ACP layer. Large messages must be handled at the transport or reference level.

---

## Versioning and Extension Rules

- The `version` field is per-message.
- A new major version of any message type must be introduced via a new `type` value (e.g., `RouteWorkV2`) or by negotiation during connection setup.
- Unknown message types must be ignored (not rejected) to allow gradual rollout.
- Extension fields may be added to existing messages as long as they are optional and do not change the meaning of existing fields.

---

## Security and Authentication (High-Level)

ACP assumes an authenticated and authorized channel between participants. Recommended approaches (in order of preference):

1. Mutual TLS with certificate-based identity for Leader, Brokers, and Workers.
2. Signed messages using Ed25519 or similar, with capability tokens embedded in `RouteWork` and `WorkerHello`.
3. Shared secret + HMAC for simpler deployments (development only).

The protocol itself does not define authentication — that is left to the transport layer or a future ACP security extension.

---

## Related

- [[wiki/reference-harness/ACP-Binding-Design.md]] — The message types and flows these schemas implement.
- [[wiki/reference-harness/Leader-Broker-ACP-Model-for-Parallel-Agents.md]] — The coordination model that uses these messages at scale.
- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground truth production ACP specification (message semantics, error handling, Arena Mode, blackboard concurrency, provenance, and headless usage) from which these schemas and wire formats are derived.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — Practical pseudocode that actually uses the message schemas defined here.
- [[wiki/mechanisms/state-primitives.md]] — Epistemic states and Merge-Arbitration that ACP feedback messages must surface.
- [[wiki/mechanisms/isolation-routing.md]] — Routing, STALLED, and termination semantics encoded in the messages.
- [[wiki/mechanisms/transactional-memory.md]] — `.ktrans` format, content hashing, and rebasement rules that `SubmitTransaction` must carry.
- [[Human/Methodology/Building-Your-First-Harness-Against-the-Kernel.md]] — The responsibilities that any ACP-speaking harness must satisfy.

This schema document, together with the two preceding reference notes, provides a complete, implementable foundation for the Korg ACP. The next logical steps are either a minimal reference implementation or supporting notes (metrics schema, observability model, security extension).