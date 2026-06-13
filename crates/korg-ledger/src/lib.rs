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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_sig_is_excluded_from_the_preimage() {
        // An event carrying an event_sig hashes identically to the same event
        // without it — the reserved signature field is not part of the chain.
        let base = json!({"seq_id": 1, "prev_hash": GENESIS_HASH, "x": "y"});
        let mut signed = base.clone();
        signed["event_sig"] = json!("ZmFrZS1zaWc=");
        assert_eq!(chain_hash(&base, None), chain_hash(&signed, None));
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
