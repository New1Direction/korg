//! Adversarial robustness: the verifier must NEVER panic on hostile input and
//! must NEVER accept something it shouldn't. Mirrors the Python Hypothesis suite
//! (`test_goldseal_properties.py`).

use korg_verify::{verify_goldseal, verify_text};
use proptest::prelude::*;
use serde_json::Value;

/// Arbitrary JSON values: nulls, bools, ints, strings, nested arrays/objects.
fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        ".*".prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::hash_map(".*", inner, 0..6)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

fn fixture() -> Value {
    let path = format!(
        "{}/tests/fixtures/goldseal-v1.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn flip(s: &str, i: usize, c: char) -> String {
    let mut v: Vec<char> = s.chars().collect();
    let i = i % v.len();
    v[i] = if v[i] == c {
        if c == 'f' {
            '0'
        } else {
            'f'
        }
    } else {
        c
    };
    v.into_iter().collect()
}

proptest! {
    /// verify_goldseal never panics on arbitrary JSON, and arbitrary input is never valid.
    #[test]
    fn verify_goldseal_never_panics_and_junk_is_invalid(v in json_value()) {
        let verdict = verify_goldseal(&v, None);
        prop_assert!(!verdict.valid, "arbitrary JSON must never verify");
    }

    /// verify_text never panics on arbitrary bytes/text (returns Ok or Err).
    #[test]
    fn verify_text_never_panics(s in ".*") {
        let _ = verify_text(&s, None, None);
    }

    /// Flipping any character of the tip is rejected.
    #[test]
    fn flipping_any_tip_char_is_rejected(i in 0usize..64, c in "[0-9a-f]") {
        let mut env = fixture();
        let tip = env["tip"].as_str().unwrap().to_string();
        env["tip"] = Value::String(flip(&tip, i, c.chars().next().unwrap()));
        prop_assert!(!verify_goldseal(&env, None).valid);
    }

    /// Flipping any character of the seal signature is rejected.
    #[test]
    fn flipping_any_seal_sig_char_is_rejected(i in 0usize..128, c in "[0-9a-f]") {
        let mut env = fixture();
        let sig = env["seal"]["sig"].as_str().unwrap().to_string();
        env["seal"]["sig"] = Value::String(flip(&sig, i, c.chars().next().unwrap()));
        prop_assert!(!verify_goldseal(&env, None).valid);
    }

    /// Replacing any event with arbitrary JSON is rejected.
    #[test]
    fn replacing_any_event_with_junk_is_rejected(idx in 0usize..5, junk in json_value()) {
        let mut env = fixture();
        let n = env["events"].as_array().unwrap().len();
        env["events"][idx % n] = junk;
        prop_assert!(!verify_goldseal(&env, None).valid);
    }
}
