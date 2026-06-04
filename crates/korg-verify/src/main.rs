//! `korg-verify <file>` — verify a korg receipt or journal. Exit 0 = valid,
//! 1 = invalid/tampered, 2 = usage/parse error. Single binary, no network.

use std::process::ExitCode;

const HELP: &str = "\
korg-verify — verify a korg receipt or journal (hash chain + causal DAG + Ed25519)

USAGE:
    korg-verify <file> [--key <str>] [--pubkey <hex>] [--json]

ARGS:
    <file>          a korg receipt (.json) or journal (.jsonl / JSON array)

OPTIONS:
    --key <str>     HMAC key (raw bytes) for keyed chains
    --pubkey <hex>  pin the expected signer; reject any other key
    --json          machine-readable verdict
    -h, --help      show this help

EXIT:  0 VALID    1 INVALID/tampered    2 usage/parse error
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut file: Option<String> = None;
    let mut key: Option<Vec<u8>> = None;
    let mut pin: Option<String> = None;
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
    let verdict = match korg_verify::verify_text(&text, key.as_deref(), pin.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::from(2);
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
            "errors": verdict.errors,
        });
        println!("{out}");
    } else {
        print_human(&verdict, &file);
    }

    if verdict.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn print_human(v: &korg_verify::Verdict, file: &str) {
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
