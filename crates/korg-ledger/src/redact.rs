//! korg-redact@v1 — redaction at persistence (Rust reference implementation).
//!
//! Normative text: `spec/korg-ledger-v1/REDACTION.md`. Byte-for-byte equivalent
//! to the Python `korg_ledger.redaction` module; both are pinned by the shared
//! cases in `spec/korg-ledger-v1/vectors/redaction/korg-redact-v1.json`.
//!
//! A chained event cannot be redacted after it is hashed (that is tampering), so
//! a producer MUST run [`redact`] on the record *before* [`crate::chain_hash`] and
//! before anything touches disk. The policy is deterministic and idempotent
//! (`redact(redact(x)) == redact(x)`), and a value is redaction-clean iff
//! [`scan`] returns no findings.
//!
//! The patterns use ASCII classes only (no `\s`, `\b`, `\w`, or `(?i)`): the
//! `regex` crate and Python `re` disagree on their Unicode semantics, and a split
//! would let one implementation persist what the other would redact.

use regex::{Captures, Regex};
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// Policy identifier recorded on every redacted event.
pub const POLICY: &str = "korg-redact@v1";

/// Tag key under `metadata.tags` that declares the policy a producer applied.
pub const TAG: &str = "korg.redaction";

/// Fields that are chain structure, never payload: excluded from redaction and
/// from the cleanliness lint.
pub const STRUCTURAL_FIELDS: &[&str] = &["prev_hash", "entry_hash", "event_sig"];

const SENSITIVE_KEYS: &[&str] = &[
    "authorization",
    "proxy_authorization",
    "cookie",
    "set_cookie",
    "api_key",
    "apikey",
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "private_key",
    "secret_key",
    "aws_secret_access_key",
    "credential",
    "credentials",
];

const SENSITIVE_KEY_SUFFIXES: &[&str] = &[
    "_api_key",
    "_apikey",
    "_secret",
    "_secret_key",
    "_private_key",
    "_password",
    "_passwd",
    "_passphrase",
    "_token",
];

/// One redaction: an RFC 6901 JSON pointer and the rule that fired there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub path: String,
    pub rule: &'static str,
}

/// The replacement text for a rule.
pub fn marker(rule: &str) -> String {
    format!("[REDACTED:{rule}]")
}

/// camelCase / kebab-case / spaced → snake_case, ASCII-lowercased
/// (`accessToken` → `access_token`, `X-Api-Key` → `x_api_key`, `APIKey` → `apikey`).
pub fn normalize_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    let mut prev: Option<char> = None;
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            if prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c == '-' || c == ' ' {
            out.push('_');
        } else {
            out.push(c);
        }
        prev = Some(c);
    }
    out
}

/// Whether an object member with this key holds a credential.
pub fn is_sensitive_key(key: &str) -> bool {
    let k = normalize_key(key);
    SENSITIVE_KEYS.contains(&k.as_str()) || SENSITIVE_KEY_SUFFIXES.iter().any(|s| k.ends_with(s))
}

type Replacer = fn(&Captures) -> String;

struct Rule {
    id: &'static str,
    re: Regex,
    repl: Replacer,
}

// Leading "boundary": start of string or a non-[A-Za-z0-9_] char, captured and
// re-emitted (the regex crate has no look-behind).
const B: &str = r"(^|[^A-Za-z0-9_])";

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |id: &'static str, pat: String, repl: Replacer| Rule {
            id,
            re: Regex::new(&pat).expect("korg-redact@v1 pattern compiles"),
            repl,
        };
        vec![
            r(
                "private_key_block",
                r"-----BEGIN[A-Z ]*PRIVATE KEY-----(?:[\s\S]*?-----END[A-Z ]*PRIVATE KEY-----|[\s\S]*)"
                    .to_string(),
                |_| marker("private_key_block"),
            ),
            r(
                "url_credentials",
                r"([A-Za-z][A-Za-z0-9+.\-]*://)[^ \t\r\n/:@\[\]]+:[^ \t\r\n/@\[\]]+@".to_string(),
                |c| format!("{}{}@", &c[1], marker("url_credentials")),
            ),
            r(
                "bearer_token",
                r"((?:Bearer|bearer|BEARER)[ \t]+)[A-Za-z0-9._~+/\-]{16,}=*".to_string(),
                |c| format!("{}{}", &c[1], marker("bearer_token")),
            ),
            r(
                "provider_api_key",
                format!(r"{B}sk-[A-Za-z0-9_\-]{{20,}}"),
                |c| format!("{}{}", &c[1], marker("provider_api_key")),
            ),
            r(
                "github_token",
                format!(r"{B}(?:gh[pousr]_[A-Za-z0-9]{{36,}}|github_pat_[A-Za-z0-9_]{{22,}})"),
                |c| format!("{}{}", &c[1], marker("github_token")),
            ),
            r(
                "slack_token",
                format!(r"{B}xox[abposr]-[A-Za-z0-9\-]{{10,}}"),
                |c| format!("{}{}", &c[1], marker("slack_token")),
            ),
            r(
                "aws_access_key_id",
                format!(r"{B}(?:AKIA|ASIA)[0-9A-Z]{{16}}"),
                |c| format!("{}{}", &c[1], marker("aws_access_key_id")),
            ),
            r(
                "google_api_key",
                format!(r"{B}AIza[0-9A-Za-z_\-]{{35}}"),
                |c| format!("{}{}", &c[1], marker("google_api_key")),
            ),
            r(
                "jwt",
                format!(r"{B}eyJ[A-Za-z0-9_\-]{{8,}}\.eyJ[A-Za-z0-9_\-]{{8,}}\.[A-Za-z0-9_\-]{{8,}}"),
                |c| format!("{}{}", &c[1], marker("jwt")),
            ),
            r(
                "env_assignment",
                format!(
                    r#"{B}([A-Z][A-Z0-9_]*(?:API_KEY|APIKEY|SECRET|SECRET_KEY|PASSWORD|PASSWD|TOKEN|PRIVATE_KEY|ACCESS_KEY))([ \t]*[=:][ \t]*)(?:"[^"\n]*"|'[^'\n]*'|[^ \t\r\n"',;&]+)"#
                ),
                |c| format!("{}{}{}{}", &c[1], &c[2], &c[3], marker("env_assignment")),
            ),
            r(
                "json_secret_field",
                r#"("(?:password|passwd|passphrase|secret|client_secret|api_key|apikey|access_token|refresh_token|auth_token|id_token|token|private_key|authorization)"[ \t]*:[ \t]*)"(?:[^"\\\n]|\\.)*""#
                    .to_string(),
                |c| format!("{}\"{}\"", &c[1], marker("json_secret_field")),
            ),
        ]
    })
}

fn marker_re() -> &'static Regex {
    static M: OnceLock<Regex> = OnceLock::new();
    M.get_or_init(|| Regex::new(r"^\[REDACTED:[a-z_]+\]$").expect("marker pattern"))
}

/// Upper bound on rule-list passes. Real input reaches its fixed point in one or
/// two passes; hitting the bound leaves the string unclean, which a producer's
/// fail-closed gate (REDACTION.md R4) then refuses to persist.
pub const MAX_PASSES: usize = 8;

/// Apply the value-pattern rules, in order, repeatedly until the string stops
/// changing (a fixed point — idempotent by construction even where one
/// replacement exposes a new match, e.g. two keys back to back). Returns the
/// redacted string and the ids of the rules that fired, in normative order.
pub fn redact_string(s: &str) -> (String, Vec<&'static str>) {
    let mut cur = s.to_string();
    let mut fired: Vec<&'static str> = Vec::new();
    for _ in 0..MAX_PASSES {
        let mut changed = false;
        for rule in rules() {
            let new = rule.re.replace_all(&cur, |c: &Captures| (rule.repl)(c));
            if new != cur {
                if !fired.contains(&rule.id) {
                    fired.push(rule.id);
                }
                cur = new.into_owned();
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let order = rule_ids();
    fired.sort_by_key(|r| order.iter().position(|x| x == r));
    (cur, fired)
}

fn pointer_escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn keeps_under_sensitive_key(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(_) => true,
        Value::String(s) => s.is_empty() || marker_re().is_match(s),
        _ => false,
    }
}

fn redact_inner(v: &Value, path: &str, findings: &mut Vec<Finding>) -> Value {
    match v {
        Value::String(s) => {
            let (new, fired) = redact_string(s);
            for rule in fired {
                findings.push(Finding {
                    path: path.to_string(),
                    rule,
                });
            }
            Value::String(new)
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .enumerate()
                .map(|(i, x)| redact_inner(x, &format!("{path}/{i}"), findings))
                .collect(),
        ),
        Value::Object(m) => {
            let mut out = Map::with_capacity(m.len());
            for (k, x) in m {
                let p = format!("{path}/{}", pointer_escape(k));
                if is_sensitive_key(k) && !keeps_under_sensitive_key(x) {
                    findings.push(Finding {
                        path: p,
                        rule: "sensitive_key",
                    });
                    out.insert(k.clone(), Value::String(marker("sensitive_key")));
                } else {
                    out.insert(k.clone(), redact_inner(x, &p, findings));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Redact a JSON value under korg-redact@v1. Returns the redacted copy and one
/// finding per rule application. Object keys are never rewritten.
pub fn redact(value: &Value) -> (Value, Vec<Finding>) {
    let mut findings = Vec::new();
    let out = redact_inner(value, "", &mut findings);
    (out, findings)
}

/// Findings that [`redact`] would produce — empty iff `value` is clean.
pub fn scan(value: &Value) -> Vec<Finding> {
    redact(value).1
}

/// Normative rule order: `sensitive_key`, then the value-pattern rules.
pub fn rule_ids() -> Vec<&'static str> {
    std::iter::once("sensitive_key")
        .chain(rules().iter().map(|r| r.id))
        .collect()
}

/// Canonical finding order: JSON pointer (code point), then normative rule order.
/// Implementations iterate object members differently, so reported lists are
/// always sorted by this.
pub fn finding_order(a: &Finding, b: &Finding) -> std::cmp::Ordering {
    let ids = rule_ids();
    let idx = |r: &str| ids.iter().position(|x| *x == r).unwrap_or(usize::MAX);
    a.path
        .cmp(&b.path)
        .then_with(|| idx(a.rule).cmp(&idx(b.rule)))
}

/// Lint one persisted ledger record (flat or nested) for unredacted secrets,
/// ignoring the structural hash fields.
pub fn scan_record(record: &Value) -> Vec<Finding> {
    match record.as_object() {
        Some(m) => {
            let body: Map<String, Value> = m
                .iter()
                .filter(|(k, _)| !STRUCTURAL_FIELDS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            scan(&Value::Object(body))
        }
        None => Vec::new(),
    }
}

/// Journal-level lint: one error per finding, localized to `seq N` and a JSON
/// pointer. Empty iff every event is redaction-clean. Strings are byte-identical
/// to Python `verify_redaction`.
pub fn verify_redaction(events: &[Value]) -> Vec<String> {
    let mut errors = Vec::new();
    for e in events.iter().filter(|e| e.is_object()) {
        let label = e
            .get("seq_id")
            .and_then(crate::json_int)
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());
        let mut findings = scan_record(e);
        findings.sort_by(finding_order);
        for f in findings {
            let path = if f.path.is_empty() { "/" } else { &f.path };
            errors.push(format!("seq {label}: unredacted {} at {path}", f.rule));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_normalization_matches_the_python_reference() {
        assert_eq!(normalize_key("accessToken"), "access_token");
        assert_eq!(normalize_key("X-Api-Key"), "x_api_key");
        assert_eq!(normalize_key("APIKey"), "apikey");
        assert_eq!(normalize_key("OAuth2Token"), "oauth2_token");
        assert!(is_sensitive_key("OPENAI_API_KEY"));
        assert!(!is_sensitive_key("prompt_tokens"));
        assert!(!is_sensitive_key("token_count"));
    }

    #[test]
    fn redaction_is_idempotent_and_keeps_non_secrets() {
        let v = json!({
            "headers": {"Authorization": "Bearer abcdefghijklmnopqrstuvwxyz0123", "accept": "json"},
            "cmd": "export OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz && run",
            "max_tokens": 5,
            "secret": false
        });
        let (r, f) = redact(&v);
        assert_eq!(r["headers"]["Authorization"], "[REDACTED:sensitive_key]");
        assert_eq!(r["headers"]["accept"], "json");
        assert_eq!(
            r["cmd"],
            "export OPENAI_API_KEY=[REDACTED:env_assignment] && run"
        );
        assert_eq!(r["max_tokens"], 5);
        assert_eq!(r["secret"], false);
        assert_eq!(f.len(), 3);
        assert_eq!(redact(&r).0, r);
        assert!(scan(&r).is_empty());
    }
}
