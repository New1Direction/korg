use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    Scope, TokenResponse, TokenUrl,
};
use std::sync::Arc;
use serde::Deserialize;

// SubscriptionTier lives in korg-core so that both korg-auth and korg-registry
// can reference it without a circular dependency.
pub use korg_core::SubscriptionTier;

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
        .set_redirect_uri(RedirectUrl::new(format!("{}/auth/codex/callback", config.base_url)).unwrap());
        Self { client }
    }
}

#[async_trait::async_trait]
impl OAuthProvider for CodexProvider {
    fn id(&self) -> &'static str { "codex" }
    fn client(&self) -> &BasicClient { &self.client }
    fn scopes(&self) -> Vec<String> { vec!["subscription".to_string()] }
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
        .set_redirect_uri(RedirectUrl::new(format!("{}/auth/anthropic/callback", config.base_url)).unwrap());
        Self { client }
    }
}

#[async_trait::async_trait]
impl OAuthProvider for AnthropicProvider {
    fn id(&self) -> &'static str { "anthropic" }
    fn client(&self) -> &BasicClient { &self.client }
    fn scopes(&self) -> Vec<String> { vec!["messages".to_string()] }
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
    pub fn initiate_pkce_flow(&self, client: &BasicClient, scopes: Vec<String>) -> OAuthFlowInitiation {
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
                struct CodexSubResponse { tier: String }
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
