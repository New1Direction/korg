//! korg-verify against a REAL receipt minted by `korgex receipt --sign` (fixture
//! generated with a fixed Ed25519 key). This is the cross-implementation proof: Rust
//! re-derives the same chain hashes AND verifies the Python-produced signature.

use korg_verify::{verify_receipt, verify_text};
use serde_json::Value;

fn fixture(name: &str) -> String {
    let p =
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn signed_receipt_from_korgex_is_valid() {
    let v = verify_text(&fixture("signed-receipt.json"), None, None).unwrap();
    assert_eq!(v.kind, "receipt");
    assert!(v.valid, "errors: {:?}", v.errors);
    assert_eq!(
        v.signature_ok,
        Some(true),
        "Python Ed25519 sig must verify in Rust"
    );
    assert!(v.tip_ok && v.chain_ok && v.dag_ok);
}

#[test]
fn tampering_an_event_breaks_the_chain() {
    let mut r: Value = serde_json::from_str(&fixture("signed-receipt.json")).unwrap();
    r["events"][1]["args"] = serde_json::json!({ "file_path": "EVIL.py" });
    let v = verify_receipt(&r, None, None);
    assert!(!v.valid);
    assert!(!v.chain_ok);
}

#[test]
fn forged_signature_is_rejected() {
    let mut r: Value = serde_json::from_str(&fixture("signed-receipt.json")).unwrap();
    r["signature"]["sig"] = serde_json::json!("00".repeat(64)); // 64 zero bytes, hex
    let v = verify_receipt(&r, None, None);
    assert_eq!(v.signature_ok, Some(false));
    assert!(!v.valid);
}

#[test]
fn wrong_pinned_signer_is_rejected() {
    let r: Value = serde_json::from_str(&fixture("signed-receipt.json")).unwrap();
    let v = verify_receipt(&r, None, Some(&"ab".repeat(32)));
    assert!(!v.valid);
    assert!(
        v.errors.iter().any(|e| e.contains("pinned key")),
        "{:?}",
        v.errors
    );
}

#[test]
fn correct_pinned_signer_passes() {
    let r: Value = serde_json::from_str(&fixture("signed-receipt.json")).unwrap();
    let signer = r["signature"]["pubkey"].as_str().unwrap().to_string();
    let v = verify_receipt(&r, None, Some(&signer));
    assert!(v.valid, "errors: {:?}", v.errors);
}

#[test]
fn the_same_events_verify_as_a_bare_journal() {
    let v = verify_text(&fixture("journal.json"), None, None).unwrap();
    assert_eq!(v.kind, "journal");
    assert!(v.valid, "errors: {:?}", v.errors);
}

/// CRITICAL forge regression: a receipt with NO tip but a signature over the empty
/// message (which any attacker can mint with their own key) must NOT verify. Before
/// the fix, `verify_tip_sig(pk, "", sig)` verified a 0-byte message → valid+signed.
#[test]
fn tipless_signed_receipt_forge_is_rejected() {
    let mut r: Value = serde_json::from_str(&fixture("signed-receipt.json")).unwrap();
    let obj = r.as_object_mut().unwrap();
    obj.remove("tip");
    // (the original signature is over the real tip; with no tip it must fail closed)
    let v = verify_receipt(&r, None, None);
    assert!(!v.valid, "a tipless signed receipt must never be valid");
    assert_eq!(v.signature_ok, Some(false));
    assert!(
        v.errors.iter().any(|e| e.contains("no tip to attest to")),
        "{:?}",
        v.errors
    );
}

/// A receipt with no events attests to nothing and must not pass.
#[test]
fn empty_receipt_is_rejected() {
    let r = serde_json::json!({ "schema": "korgex-receipt@v1", "events": [] });
    let v = verify_receipt(&r, None, None);
    assert!(!v.valid);
    assert!(
        v.errors.iter().any(|e| e.contains("no events")),
        "{:?}",
        v.errors
    );
}

/// `verify_tip_sig` must reject a non-32-byte message regardless of the signature.
#[test]
fn verify_tip_sig_rejects_empty_message() {
    // a syntactically valid (but empty-message) signature must not verify
    assert!(!korg_verify::verify_tip_sig(
        &"ab".repeat(32),
        "",
        &"00".repeat(64)
    ));
}
