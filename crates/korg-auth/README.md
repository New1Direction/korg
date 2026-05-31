# korg-auth

OAuth login, encrypted token storage, and subscription-tier resolution for korg.

`korg-auth` owns the "who is this user and what may they do" side of the korg
HTTP server. It runs the PKCE OAuth flows against upstream identity providers
(Codex, Anthropic), persists the resulting tokens to an encrypted on-disk store,
and resolves each user's `SubscriptionTier`. It does **not** decide capability
access itself — that lives in `korg-registry`, which reads the
`SubscriptionTier` this crate produces.

> [!NOTE]
> This crate provides the auth machinery (flows, storage, refresh
> coordination). The HTTP routes that drive it — `/auth/{provider}/callback`,
> session middleware, token-refresh-on-expiry — live in `korg-server`, and the
> CLI login flow lives in the top-level `korg` binary (`src/main.rs`). The
> provider client IDs/secrets default to `mock-*` values (see below), so the
> flows are runnable end-to-end against the mock path without real credentials.

## Role in the workspace

```
korg-core        →  defines SubscriptionTier (shared, no cycles)
      ▲
korg-auth        →  OAuth flow + encrypted token store → produces a UserSession
      ▲                                                    (carries SubscriptionTier)
korg-server      →  wires the auth routes, session middleware, refresh-on-expiry
korg (binary)    →  drives the interactive CLI login flow
```

`SubscriptionTier` intentionally lives in `korg-core`, not here, so that both
`korg-auth` (which stores it on `UserSession`) and `korg-registry` (which gates
capabilities on it) can depend on the same type without a circular dependency.
`korg-auth` re-exports it as `korg_auth::providers::SubscriptionTier` for
convenience.

## Modules

### `lib.rs` — config & shared state

- **`AuthConfig`** — base URL, per-provider client ID/secret, and the
  token-store path. `AuthConfig::from_env()` reads `KORG_BASE_URL`,
  `CODEX_CLIENT_ID`/`CODEX_CLIENT_SECRET`,
  `ANTHROPIC_CLIENT_ID`/`ANTHROPIC_CLIENT_SECRET`, each falling back to a
  `mock-*` default, and pins the store to `.korg/auth.json`.
- **`AuthState`** — the handle the server holds (`Arc<AuthState>`). Bundles the
  `AuthProviders`, the `JsonTokenStore`, and the `SingleflightRefresher`.
  Construct with `AuthState::new(config)`; the `config` field is crate-private
  so callers go through the sub-types.
- **`SingleflightRefresher`** — deduplicates concurrent token refreshes per
  `user_id`. The first caller for a given user runs the refresh; concurrent
  callers for the same user park on a `tokio::sync::watch` channel and receive
  the same refreshed `UserSession` (or an error) rather than each hammering the
  IdP. Used by `korg-server`'s refresh-on-expiry path.

### `providers.rs` — OAuth / PKCE flows

- **`AuthProviders`** — holds the `oauth2::BasicClient` for each provider
  (`codex_client`, `anthropic_client`) plus an in-memory map of in-flight PKCE
  verifiers keyed by CSRF state. `AuthProviders::new` **panics** if
  `config.base_url` fails `validate_base_url` — a deliberate fail-fast so a
  misconfigured or attacker-controlled origin can't be spliced into an OAuth
  callback URI.
- **`initiate_pkce_flow(client, scopes)`** — generates a fresh SHA-256 PKCE
  challenge + random CSRF state and returns an **`OAuthFlowInitiation`**
  (`authorize_url`, `csrf_state`, `pkce_verifier`).
- **`save_pending_pkce(state, verifier)` / `take_pending_pkce(state)`** — stash
  and one-shot-retrieve the PKCE verifier across the redirect. Entries carry an
  insertion timestamp and expire after `PKCE_ENTRY_TTL` (300s, the RFC 6749
  auth-code lifetime); reads sweep expired entries so abandoned flows don't
  accumulate.
- **`verify_codex_subscription(access_token)`** — resolves a `SubscriptionTier`.
  Returns `Premium` for any `mock-` token (the dev path); otherwise GETs the
  Codex subscription endpoint and maps the response, defaulting to `Standard`
  on any error or unrecognized tier.
- `validate_base_url` enforces that the OAuth origin is `https://` (or
  `http://` only for `localhost`/`127.0.0.1`/`::1`), origin-only (no
  path/query/fragment), and free of `..` traversal. Covered by unit tests.

### `store.rs` — encrypted token store

- **`UserSession`** — the persisted unit: `user_id`, the Codex and Anthropic
  access tokens, an optional `refresh_token`, `expires_at`, and the resolved
  `SubscriptionTier`.
- **`JsonTokenStore`** — an encrypted, file-locked JSON store at the configured
  path (`save_session`, `load_session`; `delete_session` exists but is
  crate-private pending a confirmed logout caller). All access goes through an
  `fs2` advisory file lock (exclusive for writes, shared for reads), and on Unix
  the file is forced to mode `0600` on create and migrated to `0600` on the next
  write.
- **Encryption** — AES-256-GCM with a key derived via PBKDF2-HMAC-SHA256 from
  the `KORG_MASTER_KEY` env var. There is no fallback key: if `KORG_MASTER_KEY`
  is unset, store access **panics** by design. The current on-disk format
  (`KOA2`, v2) uses a fresh per-blob salt + nonce and 600,000 iterations (OWASP
  2026 minimum). The legacy v1 format (hardcoded salt, 10k iterations) is still
  *readable* and is transparently rewritten as v2 on the next `save_session`.

## Usage

The server wires this crate roughly as follows (condensed from
`korg-server`/`src/main.rs`):

```rust
use std::sync::Arc;
use korg_auth::{AuthConfig, AuthState};

// KORG_MASTER_KEY must be set in the environment, e.g.:
//   export KORG_MASTER_KEY=$(openssl rand -hex 32)
let auth = Arc::new(AuthState::new(AuthConfig::from_env()));

// 1. Begin a login: mint a PKCE challenge + CSRF state and remember the verifier.
let flow = auth
    .providers
    .initiate_pkce_flow(&auth.providers.codex_client, vec!["subscription".into()]);
auth.providers.save_pending_pkce(flow.csrf_state.clone(), flow.pkce_verifier);
// → redirect the user to flow.authorize_url

// 2. On the OAuth callback, recover the verifier by the returned `state`,
//    exchange the code for tokens (via the oauth2 client), then:
let tier = auth.providers.verify_codex_subscription(&access_token).await;
auth.store.save_session(korg_auth::store::UserSession {
    user_id: "user-123".into(),
    codex_access_token: access_token,
    anthropic_access_token: String::new(),
    subscription_tier: tier,
    refresh_token: None,
    expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
})?;

// 3. Later, refresh expired tokens without stampeding the IdP:
let fresh = auth
    .refresher
    .execute_refresh("user-123", || async move { /* do the refresh, return UserSession */ })
    .await?;
```

## Status & gaps

- **Mock-first by default.** With unset env vars the provider client
  IDs/secrets are `mock-*` and any `mock-` access token resolves to `Premium`.
  This is what the demo and test paths exercise; real upstream OAuth requires
  setting the real `*_CLIENT_ID`/`*_CLIENT_SECRET` env vars.
- **`verify_codex_subscription` only.** There is a Codex subscription-tier
  lookup but no Anthropic equivalent; the Anthropic provider exists for the
  OAuth/token flow only.
- **`delete_session` is crate-private.** The logout/delete path is implemented
  and tested but not yet exposed (`pub(crate)`) — promote to `pub` when a
  caller lands.
- **Token store, not a user database.** Sessions are keyed by `user_id` in a
  single encrypted JSON file under a process-advisory lock. This matches korg's
  v1 "local, single-user workspace" trust boundary (see the workspace README);
  it is not a multi-tenant credential service.

## Tests

```bash
cargo test -p korg-auth
```

Unit tests cover `validate_base_url` (scheme/host/path/traversal rejection) and
the store's crypto: v2 round-trip with fresh salts, legacy v1 decryption, and
tamper/corrupted-magic rejection. The store tests set a throwaway
`KORG_MASTER_KEY` themselves.
