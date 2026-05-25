pub mod providers;
pub mod store;

use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub base_url: String,
    pub codex_client_id: String,
    pub codex_client_secret: String,
    pub anthropic_client_id: String,
    pub anthropic_client_secret: String,
    pub token_store_path: std::path::PathBuf,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let base_url = std::env::var("KORG_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:8080".to_string());
        let codex_client_id = std::env::var("CODEX_CLIENT_ID")
            .unwrap_or_else(|_| "mock-codex-client-id".to_string());
        let codex_client_secret = std::env::var("CODEX_CLIENT_SECRET")
            .unwrap_or_else(|_| "mock-codex-client-secret".to_string());
        let anthropic_client_id = std::env::var("ANTHROPIC_CLIENT_ID")
            .unwrap_or_else(|_| "mock-anthropic-client-id".to_string());
        let anthropic_client_secret = std::env::var("ANTHROPIC_CLIENT_SECRET")
            .unwrap_or_else(|_| "mock-anthropic-client-secret".to_string());

        let token_store_path = std::path::PathBuf::from(".korg/auth.json");

        Self {
            base_url,
            codex_client_id,
            codex_client_secret,
            anthropic_client_id,
            anthropic_client_secret,
            token_store_path,
        }
    }
}

pub struct AuthState {
    // Not pub: external callers should not reach into config fields directly.
    // Construct via AuthState::new(config) and use the sub-types (store, providers).
    pub(crate) config: AuthConfig,
    pub providers: Arc<providers::AuthProviders>,
    pub store: Arc<store::JsonTokenStore>,
    pub refresher: Arc<SingleflightRefresher>,
}

impl AuthState {
    pub fn new(config: AuthConfig) -> Self {
        let providers = Arc::new(providers::AuthProviders::new(&config));
        let store = Arc::new(store::JsonTokenStore::new(config.token_store_path.clone()));
        let refresher = Arc::new(SingleflightRefresher::new());
        Self {
            config,
            providers,
            store,
            refresher,
        }
    }
}

pub struct SingleflightRefresher {
    in_flight: tokio::sync::Mutex<std::collections::HashMap<String, tokio::sync::watch::Receiver<Option<store::UserSession>>>>,
}

impl SingleflightRefresher {
    pub fn new() -> Self {
        Self {
            in_flight: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub async fn execute_refresh<F, Fut>(
        &self,
        user_id: &str,
        refresh_op: F,
    ) -> Result<store::UserSession, anyhow::Error>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<store::UserSession, anyhow::Error>>,
    {
        let mut lock = self.in_flight.lock().await;

        if let Some(mut rx) = lock.get(user_id).cloned() {
            drop(lock);
            while rx.changed().await.is_ok() {
                if let Some(session) = rx.borrow().clone() {
                    return Ok(session);
                }
            }
            return Err(anyhow::anyhow!("Coordinated refresh failed in parallel thread."));
        }

        let (tx, rx) = tokio::sync::watch::channel(None);
        lock.insert(user_id.to_string(), rx);
        drop(lock);

        let result = refresh_op().await;

        let mut lock = self.in_flight.lock().await;
        lock.remove(user_id);

        match result {
            Ok(refreshed_session) => {
                let _ = tx.send(Some(refreshed_session.clone()));
                Ok(refreshed_session)
            }
            Err(err) => {
                let _ = tx.send(None);
                Err(err)
            }
        }
    }
}
