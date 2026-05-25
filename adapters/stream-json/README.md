# Korg stream-json Passive Auditing Adapter (v1.1)

The Korg `stream-json` adapter enables **fully passive, zero-cooperation auditing** for Claude Code sessions. 

Instead of modifying the agent's system prompt or configuring Korg as an MCP tool, the adapter reads Claude Code's standard `--output-format stream-json --verbose` output on `stdin` in real-time, parses its execution trace into a causally ordered sequence of `AgentToolCall` events, and POSTs them to Korg's HTTP Ingestion API.

---

## Architecture and Mechanics

1. **Non-Blocking Serialized Ingestion (Spec §7.5)**: 
   The adapter parses the standard input stream in the main thread and instantly places mapped events onto a thread-safe, locked, bounded `queue.Queue(maxsize=256)`. If Korg is offline or latency spikes, a drop-oldest policy protects the agent's memory, and a background daemon thread handles HTTP serialization without blocking the stream.
   
2. **Race-Free Local Causality**:
   Because events are posted asynchronously, relying on Korg's server-assigned IDs during parsing introduces race conditions. The adapter solves this by maintaining a logical clock counter (`local_seq_id`) synchronously during parsing, forming the causal `triggered_by` links on the main thread, and translating them into real server sequence IDs inside the worker thread before posting.
   
3. **Monotonic Latency Measurement**:
   Tool execution durations are calculated using Python's `time.monotonic()` clock instead of wall-clock time, ensuring that clock drift, manual adjustments, or NTP updates never corrupt duration measurements.
   
4. **Canonical Hashing & Blob-First Atomicity (Spec §3 & §7.2)**:
   Any string or JSON value in arguments or results exceeding 1KB is automatically extracted, canonically serialized with alphabetically sorted keys (`sort_keys=True`), hashed via SHA-256, and written to `.korg/blobs/<prefix>/<sha256>` *before* the parent event is sent to the server.

---

## Installation

Ensure you have the standard `requests` dependency installed:

```bash
pip install requests
```

---

## Usage

Pipe the verbose JSON stream of Claude Code directly into the adapter script:

```bash
claude -p --verbose --output-format stream-json "list files in this directory" | python3 adapters/stream-json/korg_stream_adapter.py
```

*Note: The `--verbose` flag is required by Claude Code when using `--output-format=stream-json`.*

### Operational Logs
* **unknown_events.log**: Any unrecognized, unmapped event types (e.g., `rate_limit_event`, or future CLI updates) are appended here for off-line audit review.
* **stderr**: Loud warnings are emitted to `stderr` if the stream begins without a system init block or if a mid-stream re-init is detected.

---

## Verification and Testing

Execute the robust unit test suite containing 10 strict causal and functional invariants:

```bash
python3 -m unittest discover -s adapters/stream-json/tests -p "test_*.py"
```
