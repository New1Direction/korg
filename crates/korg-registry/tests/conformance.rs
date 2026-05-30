//! korg-ledger@v1 cross-language conformance.
//!
//! The Python reference (korgex) froze the chain definition and a set of golden
//! vectors with FROZEN tip hashes (tests/conformance/conformance.json). This
//! test proves the Rust implementation reproduces those exact hashes — i.e. that
//! `korg-ledger@v1` is genuinely a multi-language standard, not a Python detail.
//! If Rust's canonicalization or hashing diverged by a single byte, the frozen
//! tip would not match and this test would fail.

use korg_registry::ledger_chain::{chain_hash, verify_chain, GENESIS_HASH};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn cdir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/conformance"))
}

fn read_jsonl(name: &str) -> Vec<Value> {
    fs::read_to_string(cdir().join(name))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn genesis_hash_is_64_zeros() {
    assert_eq!(GENESIS_HASH, "0".repeat(64));
}

#[test]
fn conformance_vectors_reproduce_the_frozen_oracle() {
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(cdir().join("conformance.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["spec_version"], "korg-ledger@v1");
    let vectors = manifest["vectors"].as_array().unwrap();
    assert!(!vectors.is_empty(), "no conformance vectors");

    for v in vectors {
        let file = v["file"].as_str().unwrap();
        let events = read_jsonl(file);
        let key_owned = v["key"].as_str().map(|s| s.as_bytes().to_vec());
        let key = key_owned.as_deref();
        let errors = verify_chain(&events, key);

        match v["verify"].as_str().unwrap() {
            "intact" => {
                // verify_chain passing already proves Rust recomputes Python's
                // hashes (it compares chain_hash() to each stored entry_hash).
                assert!(errors.is_empty(), "{file}: expected intact, got {errors:?}");
                // and the explicit cross-impl anchor: Rust chain_hash == frozen tip.
                let tip = chain_hash(events.last().unwrap(), key);
                assert_eq!(
                    tip,
                    v["tip_entry_hash"].as_str().unwrap(),
                    "{file}: Rust chain_hash must reproduce the frozen tip"
                );
            }
            _ => {
                assert!(
                    !errors.is_empty(),
                    "{file}: expected tampered, verified clean"
                );
                let needle = v["error_contains"].as_str().unwrap();
                assert!(
                    errors.iter().any(|e| e.contains(needle)),
                    "{file}: errors {errors:?} missing {needle:?}"
                );
            }
        }
    }
}

#[test]
fn hmac_chain_fails_without_the_key() {
    // tamper-PROOF: a keyed chain verified with no key must fail.
    let events = read_jsonl("hmac-intact.jsonl");
    assert!(
        !verify_chain(&events, None).is_empty(),
        "keyed chain wrongly verified without the key"
    );
}

/// Non-BMP / surrogate-pair determinism oracle.
///
/// The events in `nonbmp-intact.jsonl` carry astral-plane code points —
/// 😀 (U+1F600), 💩 (U+1F4A9), 𐀀 (U+10000, the very first astral code point) —
/// alongside BMP CJK (中文). Python's `json.dumps(ensure_ascii=True)` escapes
/// each astral code point as a UTF-16 surrogate *pair* (`😀`, …),
/// which is exactly the `cp > 0xFFFF` branch in `write_json_string`. The frozen
/// tip `045d2d8…` was computed by the Python reference. If Rust's surrogate
/// arithmetic or non-ASCII escaping diverged by a single byte, this tip would
/// not reproduce — so this test is the empirical proof that the moat holds on
/// non-BMP text, not merely on ASCII.
#[test]
fn nonbmp_surrogate_chain_reproduces_python_frozen_tip() {
    const FROZEN_TIP: &str = "045d2d865e89c54340408b309c81d6b0f1367ebf0aa4504f1af66f7f1e4cb590";
    let events = read_jsonl("nonbmp-intact.jsonl");

    // The full chain must verify (each stored entry_hash recomputed by Rust).
    let errors = verify_chain(&events, None);
    assert!(
        errors.is_empty(),
        "non-BMP chain failed to verify: {errors:?}"
    );

    // Explicit cross-impl anchor: Rust's chain_hash of the tip event must equal
    // the Python-frozen tip, byte-for-byte through the surrogate-pair escaper.
    let tip = chain_hash(events.last().unwrap(), None);
    assert_eq!(
        tip, FROZEN_TIP,
        "Rust chain_hash diverged from Python on non-BMP text — moat-breaking \
         surrogate/escaping bug in write_canonical"
    );
}
