//! Centralized path resolution for Korg.
//!
//! All runtime directories (campaigns, blackboard, worktrees, etc.) are resolved
//! dynamically using the `directories` crate instead of hardcoded paths.
//!
//! - **Config:**  `~/.config/korg/` (Linux/macOS XDG)
//! - **Cache:**   `~/.cache/korg/` (Linux) / `~/Library/Caches/korg/` (macOS)
//! - **Data:**    `~/.local/share/korg/` (Linux) / `~/Library/Application Support/korg/` (macOS)

use std::path::PathBuf;

/// Returns the Korg cache directory root.
///
/// Falls back to `/tmp/korg` if the platform directories cannot be resolved.
pub fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "korg", "korg")
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/tmp/korg"))
}

/// Returns the Korg config directory root.
///
/// Internal to this module — used only by `prompts_dir()`. Not part of the
/// public API because downstream crates have no reason to inspect the config
/// root directly; they should go through the specific path functions.
pub(crate) fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "korg", "korg")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/tmp/korg/config"))
}

/// Returns the Korg data directory root.
///
/// Internal to this module — no external callers. Kept for completeness of the
/// XDG directory set; promote to `pub` if a downstream crate needs it.
pub(crate) fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "korg", "korg")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/tmp/korg/data"))
}

/// Campaign artifacts directory for a given session.
pub fn campaign_dir(session_id: &uuid::Uuid) -> PathBuf {
    cache_dir().join("campaigns").join(session_id.to_string())
}

/// State blobs directory for a given session.
pub fn state_blobs_dir(session_id: &uuid::Uuid) -> PathBuf {
    campaign_dir(session_id).join("state-blobs")
}

/// Blackboard persistence directory.
pub fn blackboard_dir() -> PathBuf {
    cache_dir().join("blackboard")
}

/// Blackboard JSON file path.
pub fn blackboard_json() -> PathBuf {
    blackboard_dir().join("blackboard.json")
}

/// Contract persistence directory.
pub fn contracts_dir() -> PathBuf {
    cache_dir().join("contracts")
}

/// Ktrans log directory.
pub fn ktrans_dir() -> PathBuf {
    cache_dir().join("ktrans")
}

/// Worktree sandbox directory for a given persona and routing ID.
pub fn worktree_dir(persona_name: &str, routing_id: &str, suffix: &str) -> PathBuf {
    cache_dir()
        .join("worktrees")
        .join(format!("{}-{}-{}", persona_name, routing_id, suffix))
}

/// Worktree directory with two-part name (used by harness).
pub fn worktree_dir_harness(worker_id: &str, routing_id: &str) -> PathBuf {
    cache_dir()
        .join("worktrees")
        .join(format!("{}-{}", worker_id, routing_id))
}

/// Fork sandbox directory for time-travel forks.
pub fn forks_dir(tx_id: usize) -> PathBuf {
    cache_dir().join("forks").join(format!("tx_{:02}", tx_id))
}

/// Semantic merge output path.
pub fn semantic_merge_path(session_id: &uuid::Uuid) -> PathBuf {
    campaign_dir(session_id).join("semantic-merge.json")
}

/// Returns the project root directory (current working directory).
///
/// This replaces all hardcoded `/Users/…/Korg` references.
pub fn project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Prompts directory — checks config dir first, falls back to `./Prompts/`.
pub fn prompts_dir() -> PathBuf {
    let config_prompts = config_dir().join("prompts");
    if config_prompts.exists() {
        return config_prompts;
    }
    let local_prompts = project_root().join("Prompts");
    if local_prompts.exists() {
        return local_prompts;
    }
    config_prompts
}

/// Temporary patch file path.
pub fn temp_patch_path() -> PathBuf {
    cache_dir().join(format!("korg-patch-{}.patch", uuid::Uuid::new_v4()))
}

/// Returns the project root as a string (for use in path policy defaults).
pub fn project_root_string() -> String {
    project_root().to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_is_not_empty() {
        let d = cache_dir();
        assert!(!d.as_os_str().is_empty());
    }

    #[test]
    fn campaign_dir_contains_session_id() {
        let id = uuid::Uuid::new_v4();
        let d = campaign_dir(&id);
        assert!(d.to_string_lossy().contains(&id.to_string()));
    }

    #[test]
    fn project_root_returns_something() {
        let r = project_root();
        assert!(!r.as_os_str().is_empty());
    }
}
