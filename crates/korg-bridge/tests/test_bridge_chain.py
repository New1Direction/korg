"""
The korg-bridge journal is korg-ledger@v1 hash-chained (idea #3).

Once the Rust core's CapabilityJournal carries the chain (prev_hash/entry_hash
on append), every Python caller writing through this bridge — korgchat, korgex
— inherits a tamper-evident, cross-verifiable journal for free. This test
proves it from first principles: it recomputes the chain with plain stdlib
(sha256 over the canonical preimage) and checks the Rust-written entry_hashes
match. No korg/korgex import, so it cannot silently drift from the spec.

Requires the wheel built + installed (skips otherwise):
    maturin develop -m crates/korg-bridge/Cargo.toml      # or pip install the built wheel
    pytest crates/korg-bridge/tests/test_bridge_chain.py
"""
import hashlib
import json
import os
import tempfile

import pytest

korg_bridge = pytest.importorskip("korg_bridge")

GENESIS = "0" * 64


def _fresh_bridge():
    jp = os.path.join(tempfile.mkdtemp(), "capability_journal.json")
    return korg_bridge.Bridge(jp), jp


def test_triggered_by_nonexistent_parent_is_rejected_on_write():
    """The bridge is the trust boundary: a Python caller must not be able to
    write an event whose triggered_by points at a seq_id that does not exist.
    Catching it at READ time (verify_dag) is too late — the broken event is
    already in the chain. The write itself must fail with CausalityError.
    """
    bridge, jp = _fresh_bridge()
    root = bridge.record_user_prompt("plan and ship")  # seq 1
    assert root == 1

    with pytest.raises(korg_bridge.CausalityError):
        bridge.record_tool_call(
            source_agent="agent:test",
            tool_name="Edit",
            args={"file_path": "src/routes.py"},
            result={"success": True},
            success=True,
            duration_ms=12,
            triggered_by=999,  # no such parent
        )

    # The rejected write must NOT have been persisted — only the root remains.
    try:
        bridge.flush()
    except Exception:
        pass
    events = json.load(open(jp))
    assert [e["seq_id"] for e in events] == [1], "bad write leaked into the journal"
    assert bridge.last_seq_id() == 1, "seq counter advanced on a rejected write"


def test_triggered_by_must_be_strictly_earlier():
    """triggered_by must reference a STRICTLY earlier seq_id. A reference to a
    seq_id that is >= the event being written (self/future) cannot be causal and
    must be rejected at write time, matching verify_dag's strictly-earlier rule.
    """
    bridge, _ = _fresh_bridge()
    bridge.record_user_prompt("root")  # seq 1
    # The next event would be seq 2. triggered_by=2 points at itself; triggered_by
    # values >= the to-be-assigned seq_id are never causal.
    with pytest.raises(korg_bridge.CausalityError):
        bridge.record_tool_call(
            source_agent="agent:test",
            tool_name="T",
            args={},
            result={},
            success=True,
            duration_ms=1,
            triggered_by=2,  # == own seq_id → not strictly earlier
        )


def test_valid_triggered_by_still_accepted():
    """The gate must not break the happy path: a triggered_by that names a real,
    strictly-earlier seq_id is accepted and chains as before."""
    bridge, _ = _fresh_bridge()
    root = bridge.record_user_prompt("root")  # seq 1
    seq = bridge.record_tool_call(
        source_agent="agent:test",
        tool_name="Edit",
        args={"file_path": "a.py"},
        result={"ok": True},
        success=True,
        duration_ms=3,
        triggered_by=root,
    )
    assert seq == 2


def _canonical(obj):
    # korg-ledger@v1 canonicalization: sorted keys, compact, ASCII-escaped.
    return json.dumps(obj, sort_keys=True, separators=(",", ":")).encode("ascii")


def _chain_hash(event):
    preimage = {k: v for k, v in event.items() if k != "entry_hash"}
    return hashlib.sha256(_canonical(preimage)).hexdigest()


def test_bridge_journal_is_a_verifiable_chain():
    jp = os.path.join(tempfile.mkdtemp(), "capability_journal.json")
    bridge = korg_bridge.Bridge(jp)
    root = bridge.record_user_prompt("add a /healthz endpoint")
    bridge.record_tool_call(
        source_agent="agent:test",
        tool_name="Edit",
        args={"file_path": "src/routes.py"},
        result={"success": True},
        success=True,
        duration_ms=12,
        triggered_by=root,
    )
    try:
        bridge.flush()
    except Exception:
        pass

    events = json.load(open(jp))
    assert len(events) == 2, events

    # Every event chains, and each Rust-written entry_hash equals a from-scratch
    # Python sha256 of its canonical preimage — i.e. the Rust bridge and a stdlib
    # verifier agree byte-for-byte (the cross-impl guarantee).
    prev = GENESIS
    for e in events:
        assert "prev_hash" in e and "entry_hash" in e, e
        assert e["prev_hash"] == prev, f"broken link at seq {e.get('seq_id')}"
        assert _chain_hash(e) == e["entry_hash"], f"hash mismatch at seq {e.get('seq_id')}"
        prev = e["entry_hash"]
