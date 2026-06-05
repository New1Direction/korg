//! korg-verify — an independent, dependency-light verifier for korg receipts and
//! journals.
//!
//! It carries its own vendored, conformance-tested chain primitives (see
//! [`chain`]: `canonicalize` / `chain_hash` / `verify_chain` / `verify_dag` —
//! proven byte-identical to the Python and JS implementations against the frozen
//! korg-ledger@v1 vectors) and adds the receipt envelope plus the Ed25519
//! tip-signature check. No workspace deps, no network, no Python runtime: a single
//! binary anyone can run to check a sealed deliverable, with zero trust in the tool
//! that produced it.
//!
//! What a green verdict proves: the recorded events hash-chain intact and link in a
//! well-formed causal DAG (tamper-evident), the receipt's tip matches the chain head,
//! and — if signed — the holder of the named key attests to that exact tip. What it
//! does NOT prove on its own: *when* it happened (needs an external time anchor) or
//! that the key maps to a real-world identity (the relying party pins that — see
//! `--pubkey`).

pub mod chain;

use crate::chain::{verify_chain, verify_dag};
use serde_json::Value;

/// The outcome of verifying a receipt or journal. `valid` is the conjunction of every
/// applicable check; `signature_ok` is `None` when the artifact is unsigned (not
/// applicable — not a failure).
#[derive(Debug, Clone)]
pub struct Verdict {
    pub valid: bool,
    pub kind: &'static str, // "receipt" | "journal"
    pub event_count: usize,
    pub chain_ok: bool,
    pub dag_ok: bool,
    pub tip_ok: bool,
    pub signature_ok: Option<bool>,
    pub signer: Option<String>,
    pub errors: Vec<String>,
}

/// Load events from either on-disk shape: a single JSON array, or JSON Lines.
pub fn load_events(text: &str) -> Result<Vec<Value>, String> {
    if text.trim_start().starts_with('[') {
        return serde_json::from_str(text).map_err(|e| format!("invalid JSON array: {e}"));
    }
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line).map_err(|e| format!("line {}: {e}", i + 1))?);
    }
    Ok(out)
}

/// Verify an Ed25519 signature over the RAW tip-hash bytes — matching `sign_tip`,
/// which signs `bytes.fromhex(tip)` (the 32 hash bytes, not the hex string). Any
/// malformed input returns `false` rather than panicking.
pub fn verify_tip_sig(pubkey_hex: &str, tip_hex: &str, sig_hex: &str) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let (Ok(pk), Ok(msg), Ok(sig)) = (
        hex::decode(pubkey_hex),
        hex::decode(tip_hex),
        hex::decode(sig_hex),
    ) else {
        return false;
    };
    let (Ok(pk), Ok(sig)) = (
        <[u8; 32]>::try_from(pk.as_slice()),
        <[u8; 64]>::try_from(sig.as_slice()),
    ) else {
        return false;
    };
    match VerifyingKey::from_bytes(&pk) {
        Ok(vk) => vk.verify(&msg, &Signature::from_bytes(&sig)).is_ok(),
        Err(_) => false,
    }
}

/// Verify a list of events as a journal: hash chain + causal DAG.
pub fn verify_journal(events: &[Value], key: Option<&[u8]>) -> Verdict {
    let mut errors = verify_chain(events, key);
    let dag = verify_dag(events);
    let chain_ok = errors.is_empty();
    let dag_ok = dag.is_empty();
    errors.extend(dag);
    Verdict {
        valid: chain_ok && dag_ok,
        kind: "journal",
        event_count: events.len(),
        chain_ok,
        dag_ok,
        tip_ok: true, // a bare journal makes no separate tip claim
        signature_ok: None,
        signer: None,
        errors,
    }
}

/// Verify a receipt object: embedded events (chain + DAG), the recorded tip matches
/// the chain head, and — if signed — the Ed25519 signature is valid for that tip.
///
/// `pin_pubkey`: require the signer to equal this key (else INVALID). This closes the
/// self-referential hole where a bare check only proves the signature matches the
/// *returned* key, not a key the relying party already trusts.
pub fn verify_receipt(receipt: &Value, key: Option<&[u8]>, pin_pubkey: Option<&str>) -> Verdict {
    let events: Vec<Value> = receipt
        .get("events")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();

    let mut errors = verify_chain(&events, key);
    let dag = verify_dag(&events);
    let chain_ok = errors.is_empty();
    let dag_ok = dag.is_empty();
    errors.extend(dag);

    let claimed_tip = receipt.get("tip").and_then(|t| t.as_str());
    let head = events
        .last()
        .and_then(|e| e.get("entry_hash"))
        .and_then(|h| h.as_str());
    let tip_ok = match (claimed_tip, head) {
        (Some(c), Some(h)) => c == h,
        (None, _) => true,
        (Some(_), None) => false,
    };
    if !tip_ok {
        errors.push("recorded tip does not match the chain head".to_string());
    }

    let mut signature_ok = None;
    let mut signer = None;
    if let Some(sig) = receipt.get("signature") {
        let pubkey = sig.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
        let sig_hex = sig.get("sig").and_then(|v| v.as_str()).unwrap_or("");
        let mut ok = verify_tip_sig(pubkey, claimed_tip.unwrap_or(""), sig_hex);
        signer = Some(pubkey.to_string());
        if !ok {
            errors.push("signature does not verify for the recorded tip".to_string());
        }
        if let Some(pin) = pin_pubkey {
            if pin != pubkey {
                ok = false;
                errors.push(format!(
                    "signer {pubkey} does not match the pinned key {pin}"
                ));
            }
        }
        signature_ok = Some(ok);
    } else if let Some(pin) = pin_pubkey {
        signature_ok = Some(false);
        errors.push(format!("receipt is unsigned but signer {pin} was required"));
    }

    let valid = chain_ok && dag_ok && tip_ok && signature_ok != Some(false);
    Verdict {
        valid,
        kind: "receipt",
        event_count: events.len(),
        chain_ok,
        dag_ok,
        tip_ok,
        signature_ok,
        signer,
        errors,
    }
}

/// Auto-detect a receipt (`{…,"events":[…]}` or `schema: korgex-receipt@*`) vs a
/// journal (array or JSONL) and verify accordingly.
pub fn verify_text(
    text: &str,
    key: Option<&[u8]>,
    pin_pubkey: Option<&str>,
) -> Result<Verdict, String> {
    // A receipt is a single JSON object; a JSONL journal also starts with '{' but is
    // many objects, so only treat it as a receipt if the WHOLE text parses as one
    // object — otherwise fall through to the line/array journal loader.
    if text.trim_start().starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            let is_receipt = v.get("events").is_some()
                || v.get("schema")
                    .and_then(|s| s.as_str())
                    .is_some_and(|s| s.starts_with("korgex-receipt"));
            if is_receipt {
                return Ok(verify_receipt(&v, key, pin_pubkey));
            }
            return Ok(verify_journal(std::slice::from_ref(&v), key));
        }
    }
    Ok(verify_journal(&load_events(text)?, key))
}
