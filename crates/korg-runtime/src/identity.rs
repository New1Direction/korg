//! Persistent agent signing identity for korg.
//!
//! A stable Ed25519 keypair persisted at `korg_core::paths::agent_identity_path()`
//! (raw hex, file mode `0o600` — mirrors `korgex/src/signing.py`). The leader signs
//! every `.ktrans` artifact with this key; persisting it means those signatures are
//! attributable to the same identity across campaigns, instead of a throwaway
//! per-run key.
//!
//! Loading is **fail-safe**: on any I/O error it falls back to an ephemeral key (the
//! previous behavior) and warns, so a missing/read-only config dir can never crash
//! the orchestrator — it only loses cross-run signature attribution.

use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;

/// Load the persistent agent identity, creating it on first use. The path is
/// `$KORG_IDENTITY_PATH` if set (override for tests/CI), else
/// `korg_core::paths::agent_identity_path()`. **Fail-safe:** returns an ephemeral
/// key (with a warning) if the identity can't be read or created, so signing never
/// crashes the orchestrator — it only loses cross-run attribution.
pub fn load_or_create_identity() -> SigningKey {
    let path = identity_path();
    match load_or_create_identity_at(&path) {
        Ok(key) => key,
        Err(e) => {
            tracing::warn!(
                "korg identity: using an ephemeral key (could not load {}: {e}); \
                 .ktrans signatures won't be attributable across runs",
                path.display()
            );
            SigningKey::generate(&mut rand::rngs::OsRng)
        }
    }
}

/// Resolve the identity file path: `$KORG_IDENTITY_PATH` override, else the
/// korg config-dir default.
fn identity_path() -> PathBuf {
    if let Ok(p) = std::env::var("KORG_IDENTITY_PATH") {
        return PathBuf::from(p);
    }
    korg_core::paths::agent_identity_path()
}

/// Load the agent identity at `path`, generating and persisting it (mode `0o600`)
/// on first use. Subsequent calls return the same key.
pub fn load_or_create_identity_at(path: &Path) -> std::io::Result<SigningKey> {
    use std::io::{Error, ErrorKind};
    if path.exists() {
        let contents = std::fs::read_to_string(path)?;
        let bytes = hex::decode(contents.trim())
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("identity key hex: {e}")))?;
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::new(ErrorKind::InvalidData, "identity key must be 32 bytes"))?;
        Ok(SigningKey::from_bytes(&arr))
    } else {
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, hex::encode(key.to_bytes()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_same_identity_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.ed25519");
        let first = load_or_create_identity_at(&path).unwrap();
        let second = load_or_create_identity_at(&path).unwrap();
        assert_eq!(
            first.verifying_key().to_bytes(),
            second.verifying_key().to_bytes(),
            "a persisted identity must be stable across loads"
        );
    }

    #[test]
    #[cfg(unix)]
    fn persisted_key_file_is_private_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.ed25519");
        load_or_create_identity_at(&path).unwrap();
        assert!(path.exists(), "the key file must be created");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the private key file must be mode 0o600");
    }
}
