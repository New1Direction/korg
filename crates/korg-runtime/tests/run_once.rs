//! Integration test for `run_once_honest` — the user-facing honest pipeline.
//!
//! Mirrors the keystone (`honest_pipeline.rs`) setup: copy the committed fixture
//! crate into a temp git repo, then drive the *whole* pipeline through one call.
//! Two facts must hold:
//!   - the fixture task produces exactly one REAL file change that compiles, and
//!     the attested mutation count equals the real git-diff file count (==1);
//!   - an unrelated task produces an honest null — zero changes, zero attested,
//!     never a fabricated success.

use korg_runtime::run_once::run_once_honest;

async fn git(dir: &std::path::Path, args: &[&str]) {
    tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .unwrap();
}

/// Copy the committed fixture crate into a fresh temp git repo (the "before"
/// state) — the exact dance the keystone test uses.
async fn fixture_repo() -> std::path::PathBuf {
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/honest-demo-repo"
    );
    let dir = std::env::temp_dir().join(format!("korg-run-once-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::copy(format!("{src}/Cargo.toml"), dir.join("Cargo.toml")).unwrap();
    std::fs::copy(format!("{src}/src/lib.rs"), dir.join("src/lib.rs")).unwrap();
    git(&dir, &["init", "-q"]).await;
    git(&dir, &["add", "-A"]).await;
    git(
        &dir,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "base",
        ],
    )
    .await;
    dir
}

#[tokio::test]
async fn fixture_task_attests_one_real_change_that_compiles() {
    let dir = fixture_repo().await;

    let report = run_once_honest("Fix the add function in src/lib.rs so it adds", &dir).await;

    assert_eq!(report.files_changed, 1, "exactly one real file changed");
    assert_eq!(report.cargo_check, "Passed", "the applied fix compiles");
    assert_eq!(report.attested_count, 1, "attested mutation count is 1");
    assert_eq!(
        report.attested_count, report.numstat_files,
        "the attested count equals the real git-diff file count (the SP1 invariant)"
    );
    assert!(
        report.ledger_path.is_some(),
        "a verifiable ledger was written"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unrelated_task_attests_zero_no_fabrication() {
    let dir = fixture_repo().await;

    let report = run_once_honest("Write a haiku about the ocean", &dir).await;

    assert_eq!(
        report.files_changed, 0,
        "honest null: an unrelated task changes nothing"
    );
    assert_eq!(report.attested_count, 0, "honest null: nothing is attested");
    assert_eq!(
        report.attested_count, report.numstat_files,
        "attested count still equals the real diff (both zero) — no fabrication"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
