//! `korg-verify <file>` — verify a korg receipt or journal. Exit 0 = valid,
//! 1 = invalid/tampered, 2 = usage/parse error. Single binary, no network.

use std::process::ExitCode;

const HELP: &str = "\
korg-verify — verify a korg receipt or journal (hash chain + causal DAG + Ed25519)

USAGE:
    korg-verify <file> [--key <str>] [--pubkey <hex>] [--pin-event-pubkey <hex>] [--anchors <file>] [--json]

ARGS:
    <file>          a Gold Seal (goldseal@v1), a korg receipt (.json), or a journal (.jsonl / JSON array)

OPTIONS:
    --key <str>                HMAC key (raw bytes) for keyed chains
    --pubkey <hex>             pin the expected signer — the receipt-tip signer or the Gold Seal issuer
    --pin-event-pubkey <hex>   require every event's per-event Ed25519 event_sig to verify under this key
    --anchors <file>           verify an anchors.jsonl sidecar (structural: entry_hash ↔ chain)
    --json                     machine-readable verdict
    -h, --help                 show this help

EXIT:  0 VALID    1 INVALID/tampered    2 usage/parse error
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut file: Option<String> = None;
    let mut key: Option<Vec<u8>> = None;
    let mut pin: Option<String> = None;
    let mut pin_event_pubkey: Option<String> = None;
    let mut anchors_path: Option<String> = None;
    let mut json_out = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--key" => {
                key = args.get(i + 1).map(|s| s.as_bytes().to_vec());
                i += 2;
            }
            "--pubkey" => {
                pin = args.get(i + 1).cloned();
                i += 2;
            }
            "--pin-event-pubkey" => {
                pin_event_pubkey = args.get(i + 1).cloned();
                i += 2;
            }
            "--anchors" => {
                anchors_path = args.get(i + 1).cloned();
                i += 2;
            }
            "--json" => {
                json_out = true;
                i += 1;
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::from(2);
            }
            s if !s.starts_with('-') && file.is_none() => {
                file = Some(s.to_string());
                i += 1;
            }
            other => {
                eprintln!("unknown argument: {other}\n");
                eprint!("{HELP}");
                return ExitCode::from(2);
            }
        }
    }

    let Some(file) = file else {
        eprint!("{HELP}");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {file}: {e}");
            return ExitCode::from(2);
        }
    };
    // Load the anchors sidecar up front (if requested).
    let anchors: Option<Vec<serde_json::Value>> = match &anchors_path {
        Some(p) => match std::fs::read_to_string(p)
            .map_err(|e| e.to_string())
            .and_then(|t| korg_verify::load_events(&t))
        {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("cannot read anchors {p}: {e}");
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    // The per-event-signature and anchor checks apply to a journal's events. When
    // either is requested, verify as a journal (load_events handles array/JSONL);
    // otherwise auto-detect receipt vs journal as before.
    let verdict = if pin_event_pubkey.is_some() || anchors.is_some() {
        match korg_verify::load_events(&text) {
            Ok(events) => korg_verify::verify_journal_extended(
                &events,
                key.as_deref(),
                pin_event_pubkey.as_deref(),
                anchors.as_deref(),
            ),
            Err(e) => {
                eprintln!("parse error: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        match korg_verify::verify_text(&text, key.as_deref(), pin.as_deref()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("parse error: {e}");
                return ExitCode::from(2);
            }
        }
    };

    if json_out {
        let out = serde_json::json!({
            "valid": verdict.valid,
            "kind": verdict.kind,
            "event_count": verdict.event_count,
            "chain_ok": verdict.chain_ok,
            "dag_ok": verdict.dag_ok,
            "tip_ok": verdict.tip_ok,
            "signature_ok": verdict.signature_ok,
            "signer": verdict.signer,
            "event_sigs_ok": verdict.event_sigs_ok,
            "anchors_ok": verdict.anchors_ok,
            "summary_ok": verdict.summary_ok,
            "errors": verdict.errors,
        });
        println!("{out}");
    } else {
        // For a Gold Seal, surface the human-legible (and re-derived) summary —
        // that is the artifact's whole purpose.
        let seal_view = (verdict.kind == "goldseal")
            .then(|| serde_json::from_str::<serde_json::Value>(&text).ok())
            .flatten();
        print_human(&verdict, &file, seal_view.as_ref());
    }

    if verdict.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_human(v: &korg_verify::Verdict, file: &str, seal: Option<&serde_json::Value>) {
    if v.valid {
        let signed = match &v.signer {
            Some(pk) if v.signature_ok == Some(true) => {
                format!(" · signed by {}…", &pk[..pk.len().min(16)])
            }
            _ => String::new(),
        };
        println!(
            "  \u{2713} {} VALID \u{2014} {} events, hash-chain + DAG intact{}",
            v.kind, v.event_count, signed
        );
        if v.summary_ok == Some(true) {
            println!("    \u{2713} summary verified (re-derived from the events — it cannot lie)");
        }
        if v.event_sigs_ok == Some(true) {
            println!("    \u{2713} every event_sig verifies under the pinned key");
        }
        if v.anchors_ok == Some(true) {
            println!("    \u{2713} anchors match the chain (structural)");
        }
        if let Some(env) = seal {
            print_seal_summary(env);
        }
        println!("    {file}");
    } else {
        println!(
            "  \u{2717} {} INVALID \u{2014} {} problem(s):",
            v.kind,
            v.errors.len()
        );
        for e in v.errors.iter().take(8) {
            println!("      - {e}");
        }
    }
}

/// Render a verified Gold Seal's human-legible attestation. Every line here was
/// re-derived from the signed event chain, so it is exactly what happened.
fn print_seal_summary(env: &serde_json::Value) {
    if let Some(claim) = env.get("claim").and_then(|c| c.as_str()) {
        println!("    claim:  {claim}");
    }
    let Some(s) = env.get("summary").and_then(|s| s.as_object()) else {
        return;
    };
    if let Some(agents) = s.get("agents").and_then(|a| a.as_array()) {
        let list: Vec<&str> = agents.iter().filter_map(|x| x.as_str()).collect();
        if !list.is_empty() {
            println!("    agents: {}", list.join(", "));
        }
    }
    if let Some(by_tool) = s.get("by_tool").and_then(|b| b.as_object()) {
        let mut tools: Vec<String> = by_tool
            .iter()
            .map(|(k, val)| format!("{k}\u{00d7}{}", val.as_i64().unwrap_or(0)))
            .collect();
        tools.sort();
        if !tools.is_empty() {
            println!("    tools:  {}", tools.join("  "));
        }
    }
    if let Some(files) = s.get("files").and_then(|f| f.as_array()) {
        let list: Vec<&str> = files.iter().filter_map(|x| x.as_str()).collect();
        if !list.is_empty() {
            println!("    files:  {}", list.join(", "));
        }
    }
    if let (Some(a), Some(b)) = (
        s.get("seq_first").and_then(|v| v.as_i64()),
        s.get("seq_last").and_then(|v| v.as_i64()),
    ) {
        println!("    seq:    {a}\u{2013}{b}");
    }
}
