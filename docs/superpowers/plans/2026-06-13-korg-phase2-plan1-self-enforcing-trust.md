# Phase 2 — Plan 2.1: Self-Enforcing Trust Core + Tamper-Evident Rewind Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two audit-flagged trust gaps that need no new external services: the chain is currently proven only by a handful of hand-written vectors (add **property-based testing** in Rust *and* Python), and `rewind()` currently leaves **no evidence** it happened (add a non-destructive `rewind_with_seal` that records a tamper-evident `LedgerRewind` event).

**Architecture:** `proptest` (Rust) and `hypothesis` (Python) generate thousands of random event sequences and assert the chain invariants hold and any tamper is localized — turning "we have a few vectors" into "the invariant is enforced over a huge input space." `rewind_with_seal` truncates the invalidated future (so the wrong path can't be replayed) **and** appends a `LedgerRewind` event as the new chain tip, so a verifier can always see that a rewind occurred and which range it invalidated.

**Tech Stack:** Rust (`korg-ledger` + `proptest` dev-dep, `korg-registry`); Python (`korg-ledger-py` + `hypothesis`, already installed); `pytest`.

**Branch:** `feat/phase2-trust-hardening` (stacked on `feat/phase1-unified-verifiable-capture`; rebase onto main once PR #8 merges).

---

## Scope

This is the **first** Phase-2 slice — the parts that are fully verifiable in-repo with no external dependency. Deferred to later Phase-2 slices (they need crypto deps / network / outward publishing, and their own design): per-event Ed25519 **signing** (populate `event_sig`), external **time-anchoring** (RFC-3161 / transparency log), wiring the existing destructive `rewind()` callers (`korg-runtime`/`korg-tui`) onto `rewind_with_seal`, and `cargo-fuzz` targets.

---

### Task 1: Property-based tests for the Rust `korg-ledger` core

**Files:**
- Modify: `crates/korg-ledger/Cargo.toml` (`proptest` dev-dep — already added)
- Modify: `crates/korg-ledger/src/lib.rs` (proptest module)

- [ ] **Step 1: Confirm the dev-dependency is declared**

`crates/korg-ledger/Cargo.toml` must contain:

```toml
[dev-dependencies]
proptest = "1"
```

- [ ] **Step 2: Add the proptest properties to the test module**

In `crates/korg-ledger/src/lib.rs`, inside `#[cfg(test)] mod tests`, add (after the existing `use` lines):

```rust
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    /// Build a valid hash-chain from arbitrary small payloads.
    fn build_chain(payloads: &[BTreeMap<String, i64>], key: Option<&[u8]>) -> Vec<Value> {
        let mut out = Vec::new();
        let mut prev = GENESIS_HASH.to_string();
        for (i, p) in payloads.iter().enumerate() {
            let mut obj = serde_json::Map::new();
            obj.insert("seq_id".into(), json!(i as u64 + 1));
            obj.insert("prev_hash".into(), json!(prev));
            obj.insert("payload".into(), serde_json::to_value(p).unwrap());
            let mut val = Value::Object(obj);
            let h = chain_hash(&val, key);
            val.as_object_mut().unwrap().insert("entry_hash".into(), json!(h));
            prev = h;
            out.push(val);
        }
        out
    }

    proptest! {
        // Any well-formed chain verifies clean (no false positives), keyed or not.
        #[test]
        fn any_built_chain_verifies_clean(
            payloads in prop::collection::vec(
                prop::collection::btree_map("[a-z]{1,5}", any::<i64>(), 0..4), 0..12),
            use_key in any::<bool>(),
        ) {
            let key: Option<&[u8]> = if use_key { Some(b"k") } else { None };
            prop_assert!(verify_chain(&build_chain(&payloads, key), key).is_empty());
        }

        // Mutating ANY event's content is detected (no false negatives).
        #[test]
        fn tampering_any_event_is_detected(
            payloads in prop::collection::vec(
                prop::collection::btree_map("[a-z]{1,5}", any::<i64>(), 1..4), 1..8),
            idx in any::<usize>(),
        ) {
            let mut chain = build_chain(&payloads, None);
            let i = idx % chain.len();
            chain[i].as_object_mut().unwrap().insert("TAMPER".into(), json!(1));
            prop_assert!(!verify_chain(&chain, None).is_empty());
        }

        // Reordering two distinct events breaks the chain.
        #[test]
        fn reordering_breaks_the_chain(
            payloads in prop::collection::vec(
                prop::collection::btree_map("[a-z]{1,5}", any::<i64>(), 1..4), 2..8),
        ) {
            let mut chain = build_chain(&payloads, None);
            chain.swap(0, chain.len() - 1);
            prop_assert!(!verify_chain(&chain, None).is_empty());
        }

        // canonicalize is insertion-order independent (keys are sorted).
        #[test]
        fn canonicalize_is_key_order_independent(
            entries in prop::collection::vec(("[a-z]{1,6}", any::<i64>()), 0..8),
        ) {
            let mut a = serde_json::Map::new();
            for (k, v) in &entries { a.insert(k.clone(), json!(v)); }
            let mut b = serde_json::Map::new();
            for (k, v) in entries.iter().rev() { b.insert(k.clone(), json!(v)); }
            prop_assert_eq!(canonicalize(&Value::Object(a)), canonicalize(&Value::Object(b)));
        }
    }
```

- [ ] **Step 3: Run the property tests**

Run: `cargo test -p korg-ledger`
Expected: PASS — the 4 proptest cases (each runs 256 generated inputs by default) plus the existing unit tests.

- [ ] **Step 4: Commit**

```bash
git add crates/korg-ledger/Cargo.toml crates/korg-ledger/src/lib.rs Cargo.lock
git commit -m "test(korg-ledger): property-based tests for chain integrity, tamper detection, canonicalization"
```

---

### Task 2: Property-based tests for the `korg-ledger-py` producer

**Files:**
- Modify: `adapters/korg-ledger-py/pyproject.toml` (`hypothesis` dev-dep)
- Test: `adapters/korg-ledger-py/tests/test_properties.py`

- [ ] **Step 1: Declare the dev-dependency**

In `adapters/korg-ledger-py/pyproject.toml`, change the dev extra:

```toml
[project.optional-dependencies]
dev = ["pytest>=7", "hypothesis>=6"]
```

- [ ] **Step 2: Write the property tests**

```python
# adapters/korg-ledger-py/tests/test_properties.py
import json
import tempfile
from pathlib import Path

from hypothesis import given, settings, strategies as st

from korg_ledger import (
    CausalityError,
    LedgerWriter,
    agent_tool_call_event,
    verify_chain,
)

# small JSON-object payloads for args
_payloads = st.lists(
    st.dictionaries(st.text(min_size=1, max_size=5), st.integers(), max_size=4),
    max_size=10,
)


def _write(led, payloads):
    w = LedgerWriter(led)
    for p in payloads:
        w.append(
            event=agent_tool_call_event(
                source_agent="a", tool_name="t", args=p, result={},
                success=True, duration_ms=0),
            actor_id="korg:test",
        )
    return [json.loads(l) for l in led.read_text().splitlines() if l.strip()]


@settings(max_examples=150, deadline=None)
@given(payloads=_payloads)
def test_any_sequence_of_appends_verifies_clean(payloads):
    with tempfile.TemporaryDirectory() as d:
        events = _write(Path(d) / "l.jsonl", payloads)
        assert len(events) == len(payloads)
        assert verify_chain(events) == []


@settings(max_examples=150, deadline=None)
@given(payloads=_payloads.filter(lambda p: len(p) >= 1), idx=st.integers(min_value=0))
def test_tampering_any_event_breaks_verification(payloads, idx):
    with tempfile.TemporaryDirectory() as d:
        events = _write(Path(d) / "l.jsonl", payloads)
        i = idx % len(events)
        events[i]["event"]["args"]["__tamper__"] = 1  # mutate without rehashing
        assert verify_chain(events) != []


@settings(max_examples=100, deadline=None)
@given(payloads=_payloads)
def test_resume_preserves_the_chain(payloads):
    with tempfile.TemporaryDirectory() as d:
        led = Path(d) / "l.jsonl"
        _write(led, payloads)
        # a fresh writer resumes and appends one more; the whole chain stays valid
        w2 = LedgerWriter(led)
        w2.append(
            event=agent_tool_call_event(source_agent="a", tool_name="t2", args={}, result={},
                                        success=True, duration_ms=0),
            actor_id="korg:test",
        )
        events = [json.loads(l) for l in led.read_text().splitlines() if l.strip()]
        assert len(events) == len(payloads) + 1
        assert verify_chain(events) == []


@settings(max_examples=50, deadline=None)
@given(bad=st.integers(min_value=1))
def test_causality_gate_rejects_non_earlier_triggered_by(bad):
    with tempfile.TemporaryDirectory() as d:
        w = LedgerWriter(Path(d) / "l.jsonl")
        # first event is seq 1; any triggered_by >= the next seq must be rejected
        s1 = w.append(event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={},
                                                  success=True, duration_ms=0), actor_id="korg:test")
        try:
            w.append(event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={},
                                                 success=True, duration_ms=0),
                     actor_id="korg:test", triggered_by=s1 + 1 + bad)
            raised = False
        except CausalityError:
            raised = True
        assert raised
```

- [ ] **Step 3: Run them**

Run: `PYTHONPATH=adapters/korg-ledger-py/src python3 -m pytest adapters/korg-ledger-py/tests/test_properties.py -q`
Expected: PASS (4 property tests, each exercising up to 150 generated examples).

- [ ] **Step 4: Commit**

```bash
git add adapters/korg-ledger-py/pyproject.toml adapters/korg-ledger-py/tests/test_properties.py
git commit -m "test(korg-ledger-py): hypothesis property tests for chain integrity, tamper, resume, causality"
```

---

### Task 3: `rewind_with_seal` — a tamper-evident, recorded rewind

**Files:**
- Modify: `crates/korg-registry/src/log.rs`

- [ ] **Step 1: Write the failing test** (in the `chain_tests` module, after the existing rewind-adjacent tests)

```rust
    #[test]
    fn rewind_with_seal_records_a_tamper_evident_rewind_event() {
        let mut j = temp_journal("seal");
        j.append(sample_event("Read")); // seq 1
        j.append(sample_event("Edit")); // seq 2
        j.append(sample_event("Bash")); // seq 3
        j.rewind_with_seal(1, "korg:test", "wrong path").unwrap();
        // events after seq 1 are dropped, and a LedgerRewind is appended as the new tip
        assert_eq!(j.events.len(), 2);
        assert_eq!(j.events[0].seq_id, 1);
        match &j.events[1].event {
            CapabilityEvent::LedgerRewind { target_seq_id, invalidated_through, .. } => {
                assert_eq!(*target_seq_id, 1);
                assert_eq!(*invalidated_through, 3);
            }
            other => panic!("expected LedgerRewind tip, got {other:?}"),
        }
        // the chain, including the rewind record, still verifies
        assert!(j.verify_chain().is_empty());
    }

    #[test]
    fn rewind_with_seal_rejects_a_seq_that_never_existed() {
        let mut j = temp_journal("seal-bad");
        j.append(sample_event("Read"));
        assert!(j.rewind_with_seal(99, "x", "y").is_err());
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p korg-registry rewind_with_seal`
Expected: FAIL — `no method named rewind_with_seal`.

- [ ] **Step 3: Implement `rewind_with_seal`** (add immediately after the existing `rewind` method, ~line 668)

```rust
    /// Like `rewind`, but records the rewind as a tamper-evident `LedgerRewind`
    /// event appended as the new chain tip — so a verifier can always see that a
    /// rewind happened and which seq range it invalidated. The invalidated
    /// future is still dropped (so it can't be replayed), but the *fact* of the
    /// rewind is now part of the chain rather than silently erased.
    pub fn rewind_with_seal(
        &mut self,
        target_seq_id: u64,
        rewound_by: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), String> {
        if target_seq_id > self.last_seq_id {
            return Err(format!(
                "Cannot rewind to sequence ID {} which is greater than the current last sequence ID {}",
                target_seq_id, self.last_seq_id
            ));
        }
        if target_seq_id != 0 && !self.events.iter().any(|e| e.seq_id == target_seq_id) {
            return Err(format!(
                "Cannot rewind to sequence ID {} — no event with that seq_id exists",
                target_seq_id
            ));
        }
        let invalidated_through = self.last_seq_id;
        self.events.retain(|e| e.seq_id <= target_seq_id);
        self.last_seq_id = target_seq_id;
        self.clock = self
            .events
            .iter()
            .map(|e| e.metadata.emitted_at)
            .max()
            .unwrap_or_default();
        self.rebuild_triggered_by_index();
        // Append the tamper-evident rewind record as the new tip (this chains
        // onto the retained tip and flushes).
        self.append(CapabilityEvent::LedgerRewind {
            target_seq_id,
            invalidated_through,
            rewound_by: rewound_by.into(),
            reason: reason.into(),
            timestamp: Utc::now(),
        });
        Ok(())
    }
```

- [ ] **Step 4: Run it and watch it pass**

Run: `cargo test -p korg-registry rewind`
Expected: PASS — the two new `rewind_with_seal` tests plus the existing `rewind`/reversible-journal tests (the destructive `rewind` is untouched).

- [ ] **Step 5: Commit**

```bash
git add crates/korg-registry/src/log.rs
git commit -m "feat(korg-registry): rewind_with_seal — record rewinds as tamper-evident LedgerRewind events"
```

---

### Task 4: Whole-system verification

**Files:** none (verification only)

- [ ] **Step 1: Everything green across both languages**

```bash
cargo test -p korg-ledger -p korg-registry 2>&1 | tail -5
PYTHONPATH="adapters/korg-ledger-py/src:adapters/claude-code/src:adapters/korg-setup/src:adapters/recall-mcp/src" \
  python3 -m pytest -q adapters/korg-ledger-py/tests adapters/claude-code/tests adapters/korg-setup/tests \
  adapters/recall-mcp/tests/test_index.py adapters/recall-mcp/tests/test_text.py
python3 spec/korg-ledger-v1/conformance.py
node spec/korg-ledger-v1/js/conformance.mjs
```
Expected: Rust PASS (incl. proptest + rewind_with_seal); Python PASS (incl. the new hypothesis suite); both oracles PASS. The existing CI `conformance` job already runs the Python suite (hypothesis tests included), and `cargo test` runs the proptest cases.

- [ ] **Step 2: Confirm the full workspace still builds** (the destructive `rewind` callers are untouched)

```bash
cargo build 2>&1 | tail -2
```
Expected: `Finished`.

---

## Self-Review

**1. Trust-gap coverage:** "no property-based testing on the trust core" → addressed in **both** the Rust reference (Task 1: clean-chain, tamper, reorder, canonicalization-order properties) and the Python producer (Task 2: append/verify, tamper, resume, causality properties). "rewind leaves no evidence" → addressed by `rewind_with_seal` recording a `LedgerRewind` tip (Task 3). Deferred Phase-2 items (signing, anchoring, fuzz, migrating destructive-`rewind` callers) are listed under Scope.

**2. Placeholder scan:** No TBD/TODO; complete code in every step; exact commands + expected output.

**3. Type/name consistency:** `build_chain`/`chain_hash`/`verify_chain`/`canonicalize`/`GENESIS_HASH` (Rust) and `LedgerWriter`/`agent_tool_call_event`/`verify_chain`/`CausalityError` (Python) match their definitions from Plans 1–2. `rewind_with_seal(target_seq_id, rewound_by, reason)` constructs the `CapabilityEvent::LedgerRewind { target_seq_id, invalidated_through, rewound_by, reason, timestamp }` variant exactly as declared in Phase-1 Plan 2, and reuses the existing `append`, `rebuild_triggered_by_index`, validation, and `temp_journal`/`sample_event` test helpers. No gaps found.
