# korg-llm

Vendor-agnostic LLM provider layer for korg — one `LlmProvider` trait over Anthropic, OpenAI, Grok, and local Ollama, wrapped in retry/circuit-breaker/budget resilience.

`korg-llm` is the single seam through which the rest of the workspace talks to a
language model. The orchestration crates (`korg-runtime`'s Leader, Workers,
personas, and agent loop) never touch an HTTP API directly — they build a
provider from config and call `complete` / `complete_stream`. Swapping vendors,
adding failover, or running fully offline against a mock is a config change, not
a code change.

It is a pure HTTP implementation: no vendor SDKs. Every provider is built on
`reqwest` + `serde_json`, including server-sent-event streaming, which keeps the
dependency surface small and the wire format inspectable.

## What it provides

All public types live in `src/lib.rs` (single module).

### Core request/response model

A normalized message shape that every provider serializes into its own wire
format:

- `Role` (`System` / `User` / `Assistant` / `Tool`), `Message`, `ToolCall`,
  `FunctionCall`, `ToolDefinition` (JSON-schema parameters), and
  `MultiModalContent::Image`.
- `LlmRequest` — messages plus generation params (`temperature`, `max_tokens`,
  `top_p`, penalties, `stop_sequences`, `tools`, `multimodal`, `response_format`)
  and optional provenance metadata (`tx_id`, `session_id`, `policy_hash`).
  `response_format = Some("json_object")` asks OpenAI-compatible providers for
  strict JSON; other providers ignore it.
- `LlmResponse` / `LlmDelta` (streaming chunk), `TokenUsage`, `FinishReason`,
  and the `LlmError` enum (`Http`, `Timeout`, `RateLimit`, `Auth`, `Parser`,
  `Network`, `CircuitBreakerOpen`, `Unknown`).

### The provider trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
    async fn complete_stream(&self, req: LlmRequest)
        -> Result<Pin<Box<dyn Stream<Item = Result<LlmDelta, LlmError>> + Send>>, LlmError>;
}
```

### Concrete providers

- **`OpenAIProvider`** — `/chat/completions`, OpenAI tool schema, SSE streaming.
- **`AnthropicProvider`** — `/messages`. Hoists `System` messages into the
  top-level `system` field, maps `tools` → `input_schema`, forces a default
  `max_tokens` (Anthropic requires it), and parses `text` / `tool_use` content
  blocks. Note: the request serializer currently collapses non-system roles to
  `user`/`assistant` and does not re-emit prior `tool` results as Anthropic
  tool-result blocks.
- **`GrokProvider`** and **`LocalOllamaProvider`** — thin wrappers that
  delegate to `OpenAIProvider` with vendor-specific base URLs/models
  (`api.x.ai`, `localhost:11434`). Ollama uses a dummy API key.
- **`MockProvider`** — offline provider for tests; queue canned
  `Result<LlmResponse, _>` values via `set_response`, or stream deltas via
  `set_stream_deltas`. Falls back to echoing the last user message.
- **`DeterministicProvider`** — hermetic, fixture-only offline provider
  (`src/deterministic.rs`). Produces reproducible, role-shaped artifacts for a
  known fixture task and an honest null (empty mutations, low confidence) for
  anything else — it never fabricates. Selected via `default_llm = "deterministic"`
  and used by the honest `korg run-once` pipeline.

### Resilience decorators

- **`RotatorProvider`** — free-tier failover. Holds a list of
  `RotatorCandidateState`, tries them in order, and on failure places a
  candidate on a 60s cooldown. Cooldowns are **persisted to disk and
  cross-process**: `.korg/rotator_cooldowns.json`, guarded by an `fs2` file
  lock plus an in-process mutex, with an expiry/clock-drift sweep on read.
- **`ResilientLlmProvider`** — the outermost wrapper applied by the factory.
  Adds exponential backoff (config-driven retries/delays), a `CircuitBreaker`
  (5 consecutive failures → 10s open, then half-open), per-request and
  per-campaign **token budget enforcement** (errors as `RateLimit` when
  exceeded), and an optional `SemanticLlmCache`.
- **`SemanticLlmCache`** — despite the name, this is an **exact-match** cache:
  it SHA-256-hashes the request (messages + generation params) and stores
  responses in `.korg/semantic_llm_cache.json` under a file lock. There is no
  embedding/similarity matching here; identical requests hit, anything else
  misses. Disabled by default (`resilience.enable_semantic_cache`).
- Global metrics counters (atomics) read by the runtime/TUI:
  `CAMPAIGN_TOKENS`, `ROTATOR_HITS`, `HEALS_RESOLVED`, `COMPLETIONS_COUNT`,
  `TOTAL_LATENCY_MS`.

### Config & factory

- **`KorgConfig`** — resolved runtime config. `from_env()` reads env vars only;
  `load()` layers a `korg.toml` (project-local, else the platform config dir)
  underneath env-var overrides. Carries provider keys/URLs, per-persona
  overrides, rotator candidates, `ResilienceConfig`, and the security policy
  structs (`VisionPolicyConfig`, `PathsPolicyConfig`, `NetworkPolicyConfig`,
  `TokensPolicyConfig`). The `Toml*` structs are the deserialization mirror.
- **`build_provider(&KorgConfig)`** — builds the default provider (selected by
  `default_llm`: `openai` / `anthropic` / `grok` / `ollama` / `rotator` /
  `deterministic`, else `mock`), already wrapped in `ResilientLlmProvider`.
- **`build_provider_for_persona(&KorgConfig, name)`** — applies a
  `[personas.<name>]` override (provider/model/temperature) for the Leader,
  Workers (`captain`/`harper`/`benjamin`/`lucas`), and `evaluator`; returns the
  provider plus an optional temperature override.

> The security policy types (vision/paths/network/tokens) are **defined and
> parsed here**, but only the token budget is enforced inside this crate. The
> vision/paths/network policies are consumed by `korg-runtime` (e.g.
> `vision_policy.rs`); this crate is just their canonical home and config
> loader.

## Where it sits in the workspace

```
korg-core      (paths, primitives)
   └── korg-llm        ← this crate: provider trait, vendors, resilience, config
          ├── korg-runtime    (Leader/Workers/personas/agent call complete())
          ├── korg-server     (loads KorgConfig)
          └── korg-embeddings
```

- Depends only on **`korg-core`** within the workspace — it uses
  `korg_core::paths` to locate the `.korg/` directory for cooldown and cache
  files. It has no knowledge of the ledger, registry, or runtime.
- **`korg-runtime`** is the primary consumer: personas build providers via
  `build_provider_for_persona`, the agent loop drives `complete`/tool-calls,
  and the Leader reads the global token/latency counters for telemetry.
- **`korg-server`** loads `KorgConfig` to surface model/policy settings.
- Unlike `korg-bridge`, this crate is Rust-only and is **not** tied to the WAL
  or hash-chained journal — those concerns live in `korg-registry`. `LlmRequest`
  merely carries optional `tx_id` / `session_id` / `policy_hash` so callers can
  correlate a model call with a ledger entry they record themselves.

## Usage

```rust
use korg_llm::{build_provider, KorgConfig, LlmProvider, LlmRequest, Message, Role};

# async fn run() -> Result<(), korg_llm::LlmError> {
// Resolves korg.toml + env vars, then wraps the chosen vendor in
// retry + circuit-breaker + token-budget enforcement.
let config = KorgConfig::load();
let provider = build_provider(&config);

let req = LlmRequest {
    messages: vec![Message {
        role: Role::User,
        content: "Summarize the diff in one sentence.".into(),
        name: None,
        tool_calls: None,
    }],
    temperature: 0.2,
    max_tokens: Some(256),
    tools: None,
    stop_sequences: None,
    multimodal: None,
    tx_id: None,
    session_id: None,
    policy_hash: None,
    top_p: None,
    presence_penalty: None,
    frequency_penalty: None,
    response_format: None,
};

let resp = provider.complete(req).await?;
println!("{} ({} tokens)", resp.content, resp.usage.total_tokens);
# Ok(())
# }
```

Run fully offline by setting `KORG_DEFAULT_LLM=mock` (the default when no
provider is configured), or in tests construct a `MockProvider` directly.

## Tests

Unit tests are inline in `src/lib.rs` and run offline (no network):
payload serialization for OpenAI/Anthropic, retry/backoff exhaustion, rotator
failover and cooldown-skip, TOML parsing, and the SHA-256 cache round-trip.

```bash
cargo test -p korg-llm
```

## Status / gaps

- **Tool-call round-trips are one-directional.** Tool *definitions* and the
  model's tool *calls* are handled, but neither serializer re-encodes prior
  `Role::Tool` messages back into provider-native tool-result blocks, so
  multi-turn tool loops must be assembled by the caller.
- **Streaming tool calls** are parsed for OpenAI but not surfaced from the
  Anthropic stream (only text deltas and the final stop reason are emitted).
- **`SemanticLlmCache` is exact-match, not semantic** (see above) — the name is
  aspirational.
- **`MultiModalContent` is defined but not wired into request serialization** —
  images on `LlmRequest.multimodal` are not yet sent to any provider.
- `RotatorProvider` contains a no-op `default_model` branch
  (`build_provider_with`'s candidate model is applied at construction, not per
  request); harmless but dead.
- The module doc comment references a historical `src/llm.rs` path; the code now
  lives in `src/lib.rs`.

## License

MIT OR Apache-2.0.
