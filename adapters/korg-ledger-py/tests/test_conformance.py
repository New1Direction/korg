import json
from pathlib import Path

import pytest

from korg_ledger import chain_hash, verify_chain

# tests/ -> korg-ledger-py -> adapters -> <repo root>
SPEC = Path(__file__).resolve().parents[3] / "spec" / "korg-ledger-v1"


def _vectors():
    manifest = json.loads((SPEC / "conformance.json").read_text())
    assert manifest["spec_version"] == "korg-ledger@v1"
    return manifest["vectors"]


def _read(name):
    text = (SPEC / "vectors" / name).read_text()
    return [json.loads(line) for line in text.splitlines() if line.strip()]


@pytest.mark.skipif(not SPEC.exists(), reason="spec vectors not present")
def test_reproduces_frozen_tip_hashes():
    for v in _vectors():
        if v["verify"] != "intact":
            continue
        events = _read(v["file"])
        key = v["key"].encode() if v.get("key") else None
        assert verify_chain(events, key) == [], f"{v['file']} should verify clean"
        assert chain_hash(events[-1], key) == v["tip_entry_hash"], v["file"]


@pytest.mark.skipif(not SPEC.exists(), reason="spec vectors not present")
def test_detects_tampering():
    for v in _vectors():
        if v["verify"] != "tampered":
            continue
        events = _read(v["file"])
        key = v["key"].encode() if v.get("key") else None
        errors = verify_chain(events, key)
        assert errors, f"{v['file']} should report tampering"
        assert any(v["error_contains"] in e for e in errors), errors
