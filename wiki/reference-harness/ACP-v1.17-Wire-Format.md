---
date: 2026-05-20
type: reference-architecture
tags: [reference-architecture, acp, protocol, wire-format, security, serialization]
harness: korg
domain: acp, protocol-design, security, streaming
status: active
ai-first: true
---

# ACP v1.17 Wire Format

**Exact wire-level specification for the Agent Control Protocol (ACP v1.17), defining JCS canonicalization (RFC 8785), Ed25519 signatures, CRLF-delimited streaming, and a comprehensive typed message + error registry.**

This note condenses and structures the primary-source specification for use as the authoritative reference in the Korg harness layer.

---

## Wire Format Rules

- **Serialization**: JCS (JSON Canonicalization Scheme, RFC 8785)
- **Streaming Mode**: CRLF-delimited JSON frames over TCP/TLS or WebSocket text frames
- **Signature Schema**: Ed25519 signature over the SHA-256 hash of the canonicalized JCS-serialized message payload (the `signature` object itself is excluded from the signed data)

Every message **must** carry a `signature` object containing:
- `public_key` (64 hex characters)
- `signature_bytes` (128 hex characters)

---

## Message Registry (Selected)

The registry defines many messages. Below are the ones most relevant to Korg and the current Rust skeleton.

### `task.create`
- Creates a new task with instructions, context, and constraints.
- Payload includes `workspace_path`, environment variables, `max_duration_seconds`, and `allowed_tools`.

### `result.stream`
- Streaming results from an agent.
- Supports `stdout`, `stderr`, `telemetry`, `state_update`.
- Includes `sequence_number` and `is_final` flag.

### `conflict.resolve`
- Used when the Merge-Arbitration Engine (or equivalent) must resolve a conflict.
- Supports strategies: `ours`, `theirs`, `merge`, `custom`.

### `tool.invoke`
- Explicit tool calling with `tool_call_id`, `tool_name`, and `arguments`.

### `task.approve`
- Human-in-the-loop approval/rejection gate.
- Carries `approved` boolean and `comment`.

### `PlanPresentation`
- Leader presents a structured plan (DAG of steps) for human review.
- Directly supports the contract negotiation and approval gates in the Heavy-Adversarial pattern.

### `ArenaResult`
- Reports the outcome of an adversarial evaluation round.
- Includes execution metrics (`duration_ms`, `cpu_utilization`, `memory_bytes`) and error details.

---

## Error Taxonomy

The specification includes a rich, actionable error taxonomy. Key categories relevant to long-running adversarial harnesses:

- **PROHIB-*** (sandbox / policy violations)
  - Non-retryable without policy change.
  - Immediate local suspension.

- **AUTH-*** (signature / token problems)
  - Retryable after key rotation or token refresh.
  - Requires re-authentication.

- **RISK-*** (resource exhaustion or high-risk operations)
  - Retryable after human approval (`task.approve`) or backoff.
  - Often requires state rollback to last approved checkpoint.

These error semantics map cleanly to Korg’s `CONTESTED` state, productive death, and rollback via `.ktrans`.

---

## Korg Integration Notes

This wire format is a natural evolution of the ACP described in the earlier reference-harness notes.

- **JCS + signatures** provide the verifiable provenance that our `.ktrans` + blackboard model assumes.
- **PlanPresentation** + **task.approve** directly support the explicit contract negotiation step in the Heavy-Adversarial Hybrid pattern.
- **ArenaResult** + structured metrics give the Evaluator persona concrete data for harsh, contract-based grading.
- The error taxonomy supplies precise state-invalidation rules that can drive our `CONTESTED` transitions and re-dispatch logic.

The current Rust skeleton (`grok-acp-harness`) uses a simplified JSON-over-stdio version. Future increments should align the `acp.rs` module with the JCS + signature rules defined here.

---

## Related

- [[wiki/reference-harness/ACP-Binding-Design.md]] — Higher-level binding and message intent.
- [[wiki/reference-harness/ACP-Message-Schema.md]] — Earlier, less formal schema work.
- [[wiki/reference-harness/Minimal-ACP-Client-Pseudocode.md]] — Pseudocode that can now target this wire format.
- [[wiki/patterns/Heavy-Adversarial-Hybrid-Harness.md]] — Strategic pattern that relies on the contract negotiation and adversarial evaluation messages defined here.
- [[wiki/reference-harness/Grok-4.20-Heavy-Leader-Process-and-ACP.md]] — Ground-truth architecture this protocol is designed to support.
- [[reference-implementations/rust/grok-acp-harness/]] — Current executable reference implementation (to be evolved toward v1.17 compliance).

---

*This note was created 2026-05-20 as the canonical reference for ACP v1.17 wire format within the Korg harness layer.*