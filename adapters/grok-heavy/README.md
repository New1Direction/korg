# grok-heavy-adapter

Translate Grok Heavy NDJSON traffic into korg `AgentToolCall` events.

## What this proves

Grok Heavy is a 16-agent symmetric multi-agent system. It's the adversarial
stress test for korg's single-parent `triggered_by` causal model: if a fan-out
of 16 concurrent agents under one user prompt round-trips through the ledger
without losing causal structure, the model holds.

## Causal mapping

```
user_prompt (triggered_by=None, source=agent:grok-heavy@orchestrator)
  ├─ llm_inference (source=agent:grok-heavy-grok@4-heavy)         tools from Grok ⇒ children
  ├─ llm_inference (source=agent:grok-heavy-agent-1@4-heavy)      tools from Agent 1 ⇒ children
  ├─ ...
  └─ llm_inference (source=agent:grok-heavy-agent-15@4-heavy)     tools from Agent 15 ⇒ children
```

All 16 agents are **siblings** under the root. Tool calls are children of
their originating agent's `llm_inference`.

## Known limitation: ingest order is not wall-clock concurrency

The 16 agents run concurrently on Grok's side. The adapter emits their events
serially through `emit()`, so the ledger records ingest order — not actual
parallelism. Anyone replaying a Grok session from the ledger reproduces a
topological order consistent with `triggered_by`, but not the original timing.

This is fine for causal reasoning ("what depended on what") and incorrect for
performance reasoning ("which agent finished first"). If wall-clock fidelity
becomes a real need, the answer is HLC timestamps per agent inference, not
reordering the ledger.

## Known limitation: chatroom_send cross-edges

Grok's `chatroom_send` tool lets agents message each other. The recipient is
named in `args.to`. When Grok receives a message and takes another inference
turn, that turn is causally triggered by the message — a cross-agent edge.

korg's `triggered_by` is a single parent. v1 records `chatroom_send` as a
normal tool call with the recipient in args. Cross-agent causal edges are
**queryable** (find all chatroom_send events where args.to=X, find Grok's
next llm_inference) but **not structural** (no second `caused_by` field).

If multi-agent becomes common, the natural extension is a `caused_by: [seq_ids]`
array on `llm_inference` events. Don't extend the schema until there's a
second multi-agent system in production. One example is not a generalization.

## Usage

```python
from grok_heavy_adapter import GrokHeavyAdapter

def emit(body):
    r = requests.post("http://localhost:8080/api/agent/tool-call", json=body)
    return r.json()["seq_id"]

with open("grok_session.ndjson") as f:
    stats = GrokHeavyAdapter(emit).ingest(f)
```

## Frame input shape

A stream of NDJSON lines from `POST /_data/v1/a/t/`. Each line is an envelope:

```json
{"result": {"response": {<variant>}}}
```

The parser only consumes the variants it needs (`userResponse`, `uiLayout`,
streaming `token`). Anything else is skipped — feed unfiltered captures.
