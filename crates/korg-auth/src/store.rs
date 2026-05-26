use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Seek;
use std::io::{Read, Write};
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

// ── Auth blob format ────────────────────────────────────────────────────────
//
// v2 (current, written on every save):
//     [4-byte magic "KOA2"][1-byte version=0x02][16-byte salt][12-byte nonce][ciphertext+tag]
//
// v1 (legacy, still readable for existing on-disk files):
//     [12-byte nonce][ciphertext+tag]   — uses hardcoded salt + 10k PBKDF2 iterations
//
// Bumping iterations or changing the KDF means bumping the version byte and
// extending the parser below. Existing v1 files stay readable until the user
// re-saves, at which point they get rewritten as v2.

const KOA2_MAGIC: &[u8] = b"KOA2";
const KOA2_VERSION: u8 = 0x02;
const KOA2_SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const PBKDF2_ITERATIONS_V2: u32 = 600_000; // OWASP 2026 minimum for PBKDF2-HMAC-SHA256

// Legacy v1 params — only used to decrypt files written before v2 landed.
const PBKDF2_ITERATIONS_V1: u32 = 10_000;
const V1_SALT: &[u8] = b"korg-auth-salt-constant";

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

fn require_master_password() -> String {
    std::env::var("KORG_MASTER_KEY").expect(
        "KORG_MASTER_KEY must be set before the auth store is used. \
         Generate one with `openssl rand -hex 32` and export it in the environment. \
         Refusing to encrypt with a hardcoded fallback key.",
    )
}

fn derive_cipher(salt: &[u8], iterations: u32) -> Aes256Gcm {
    let master_password = require_master_password();
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(master_password.as_bytes(), salt, iterations, &mut key);
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
}

fn encrypt_payload(plain_text: &str) -> Result<Vec<u8>, anyhow::Error> {
    // v2 format. A fresh salt + nonce per encryption means even identical
    // plaintexts produce different ciphertexts, and a stolen auth.json can't
    // be brute-forced via a precomputed table.
    let mut rng = rand::thread_rng();
    let mut salt = [0u8; KOA2_SALT_LEN];
    rng.fill_bytes(&mut salt);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut nonce_bytes);

    let cipher = derive_cipher(&salt, PBKDF2_ITERATIONS_V2);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher_text = cipher
        .encrypt(nonce, plain_text.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))?;

    let mut payload =
        Vec::with_capacity(KOA2_MAGIC.len() + 1 + KOA2_SALT_LEN + NONCE_LEN + cipher_text.len());
    payload.extend_from_slice(KOA2_MAGIC);
    payload.push(KOA2_VERSION);
    payload.extend_from_slice(&salt);
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&cipher_text);
    Ok(payload)
}

fn decrypt_payload(payload: &[u8]) -> Result<String, anyhow::Error> {
    if payload.starts_with(KOA2_MAGIC) {
        decrypt_v2(payload)
    } else {
        decrypt_v1(payload)
    }
}

fn decrypt_v2(payload: &[u8]) -> Result<String, anyhow::Error> {
    let header_len = KOA2_MAGIC.len() + 1 + KOA2_SALT_LEN + NONCE_LEN;
    if payload.len() < header_len {
        return Err(anyhow::anyhow!("Invalid v2 payload: too short"));
    }
    let version = payload[KOA2_MAGIC.len()];
    if version != KOA2_VERSION {
        return Err(anyhow::anyhow!(
            "Unsupported auth blob version: 0x{:02x}",
            version
        ));
    }
    let salt_start = KOA2_MAGIC.len() + 1;
    let nonce_start = salt_start + KOA2_SALT_LEN;
    let ct_start = nonce_start + NONCE_LEN;
    let salt = &payload[salt_start..nonce_start];
    let nonce_bytes = &payload[nonce_start..ct_start];
    let ciphertext = &payload[ct_start..];

    let cipher = derive_cipher(salt, PBKDF2_ITERATIONS_V2);
    let nonce = Nonce::from_slice(nonce_bytes);
    let decrypted_bytes = cipher.decrypt(nonce, ciphertext).map_err(|e| {
        anyhow::anyhow!(
            "Decryption failed (tampering or invalid master key): {:?}",
            e
        )
    })?;
    Ok(String::from_utf8(decrypted_bytes)?)
}

fn decrypt_v1(payload: &[u8]) -> Result<String, anyhow::Error> {
    // Legacy 10k-iteration, hardcoded-salt format. Existing files only —
    // never written. After a successful read the next save_session() call
    // rewrites the file as v2.
    if payload.len() < NONCE_LEN {
        return Err(anyhow::anyhow!("Invalid v1 payload: too short"));
    }
    let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
    let cipher = derive_cipher(V1_SALT, PBKDF2_ITERATIONS_V1);
    let nonce = Nonce::from_slice(nonce_bytes);
    let decrypted_bytes = cipher.decrypt(nonce, ciphertext).map_err(|e| {
        anyhow::anyhow!(
            "Decryption failed (tampering or invalid master key): {:?}",
            e
        )
    })?;
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

        let mut opts = OpenOptions::new();
        opts.read(true).write(write_mode).create(write_mode);
        // Restrict permissions to owner read/write on Unix. Without this the
        // OS umask decides, which typically yields 0644 — letting any local
        // user read the (encrypted) credentials.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&self.path)?;
        // If the file already existed before this call, OpenOptions::mode is
        // a no-op (it only applies on create). Fix up perms explicitly so
        // pre-existing files migrate to 0600 on the next save.
        #[cfg(unix)]
        if write_mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    fn ensure_master_key() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            if std::env::var("KORG_MASTER_KEY").is_err() {
                std::env::set_var("KORG_MASTER_KEY", "auth-store-unit-test-master-key");
            }
        });
    }

    /// Build a v1-format blob using the legacy 10k-iteration KDF and hardcoded
    /// salt. Mirrors what auth.json on disk looked like before v2 landed.
    fn make_v1_blob(plain_text: &str) -> Vec<u8> {
        let master_password = std::env::var("KORG_MASTER_KEY").unwrap();
        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha256>(
            master_password.as_bytes(),
            V1_SALT,
            PBKDF2_ITERATIONS_V1,
            &mut key,
        );
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let nonce_bytes = rand::random::<[u8; NONCE_LEN]>();
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plain_text.as_bytes())
            .unwrap();
        let mut blob = nonce_bytes.to_vec();
        blob.extend(ct);
        blob
    }

    #[test]
    fn v2_roundtrip_uses_fresh_salt() {
        ensure_master_key();
        let a = encrypt_payload("hello").unwrap();
        let b = encrypt_payload("hello").unwrap();
        // Different per-blob salt + nonce → different ciphertexts for the same plaintext.
        assert_ne!(a, b);
        assert!(a.starts_with(KOA2_MAGIC));
        assert_eq!(decrypt_payload(&a).unwrap(), "hello");
        assert_eq!(decrypt_payload(&b).unwrap(), "hello");
    }

    #[test]
    fn v1_blobs_still_decrypt() {
        ensure_master_key();
        let blob = make_v1_blob("legacy-secret");
        // No KOA2 magic on legacy files.
        assert!(!blob.starts_with(KOA2_MAGIC));
        assert_eq!(decrypt_payload(&blob).unwrap(), "legacy-secret");
    }

    #[test]
    fn v2_blob_with_corrupted_magic_falls_into_v1_path_and_fails() {
        ensure_master_key();
        let mut blob = encrypt_payload("hello").unwrap();
        // Flip one magic byte — should no longer match KOA2, so the decoder
        // tries v1 and fails cleanly rather than returning garbage.
        blob[0] ^= 0xff;
        assert!(decrypt_payload(&blob).is_err());
    }
}
