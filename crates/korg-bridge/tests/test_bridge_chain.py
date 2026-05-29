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
