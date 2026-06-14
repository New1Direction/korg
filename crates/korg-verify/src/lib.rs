//! korg-verify — an independent, dependency-light verifier for korg receipts and
//! journals.
//!
//! It reuses the conformance-tested chain primitives in `korg-ledger`
//! (`canonicalize` / `chain_hash` / `verify_chain` / `verify_dag` — proven
//! byte-identical to the Python and JS implementations against the frozen
//! korg-ledger@v1 vectors) and adds the receipt envelope plus the Ed25519
//! tip-signature check. No network, no Python runtime: a single binary anyone can
//! run to check a sealed deliverable, with zero trust in the tool that produced it.
//!
//! What a green verdict proves: the recorded events hash-chain intact and link in a
//! well-formed causal DAG (tamper-evident), the receipt's tip matches the chain head,
//! and — if signed — the holder of the named key attests to that exact tip. What it
//! does NOT prove on its own: *when* it happened (needs an external time anchor) or
//! that the key maps to a real-world identity (the relying party pins that — see
//! `--pubkey`).

use korg_ledger::{verify_chain, verify_dag};
use serde_json::Value;

/// Verify a single event's `event_sig` (lowercase hex) against a hex Ed25519
/// public key, over the event's canonical preimage. Delegates to the
/// conformance-tested `korg_ledger::verify_event_sig` so Rust, Python, and JS
/// all check the identical message bytes. False on any error.
pub fn verify_event_sig(pubkey_hex: &str, event: &Value, sig_hex: &str) -> bool {
    korg_ledger::verify_event_sig(pubkey_hex, event, sig_hex)
}

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
    /// `None` when no `--pin-event-pubkey` was supplied; `Some(true/false)` when
    /// per-event `event_sig`s were checked against the pinned key.
    pub event_sigs_ok: Option<bool>,
    /// `None` when no `--anchors` sidecar was supplied; `Some(true/false)` for the
    /// structural anchor check (each anchor's entry_hash matches the chain).
    pub anchors_ok: Option<bool>,
    /// `None` for receipts/journals; `Some(true/false)` for a goldseal@v1 — whether
    /// the embedded summary byte-matches the summary re-derived from the events.
    pub summary_ok: Option<bool>,
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
    // The message MUST be a 32-byte tip hash. Without this, an empty/short tip
    // (`tip_hex == ""`) decodes to a 0-byte message that an attacker can sign with
    // their own key — forging a "validly signed" tipless receipt.
    let (Ok(pk), Ok(msg), Ok(sig)) = (
        <[u8; 32]>::try_from(pk.as_slice()),
        <[u8; 32]>::try_from(msg.as_slice()),
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
        event_sigs_ok: None,
        anchors_ok: None,
        summary_ok: None,
        errors,
    }
}

/// Journal verification plus the optional Phase-2 checks. When
/// `pin_event_pubkey` is supplied, every event's `event_sig` must verify under
/// that key (a missing or invalid signature fails the verdict). When `anchors`
/// is supplied, each anchor's `entry_hash` must match the chain at its `seq_id`
/// (the structural half; the external git-tip proof is a separate network step).
pub fn verify_journal_extended(
    events: &[Value],
    key: Option<&[u8]>,
    pin_event_pubkey: Option<&str>,
    anchors: Option<&[Value]>,
) -> Verdict {
    let mut v = verify_journal(events, key);
    if let Some(pk) = pin_event_pubkey {
        let mut all_ok = true;
        for e in events {
            let seq = e
                .get("seq_id")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "?".into());
            match e.get("event_sig").and_then(|s| s.as_str()) {
                Some(sig) if verify_event_sig(pk, e, sig) => {}
                Some(_) => {
                    all_ok = false;
                    v.errors.push(format!(
                        "seq {seq}: event_sig does not verify for the pinned key"
                    ));
                }
                None => {
                    all_ok = false;
                    v.errors.push(format!(
                        "seq {seq}: missing event_sig but a signer was required"
                    ));
                }
            }
        }
        v.event_sigs_ok = Some(all_ok);
        if !all_ok {
            v.valid = false;
        }
    }
    if let Some(anchors) = anchors {
        let errs = korg_ledger::verify_anchors(events, anchors);
        let ok = errs.is_empty();
        v.anchors_ok = Some(ok);
        v.errors.extend(errs);
        if !ok {
            v.valid = false;
        }
    }
    v
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

    // A receipt with no events makes no attestation — reject it rather than
    // letting an empty/tipless object pass as "valid".
    let events_ok = !events.is_empty();
    if !events_ok {
        errors.push("receipt contains no events".to_string());
    }

    let mut signature_ok = None;
    let mut signer = None;
    if let Some(sig) = receipt.get("signature") {
        let pubkey = sig.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
        let sig_hex = sig.get("sig").and_then(|v| v.as_str()).unwrap_or("");
        signer = Some(pubkey.to_string());
        // A signature must attest to a real tip. Fail closed if there is none,
        // rather than verifying over an empty message.
        let mut ok = match claimed_tip {
            Some(tip) => {
                let v = verify_tip_sig(pubkey, tip, sig_hex);
                if !v {
                    errors.push("signature does not verify for the recorded tip".to_string());
                }
                v
            }
            None => {
                errors.push("receipt carries a signature but no tip to attest to".to_string());
                false
            }
        };
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

    let domain_ok = match korg_ledger::canon_domain_error(receipt) {
        Some(msg) => {
            errors.push(msg);
            false
        }
        None => true,
    };
    let valid =
        events_ok && chain_ok && dag_ok && tip_ok && domain_ok && signature_ok != Some(false);
    Verdict {
        valid,
        kind: "receipt",
        event_count: events.len(),
        chain_ok,
        dag_ok,
        tip_ok,
        signature_ok,
        signer,
        event_sigs_ok: None,
        anchors_ok: None,
        summary_ok: None,
        errors,
    }
}

/// A Gold Seal's bound, human-legible summary, derived as a *pure function* of the
/// event chain. Byte-identical to the Python (`korg_ledger.goldseal.derive_summary`)
/// and JS (`deriveSummary`) implementations — a verifier re-derives it and rejects
/// any mismatch, so the summary cannot lie about what the agent did.
fn derive_summary(events: &[Value]) -> Value {
    use serde_json::Map;
    use std::collections::{BTreeMap, BTreeSet};

    let mut by_tool: BTreeMap<String, i64> = BTreeMap::new();
    let mut files: BTreeSet<String> = BTreeSet::new();
    let mut agents: BTreeSet<String> = BTreeSet::new();
    let mut seqs: Vec<i64> = Vec::new();

    for e in events {
        // Normalize either event shape: nested JournalEvent (`event: {…}`) or flat.
        let view = e.get("event").filter(|v| v.is_object()).unwrap_or(e);
        if let Some(t) = view.get("tool_name").and_then(|v| v.as_str()) {
            *by_tool.entry(t.to_string()).or_insert(0) += 1;
        }
        if let Some(a) = view.get("source_agent").and_then(|v| v.as_str()) {
            agents.insert(a.to_string());
        }
        if let Some(args) = view.get("args").and_then(|v| v.as_object()) {
            for key in ["file_path", "path"] {
                if let Some(p) = args.get(key).and_then(|v| v.as_str()) {
                    files.insert(p.to_string());
                }
            }
        }
        if let Some(s) = e.get("seq_id").and_then(|v| v.as_i64()) {
            seqs.push(s);
        }
    }

    let by_tool_obj: Map<String, Value> = by_tool
        .into_iter()
        .map(|(k, v)| (k, Value::from(v)))
        .collect();
    serde_json::json!({
        "agents": agents.into_iter().collect::<Vec<_>>(),
        "by_tool": Value::Object(by_tool_obj),
        "files": files.into_iter().collect::<Vec<_>>(),
        "seq_first": seqs.iter().min().copied().unwrap_or(0),
        "seq_last": seqs.iter().max().copied().unwrap_or(0),
    })
}

/// The signed portion of a Gold Seal: the envelope minus `events` and `seal` (so
/// it includes `anchors` when present — the seal commits to the anchor set). Its
/// canonicalization is the seal-signature preimage (identical at mint and verify).
fn seal_header(envelope: &Value) -> Value {
    let mut obj = envelope.as_object().cloned().unwrap_or_default();
    obj.remove("events");
    obj.remove("seal");
    Value::Object(obj)
}

/// Verify an Ed25519 seal signature over a header's canonical bytes — the
/// seal-level analogue of `verify_event_sig`. False on any malformed input.
fn verify_seal_sig(pubkey_hex: &str, header: &Value, sig_hex: &str) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let msg = korg_ledger::canonicalize(header);
    let (Ok(pk), Ok(sig)) = (hex::decode(pubkey_hex), hex::decode(sig_hex)) else {
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

/// Verify a `goldseal@v1` certificate: the embedded chain + DAG, the recorded tip
/// and event_count, the **re-derived summary** (byte-equal to the embedded one),
/// and the issuer's Ed25519 seal over the canonical header. A goldseal is a receipt
/// superset, so it also passes an older receipt-only verifier (chain + DAG + tip) —
/// minus the summary/seal guarantees. A stripped seal fails the verdict (downgrade).
///
/// `pin_pubkey`: require the issuer to equal a key the relying party already trusts.
pub fn verify_goldseal(envelope: &Value, pin_pubkey: Option<&str>) -> Verdict {
    let events: Vec<Value> = envelope
        .get("events")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();

    let mut errors = verify_chain(&events, None);
    let dag = verify_dag(&events);
    let chain_ok = errors.is_empty();
    let dag_ok = dag.is_empty();
    errors.extend(dag);

    let schema_ok = envelope.get("schema").and_then(|s| s.as_str()) == Some("goldseal@v1");
    if !schema_ok {
        errors.push("schema is not goldseal@v1".to_string());
    }

    let claimed_tip = envelope.get("tip").and_then(|t| t.as_str());
    let head = events
        .last()
        .and_then(|e| e.get("entry_hash"))
        .and_then(|h| h.as_str());
    let tip_ok = matches!((claimed_tip, head), (Some(c), Some(h)) if c == h);
    if !tip_ok {
        errors.push("recorded tip does not match the chain head".to_string());
    }

    let count_ok =
        envelope.get("event_count").and_then(|c| c.as_u64()) == Some(events.len() as u64);
    if !count_ok {
        errors.push(format!(
            "event_count does not match the {} embedded events",
            events.len()
        ));
    }

    let derived = derive_summary(&events);
    let claimed_summary = envelope.get("summary").cloned().unwrap_or(Value::Null);
    let summary_ok =
        korg_ledger::canonicalize(&derived) == korg_ledger::canonicalize(&claimed_summary);
    if !summary_ok {
        errors.push("summary does not match the events (re-derivation mismatch)".to_string());
    }

    let anchors_ok = match envelope
        .get("anchors")
        .and_then(|a| a.as_array())
        .filter(|a| !a.is_empty())
    {
        Some(anchors) => {
            let errs = korg_ledger::verify_anchors(&events, anchors);
            let ok = errs.is_empty();
            errors.extend(errs);
            Some(ok)
        }
        None => None,
    };

    let (signature_ok, signer) = if let Some(seal) = envelope.get("seal").filter(|v| v.is_object())
    {
        let pubkey = seal.get("pubkey").and_then(|v| v.as_str()).unwrap_or("");
        let sig = seal.get("sig").and_then(|v| v.as_str()).unwrap_or("");
        let header = seal_header(envelope);
        let mut ok = verify_seal_sig(pubkey, &header, sig);
        if !ok {
            errors.push("seal signature does not verify for the header".to_string());
        }
        if let Some(pin) = pin_pubkey {
            if pin != pubkey {
                ok = false;
                errors.push(format!(
                    "issuer {pubkey} does not match the pinned key {pin}"
                ));
            }
        }
        (Some(ok), Some(pubkey.to_string()))
    } else {
        // A goldseal@v1 MUST carry a seal — a stripped seal is a downgrade, not a
        // merely-unsigned artifact. Fails the verdict in all implementations.
        errors.push(match pin_pubkey {
            Some(pin) => format!("seal is absent but signer {pin} was required"),
            None => "seal is absent (unsigned Gold Seal)".to_string(),
        });
        (Some(false), None)
    };

    // Reject envelope-level out-of-domain numbers (e.g. a big issued_at/summary
    // count) so Rust agrees with Python/JS, which reject them at canonicalize time.
    let domain_ok = match korg_ledger::canon_domain_error(envelope) {
        Some(msg) => {
            errors.push(msg);
            false
        }
        None => true,
    };

    let valid = chain_ok
        && dag_ok
        && schema_ok
        && tip_ok
        && count_ok
        && summary_ok
        && domain_ok
        && signature_ok != Some(false)
        && anchors_ok != Some(false);
    Verdict {
        valid,
        kind: "goldseal",
        event_count: events.len(),
        chain_ok,
        dag_ok,
        tip_ok,
        signature_ok,
        signer,
        event_sigs_ok: None,
        anchors_ok,
        summary_ok: Some(summary_ok),
        errors,
    }
}

/// Auto-detect a goldseal@v1 certificate, a receipt (`{…,"events":[…]}` or
/// `schema: korgex-receipt@*`), or a journal (array or JSONL) and verify accordingly.
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
            if v.get("schema")
                .and_then(|s| s.as_str())
                .is_some_and(|s| s.starts_with("goldseal"))
            {
                return Ok(verify_goldseal(&v, pin_pubkey));
            }
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
