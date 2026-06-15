//! korg-ledger@v1 — tamper-evident hash-chain (Rust reference implementation).
//!
//! The Rust implementation of the frozen `korg-ledger@v1` spec. It produces
//! byte-identical hashes to the Python and JS references; that cross-language
//! equivalence is pinned by frozen conformance vectors (see the `korg-verify`
//! and `korg-registry` conformance tests, which assert tip hashes computed by
//! the Python reference).
//!
//! Guarantee: a sequence of events is hash-chained — each carries `prev_hash`
//! (the previous event's `entry_hash`, GENESIS for the first) and `entry_hash`
//! (hash of its own canonical preimage). Any edit/delete/insert/reorder breaks
//! the chain and is localized to a `seq_id`. With an HMAC key the chain is
//! tamper-PROOF (unforgeable without the key), not merely tamper-evident.

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The chain anchor: `prev_hash` of the first event in a journal (64 zero hex chars).
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Reserved (Phase 2): out-of-band external-anchor sidecar file name. Holds
/// `{seq_id, entry_hash, anchor_proof, anchored_at}` records that notarize chain
/// tips. Kept OUTSIDE the chain preimage so it never affects `entry_hash`.
pub const ANCHORS_FILE: &str = "anchors.jsonl";

/// Anchor kind: the chain tip's `entry_hash` is committed to a public git repo,
/// whose immutable commit serves as the external witness that closes the
/// owner-rewrite-undetectably gap. `anchor_proof` carries
/// `{"repo": "<url>", "commit": "<sha>"}`.
pub const ANCHOR_KIND_GIT_TIP: &str = "git-tip";

/// Fields that ARE the hash/signature and so are excluded from the preimage.
/// `event_sig` is the reserved Phase-2 per-event signature slot: excluding it
/// in lockstep across all implementations means a signed event hashes the same
/// as the unsigned one, and unsigned events (which omit the field) are
/// unaffected.
const HASH_FIELDS: &[&str] = &["entry_hash", "event_sig"];

/// Canonical byte encoding of a JSON value (korg-ledger@v1 §2).
///
/// Reproduces Python `json.dumps(value, sort_keys=True, separators=(",",":"))`
/// with the default `ensure_ascii=True`:
///   - object keys sorted ascending by code point (UTF-8 byte order == code
///     point order for valid UTF-8);
///   - no insignificant whitespace;
///   - non-ASCII escaped as `\uXXXX` (lowercase), so output is pure ASCII.
pub fn canonicalize(value: &Value) -> Vec<u8> {
    let mut s = String::new();
    write_canonical(value, &mut s);
    s.into_bytes()
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_json_string(s, out),
        Value::Array(arr) => {
            out.push('[');
            for (i, e) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(e, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort(); // lexicographic by code point
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(k, out);
                out.push(':');
                write_canonical(&map[*k], out);
            }
            out.push('}');
        }
    }
}

/// Escape a string exactly as Python's `json.dumps(..., ensure_ascii=True)`:
/// short escapes for the standard controls, `\uXXXX` for everything outside the
/// printable-ASCII range `0x20..=0x7e` (surrogate pairs above U+FFFF).
fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if ('\u{20}'..='\u{7e}').contains(&c) => out.push(c),
            c => {
                let cp = c as u32;
                if cp > 0xFFFF {
                    let v = cp - 0x10000;
                    out.push_str(&format!("\\u{:04x}", 0xD800 + (v >> 10)));
                    out.push_str(&format!("\\u{:04x}", 0xDC00 + (v & 0x3FF)));
                } else {
                    out.push_str(&format!("\\u{:04x}", cp));
                }
            }
        }
    }
    out.push('"');
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Compute an event's `entry_hash` (korg-ledger@v1 §3).
///
/// Preimage = canonical encoding of the event with its hash field(s) removed
/// (`prev_hash` is kept — that's the chain link). With `key`, HMAC-SHA256;
/// otherwise SHA-256. Returns lowercase hex.
pub fn chain_hash(event: &Value, key: Option<&[u8]>) -> String {
    let mut obj = event.as_object().cloned().unwrap_or_default();
    for f in HASH_FIELDS {
        obj.remove(*f);
    }
    let data = canonicalize(&Value::Object(obj));
    match key {
        Some(k) => {
            let mut mac =
                Hmac::<Sha256>::new_from_slice(k).expect("HMAC accepts keys of any length");
            mac.update(&data);
            hex_lower(&mac.finalize().into_bytes())
        }
        None => {
            let mut h = Sha256::new();
            h.update(&data);
            hex_lower(&h.finalize())
        }
    }
}

/// Canonical preimage bytes for an event — exactly the bytes `chain_hash`
/// hashes (the event with `HASH_FIELDS` removed, canonicalized). Exposed so
/// signers/verifiers/anchors reuse the same bytes without duplicating logic.
pub fn event_preimage(event: &Value) -> Vec<u8> {
    let mut obj = event.as_object().cloned().unwrap_or_default();
    for f in HASH_FIELDS {
        obj.remove(*f);
    }
    canonicalize(&Value::Object(obj))
}

/// Ed25519-sign an event's canonical preimage (RFC 8032, pure — the raw
/// preimage bytes are the message, NOT their hash). Returns lowercase hex.
#[cfg(feature = "signing")]
pub fn sign_event(key: &ed25519_dalek::SigningKey, event: &Value) -> String {
    use ed25519_dalek::Signer;
    hex::encode(key.sign(&event_preimage(event)).to_bytes())
}

/// Verify an event's `event_sig` (lowercase hex) against a hex Ed25519 public
/// key. Returns false on any decode/length/verification error.
#[cfg(feature = "signing")]
pub fn verify_event_sig(pubkey_hex: &str, event: &Value, sig_hex: &str) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let pk: [u8; 32] = match hex::decode(pubkey_hex).ok().and_then(|b| b.try_into().ok()) {
        Some(p) => p,
        None => return false,
    };
    let vk = match VerifyingKey::from_bytes(&pk) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let sig: [u8; 64] = match hex::decode(sig_hex).ok().and_then(|b| b.try_into().ok()) {
        Some(s) => s,
        None => return false,
    };
    vk.verify(&event_preimage(event), &Signature::from_bytes(&sig))
        .is_ok()
}

/// Recompute the hash-chain and report tampering (korg-ledger@v1 §5).
/// Returns an empty vec iff the chain is intact; each error names a `seq_id`.
pub fn verify_chain(events: &[Value], key: Option<&[u8]>) -> Vec<String> {
    let mut errors = Vec::new();
    let mut expected_prev = GENESIS_HASH.to_string();
    for e in events {
        let sid = e
            .get("seq_id")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".to_string());
        let stored = e.get("entry_hash").and_then(|v| v.as_str());
        match stored {
            None => {
                errors.push(format!(
                    "seq {sid}: missing entry_hash (event is not chained)"
                ));
                // sentinel that cannot equal any real 64-hex hash → next link fails
                expected_prev = String::new();
            }
            Some(stored) => {
                let prev = e.get("prev_hash").and_then(|v| v.as_str()).unwrap_or("");
                if prev != expected_prev {
                    errors.push(format!(
                        "seq {sid}: prev_hash breaks the chain \
                         (an event was inserted, deleted, or reordered)"
                    ));
                }
                if chain_hash(e, key) != stored {
                    errors.push(format!(
                        "seq {sid}: entry_hash mismatch (content was tampered)"
                    ));
                }
                expected_prev = stored.to_string();
            }
        }
    }
    errors
}

/// Check the causal DAG is well-formed (korg-ledger@v1 §5): unique `seq_id`s,
/// and every `triggered_by` references an existing, strictly-earlier `seq_id`.
pub fn verify_dag(events: &[Value]) -> Vec<String> {
    let mut errors = Vec::new();
    let seqs: Vec<i64> = events
        .iter()
        .filter_map(|e| e.get("seq_id").and_then(|v| v.as_i64()))
        .collect();
    let seqset: std::collections::HashSet<i64> = seqs.iter().copied().collect();
    if seqset.len() != seqs.len() {
        errors.push("duplicate seq_id present".to_string());
    }
    for e in events {
        let tb = match e.get("triggered_by").and_then(|v| v.as_i64()) {
            Some(tb) => tb,
            None => continue,
        };
        let sid = e.get("seq_id").and_then(|v| v.as_i64());
        if !seqset.contains(&tb) {
            errors.push(format!("seq {sid:?}: triggered_by {tb} does not exist"));
        } else if let Some(sid) = sid {
            if tb >= sid {
                errors.push(format!(
                    "seq {sid}: triggered_by {tb} is not strictly earlier"
                ));
            }
        }
    }
    errors
}

/// Structural verification of an `anchors.jsonl` sidecar against an
/// already-verified chain (korg-ledger@v1 §8). For each anchor record, the
/// event at `seq_id` must exist in the chain and its `entry_hash` must equal the
/// anchor's. Returns an empty vec iff every anchor matches.
///
/// This is the LOCAL half of anchoring and is always hermetic. The EXTERNAL
/// half — checking that the anchor's `anchor_proof` (e.g. a public git commit)
/// actually witnesses that `entry_hash` — is what closes the owner-rewrite gap;
/// it is verified separately (network) and documented in the spec.
pub fn verify_anchors(chain: &[Value], anchors: &[Value]) -> Vec<String> {
    let mut errors = Vec::new();
    for a in anchors {
        let seq = a.get("seq_id").and_then(|v| v.as_u64());
        let want = a.get("entry_hash").and_then(|v| v.as_str());
        match (seq, want) {
            (Some(seq), Some(want)) => {
                match chain
                    .iter()
                    .find(|e| e.get("seq_id").and_then(|v| v.as_u64()) == Some(seq))
                {
                    None => errors.push(format!(
                        "anchor seq {seq}: no event with that seq_id in the chain"
                    )),
                    Some(e) if e.get("entry_hash").and_then(|v| v.as_str()) != Some(want) => errors
                        .push(format!(
                            "anchor seq {seq}: entry_hash does not match the chain"
                        )),
                    Some(_) => {}
                }
            }
            _ => errors.push("anchor record missing seq_id or entry_hash".into()),
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verify_anchors_accepts_correct_and_flags_wrong_or_missing() {
        use std::collections::BTreeMap;
        let payloads: Vec<BTreeMap<String, i64>> = (0..3)
            .map(|i| {
                let mut m = BTreeMap::new();
                m.insert("i".to_string(), i as i64);
                m
            })
            .collect();
        let chain = chain_for_anchors(&payloads);
        let tip = chain[2]["entry_hash"].as_str().unwrap().to_string();

        let good = json!([{"seq_id": 3, "entry_hash": tip, "anchor_kind": ANCHOR_KIND_GIT_TIP}]);
        assert!(verify_anchors(&chain, good.as_array().unwrap()).is_empty());

        let wrong = json!([{"seq_id": 3, "entry_hash": "deadbeef"}]);
        let errs = verify_anchors(&chain, wrong.as_array().unwrap());
        assert!(errs.iter().any(|e| e.contains("seq 3")), "{errs:?}");

        let missing = json!([{"seq_id": 99, "entry_hash": tip}]);
        assert!(!verify_anchors(&chain, missing.as_array().unwrap()).is_empty());
    }

    // local helper so the anchor test doesn't depend on the proptest build_chain
    fn chain_for_anchors(payloads: &[std::collections::BTreeMap<String, i64>]) -> Vec<Value> {
        let mut out = Vec::new();
        let mut prev = GENESIS_HASH.to_string();
        for (i, p) in payloads.iter().enumerate() {
            let mut obj = serde_json::Map::new();
            obj.insert("seq_id".into(), json!(i as u64 + 1));
            obj.insert("prev_hash".into(), json!(prev));
            obj.insert("payload".into(), serde_json::to_value(p).unwrap());
            let mut val = Value::Object(obj);
            let h = chain_hash(&val, None);
            val.as_object_mut()
                .unwrap()
                .insert("entry_hash".into(), json!(h));
            prev = h;
            out.push(val);
        }
        out
    }

    #[test]
    fn event_sig_is_excluded_from_the_preimage() {
        // An event carrying an event_sig hashes identically to the same event
        // without it — the reserved signature field is not part of the chain.
        let base = json!({"seq_id": 1, "prev_hash": GENESIS_HASH, "x": "y"});
        let mut signed = base.clone();
        signed["event_sig"] = json!("ZmFrZS1zaWc=");
        assert_eq!(chain_hash(&base, None), chain_hash(&signed, None));
    }

    use proptest::prelude::*;
    use std::collections::BTreeMap;

    /// Build a valid hash-chain from arbitrary small payloads.
    fn build_chain(payloads: &[BTreeMap<String, i64>], key: Option<&[u8]>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut prev = GENESIS_HASH.to_string();
        for (i, p) in payloads.iter().enumerate() {
            let mut obj = serde_json::Map::new();
            obj.insert("seq_id".into(), json!(i as u64 + 1));
            obj.insert("prev_hash".into(), json!(prev));
            obj.insert("payload".into(), serde_json::to_value(p).unwrap());
            let mut val = Value::Object(obj);
            let h = chain_hash(&val, key);
            val.as_object_mut()
                .unwrap()
                .insert("entry_hash".into(), json!(h));
            prev = h;
            out.push(val);
        }
        out
    }

    proptest! {
        // Any well-formed chain verifies clean (no false positives), keyed or not.
        #[test]
        fn any_built_chain_verifies_clean(
            payloads in prop::collection::vec(
                prop::collection::btree_map("[a-z]{1,5}", any::<i64>(), 0..4), 0..12),
            use_key in any::<bool>(),
        ) {
            let key: Option<&[u8]> = if use_key { Some(b"k") } else { None };
            prop_assert!(verify_chain(&build_chain(&payloads, key), key).is_empty());
        }

        // Mutating ANY event's content is detected (no false negatives).
        #[test]
        fn tampering_any_event_is_detected(
            payloads in prop::collection::vec(
                prop::collection::btree_map("[a-z]{1,5}", any::<i64>(), 1..4), 1..8),
            idx in any::<usize>(),
        ) {
            let mut chain = build_chain(&payloads, None);
            let i = idx % chain.len();
            chain[i].as_object_mut().unwrap().insert("TAMPER".into(), json!(1));
            prop_assert!(!verify_chain(&chain, None).is_empty());
        }

        // Reordering two distinct events breaks the chain.
        #[test]
        fn reordering_breaks_the_chain(
            payloads in prop::collection::vec(
                prop::collection::btree_map("[a-z]{1,5}", any::<i64>(), 1..4), 2..8),
        ) {
            let mut chain = build_chain(&payloads, None);
            let last = chain.len() - 1;
            chain.swap(0, last);
            prop_assert!(!verify_chain(&chain, None).is_empty());
        }

        // canonicalize is stable across a JSON round-trip for arbitrary unicode.
        #[test]
        fn canonicalize_round_trips_unicode(s in ".*") {
            let v = json!({ "k": s });
            let once = canonicalize(&v);
            let reparsed: Value = serde_json::from_slice(&once).unwrap();
            prop_assert_eq!(once, canonicalize(&reparsed));
        }

        // canonicalize is insertion-order independent (keys are sorted).
        // Use a btree_map input so keys are unique — duplicate keys would make
        // forward vs reverse insertion legitimately differ (last-write-wins).
        #[test]
        fn canonicalize_is_key_order_independent(
            m in prop::collection::btree_map("[a-z]{1,6}", any::<i64>(), 0..8),
        ) {
            let mut a = serde_json::Map::new();
            for (k, v) in m.iter() { a.insert(k.clone(), json!(v)); }
            let mut b = serde_json::Map::new();
            for (k, v) in m.iter().rev() { b.insert(k.clone(), json!(v)); }
            prop_assert_eq!(canonicalize(&Value::Object(a)), canonicalize(&Value::Object(b)));
        }
    }

    #[test]
    fn canonicalize_sorts_keys_and_is_compact() {
        assert_eq!(
            canonicalize(&json!({"z":[3,2],"a":{"y":1,"x":2}})),
            b"{\"a\":{\"x\":2,\"y\":1},\"z\":[3,2]}"
        );
    }

    #[test]
    fn canonicalize_escapes_non_ascii() {
        // matches Python ensure_ascii: é → é
        assert_eq!(
            canonicalize(&json!({"a":"é"})),
            b"{\"a\":\"\\u00e9\"}".to_vec()
        );
    }

    #[test]
    fn entry_hash_excludes_itself_but_keeps_prev_hash() {
        let ev = json!({"seq_id":1,"tool_name":"x","prev_hash":GENESIS_HASH});
        let h = chain_hash(&ev, None);
        let mut with = ev.clone();
        with["entry_hash"] = json!("anything");
        assert_eq!(chain_hash(&with, None), h);
        // changing prev_hash DOES change the hash (it's part of the preimage)
        let mut other = ev.clone();
        other["prev_hash"] = json!("ff");
        assert_ne!(chain_hash(&other, None), h);
    }
}

#[cfg(all(test, feature = "signing"))]
mod signing_tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    #[test]
    fn sign_then_verify_roundtrips_and_detects_tampering() {
        let key = SigningKey::from_bytes(&[42u8; 32]);
        let pk_hex = hex::encode(key.verifying_key().to_bytes());
        let ev = json!({"seq_id": 1, "prev_hash": GENESIS_HASH, "payload": {"a": 1}, "entry_hash": "deadbeef"});
        let sig = sign_event(&key, &ev);
        assert_eq!(sig.len(), 128); // 64-byte signature as lowercase hex
        assert!(verify_event_sig(&pk_hex, &ev, &sig));
        // tampering event content breaks verification
        let mut tampered = ev.clone();
        tampered["payload"] = json!({"a": 2});
        assert!(!verify_event_sig(&pk_hex, &tampered, &sig));
        // a wrong pubkey / malformed sig returns false (never panics)
        assert!(!verify_event_sig("not-hex", &ev, &sig));
        assert!(!verify_event_sig(&pk_hex, &ev, "00"));
    }

    #[test]
    fn signature_excludes_entry_hash_and_event_sig_from_the_preimage() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let a = json!({"seq_id": 1, "prev_hash": GENESIS_HASH, "x": "y"});
        let mut b = a.clone();
        b["entry_hash"] = json!("anything");
        b["event_sig"] = json!("anything");
        assert_eq!(sign_event(&key, &a), sign_event(&key, &b));
    }
}
