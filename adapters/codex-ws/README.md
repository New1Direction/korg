# codex-adapter

Translate OpenAI Codex CLI WebSocket frames into korg `AgentToolCall` events.

## What this proves

Codex is the most architecturally different production coding agent from korgex:
WebSocket transport (not HTTP), OpenAI tool-shape (not Anthropic), `custom_tool_call`
for `apply_patch` (freeform text, not JSON args). If a Codex session round-trips
through korg's ledger with the same causal shape korgex produces, the universal-
infrastructure claim is real.

## Usage

```python
from codex_adapter import CodexAdapter

# emit(body) → seq_id. Wire this to korg however you want.
def emit(body):
    r = requests.post("http://localhost:8080/api/agent/tool-call", json=body)
    return r.json()["seq_id"]

adapter = CodexAdapter(emit, source_agent="agent:codex@gpt-5.4")
stats = adapter.ingest(frames)
print(stats)
```

## Frame input shape

Each frame is a dict `{"direction": "in"|"out", "frame": <ws_payload>}`.
`"out"` = client→server (e.g. `response.create`).
`"in"` = server→client (`response.completed`, `response.output_item.done`, ...).

The parser only reads the frames it needs. Anything else is ignored, so a
mitmproxy WS dump can be fed in unfiltered.

## Causal mapping

| Codex frame | korg event | triggered_by |
|---|---|---|
| `response.create` with new user message | `user_prompt` | None |
| `response.completed` | `llm_inference` | prior llm or user_prompt |
| `response.output_item.done` (function_call / custom_tool_call) | tool name verbatim | current llm |

Tool results are recovered from the *next* `response.create.input` (matched by `call_id`).

## Not handled (intentional)

- `obfuscation` field — irrelevant to causality.
- Streaming `output_text.delta` chunks — only final text matters for the ledger.
- `codex.rate_limits` — out of scope.
- `previous_response_id` cross-session linking — single-session for v1.
