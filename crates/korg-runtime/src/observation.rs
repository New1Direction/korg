//! observation.rs — pure, testable "measure reality" primitives for the worker.
//!
//! The worker child applies a persona's patch to its git worktree, then measures
//! the real consequences (compile result, diff size, tokens, load) so the ledger
//! attests facts instead of fabricated numbers. Each primitive is independently
//! testable against a throwaway git repo.

use std::path::Path;

/// Three-state compile result. `Unavailable` (cargo missing / failed to spawn)
/// is distinct from `Passed` — §6 requires that distinction so a degraded host
/// records `tool_unavailable`, never a fabricated pass.
#[derive(Debug, Clone)]
pub enum CargoCheck {
    Passed,
    Failed(String),
    Unavailable,
}

/// Run `cargo check` in `worktree` and classify the outcome.
pub async fn cargo_check(worktree: &Path) -> CargoCheck {
    match tokio::process::Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(worktree)
        .output()
        .await
    {
        Ok(o) if o.status.success() => CargoCheck::Passed,
        Ok(o) => CargoCheck::Failed(String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(_) => CargoCheck::Unavailable, // cargo absent / failed to spawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("korg-obs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn git(dir: &std::path::Path, args: &[&str]) {
        tokio::process::Command::new("git").args(args).current_dir(dir)
            .output().await.unwrap();
    }

    async fn init_crate(dir: &std::path::Path, lib_body: &str) {
        std::fs::write(dir.join("Cargo.toml"),
            "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"src/lib.rs\"\n").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), lib_body).unwrap();
        git(dir, &["init", "-q"]).await;
        git(dir, &["add", "-A"]).await;
        git(dir, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]).await;
    }

    #[tokio::test]
    async fn cargo_check_passed_on_valid_crate() {
        let d = tmp();
        init_crate(&d, "pub fn f() -> i64 { 1 }\n").await;
        assert!(matches!(cargo_check(&d).await, CargoCheck::Passed));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn cargo_check_failed_on_broken_crate() {
        let d = tmp();
        init_crate(&d, "pub fn f() -> i64 { \"nope\" }\n").await;
        assert!(matches!(cargo_check(&d).await, CargoCheck::Failed(_)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
