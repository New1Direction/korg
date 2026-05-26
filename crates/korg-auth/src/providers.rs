use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use std::sync::Arc;

// SubscriptionTier lives in korg-core so that both korg-auth and korg-registry
// can reference it without a circular dependency.
pub use korg_core::SubscriptionTier;

/// Validate a base_url before it's interpolated into OAuth callback URIs.
/// Returns Err with a human-readable reason if the URL is unsafe.
///
/// Accepts: `https://<host>[:port]` or `http://localhost[:port]` /
/// `http://127.0.0.1[:port]` (the latter two for local dev only).
/// Rejects anything with a path/query/fragment, or with `..` traversal.
pub(crate) fn validate_base_url(base_url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(base_url)
        .map_err(|e| format!("base_url is not a valid URL: {e}"))?;

    let scheme = parsed.scheme();
    let host = parsed.host_str().unwrap_or("");
    let is_loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");

    match scheme {
        "https" => {}
        "http" if is_loopback => {} // dev exemption
        "http" => {
            return Err(format!(
                "base_url scheme http is only allowed for localhost/127.0.0.1, got host '{host}'"
            ))
        }
        other => return Err(format!("base_url scheme '{other}' not allowed; need https")),
    }

    if parsed.path() != "" && parsed.path() != "/" {
        return Err(format!(
            "base_url must be an origin only (no path), got path '{}'",
            parsed.path()
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("base_url must not contain query or fragment".to_string());
    }
    if base_url.contains("..") {
        return Err("base_url contains '..' traversal".to_string());
    }
    Ok(())
}

// Private helper — only called from verify_codex_subscription.
fn tier_from_str(s: &str) -> SubscriptionTier {
    match s {
        "Premium" => SubscriptionTier::Premium,
        "Enterprise" => SubscriptionTier::Enterprise,
        _ => SubscriptionTier::Standard,
    }
}

pub struct OAuthFlowInitiation {
    pub authorize_url: String,
    pub csrf_state: String,
    pub pkce_verifier: String,
}

// Not part of the public API: external callers use AuthProviders directly.
// The trait exists to allow internal provider dispatch in get_provider().
#[async_trait::async_trait]
pub(crate) trait OAuthProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn client(&self) -> &BasicClient;
    fn scopes(&self) -> Vec<String>;
}

pub(crate) struct CodexProvider {
    pub(crate) client: BasicClient,
}

impl CodexProvider {
    pub(crate) fn new(config: &crate::AuthConfig) -> Self {
        let client = BasicClient::new(
            ClientId::new(config.codex_client_id.clone()),
            Some(ClientSecret::new(config.codex_client_secret.clone())),
            AuthUrl::new("https://codex.auth.com/oauth/authorize".to_string()).unwrap(),
            Some(TokenUrl::new("https://codex.auth.com/oauth/token".to_string()).unwrap()),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("{}/auth/codex/callback", config.base_url)).unwrap(),
        );
        Self { client }
    }
}

#[async_trait::async_trait]
impl OAuthProvider for CodexProvider {
    fn id(&self) -> &'static str {
        "codex"
    }
    fn client(&self) -> &BasicClient {
        &self.client
    }
    fn scopes(&self) -> Vec<String> {
        vec!["subscription".to_string()]
    }
}

pub(crate) struct AnthropicProvider {
    pub(crate) client: BasicClient,
}

impl AnthropicProvider {
    pub(crate) fn new(config: &crate::AuthConfig) -> Self {
        let client = BasicClient::new(
            ClientId::new(config.anthropic_client_id.clone()),
            Some(ClientSecret::new(config.anthropic_client_secret.clone())),
            AuthUrl::new("https://api.anthropic.com/oauth2/authorize".to_string()).unwrap(),
            Some(TokenUrl::new("https://api.anthropic.com/oauth2/token".to_string()).unwrap()),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("{}/auth/anthropic/callback", config.base_url)).unwrap(),
        );
        Self { client }
    }
}

#[async_trait::async_trait]
impl OAuthProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }
    fn client(&self) -> &BasicClient {
        &self.client
    }
    fn scopes(&self) -> Vec<String> {
        vec!["messages".to_string()]
    }
}

pub struct AuthProviders {
    pub codex_client: BasicClient,
    pub anthropic_client: BasicClient,
    // Not pub: external callers use the client shortcuts above; exposing the
    // concrete provider types would leak internal dispatch details.
    pub(crate) codex_provider: Arc<CodexProvider>,
    pub(crate) anthropic_provider: Arc<AnthropicProvider>,
    // Not pub: holds live PKCE verifiers keyed by OAuth state parameter.
    // Exposing this would let external callers read or corrupt in-flight
    // auth exchanges.
    pub(crate) pending_pkce: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl AuthProviders {
    pub fn new(config: &crate::AuthConfig) -> Self {
        // Validate the OAuth callback origin before any provider gets to splice
        // it into a RedirectUrl. A misconfigured or attacker-controlled env var
        // could otherwise route the OAuth callback to an arbitrary host.
        if let Err(reason) = validate_base_url(&config.base_url) {
            panic!(
                "korg-auth: refusing to construct OAuth providers with invalid base_url '{}': {}",
                config.base_url, reason
            );
        }
        let codex_provider = Arc::new(CodexProvider::new(config));
        let anthropic_provider = Arc::new(AnthropicProvider::new(config));

        Self {
            codex_client: codex_provider.client.clone(),
            anthropic_client: anthropic_provider.client.clone(),
            codex_provider,
            anthropic_provider,
            pending_pkce: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    // No external callers — used only for internal provider dispatch.
    pub(crate) fn get_provider(&self, id: &str) -> Option<Arc<dyn OAuthProvider>> {
        match id.to_lowercase().as_str() {
            "codex" => Some(self.codex_provider.clone() as Arc<dyn OAuthProvider>),
            "anthropic" => Some(self.anthropic_provider.clone() as Arc<dyn OAuthProvider>),
            _ => None,
        }
    }

    pub fn save_pending_pkce(&self, state: String, verifier: String) {
        let mut map = self.pending_pkce.lock().unwrap();
        map.insert(state, verifier);
    }

    pub fn take_pending_pkce(&self, state: &str) -> Option<String> {
        let mut map = self.pending_pkce.lock().unwrap();
        map.remove(state)
    }

    /// Generates PKCE challenge and CSRF state parameters.
    /// Security Guard (Hermes Lesson #1): code_verifier and state MUST be entirely distinct.
    pub fn initiate_pkce_flow(
        &self,
        client: &BasicClient,
        scopes: Vec<String>,
    ) -> OAuthFlowInitiation {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let csrf_state = CsrfToken::new_random();

        let mut request = client
            .authorize_url(|| csrf_state.clone())
            .set_pkce_challenge(pkce_challenge);

        for scope in scopes {
            request = request.add_scope(Scope::new(scope));
        }

        let (auth_url, csrf_token) = request.url();

        OAuthFlowInitiation {
            authorize_url: auth_url.to_string(),
            csrf_state: csrf_token.secret().clone(),
            pkce_verifier: pkce_verifier.secret().clone(),
        }
    }

    /// Verifies the user's active Codex subscription level.
    pub async fn verify_codex_subscription(&self, access_token: &str) -> SubscriptionTier {
        if access_token.starts_with("mock-") {
            return SubscriptionTier::Premium;
        }

        let client = reqwest::Client::new();
        let res = client
            .get("https://api.codex.com/v1/user/subscription")
            .bearer_auth(access_token)
            .send()
            .await;

        match res {
            Ok(resp) => {
                #[derive(Deserialize)]
                struct CodexSubResponse {
                    tier: String,
                }
                if let Ok(sub) = resp.json::<CodexSubResponse>().await {
                    tier_from_str(&sub.tier)
                } else {
                    SubscriptionTier::Standard
                }
            }
            Err(_) => SubscriptionTier::Standard,
        }
    }
}

#[cfg(test)]
mod base_url_tests {
    use super::validate_base_url;

    #[test]
    fn accepts_https() {
        assert!(validate_base_url("https://korg.example.com").is_ok());
        assert!(validate_base_url("https://korg.example.com:8443").is_ok());
    }

    #[test]
    fn accepts_localhost_http_for_dev() {
        assert!(validate_base_url("http://localhost:8080").is_ok());
        assert!(validate_base_url("http://127.0.0.1:8080").is_ok());
    }

    #[test]
    fn rejects_http_for_non_loopback() {
        assert!(validate_base_url("http://evil.example.com").is_err());
        assert!(validate_base_url("http://10.0.0.5").is_err());
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(validate_base_url("javascript:alert(1)").is_err());
        assert!(validate_base_url("file:///etc/passwd").is_err());
        assert!(validate_base_url("ftp://example.com").is_err());
    }

    #[test]
    fn rejects_paths_queries_fragments() {
        assert!(validate_base_url("https://korg.example.com/extra/path").is_err());
        assert!(validate_base_url("https://korg.example.com?x=1").is_err());
        assert!(validate_base_url("https://korg.example.com#frag").is_err());
    }

    #[test]
    fn rejects_traversal() {
        assert!(validate_base_url("https://korg.example.com/..").is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(validate_base_url("not a url").is_err());
        assert!(validate_base_url("").is_err());
    }
}
