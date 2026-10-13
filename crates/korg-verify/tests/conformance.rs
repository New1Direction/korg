//! korg-verify against the SHARED frozen korg-ledger@v1 vectors in the canonical
//! spec directory (`spec/korg-ledger-v1/conformance.json` + `vectors/`) — the
//! same manifest the Python and JS implementations are checked against. A green
//! run here means a third independent implementation reproduces the chain, the
//! exact causal-DAG errors (§9), and the redaction lint (REDACTION.md).

use korg_verify::{verify_journal, verify_redaction, verify_text};
use serde_json::Value;
use std::path::PathBuf;

fn spec_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/korg-ledger-v1"
    ))
}

fn read(name: &str) -> String {
    let p = spec_dir().join("vectors").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn manifest() -> Value {
    serde_json::from_str(&std::fs::read_to_string(spec_dir().join("conformance.json")).unwrap())
        .unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn every_manifest_vector_reproduces_chain_dag_and_redaction() {
    let m = manifest();
    assert_eq!(m["spec_version"], "korg-ledger@v1");
    let vectors = m["vectors"].as_array().unwrap();
    assert!(vectors.len() >= 11, "manifest lost vectors");
    for v in vectors {
        let file = v["file"].as_str().unwrap();
        let events = korg_verify::load_events(&read(file)).unwrap();
        let key = v["key"].as_str().map(|k| k.as_bytes().to_vec());
        let verdict = verify_journal(&events, key.as_deref());
        let chain_errs = korg_ledger::verify_chain(&events, key.as_deref());
        match v["verify"].as_str().unwrap() {
            "intact" => {
                assert!(chain_errs.is_empty(), "{file}: {chain_errs:?}");
                let tip = korg_ledger::chain_hash(events.last().unwrap(), key.as_deref());
                assert_eq!(tip, v["tip_entry_hash"].as_str().unwrap(), "{file}: tip");
            }
            _ => {
                let needle = v["error_contains"].as_str().unwrap();
                assert!(
                    chain_errs.iter().any(|e| e.contains(needle)),
                    "{file}: {chain_errs:?}"
                );
            }
        }
        let want_dag = if v["dag"] == "broken" {
            strings(&v["dag_errors"])
        } else {
            vec![]
        };
        assert_eq!(
            korg_ledger::verify_dag(&events),
            want_dag,
            "{file}: dag errors"
        );
        assert_eq!(verdict.dag_ok, want_dag.is_empty(), "{file}: dag_ok");
        if let Some(r) = v["redaction"].as_str() {
            let want = if r == "unclean" {
                strings(&v["redaction_errors"])
            } else {
                vec![]
            };
            assert_eq!(verify_redaction(&events), want, "{file}: redaction lint");
        }
    }
}

#[test]
fn manifest_lists_every_vector_on_disk() {
    let listed: std::collections::BTreeSet<String> = manifest()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["file"].as_str().unwrap().to_string())
        .collect();
    let on_disk: std::collections::BTreeSet<String> = std::fs::read_dir(spec_dir().join("vectors"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".jsonl"))
        .collect();
    assert_eq!(listed, on_disk);
}

#[test]
fn hmac_intact_valid_only_with_the_key() {
    let txt = read("hmac-intact.jsonl");
    assert!(
        verify_text(&txt, Some(b"korg-conformance-key"), None)
            .unwrap()
            .valid
    );
    assert!(
        !verify_text(&txt, None, None).unwrap().valid,
        "a keyed chain must NOT verify without the key"
    );
}

#[test]
fn shared_redaction_cases() {
    use korg_ledger::redact::{finding_order, redact};
    let cases: Value = serde_json::from_str(
        &std::fs::read_to_string(spec_dir().join("vectors/redaction/korg-redact-v1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(cases["policy"], "korg-redact@v1");
    for c in cases["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let want = if c["output"].is_null() {
            &c["input"]
        } else {
            &c["output"]
        };
        let (got, mut findings) = redact(&c["input"]);
        assert_eq!(&got, want, "{name}: output");
        findings.sort_by(finding_order);
        let got_f: Vec<(String, String)> = findings
            .iter()
            .map(|f| (f.path.clone(), f.rule.to_string()))
            .collect();
        let want_f: Vec<(String, String)> = c["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_string(),
                    f["rule"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(got_f, want_f, "{name}: findings");
        let (again, more) = redact(&got);
        assert_eq!(again, got, "{name}: idempotent");
        assert!(more.is_empty(), "{name}: output is clean");
    }
}
