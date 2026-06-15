//! Gated end-to-end test for the multi-persona swarm campaign.
//!
//! Proves the campaign actually runs: each persona executes as a real `korg
//! worker` subprocess in a git worktree, completes its work, and sends a
//! `TerminationReport`, so the leader records it as DONE — not the false
//! `exit_code=-1` "crash" that stdout pollution used to cause for *every*
//! worker. Guards the swarm-real fix (worker sends TerminationReport; all logs
//! go to stderr so stdout stays a clean ACP channel).
//!
//! GATED: spawns the `korg` binary plus worker subprocesses in git worktrees —
//! CI-hostile and slow (~60-90s). Run locally:
//!   cargo test --test campaign_e2e -- --ignored --nocapture

use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git spawn failed");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Last N lines of a log, for readable assertion failures.
fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(40);
    lines[start..].join("\n")
}

#[test]
#[ignore = "spawns korg + worker subprocesses in git worktrees; CI-hostile/slow — run locally with --ignored"]
fn campaign_workers_complete_and_attest_real_work() {
    // A fixture crate with the canonical add-bug (`a - b`). The deterministic
    // provider produces Benjamin's real fix for this task, applied + measured.
    let dir = std::env::temp_dir().join(format!("korg-campaign-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"e2e\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn add(a: i64, b: i64) -> i64 { a - b }\n",
    )
    .unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["add", "-A"]);
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
    );

    let out = Command::new(env!("CARGO_BIN_EXE_korg"))
        .args([
            "Fix the add function in src/lib.rs so it adds",
            "--goal",
            "--provider",
            "deterministic",
        ])
        .current_dir(&dir)
        .output()
        .expect("run korg campaign");
    let log = String::from_utf8_lossy(&out.stderr);

    let _ = std::fs::remove_dir_all(&dir);

    // The workers must terminate SUCCESS — before the fix, stdout pollution
    // corrupted the ACP stream and the leader stamped every worker crashed.
    assert!(
        log.contains("exit_status=success"),
        "expected workers to terminate success (TerminationReport received); got:\n{}",
        tail(&log)
    );
    assert!(
        !log.contains("worker_crashed"),
        "no worker should be falsely marked crashed; got:\n{}",
        tail(&log)
    );
    // Benjamin (the implementer) attests exactly one REAL measured mutation —
    // the applied fix on the fixture — proving real per-persona work flows
    // through the campaign, not theater.
    assert!(
        log.contains("persona=\"Benjamin\" mutations=1"),
        "Benjamin should attest one real measured mutation; got:\n{}",
        tail(&log)
    );
}
