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

## Known Limitations & Roadmap

### 1. Synthesized Prompt Root (For Non-Interactive CLI Runs)
* **v1.1 limitation**: When Claude Code is run non-interactively using the CLI command parameter (e.g. `claude -p "prompt"`), the original CLI prompt is **not present** in the stream-json output. To preserve the causal backward walk root invariant (spec §7.6), the adapter synthesizes an honest placeholder root event:
  `"prompt": "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]"`
* **interactive runs**: For interactive sessions, any user plain text turns *are* correctly captured and emitted as real, non-synthesized `user_prompt` root events.
* **v1.2 fix**: Capture CLI prompts directly from the shell invocation context (via environment variables or custom shell aliases).

### 2. Payload Refs Label Disambiguation
* **v1.1 limitation**: Large tool outputs currently register their `payload_refs` label matching the tool name (e.g. `label: "Bash"`). Disambiguating multiple large bash tool runs requires manual digest correlation.
* **v1.2 fix**: Capture and append descriptive labels like `"Bash:stdout"`, `"Bash[ls -la]"`, or `"<seq_id>:Bash"`.

---

## Verification and Testing

Execute the robust unit test suite containing 12 strict causal and functional invariants:

```bash
python3 -m unittest discover -s adapters/stream-json/tests -p "test_*.py"
```
