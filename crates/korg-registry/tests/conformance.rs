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
