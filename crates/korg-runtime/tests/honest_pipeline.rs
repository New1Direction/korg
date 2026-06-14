//! Keystone: prove the honest pipeline attests a TRUE fact — the mutation count
//! the ledger would record equals the real git diff file count.

use korg_llm::{DeterministicProvider, LlmProvider, LlmRequest, Message, Role};
use korg_runtime::observation::{
    apply_mutations, cargo_check, honest_metrics, numstat, CargoCheck,
};

fn req(system: &str, user: &str) -> LlmRequest {
    LlmRequest {
        messages: vec![
            Message {
                role: Role::System,
                content: system.into(),
                name: None,
                tool_calls: None,
            },
            Message {
                role: Role::User,
                content: user.into(),
                name: None,
                tool_calls: None,
            },
        ],
        temperature: 0.3,
        max_tokens: None,
        tools: None,
        stop_sequences: None,
        multimodal: None,
        tx_id: None,
        session_id: None,
        policy_hash: None,
        top_p: None,
        presence_penalty: None,
        frequency_penalty: None,
    }
}

async fn git(dir: &std::path::Path, args: &[&str]) {
    tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .unwrap();
}

#[tokio::test]
async fn honest_pipeline_attests_real_diff_count() {
    // 1. Copy the committed fixture crate into a temp git repo (the "before" state).
    let src = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/honest-demo-repo"
    );
    let dir = std::env::temp_dir().join(format!("korg-keystone-{}", uuid::Uuid::new_v4()));
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

    // 2. Ask the hermetic default provider (as Benjamin) for the fixture patch.
    let provider = DeterministicProvider::new();
    let resp = provider
        .complete(req(
            "You are Benjamin, the Builder & Implementer.",
            "Fix the add function in src/lib.rs so it adds",
        ))
        .await
        .unwrap();

    // 3. Parse mutations the way the worker does, then APPLY them to the worktree.
    let (output, _c, _f) = korg_runtime::personas::parse_structured_response(&resp.content);
    let muts = output
        .get("mutations")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let apply = apply_mutations(&dir, &muts).await;
    assert_eq!(apply.applied, 1, "the canonical patch applies one file");

    // 4. Measure reality.
    let n = numstat(&dir).await;
    let check = cargo_check(&dir).await;

    // 5. The honest invariant: attested mutation count == real diff file count == 1,
    //    and the fix compiles (verified_count_delta == 1).
    assert_eq!(n.files, 1, "exactly one real file changed");
    assert!(
        matches!(check, CargoCheck::Passed),
        "the applied fix compiles"
    );
    let metrics = honest_metrics(
        &apply,
        &check,
        &n,
        resp.usage.total_tokens,
        1.0,
        0.0,
        "keystone",
    );
    assert_eq!(metrics["verified_count_delta"].as_i64(), Some(1));
    assert_eq!(metrics["files_changed"].as_u64(), Some(1));

    let _ = std::fs::remove_dir_all(&dir);
}
