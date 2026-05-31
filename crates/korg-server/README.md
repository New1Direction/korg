# korg-server

Axum HTTP server, SSE telemetry stream, and OAuth/LLM proxy for korg.

`korg-server` is the web-facing front end of the workspace. It hosts a live campaign
(driving a `korg_runtime::leader::LeaderOrchestrator` in the background), streams that
campaign's telemetry to browsers over Server-Sent Events, exposes the capability journal
and runtime state as JSON, ingests tool-call events from external agents, and proxies
authenticated requests to the Anthropic Messages API. It's the HTTP counterpart to
`korg-tui` (which renders the same `TuiUpdate` stream in a terminal) and the HTTP
counterpart to `korg-bridge` (which lets in-process Python skip the network entirely).

## Role in the workspace

```
korg-runtime  ──TuiUpdate──▶  korg-server  ──SSE──▶  browser
   (leader)                       │
                                  ├── korg-registry  (CapabilityResolver / journal — single state authority)
                                  ├── korg-auth      (PKCE OAuth, encrypted token store, singleflight refresh)
                                  ├── korg-llm       (KorgConfig: vision redaction policy)
                                  ├── korg-embeddings(semantic code search)
                                  └── korg-core      (paths, metrics, SubscriptionTier)
```

The crate is a **library** (`korg-server` has no binary). The root `korg` binary
(`src/main.rs`) is the entry point; it calls the two public run functions and reuses
the OAuth callback handlers and `AppState` directly. There is no `main.rs` and no
standalone test directory — all tests are inline `#[cfg(test)]` units in `lib.rs`.

## Public API

Everything lives in `src/lib.rs`. The exported surface is small:

| Item | Kind | Purpose |
|:---|:---|:---|
| `run_web_with_campaign(prompt, session, mode)` | `async fn` | Spawns a fresh `LeaderOrchestrator` for `prompt`, wires its telemetry into a broadcast channel, builds the router, binds `0.0.0.0:8080`, and serves. |
| `run_web_with_leader(leader)` | `async fn` | Same, but attaches to a caller-supplied `LeaderOrchestrator` (reusing its existing `RuntimeCoordinator` and `CapabilityResolver`). |
| `AppState` | `struct` | Shared server state: the `TuiUpdate` broadcaster, the override feedback channel, the `Arc<Mutex<CapabilityResolver>>`, the optional `RuntimeCoordinator`, and the `korg_auth::AuthState`. |
| `oauth_codex_callback_handler` / `oauth_anthropic_callback_handler` | `async fn` | OAuth redirect callbacks, reused by the binary to run a standalone callback listener. |
| `CallbackQuery`, `AuthenticatedUser` | `struct` | OAuth callback query params; request extractor that resolves a `UserSession` from a `Bearer` header or `korg_session` cookie. |

Both run functions bind the same hardcoded `0.0.0.0:8080`, auto-open the system
browser after 500 ms, and register an identical route table.

## Endpoints

All routes are defined inline in the two `run_web_*` functions.

**Pages / assets**
- `GET /`, `/dashboard`, `/cockpit`, `/index.html` — all serve the same embedded
  `LANDING_HTML` (a static monochrome marketing/landing page baked into the binary).
- `GET /assets/hero-loop.mp4`, `/assets/hero-mesh.glb` — `include_bytes!`-embedded media.
- `GET /korg-frontend.js`, `/korg-frontend_bg.wasm` (+ `/static/...`) — **stubs**; the
  handlers return empty bodies with the right `Content-Type`. There is no bundled WASM
  frontend in this crate; the live UI is the SSE stream consumed by the landing page / `korg-tui`.

**Telemetry & state**
- `GET /api/events` — SSE stream. Each `TuiUpdate` from the broadcast channel is
  serialized to JSON and pushed as an event; keepalive comments fill lagged gaps.
- `GET /api/state` — current `blackboard.json` snapshot (from `korg_core::paths`),
  annotated with the resolver's `cognition_mode`.
- `GET /api/journal?triggered_by=<seq>` — last 100 capability events as NDJSON, optionally
  filtered to a causal subtree.
- `GET /api/metrics` — `korg_core::metrics::snapshot()` plus live `active_processes` /
  `remaining_retry_budget` from the coordinator.
- `GET /api/workspaces` — workspace-manager snapshot (state, persona, routing_id, path) + counters.
- `GET /api/capabilities`, `GET /api/projections/campaign` — resolver node graph / campaign projection.
- `GET /api/screenshots` — `korg_runtime::vision_policy::VISUAL_HISTORY`.
- `GET /api/diff` — `git diff HEAD` across the working tree and any `korg-branch-*` branches.

**Control**
- `POST /api/override`, `POST /api/input` — forward a `ContractResponse` (approve / reject /
  force / freeform override) back to the running leader via the feedback channel.
- `POST /api/mode` — request a cognition-mode transition through the `CapabilityResolver`
  (the resolver is the authority; the web layer mirrors back the mode it actually applied).
- `POST /api/capabilities/toggle` — submit a raw `korg_registry::TransitionRequest`.
- `POST /api/campaign/abort` — call `abort()` on the coordinator.
- `POST /api/semantic_search` — embed the query and rank code blocks against `.korg/index.json`
  (falls back to `FakeEmbeddingModel` if the Candle model can't load).

**External-agent ingestion**
- `POST /api/agent/tool-call` — accepts an `AgentToolCallRequest` from any agent runtime
  (korgex, Claude Code via MCP, etc.), appends it to the live journal with a fresh HLC
  timestamp, and returns the assigned `seq_id` for `triggered_by` chaining. This is the
  HTTP equivalent of what `korg-bridge` does in-process. Per `agent_event_spec.md`, it
  always uses `append_with_metadata` and preserves the caller's `triggered_by` exactly
  (including `None` for root events) so external causal chains don't get silently grafted
  onto internal governance events. `actor_id` is fixed to `"korg:api"` (recorder identity).
- `GET /api/blob/:sha256` — content-addressed blob fetch from `.korg/blobs/{sha256[:2]}/{sha256}`.
  This is the escape hatch for payloads that exceed the 10 MB MCP JSON-RPC cap. The sha256
  is validated as 64 lowercase hex chars before any filesystem access.

**Auth / LLM proxy**
- `GET /auth/login?provider=codex|anthropic` — starts a PKCE flow via `korg_auth` and redirects.
- `GET /auth/codex/callback`, `GET /auth/anthropic/callback` — exchange the auth code,
  persist the `UserSession` in the encrypted token store.
- `POST /api/v1/anthropic/messages` — authenticated proxy to `api.anthropic.com/v1/messages`.
  Requires an `AuthenticatedUser`; logs a `ProxyAuditTrail` ledger event (user, tier, model,
  estimated tokens/cost) before forwarding; performs proactive + reactive singleflight token
  refresh on expiry or upstream `401`; and forwards only a vetted allowlist of response headers
  (so a compromised upstream can't inject `Set-Cookie` / `Location`).

## Run it

There's no `korg-server` binary; drive it through the `korg` root crate:

```bash
# From the workspace root — launches the web dashboard on http://localhost:8080
# and opens a browser. This calls korg_server::run_web_with_campaign under the hood.
cargo run -- campaign --web --prompt "Refactor the auth layer to use JWTs"
```

Or embed the library directly:

```rust
// Drive a campaign and serve its telemetry on :8080
korg_server::run_web_with_campaign(
    "Optimize the database connection pool".to_string(),
    None,          // session id (None = fresh)
    Some("balanced"),
).await?;
```

## Notes, stubs & scope

- **Hardcoded bind / port.** Both run functions bind `0.0.0.0:8080` with no config knob,
  and serve over plain HTTP. Per the workspace README's trust-boundary note, korg is built
  for **local, single-user** use; binding `0.0.0.0` exposes workspace read/write (and the
  `/api/diff`, blob, and override endpoints) to anything on the network. There is no auth on
  the dashboard/telemetry/control routes — only the Anthropic proxy enforces `AuthenticatedUser`.
- **WASM frontend handlers are empty stubs.** `wasm_js_handler` / `wasm_bytes_handler` return
  zero-length bodies. The module doc comment still describes a "glassmorphism SPA," but in the
  current code every page route serves the same static `LANDING_HTML`.
- **Vision redaction.** Before broadcasting, `Ktrans` updates carrying `vision_attachments`
  marked `REDACTED`/`BLOCKED` have their image bytes swapped for a blackout PNG, unless
  `KorgConfig.security_vision.allow_raw_screenshots` is set.
- **Mock auth.** `AuthenticatedUser` has a dev/CI fallback gated behind *both*
  `cfg(debug_assertions)` and the `KORG_ALLOW_MOCK_AUTH` env var; the mock session is
  request-scoped and never persisted. Release builds can't reach this path.
- **`server` feature** is declared and on by default, but the code is not currently
  gated behind it.

## Tests

Inline `#[cfg(test)]` units in `lib.rs` cover the load-bearing logic, not the routing glue:
`actor_id` is always `korg:api` on ingested events, stale-token auto-refresh, singleflight
de-duplication of concurrent refreshes, and the proxy emitting a `ProxyAuditTrail` ledger event.
