//! Cross-implementation goldseal@v1 interop + adversarial tamper coverage.
//!
//! `goldseal-v1.json` is a frozen fixture MINTED BY PYTHON
//! (`spec/korg-ledger-v1/tools/mint_goldseal_fixture.py`, seed `[42; 32]`). This
//! proves a Python-minted Gold Seal verifies — unchanged — under the independent
//! Rust verifier (the JS verifier checks the same bytes in
//! `spec/korg-ledger-v1/js/conformance.mjs`). It also pins the security
//! properties: the summary cannot lie, the claim cannot move, and a stripped
//! seal is a downgrade — not a merely-unsigned artifact.

use korg_verify::{verify_goldseal, verify_text};
use serde_json::Value;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    ))
    .unwrap()
}

fn seal() -> Value {
    serde_json::from_str(&fixture("goldseal-v1.json")).unwrap()
}

fn pubkey() -> String {
    fixture("goldseal-v1.pubkey").trim().to_string()
}

#[test]
fn python_minted_goldseal_verifies_under_rust() {
    let v = verify_text(&fixture("goldseal-v1.json"), None, None).unwrap();
    assert_eq!(
        v.kind, "goldseal",
        "auto-detect must route to the goldseal path"
    );
    assert!(
        v.valid,
        "Python-minted seal must verify under Rust: {:?}",
        v.errors
    );
    assert_eq!(v.summary_ok, Some(true));
    assert_eq!(v.signature_ok, Some(true));
    assert_eq!(v.event_count, 5);
}

#[test]
fn pinned_issuer_key_is_enforced() {
    let env = seal();
    assert!(
        verify_goldseal(&env, Some(&pubkey())).valid,
        "correct pin must pass"
    );

    let wrong = "00".repeat(32);
    let v = verify_goldseal(&env, Some(&wrong));
    assert!(!v.valid, "a wrong pinned issuer key must fail");
    assert_eq!(v.signature_ok, Some(false));
}

#[test]
fn a_lying_summary_is_rejected() {
    // Drop a touched file from the summary — the re-derivation must catch it.
    let mut env = seal();
    env["summary"]["files"] = serde_json::json!(["src/app.py"]);
    let v = verify_goldseal(&env, None);
    assert!(!v.valid);
    assert_eq!(v.summary_ok, Some(false));
}

#[test]
fn tampering_an_event_breaks_the_chain() {
    let mut env = seal();
    env["events"][2]["args"]["file_path"] = Value::String("src/evil.py".into());
    let v = verify_goldseal(&env, None);
    assert!(!v.valid);
    assert!(!v.chain_ok);
}

#[test]
fn moving_the_claim_breaks_the_seal() {
    // The claim is issuer-asserted but signature-protected: it cannot be edited
    // without invalidating the seal (the chain stays intact, so only the seal fails).
    let mut env = seal();
    env["claim"] = Value::String("did something else entirely".into());
    let v = verify_goldseal(&env, None);
    assert!(!v.valid);
    assert!(v.chain_ok, "the event chain is untouched");
    assert_eq!(v.signature_ok, Some(false));
}

#[test]
fn a_stripped_seal_is_a_downgrade_not_merely_unsigned() {
    let mut env = seal();
    env.as_object_mut().unwrap().remove("seal");
    let v = verify_goldseal(&env, None);
    assert!(!v.valid, "a goldseal@v1 without a seal must not verify");
    assert_eq!(v.signature_ok, Some(false));
}
