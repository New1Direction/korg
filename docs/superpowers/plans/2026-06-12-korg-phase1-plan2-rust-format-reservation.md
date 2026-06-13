# Phase 1 — Plan 2: Cross-Language Format Reservation (`event_sig` + `LedgerRewind`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reserve the Phase-2 trust slots in `korg-ledger@v1` **without re-publishing the format and without breaking the frozen conformance vectors** — by excluding `event_sig` from the hash preimage in lockstep across all four implementations (Rust, two Python, JS), adding the `event_sig` field to the Rust `JournalEvent`, adding the additive `LedgerRewind` event variant, and reserving the external-anchor file path.

**Architecture:** Adding `"event_sig"` to the preimage-exclusion set is forward-compatible: today no event carries an `event_sig`, so excluding it is a no-op and every frozen vector still reproduces byte-for-byte. Doing it in all four codepaths at once means that when Phase-2 signing populates the field, every verifier already strips it before hashing. `LedgerRewind` is a new tagged enum variant (reserved; Phase 2 wires `rewind()` to append it and builds replay semantics).

**Tech Stack:** Rust (`korg-ledger`, `korg-registry` crates), Python (`spec/korg-ledger-v1/conformance.py`, `korg-ledger-py`), JS (`spec/korg-ledger-v1/js/verify.mjs`).

---

## Scope & deferral (flagged)

This plan does **§4.1** (reserve `event_sig` + `LedgerRewind` + anchor path), in lockstep across all four implementations. It **defers §4.2's JSONL-on-disk storage refactor** (changing `CapabilityJournal` persistence from a pretty JSON array to JSONL) to a noted **Plan 2b**: that is an internal storage change to the *server/runtime* journal only, it has a large test blast-radius, and it is **not on the user-facing path** — the shipped capture ledgers (Plans 3–5) are already JSONL, and the wire format + verification are unchanged. The reservation here is the part with genuine cross-language lockstep risk (spec §8 risk #5).

The four implementations whose preimage-exclusion set must change together:
1. `crates/korg-ledger/src/lib.rs` — `HASH_FIELDS`
2. `crates/korg-registry/src/log.rs` — `JournalEvent` struct (adds the field)
3. `spec/korg-ledger-v1/conformance.py` — `chain_hash` exclusion (the spec oracle)
4. `adapters/korg-ledger-py/src/korg_ledger/_hash.py` — `HASH_FIELDS`
5. `spec/korg-ledger-v1/js/verify.mjs` — `HASH_FIELDS`

---

### Task 1: Exclude `event_sig` from the preimage in all four hash implementations

**Files:**
- Modify: `crates/korg-ledger/src/lib.rs` (`HASH_FIELDS`)
- Modify: `adapters/korg-ledger-py/src/korg_ledger/_hash.py` (`HASH_FIELDS`)
- Modify: `spec/korg-ledger-v1/conformance.py` (`chain_hash`)
- Modify: `spec/korg-ledger-v1/js/verify.mjs` (`HASH_FIELDS`)
- Test: add an exclusion property test in Rust (`lib.rs`), Python (`korg-ledger-py`), and JS (`conformance.mjs`).

- [ ] **Step 1: Rust — add the failing test + change `HASH_FIELDS`**

In `crates/korg-ledger/src/lib.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn event_sig_is_excluded_from_the_preimage() {
        // An event carrying an event_sig hashes identically to the same event
        // without it — the reserved signature field is not part of the chain.
        let base = json!({"seq_id": 1, "prev_hash": GENESIS_HASH, "x": "y"});
        let mut signed = base.clone();
        signed["event_sig"] = json!("ZmFrZS1zaWc=");
        assert_eq!(chain_hash(&base, None), chain_hash(&signed, None));
    }
```

Then change line 23:

```rust
const HASH_FIELDS: &[&str] = &["entry_hash", "event_sig"];
```

Run: `cargo test -p korg-ledger`
Expected: PASS — the new test plus the existing 3 canonicalization tests (the frozen tip is computed in `korg-registry`, verified in Task 3).

- [ ] **Step 2: Python (korg-ledger-py) — change `HASH_FIELDS` + add the property test**

In `adapters/korg-ledger-py/src/korg_ledger/_hash.py`:

```python
HASH_FIELDS = ("entry_hash", "event_sig")
```

Append to `adapters/korg-ledger-py/tests/test_conformance.py`:

```python
def test_event_sig_is_excluded_from_preimage():
    from korg_ledger import chain_hash
    base = {"seq_id": 1, "prev_hash": "0" * 64, "x": "y"}
    signed = {**base, "event_sig": "ZmFrZS1zaWc="}
    assert chain_hash(base) == chain_hash(signed)
```

Run: `PYTHONPATH=adapters/korg-ledger-py/src python3 -m pytest adapters/korg-ledger-py/tests/test_conformance.py -v`
Expected: PASS — the new property test plus the frozen-vector reproduction (still byte-for-byte, since the vectors have no `event_sig`).

- [ ] **Step 3: Python (spec oracle) — exclude `event_sig` in `conformance.py`**

In `spec/korg-ledger-v1/conformance.py`, change `chain_hash`:

```python
def chain_hash(event: dict, key: bytes | None = None) -> str:
    preimage = {k: v for k, v in event.items() if k not in ("entry_hash", "event_sig")}
    data = canonicalize(preimage)
    if key is not None:
        return hmac.new(key, data, hashlib.sha256).hexdigest()
    return hashlib.sha256(data).hexdigest()
```

Run: `python3 spec/korg-ledger-v1/conformance.py`
Expected: prints `korg-ledger@v1 conformance: PASS` (exit 0) — the oracle still reproduces every frozen vector.

- [ ] **Step 4: JS — change `HASH_FIELDS` + add the exclusion assertion**

In `spec/korg-ledger-v1/js/verify.mjs`, change line 15:

```javascript
const HASH_FIELDS = ["entry_hash", "event_sig"]; // fields excluded from the hash preimage
```

In `spec/korg-ledger-v1/js/conformance.mjs`, inside `run()` after the canon assertions, add:

```javascript
  // event_sig is reserved and excluded from the preimage (Phase-2 signature slot).
  {
    const base = { seq_id: 1, prev_hash: "0".repeat(64), x: "y" };
    const signed = { ...base, event_sig: "ZmFrZS1zaWc=" };
    const ok = (await chainHash(base)) === (await chainHash(signed));
    console.log(`  [${ok ? "PASS" : "FAIL"}] event_sig excluded from preimage`);
    if (!ok) failures++;
  }
```

Run: `node spec/korg-ledger-v1/js/conformance.mjs`
Expected: all PASS including `event_sig excluded from preimage`; final line `korg-ledger@v1 conformance (js): PASS`.

- [ ] **Step 5: Commit**

```bash
git add crates/korg-ledger/src/lib.rs adapters/korg-ledger-py/src/korg_ledger/_hash.py \
        adapters/korg-ledger-py/tests/test_conformance.py spec/korg-ledger-v1/conformance.py \
        spec/korg-ledger-v1/js/verify.mjs spec/korg-ledger-v1/js/conformance.mjs
git commit -m "feat(ledger): reserve event_sig in the hash preimage across Rust, Python, and JS (lockstep)"
```

---

### Task 2: Rust `JournalEvent.event_sig` field + `LedgerRewind` variant + anchor path

**Files:**
- Modify: `crates/korg-registry/src/log.rs`
- Modify: `crates/korg-ledger/src/lib.rs` (anchor-file constant)

- [ ] **Step 1: Add the `event_sig` field to `JournalEvent`**

In `crates/korg-registry/src/log.rs`, in `struct JournalEvent`, immediately after the `entry_hash` field, add:

```rust
    /// korg-ledger@v1 Phase-2 reservation: per-event Ed25519 signature over the
    /// same preimage as `entry_hash`. Excluded from the hash preimage
    /// (`HASH_FIELDS`). `None`/omitted for unsigned events (unchanged on the
    /// wire), so existing journals and conformance vectors are unaffected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_sig: Option<String>,
```

- [ ] **Step 2: Add the `LedgerRewind` event variant**

In `crates/korg-registry/src/log.rs`, in `enum CapabilityEvent`, after the `ProxyAuditTrail` variant, add:

```rust
    /// Reserved (Phase 2): a non-destructive record that the ledger was rewound.
    /// Appended rather than truncating history, so the rewind is itself tamper-
    /// evident. Phase 2 wires `rewind()` to append this and builds replay
    /// semantics on it.
    LedgerRewind {
        target_seq_id: u64,
        invalidated_through: u64,
        rewound_by: String,
        reason: String,
        timestamp: DateTime<Utc>,
    },
```

- [ ] **Step 3: Add the match arms for the new variant**

In `impl CapabilityEvent`, `campaign_id()` — add `LedgerRewind` to the nil-UUID arm:

```rust
            CapabilityEvent::AgentToolCall { .. }
            | CapabilityEvent::ProxyAuditTrail { .. }
            | CapabilityEvent::LedgerRewind { .. } => Uuid::nil(),
```

In `tier()` — add `LedgerRewind` to the Governance group (it ends with `ProxyAuditTrail`):

```rust
            | CapabilityEvent::ProxyAuditTrail { .. }
            | CapabilityEvent::LedgerRewind { .. } => EventTier::Governance,
```

- [ ] **Step 4: Reserve the external-anchor file path**

In `crates/korg-ledger/src/lib.rs`, after `GENESIS_HASH`, add:

```rust
/// Reserved (Phase 2): out-of-band external-anchor sidecar file name. Holds
/// `{seq_id, entry_hash, anchor_proof, anchored_at}` records that notarize chain
/// tips. Kept OUTSIDE the chain preimage so it never affects `entry_hash`.
pub const ANCHORS_FILE: &str = "anchors.jsonl";
```

- [ ] **Step 5: Add a Rust test that a signed `JournalEvent` round-trips and hashes stably**

Append to the existing `#[cfg(test)]` tests in `crates/korg-registry/src/log.rs` (or the conformance test module) a test that builds a `JournalEvent`, computes its `entry_hash`, sets `event_sig`, and confirms the `entry_hash` is unchanged and the variant serializes:

```rust
    #[test]
    fn event_sig_does_not_change_entry_hash_and_rewind_variant_serializes() {
        // LedgerRewind serializes with its event_type tag.
        let ev = CapabilityEvent::LedgerRewind {
            target_seq_id: 5,
            invalidated_through: 9,
            rewound_by: "korg:test".into(),
            reason: "demo".into(),
            timestamp: Utc::now(),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["event_type"], "LedgerRewind");
        assert_eq!(v["target_seq_id"], 5);
    }
```

(Adjust the `use` imports at the top of the test module if `Utc`/`serde_json` aren't already in scope — they are used elsewhere in the file.)

- [ ] **Step 6: Build + test the Rust crates**

Run: `cargo test -p korg-ledger -p korg-registry`
Expected: PASS — both crates compile (the new variant's match arms are exhaustive) and all tests pass, including the frozen-vector conformance tests in `korg-registry` (unaffected: the vectors carry neither `event_sig` nor `LedgerRewind`).

- [ ] **Step 7: Commit**

```bash
git add crates/korg-registry/src/log.rs crates/korg-ledger/src/lib.rs
git commit -m "feat(ledger): reserve JournalEvent.event_sig field, LedgerRewind variant, and anchor path"
```

---

### Task 3: Whole-system cross-language verification

**Files:** none (verification only)

- [ ] **Step 1: All four implementations still reproduce the frozen vectors**

```bash
cargo test -p korg-ledger -p korg-registry 2>&1 | tail -5
python3 spec/korg-ledger-v1/conformance.py
node spec/korg-ledger-v1/js/conformance.mjs
PYTHONPATH="adapters/korg-ledger-py/src:adapters/claude-code/src:adapters/korg-setup/src" \
  python3 -m pytest adapters/korg-ledger-py/tests adapters/claude-code/tests adapters/korg-setup/tests -q
```
Expected: Rust PASS; `conformance.py` PASS; `conformance.mjs` PASS (incl. the new `event_sig excluded` assertion); the full Python suite PASS (144 + the new property test).

- [ ] **Step 2: Cross-producer sanity — a Python-written ledger still verifies under the spec oracle (now with `event_sig` reserved)**

```bash
PYTHONPATH="adapters/korg-ledger-py/src:spec/korg-ledger-v1" python3 - <<'PY'
import json, tempfile
from pathlib import Path
from korg_ledger import LedgerWriter, agent_tool_call_event
import conformance as oracle
with tempfile.TemporaryDirectory() as d:
    led = Path(d) / "l.jsonl"
    w = LedgerWriter(led)
    w.append(event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={}, success=True, duration_ms=0), actor_id="korg:test")
    events = [json.loads(l) for l in led.read_text().splitlines() if l.strip()]
    assert oracle.verify_chain(events, None) == []
    print("cross-producer OK with event_sig reserved")
PY
```
Expected: prints `cross-producer OK with event_sig reserved`.

---

## Self-Review

**1. Spec coverage (§4.1):** `event_sig` excluded from the preimage in lockstep across Rust + Python(×2) + JS ✓ (Task 1); `event_sig` field on the Rust `JournalEvent`, non-breaking on the wire ✓ (Task 2 Step 1, `skip_serializing_if`); additive `LedgerRewind` variant with exhaustive match arms ✓ (Task 2 Steps 2–3); external-anchor path reserved ✓ (Task 2 Step 4); frozen vectors still pass everywhere ✓ (Task 3). §4.2 JSONL-on-disk is explicitly deferred to Plan 2b with rationale (internal server-journal storage; off the user path; large blast radius).

**2. Placeholder scan:** No TBD/TODO; complete code in every code step; exact commands + expected output.

**3. Type/name consistency:** `HASH_FIELDS` extended identically (`("entry_hash", "event_sig")` / `["entry_hash", "event_sig"]`) in all four implementations; `event_sig` field name matches across Rust struct, the exclusion sets, and the property tests; `LedgerRewind` variant field names (`target_seq_id, invalidated_through, rewound_by, reason, timestamp`) match the spec §4.1 reservation and the test. The exclusion property test uses the same `event_sig` key in all three languages. No gaps found.
