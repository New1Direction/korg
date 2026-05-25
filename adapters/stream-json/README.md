# Korg stream-json Passive Auditing Adapter (v1.2)

The Korg `stream-json` adapter enables **fully passive, zero-cooperation auditing** for Claude Code sessions, including sub-agent (Task tool) spawning.

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

5. **Sub-agent causal chains (v1.2, Spec §2b)**:
   Claude Code's Task tool (wire name: `Agent`) spawns sub-agents whose events carry a `parent_tool_use_id` field. The adapter routes these events to a per-`parent_tool_use_id` causality bucket. The sub-agent's first user_prompt gets `triggered_by` = the main spine's last `llm_inference` seq_id (cross-spine link). Subsequent sub-agent events use the sub-spine's own causality chain. Source agents are disambiguated: `agent:claude-code/main@{version}` for the main agent, `agent:claude-code/sub-{ptuid[:8]}@{version}` for each sub-agent.

6. **Dynamic tool recognition (v1.2)**:
   The adapter now populates its recognized tool set from the `system/init` event's `tools` list rather than a hardcoded allowlist. The sole wire/display name mismatch (as of Claude Code 2.1.150) is `"Agent"` (wire) vs `"Task"` (display name); the adapter handles this mapping explicitly.

7. **Streaming assistant event deduplication (v1.2)**:
   Claude Code's `--verbose` stream can deliver the same logical assistant turn as multiple events with the same `message.id`. The adapter deduplicates by `message.id`: only the first occurrence emits an `llm_inference` event; subsequent deltas only buffer new tool_uses.

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
* **unknown_events.log**: Any unrecognized, unmapped event types are appended here for off-line audit review.
* **stderr**: Loud warnings are emitted to `stderr` if the stream begins without a system init block or if a mid-stream re-init is detected.

---

## System Event Subtype Decisions

The following `system` event subtypes are received from Claude Code but have explicit handling decisions:

| Subtype | Decision | Rationale |
|---|---|---|
| `init` | **Audited** | Session metadata, version, tool list |
| `hook_started` | **Intentionally-not** | Hook lifecycle noise — not agent behavior |
| `hook_response` | **Intentionally-not** | Hook lifecycle noise — not agent behavior |
| `task_started` | **Intentionally-not** | Task lifecycle — redundant with sub-agent events |
| `task_notification` | **Intentionally-not** | Task completion notice — redundant with Agent tool_result |
| `task_progress` | **Deferred** | May emit `ProgressUpdate` events in v1.3 |

Top-level `rate_limit_event` is also silently dropped (rate-limit noise; not agent behavior).

---

## Wire Name vs Display Name: Agent / Task

Claude Code's Task tool (the sub-agent spawning mechanism) has a **wire name / display name mismatch** as of v2.1.150:

- **Display name** (shown in UI, documentation): `Task`
- **Wire name** (appears in `tool_use.name` in stream-json): `Agent`

The `system/init` event lists `"Task"` in its `tools` array. When assistant events arrive containing a `tool_use` block, the name is `"Agent"`. The adapter bridges this gap: a tool named `"Agent"` is recognized as known when `"Task"` appears in `known_tools`.

---

## Known Limitations & Roadmap

### 1. Synthesized Prompt Root (For Non-Interactive CLI Runs)
* **v1.1/v1.2 limitation**: When Claude Code is run non-interactively using the CLI command parameter (e.g. `claude -p "prompt"`), the original CLI prompt is **not present** in the stream-json output. To preserve the causal backward walk root invariant (spec §7.6), the adapter synthesizes an honest placeholder root event:
  `"prompt": "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]"`
* **interactive runs**: For interactive sessions, any user plain text turns *are* correctly captured and emitted as real, non-synthesized `user_prompt` root events.
* **v1.2 fix**: Capture CLI prompts directly from the shell invocation context (via environment variables or custom shell aliases).

### 2. Payload Refs Label Disambiguation
* **v1.1/v1.2 limitation**: Large tool outputs currently register their `payload_refs` label matching the tool name (e.g. `label: "Bash"`). Disambiguating multiple large bash tool runs requires manual digest correlation.
* **v1.3 fix**: Capture and append descriptive labels like `"Bash:stdout"`, `"Bash[ls -la]"`, or `"<seq_id>:Bash"`.

---

## Verification and Testing

Execute the robust unit test suite containing 13 strict causal and functional invariants:

```bash
python3 -m unittest discover -s adapters/stream-json/tests -p "test_*.py"
```
