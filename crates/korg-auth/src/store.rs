use std::path::PathBuf;
use std::io::Seek;
use std::fs::{OpenOptions, File};
use std::io::{Read, Write};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use fs2::FileExt;

use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserSession {
    pub user_id: String,
    pub codex_access_token: String,
    pub subscription_tier: korg_core::SubscriptionTier,
    pub anthropic_access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Default)]
struct AuthDatabase {
    pub sessions: HashMap<String, UserSession>,
}

pub struct JsonTokenStore {
    // Not pub: the path is an implementation detail of the store.
    // Callers construct via JsonTokenStore::new(path) and use the session methods.
    pub(crate) path: PathBuf,
}

fn get_aes_cipher() -> Aes256Gcm {
    let master_password = std::env::var("KORG_MASTER_KEY")
        .unwrap_or_else(|_| "korg-fallback-secure-master-password-constant".to_string());

    let mut key = [0u8; 32];
    let salt = b"korg-auth-salt-constant";
    pbkdf2_hmac::<Sha256>(master_password.as_bytes(), salt, 10_000, &mut key);

    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
}

fn encrypt_payload(plain_text: &str) -> Result<Vec<u8>, anyhow::Error> {
    let cipher = get_aes_cipher();
    let nonce_bytes = rand::random::<[u8; 12]>();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher_text = cipher.encrypt(nonce, plain_text.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;
    let mut payload = nonce_bytes.to_vec();
    payload.extend(cipher_text);
    Ok(payload)
}

fn decrypt_payload(payload: &[u8]) -> Result<String, anyhow::Error> {
    let cipher = get_aes_cipher();
    if payload.len() < 12 {
        return Err(anyhow::anyhow!("Invalid payload format"));
    }
    let (nonce_bytes, ciphertext) = payload.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let decrypted_bytes = cipher.decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed (tampering or invalid master key): {:?}", e))?;
    Ok(String::from_utf8(decrypted_bytes)?)
}

impl JsonTokenStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn with_lock<F, R>(&self, write_mode: bool, op: F) -> Result<R, anyhow::Error>
    where
        F: FnOnce(&mut File) -> Result<R, anyhow::Error>,
    {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(write_mode)
            .create(write_mode)
            .open(&self.path)?;

        if write_mode {
            file.lock_exclusive()?;
        } else {
            file.lock_shared()?;
        }

        let result = op(&mut file);
        file.unlock()?;
        Ok(result?)
    }

    pub fn save_session(&self, session: UserSession) -> Result<(), anyhow::Error> {
        self.with_lock(true, |file| {
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;

            let mut db: AuthDatabase = if content.is_empty() {
                AuthDatabase::default()
            } else {
                match decrypt_payload(&content) {
                    Ok(decrypted) => serde_json::from_str(&decrypted).unwrap_or_default(),
                    Err(_) => AuthDatabase::default(),
                }
            };

            db.sessions.insert(session.user_id.clone(), session);
            let serialized = serde_json::to_string_pretty(&db)?;
            let encrypted = encrypt_payload(&serialized)?;
            file.set_len(0)?;
            file.seek(std::io::SeekFrom::Start(0))?;
            file.write_all(&encrypted)?;
            Ok(())
        })
    }

    pub fn load_session(&self, user_id: &str) -> Option<UserSession> {
        let res = self.with_lock(false, |file| {
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;

            let db: AuthDatabase = if content.is_empty() {
                AuthDatabase::default()
            } else {
                match decrypt_payload(&content) {
                    Ok(decrypted) => serde_json::from_str(&decrypted).unwrap_or_default(),
                    Err(_) => AuthDatabase::default(),
                }
            };

            Ok(db.sessions.get(user_id).cloned())
        });

        match res {
            Ok(opt) => opt,
            Err(_) => None,
        }
    }

    // No external callers currently. Kept for logout flows; promote to pub when
    // a confirmed external caller exists.
    pub(crate) fn delete_session(&self, user_id: &str) -> Result<(), anyhow::Error> {
        self.with_lock(true, |file| {
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;

            let mut db: AuthDatabase = if content.is_empty() {
                AuthDatabase::default()
            } else {
                match decrypt_payload(&content) {
                    Ok(decrypted) => serde_json::from_str(&decrypted).unwrap_or_default(),
                    Err(_) => AuthDatabase::default(),
                }
            };

            if db.sessions.remove(user_id).is_some() {
                let serialized = serde_json::to_string_pretty(&db)?;
                let encrypted = encrypt_payload(&serialized)?;
                file.set_len(0)?;
                file.seek(std::io::SeekFrom::Start(0))?;
                file.write_all(&encrypted)?;
            }
            Ok(())
        })
    }
}
