//! korg-verify against the SHARED frozen korg-ledger@v1 vectors — the same oracle
//! the Python and JS implementations are checked against. A green run here means a
//! third independent implementation reproduces the chain, not just a second one.

use korg_verify::verify_text;
use std::path::PathBuf;

fn read(name: &str) -> String {
    // Vendored local copy of the SHARED frozen vectors — keeps this crate
    // self-contained and publishable (no sibling-crate path dependency).
    let p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/conformance")).join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn basic_intact_is_valid() {
    let v = verify_text(&read("basic-intact.jsonl"), None, None).unwrap();
    assert!(v.valid, "errors: {:?}", v.errors);
    assert!(v.chain_ok && v.dag_ok);
}

#[test]
fn hmac_intact_valid_only_with_the_key() {
    let txt = read("hmac-intact.jsonl");
    assert!(
        verify_text(&txt, Some(b"korg-conformance-key"), None)
            .unwrap()
            .valid,
        "should be valid with the right HMAC key"
    );
    assert!(
        !verify_text(&txt, None, None).unwrap().valid,
        "a keyed chain must NOT verify without the key"
    );
}

#[test]
fn nonbmp_intact_is_valid() {
    // astral-plane code points — exercises the \uXXXX surrogate-pair canonicalization
    let v = verify_text(&read("nonbmp-intact.jsonl"), None, None).unwrap();
    assert!(v.valid, "errors: {:?}", v.errors);
}

#[test]
fn tampered_content_flags_seq_2() {
    let v = verify_text(&read("tampered-content.jsonl"), None, None).unwrap();
    assert!(!v.valid);
    assert!(
        v.errors.iter().any(|e| e.contains("seq 2")),
        "{:?}",
        v.errors
    );
}

#[test]
fn tampered_deletion_flags_seq_3() {
    let v = verify_text(&read("tampered-deletion.jsonl"), None, None).unwrap();
    assert!(!v.valid);
    assert!(
        v.errors.iter().any(|e| e.contains("seq 3")),
        "{:?}",
        v.errors
    );
}
