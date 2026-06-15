//! Gated live-model integration test for the honest pipeline.
//!
//! Proves the SP1 honesty claim on a REAL local model (ollama) rather than the
//! deterministic stub: when a live model fixes a real, non-fixture bug, the
//! pipeline's attested mutation count equals an INDEPENDENT git measurement of
//! what actually changed on disk. The attestation cannot drift from reality —
//! that is the whole point, and here we prove it with a model that has no canned
//! answer for this crate.
//!
//! It is **gated**: it skips (does nothing) unless the ollama daemon is
//! reachable on `127.0.0.1:11434`, so CI and bare hosts are unaffected — the
//! same opt-in discipline the signing tests use. To run it, have ollama up with
//! a code model pulled (default `qwen2.5:7b`, override via `KORG_OLLAMA_MODEL`):
//!
//! ```text
//! ollama serve & ollama pull qwen2.5:7b
//! cargo test -p korg-runtime --test live_ollama -- --nocapture
//! ```

use korg_llm::LocalOllamaProvider;
use korg_runtime::run_once::run_once_honest_with;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// The buggy baseline: `max` returns the minimum. The deterministic provider
/// has no entry for this crate, so any real fix here comes from the live model.
const BUGGY_LIB: &str = "/// Returns the maximum of two integers.\n\
pub fn max(a: i64, b: i64) -> i64 {\n\
\x20   // BUG: returns the minimum, not the maximum.\n\
\x20   if a < b { a } else { b }\n\
}\n";

/// True when the ollama daemon accepts a TCP connection on its default port.
fn ollama_reachable() -> bool {
    let addr = match "127.0.0.1:11434".to_socket_addrs() {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => return false,
        },
        Err(_) => return false,
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok()
}

async fn git(dir: &std::path::Path, args: &[&str]) {
    tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .unwrap();
}

/// A fresh temp git repo whose committed baseline is the buggy `max` crate.
async fn buggy_repo() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("korg-live-ollama-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"korg-live-bug\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"), BUGGY_LIB).unwrap();
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
            "buggy baseline",
        ],
    )
    .await;
    dir
}

/// Independent measurement of files changed vs HEAD — a DIFFERENT git
/// invocation (`git diff HEAD --name-only`, counting lines) than the pipeline's
/// `numstat` (`git add -A` + `git diff --cached --numstat`, parsing tab rows).
/// The pipeline has already staged everything by the time this runs, so both
/// observe the same worktree state — but via independent code paths, so a bug
/// in `numstat`'s row parser (or a fabricated count) would surface as a
/// mismatch. It is a cross-check of the *counting*, not a restatement of it.
async fn independent_files_changed(dir: &std::path::Path) -> usize {
    let out = tokio::process::Command::new("git")
        .args(["diff", "HEAD", "--name-only"])
        .current_dir(dir)
        .output()
        .await
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

#[tokio::test]
async fn live_ollama_attestation_matches_independent_reality() {
    if !ollama_reachable() {
        eprintln!(
            "[skip] ollama daemon not reachable on 127.0.0.1:11434 — gated live test skipped"
        );
        return;
    }
    let model = std::env::var("KORG_OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_string());
    let dir = buggy_repo().await;

    let task = format!(
        "Fix the bug in src/lib.rs: the `max` function returns the minimum instead of the \
         maximum. Output the COMPLETE corrected contents of src/lib.rs.\n\n\
         Current src/lib.rs:\n```rust\n{BUGGY_LIB}\n```"
    );
    let provider = LocalOllamaProvider::new(None, Some(model.clone()));
    let report = run_once_honest_with(&task, &dir, &provider).await;

    // The core honesty claim, cross-checked against an INDEPENDENT git measure:
    // the attested count is exactly what really changed on disk — no drift.
    let independent = independent_files_changed(&dir).await;
    assert_eq!(
        report.attested_count, independent,
        "attested mutation count ({}) must equal an independent git-diff measurement ({})",
        report.attested_count, independent
    );

    eprintln!(
        "[live] model={model} files_changed={} cargo_check={} attested={} (independent={independent})",
        report.files_changed, report.cargo_check, report.attested_count
    );

    // A ledger is always written for a completed run.
    assert!(
        report.ledger_path.is_some(),
        "a verifiable korg-ledger@v1 journal must be written for the run"
    );

    // We deliberately do NOT assert `files_changed >= 1`: a 7B local model is
    // non-deterministic and may not always emit a parseable patch. That is the
    // honesty boundary working as designed — when the model delivers, the change
    // is real and measured (files_changed >= 1); when it does not, the pipeline
    // reports an honest null (0). EITHER WAY the attestation equals reality, which
    // is the invariant asserted above and the only guarantee Korg makes. The
    // "real model does real work" claim is demonstrated end-to-end by the README
    // walkthrough, not by a flaky assertion on a small model's output here.
    if report.files_changed > 0 {
        assert_eq!(
            report.cargo_check, "Passed",
            "a real applied change should leave the crate compiling"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
