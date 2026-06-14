import json

import pytest

pytest.importorskip("cryptography")

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from korg_ledger import LedgerWriter, agent_tool_call_event
from korg_ledger.signing import sign_event, verify_event_sig

SEED = bytes(range(32))
PUB = Ed25519PrivateKey.from_private_bytes(SEED).public_key().public_bytes_raw()


def test_sign_verify_roundtrip_and_tamper():
    ev = {"seq_id": 1, "prev_hash": "0" * 64, "payload": {"a": 1}}
    sig = sign_event(SEED, ev)
    assert len(sig) == 128  # 64-byte signature as lowercase hex
    assert verify_event_sig(PUB, ev, sig)
    assert not verify_event_sig(PUB, {**ev, "payload": {"a": 2}}, sig)
    assert not verify_event_sig(PUB, ev, "00")  # malformed sig → False, no raise


def test_signature_excludes_entry_hash_and_event_sig():
    a = {"seq_id": 1, "prev_hash": "0" * 64, "x": "y"}
    b = {**a, "entry_hash": "anything", "event_sig": "anything"}
    assert sign_event(SEED, a) == sign_event(SEED, b)


def test_writer_with_signing_key_sets_verifiable_event_sig(tmp_path):
    led = tmp_path / "l.jsonl"
    w = LedgerWriter(led, signing_key=SEED)
    w.append(
        event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={},
                                    success=True, duration_ms=0),
        actor_id="korg:test",
    )
    rec = json.loads(led.read_text().splitlines()[0])
    assert "event_sig" in rec and len(rec["event_sig"]) == 128
    assert verify_event_sig(PUB, rec, rec["event_sig"])


def test_writer_without_key_omits_event_sig(tmp_path):
    led = tmp_path / "l.jsonl"
    w = LedgerWriter(led)
    w.append(
        event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={},
                                    success=True, duration_ms=0),
        actor_id="korg:test",
    )
    assert "event_sig" not in json.loads(led.read_text().splitlines()[0])
