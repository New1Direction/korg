# Phase 2 — Plan 2.2: Per-Event Signing + Git-Tip Anchoring + Rewind Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the trust claim hold against the workspace owner, not just third parties — per-event Ed25519 signatures (`event_sig`), an external public-git anchor that records chain tips immutably, and migrate the destructive `rewind()` callers onto the tamper-evident `rewind_with_seal`. Plus proptest-based fuzzing of the chain.

**Architecture:** Three independent workstreams, each TDD'd and verifiable with no network in CI. (A) Signing reuses the canonical preimage `chain_hash` already computes; Rust behind a `signing` feature, Python behind a `[signing]` extra (lazy import), JS via Web Crypto Ed25519; a fixed-seed cross-impl fixture proves Rust-sign / Python-verify / JS-verify interop. (B) Anchoring adds an `AnchorRecord` + structural `verify_anchors` (hermetic, all 3 langs) with the live git-tip check Rust-only behind a feature flag. (C) Rewind migration swaps the safe callers to `rewind_with_seal` and adds proptests.

**Tech Stack:** Rust (korg-ledger/registry/verify/runtime), Python (korg-ledger-py + optional `cryptography`), JS (verify.mjs), `proptest`, `pytest`, `hypothesis`.

**Branch:** `feat/phase2-trust-hardening` (stacked on Phase 1).

---

## Workstream sequencing

Implement in this order (each fully verified + committed before the next):

1. **C — Rewind migration + fuzz** (Rust-only, no deps, no decisions; completable now)
2. **A — Signing** (Rust primitives → korg-registry wiring → cross-impl fixture → Python → JS)
3. **B — Anchoring** (Rust structural `verify_anchors` → korg-verify `--anchors` → Python/JS structural → git-tip resolver behind feature → docs)

---

## Workstream C — Rewind migration + proptest fuzz

### Task C1: proptest the seal invariants (korg-registry)

**Files:** Modify `crates/korg-registry/Cargo.toml` (proptest dev-dep), `crates/korg-registry/src/log.rs` (chain_tests).

- [ ] **Step 1: Add the failing proptests** in the `chain_tests` module:

```rust
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn rewind_with_seal_chain_always_verifies(n in 1usize..8, k in 1u64..8) {
            let mut j = temp_journal("seal-prop");
            for i in 0..n { j.append(sample_event(&format!("t{i}"))); }
            let target = k.min(n as u64);
            j.rewind_with_seal(target, "korg:test", "proptest").unwrap();
            prop_assert!(j.verify_chain().is_empty());
            prop_assert_eq!(j.events.len(), target as usize + 1);
            match &j.events.last().unwrap().event {
                CapabilityEvent::LedgerRewind { target_seq_id, invalidated_through, .. } => {
                    prop_assert_eq!(*target_seq_id, target);
                    prop_assert_eq!(*invalidated_through, n as u64);
                }
                _ => prop_assert!(false, "tip must be LedgerRewind"),
            }
        }

        #[test]
        fn rewind_then_append_extends_the_chain(n in 1usize..6, k in 1u64..6) {
            let mut j = temp_journal("seal-append");
            for i in 0..n { j.append(sample_event(&format!("t{i}"))); }
            let target = k.min(n as u64);
            j.rewind_with_seal(target, "korg:test", "proptest").unwrap();
            j.append(sample_event("after"));
            prop_assert!(j.verify_chain().is_empty());
        }
    }
```

- [ ] **Step 2: Add `proptest = "1"` to `crates/korg-registry/Cargo.toml` `[dev-dependencies]`, run**

Run: `cargo test -p korg-registry rewind_with_seal_chain_always_verifies rewind_then_append`
Expected: PASS (256 generated cases each).

- [ ] **Step 3: Commit**

```bash
git add crates/korg-registry/Cargo.toml crates/korg-registry/src/log.rs Cargo.lock
git commit -m "test(korg-registry): proptest rewind_with_seal chain invariants"
```

### Task C2: canonicalize unicode round-trip proptest (korg-ledger)

- [ ] **Step 1: Add to the korg-ledger proptest block:**

```rust
        // canonicalize is stable across a JSON round-trip for arbitrary unicode.
        #[test]
        fn canonicalize_round_trips_unicode(s in ".*") {
            let v = json!({ "k": s });
            let once = canonicalize(&v);
            let reparsed: Value = serde_json::from_slice(&once).unwrap();
            prop_assert_eq!(once, canonicalize(&reparsed));
        }
```

- [ ] **Step 2: Run + commit**

Run: `cargo test -p korg-ledger canonicalize_round_trips_unicode`
Expected: PASS.
```bash
git add crates/korg-ledger/src/lib.rs && git commit -m "test(korg-ledger): proptest canonicalize unicode round-trip stability"
```

### Task C3: migrate the safe rewind callers

Per the caller audit:
- **Caller 1** `src/main.rs:747` (CLI `Rewind`) → `rewind_with_seal(seq, "korg:cli", "operator rewind")`.
- **Caller 3** `crates/korg-runtime/src/leader.rs:343` (TUI confirmed rewind) → `rewind_with_seal(seq_id, "korg:operator", "operator-confirmed TUI rewind")`.
- **Caller 4** `crates/korg-registry/src/lib.rs:807` (`restore_checkpoint`) → migrate **only after** confirming `ProjectionEngine::rebuild_all` tolerates `LedgerRewind` (read `crates/korg-registry/src/projection.rs`; if it has a catch-all/`_ =>` arm it's safe; else add a no-op arm first).
- **Caller 2** `src/main.rs:1965` (demo) → **keep** `rewind(391)`; add `// TODO(rewind-seal): demo manually reassigns seq_ids post-rewind; refactor to the normal append path before sealing.`

- [ ] **Step 1: Read `projection.rs`** to confirm `LedgerRewind` is ignored by `rebuild_all`/`apply`. If a non-exhaustive match exists, add `CapabilityEvent::LedgerRewind { .. } => {}`.
- [ ] **Step 2: Apply the three migrations + the demo TODO** (exact replacements above).
- [ ] **Step 3: Build + test the full workspace**

Run: `cargo build && cargo test -p korg-registry -p korg-runtime 2>&1 | tail -8`
Expected: `Finished`; tests green (the demo invariants test still passes because the demo path is untouched).

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "refactor(rewind): route operator/CLI/checkpoint rewinds through rewind_with_seal (demo unchanged)"
```

---

## Workstream A — Per-event Ed25519 signing

### Task A1: signing primitives in `korg-ledger` (behind a `signing` feature)

**Files:** `crates/korg-ledger/Cargo.toml`, `crates/korg-ledger/src/lib.rs`.

- [ ] **Step 1: Add the feature + deps to `Cargo.toml`**

```toml
[dependencies]
serde_json = { workspace = true }
sha2 = "0.10"
hmac = "0.12"
ed25519-dalek = { version = "2", optional = true }
hex = { version = "0.4", optional = true }

[features]
signing = ["dep:ed25519-dalek", "dep:hex"]
```

- [ ] **Step 2: Add the failing tests** (in `lib.rs`, guarded `#[cfg(all(test, feature = "signing"))]` — or a `#[cfg(feature="signing")]` test module):

```rust
#[cfg(all(test, feature = "signing"))]
mod signing_tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    #[test]
    fn sign_then_verify_roundtrips_and_detects_tampering() {
        let key = SigningKey::from_bytes(&[42u8; 32]);
        let pk_hex = hex::encode(key.verifying_key().to_bytes());
        let ev = json!({"seq_id": 1, "prev_hash": GENESIS_HASH, "payload": {"a": 1}, "entry_hash": "deadbeef"});
        let sig = sign_event(&key, &ev);
        assert_eq!(sig.len(), 128); // 64-byte sig as lowercase hex
        assert!(verify_event_sig(&pk_hex, &ev, &sig));
        // tampering content breaks it
        let mut tampered = ev.clone();
        tampered["payload"] = json!({"a": 2});
        assert!(!verify_event_sig(&pk_hex, &tampered, &sig));
    }

    #[test]
    fn signature_excludes_entry_hash_and_event_sig() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let a = json!({"seq_id": 1, "prev_hash": GENESIS_HASH, "x": "y"});
        let mut b = a.clone();
        b["entry_hash"] = json!("anything");
        b["event_sig"] = json!("anything");
        assert_eq!(sign_event(&key, &a), sign_event(&key, &b)); // both excluded from preimage
    }
}
```

- [ ] **Step 3: Implement the primitives** in `lib.rs` (guarded `#[cfg(feature = "signing")]`):

```rust
/// Canonical preimage bytes for an event — the same bytes `chain_hash` hashes
/// (event minus HASH_FIELDS, canonicalized). Exposed so signers/verifiers reuse it.
pub fn event_preimage(event: &Value) -> Vec<u8> {
    let mut obj = event.as_object().cloned().unwrap_or_default();
    for f in HASH_FIELDS {
        obj.remove(*f);
    }
    canonicalize(&Value::Object(obj))
}

#[cfg(feature = "signing")]
/// Ed25519-sign an event's canonical preimage (RFC 8032, pure). Lowercase hex.
pub fn sign_event(key: &ed25519_dalek::SigningKey, event: &Value) -> String {
    use ed25519_dalek::Signer;
    let sig = key.sign(&event_preimage(event));
    hex::encode(sig.to_bytes())
}

#[cfg(feature = "signing")]
/// Verify an event's `event_sig` (lowercase hex) against a hex pubkey. False on any error.
pub fn verify_event_sig(pubkey_hex: &str, event: &Value, sig_hex: &str) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let pk_bytes = match hex::decode(pubkey_hex) {
        Ok(b) if b.len() == 32 => b,
        _ => return false,
    };
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&pk_bytes);
    let vk = match VerifyingKey::from_bytes(&pk) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let sig_bytes = match hex::decode(sig_hex) {
        Ok(b) if b.len() == 64 => b,
        _ => return false,
    };
    let mut sb = [0u8; 64];
    sb.copy_from_slice(&sig_bytes);
    vk.verify(&event_preimage(event), &Signature::from_bytes(&sb)).is_ok()
}
```

Note: `event_preimage` is **not** feature-gated (pure, reused by anchoring too).

- [ ] **Step 4: Run + commit**

Run: `cargo test -p korg-ledger --features signing signing_tests`
Expected: PASS.
```bash
git add crates/korg-ledger/Cargo.toml crates/korg-ledger/src/lib.rs Cargo.lock
git commit -m "feat(korg-ledger): Ed25519 per-event sign/verify over the canonical preimage (signing feature)"
```

### Task A2: Python optional signing (`korg-ledger-py`)

**Files:** `adapters/korg-ledger-py/pyproject.toml`, `src/korg_ledger/signing.py` (new), `src/korg_ledger/writer.py`, tests.

- [ ] **Step 1: Add the extra** — `pyproject.toml`: `signing = ["cryptography>=41"]`.
- [ ] **Step 2: Failing test** `tests/test_signing.py` (skip if `cryptography` absent):

```python
import json, tempfile
from pathlib import Path
import pytest
cryptography = pytest.importorskip("cryptography")
from korg_ledger import LedgerWriter, agent_tool_call_event
from korg_ledger.signing import sign_event, verify_event_sig
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

SEED = bytes(range(32))

def test_sign_verify_roundtrip_and_tamper():
    ev = {"seq_id": 1, "prev_hash": "0"*64, "payload": {"a": 1}}
    sig = sign_event(SEED, ev)
    assert len(sig) == 128
    pk = Ed25519PrivateKey.from_private_bytes(SEED).public_key().public_bytes_raw()
    assert verify_event_sig(pk, ev, sig)
    assert not verify_event_sig(pk, {**ev, "payload": {"a": 2}}, sig)

def test_writer_with_signing_key_sets_event_sig(tmp_path):
    led = tmp_path / "l.jsonl"
    w = LedgerWriter(led, signing_key=SEED)
    w.append(event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={}, success=True, duration_ms=0), actor_id="korg:test")
    rec = json.loads(led.read_text().splitlines()[0])
    assert "event_sig" in rec and len(rec["event_sig"]) == 128

def test_writer_without_key_omits_event_sig(tmp_path):
    led = tmp_path / "l.jsonl"
    w = LedgerWriter(led)
    w.append(event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={}, success=True, duration_ms=0), actor_id="korg:test")
    assert "event_sig" not in json.loads(led.read_text().splitlines()[0])
```

- [ ] **Step 3: Implement `signing.py`** (lazy-importable; mirrors the Rust preimage):

```python
"""Optional Ed25519 signing for korg-ledger@v1 (requires `cryptography`).

NOT imported by korg_ledger/__init__.py — import lazily so the stdlib-only core
never depends on it. Signs the same canonical preimage as entry_hash, lowercase hex.
"""
from __future__ import annotations

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey, Ed25519PublicKey,
)
from ._hash import HASH_FIELDS, canonicalize


def _preimage(event: dict) -> bytes:
    return canonicalize({k: v for k, v in event.items() if k not in HASH_FIELDS})


def sign_event(private_seed: bytes, event: dict) -> str:
    key = Ed25519PrivateKey.from_private_bytes(private_seed)
    return key.sign(_preimage(event)).hex()


def verify_event_sig(public_bytes: bytes, event: dict, sig_hex: str) -> bool:
    try:
        Ed25519PublicKey.from_public_bytes(public_bytes).verify(bytes.fromhex(sig_hex), _preimage(event))
        return True
    except Exception:
        return False
```

- [ ] **Step 4: Wire `signing_key` into `LedgerWriter`** — `__init__(..., signing_key: bytes | None = None)`; in `append`, after computing `entry_hash`, if `self._signing_key` is set: `from . import signing; record["event_sig"] = signing.sign_event(self._signing_key, record)` (lazy import inside the method).

- [ ] **Step 5: Run + commit**

Run: `PYTHONPATH=adapters/korg-ledger-py/src python3 -m pytest adapters/korg-ledger-py/tests/test_signing.py -q`
Expected: PASS (or skip cleanly if `cryptography` is unavailable).
```bash
git add adapters/korg-ledger-py && git commit -m "feat(korg-ledger-py): optional Ed25519 signing (cryptography extra; stdlib core unchanged)"
```

### Task A3: cross-impl fixture + JS verifyEventSig + korg-verify per-event check

- [ ] **Step 1: Generate the frozen fixture** with a fixed key. Add a Rust test in `crates/korg-verify/tests/event_sig.rs` that builds a 2-event signed chain from `SigningKey::from_bytes(&[42u8;32])`, writes it to `crates/korg-verify/tests/fixtures/signed-events.jsonl` if absent, then verifies each `event_sig` with the known pubkey hex (`verify_event_sig`). Commit the fixture as a frozen artifact + record the pubkey hex in a `signed-events.pubkey` file.
- [ ] **Step 2: JS** — add `export async function verifyEventSig(pubkeyHex, event, sigHex)` to `verify.mjs` (mirror `verifyTipSig` but message = canonical preimage bytes); add a `conformance.mjs` block that reads `signed-events.jsonl` + the pubkey and asserts every `event_sig` verifies (and a one-byte flip fails).
- [ ] **Step 3: Python** — add to `test_signing.py` a test that loads the same `signed-events.jsonl` + pubkey and verifies each with `signing.verify_event_sig` (Rust-sign / Python-verify interop).
- [ ] **Step 4: korg-verify** — add `verify_event_sig` passthrough + `Verdict.event_sigs_ok: Option<bool>` + a `--pin-event-pubkey` flag in `main.rs`; when supplied, fold per-event verification into the verdict. Tests in `event_sig.rs`.
- [ ] **Step 5: Run all three toolchains + commit** (cargo `--features signing`, node conformance, pytest).

---

## Workstream B — Git-tip anchoring

### Task B1: `AnchorRecord` + structural `verify_anchors` (korg-ledger, no deps)

- [ ] **Step 1: Failing tests** in `lib.rs` tests: a 3-event chain + one anchor for seq 3 with the correct `entry_hash` → `verify_anchors` returns `[]`; wrong `entry_hash` → error mentioning `seq 3`; `seq_id` not in chain → error.
- [ ] **Step 2: Implement** (no new deps):

```rust
/// Anchor kind: the chain tip's entry_hash is committed to a public git repo.
pub const ANCHOR_KIND_GIT_TIP: &str = "git-tip";

/// Structural verification of an anchors.jsonl sidecar against a verified chain.
/// For each anchor: the event at `seq_id` must exist and its `entry_hash` must
/// equal the anchor's. (The external/network proof — e.g. the git commit — is
/// checked separately and is documented as the actual owner-rewrite defense.)
pub fn verify_anchors(chain: &[Value], anchors: &[Value]) -> Vec<String> {
    let mut errors = Vec::new();
    for a in anchors {
        let seq = a.get("seq_id").and_then(|v| v.as_u64());
        let want = a.get("entry_hash").and_then(|v| v.as_str());
        match (seq, want) {
            (Some(seq), Some(want)) => {
                match chain.iter().find(|e| e.get("seq_id").and_then(|v| v.as_u64()) == Some(seq)) {
                    None => errors.push(format!("anchor seq {seq}: no event with that seq_id in the chain")),
                    Some(e) if e.get("entry_hash").and_then(|v| v.as_str()) != Some(want) =>
                        errors.push(format!("anchor seq {seq}: entry_hash does not match the chain")),
                    Some(_) => {}
                }
            }
            _ => errors.push("anchor record missing seq_id or entry_hash".into()),
        }
    }
    errors
}
```

- [ ] **Step 3: Run + commit** (`cargo test -p korg-ledger verify_anchors`).

### Task B2: structural `verify_anchors` in Python + JS (mirror), + a frozen anchor vector

- [ ] Python: `def verify_anchors(chain, anchors) -> list[str]` in `_hash.py` (stdlib only) + tests.
- [ ] JS: `export function verifyAnchors(chain, anchors)` in `verify.mjs` + a `conformance.mjs` `anchors-intact` case.
- [ ] Add frozen `spec/korg-ledger-v1/vectors/anchors-intact.jsonl` (3-event chain) + `anchors-intact-anchors.jsonl` (1 anchor at the frozen tip); all three harnesses assert structural OK.

### Task B3: korg-verify `--anchors` + the git-tip resolver (feature-gated) + docs

- [ ] korg-verify: `Verdict.anchors_ok: Option<bool>` + `--anchors <file>`; structural check always; a `GitTipResolver` behind `--features git-tip-verify` (default off) that does `git ls-remote`/REST lookup; a `NullResolver` for tests.
- [ ] A `korg-anchor publish` path (documented; appends to `anchors.jsonl` + the operator pushes a tip-commit) — documented in SPEC.md §8 with the honest "what this closes vs not" (proves the chain was published before any third party fetched it; `anchored_at` is local wall-clock, not a trusted time source).
- [ ] Update `spec/korg-ledger-v1/SPEC.md` + `crates/korg-verify` docs.

---

## Self-Review

**1. Blueprint coverage:** Signing (A) reuses the canonical preimage, hex encoding matching `verify_tip_sig`/the receipt fixture, Rust feature-gated, Python optional/lazy, JS Web-Crypto, fixed-seed cross-impl fixture ✓. Anchoring (B) git-tip kind, structural `verify_anchors` hermetic in all 3 langs + frozen vector, network check Rust-only behind a feature, honest SPEC note ✓. Rewind (C) migrate callers 1/3/4 (4 gated on projection safety), keep the demo, proptest not cargo-fuzz ✓.

**2. Placeholder scan:** Tasks A3/B2/B3 are described at the interface level (the blueprints carry the exact signatures); each sub-step still states the file, the assertion, and the run command. No "TODO/implement later" in the code shown.

**3. Lockstep:** `event_sig`/`entry_hash` already excluded from the preimage in all impls; signatures are over the identical preimage bytes (RFC-8032 pure, raw bytes not prehash) and hex-encoded everywhere; `AnchorRecord` field names are fixed and shared; `verify_anchors` returns the empty-collection-is-OK convention in all three languages. The frozen fixtures (`signed-events.jsonl`, `anchors-intact*.jsonl`) are the cross-impl oracle.
