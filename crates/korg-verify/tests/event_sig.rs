//! Cross-implementation per-event signature interop.
//!
//! `signed-events.jsonl` is a frozen fixture signed by the PYTHON implementation
//! (seed `[42; 32]`). This proves: (1) Python-signed events verify under Rust,
//! and (2) Rust re-signs the identical events to byte-identical signatures
//! (Ed25519 is deterministic — RFC 8032), so the message bytes are identical
//! across implementations. The JS verifier checks the same fixture in
//! `spec/korg-ledger-v1/js/conformance.mjs`.

use ed25519_dalek::SigningKey;
use korg_ledger::sign_event;
use serde_json::Value;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    ))
    .unwrap()
}

fn fixture_events() -> Vec<Value> {
    fixture("signed-events.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn python_signed_events_verify_under_rust_and_resign_identically() {
    let pubkey = fixture("signed-events.pubkey");
    let pubkey = pubkey.trim();
    let events = fixture_events();
    assert_eq!(events.len(), 2);
    let key = SigningKey::from_bytes(&[42u8; 32]);

    for e in &events {
        let sig = e["event_sig"]
            .as_str()
            .expect("fixture event must carry event_sig");
        // (1) Python-signed signature verifies under the Rust verifier.
        assert!(
            korg_verify::verify_event_sig(pubkey, e, sig),
            "Python-signed event_sig must verify under Rust at seq {}",
            e["seq_id"]
        );
        // (2) Rust re-signs the same event to byte-identical hex (deterministic).
        assert_eq!(
            sign_event(&key, e),
            sig,
            "Rust sign_event must equal the Python signature at seq {}",
            e["seq_id"]
        );
    }
}

#[test]
fn verify_journal_extended_folds_event_sigs_and_anchors() {
    let pubkey = fixture("signed-events.pubkey");
    let pubkey = pubkey.trim();
    let events = fixture_events();
    let tip = events.last().unwrap()["entry_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let tip_seq = events.last().unwrap()["seq_id"].as_u64().unwrap();

    // pinned event pubkey: every event_sig verifies
    let v = korg_verify::verify_journal_extended(&events, None, Some(pubkey), None);
    assert_eq!(v.event_sigs_ok, Some(true));
    assert!(v.valid);

    // a wrong pinned pubkey fails the per-event check
    let wrong = "00".repeat(32);
    let v = korg_verify::verify_journal_extended(&events, None, Some(&wrong), None);
    assert_eq!(v.event_sigs_ok, Some(false));
    assert!(!v.valid);

    // structural anchors: a correct anchor passes, a wrong one fails
    let good =
        vec![serde_json::json!({"seq_id": tip_seq, "entry_hash": tip, "anchor_kind": "git-tip"})];
    let v = korg_verify::verify_journal_extended(&events, None, None, Some(&good));
    assert_eq!(v.anchors_ok, Some(true));
    assert!(v.valid);

    let bad = vec![serde_json::json!({"seq_id": tip_seq, "entry_hash": "deadbeef"})];
    let v = korg_verify::verify_journal_extended(&events, None, None, Some(&bad));
    assert_eq!(v.anchors_ok, Some(false));
    assert!(!v.valid);
}

#[test]
fn tampered_signature_is_rejected() {
    let pubkey = fixture("signed-events.pubkey");
    let events = fixture_events();
    assert!(!korg_verify::verify_event_sig(
        pubkey.trim(),
        &events[0],
        &"0".repeat(128)
    ));
}
