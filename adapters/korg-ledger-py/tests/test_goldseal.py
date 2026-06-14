"""goldseal@v1 — derivation, structural verification, and (crypto) seal tests.

The structural half (derivation + chain/tip/summary checks) is stdlib-only and
always runs. The Ed25519 seal half needs `cryptography` and is skipped when the
optional [signing] extra is absent — mirroring test_signing.py.
"""
import copy
import json
from pathlib import Path

import pytest

from korg_ledger import chain_hash
from korg_ledger.goldseal import (
    SCHEMA,
    build_envelope,
    derive_summary,
    seal_header,
    verify_structure,
)

AGENT = "agent:korgex@0.14.1"
REPO = Path(__file__).resolve().parents[3]
FIXTURE = REPO / "crates" / "korg-verify" / "tests" / "fixtures" / "goldseal-v1.json"


def _chain(steps):
    """Build a flat-shape korg-ledger@v1 chain from (tool, args, triggered_by)."""
    events = []
    prev = "0" * 64
    for i, (tool, args, triggered_by) in enumerate(steps, start=1):
        rec = {
            "schema_version": "1.0",
            "seq_id": i,
            "source_agent": AGENT,
            "tool_name": tool,
            "args": args,
            "result": {},
            "payload_refs": [],
            "success": True,
            "duration_ms": 0,
            "prev_hash": prev,
        }
        if triggered_by is not None:
            rec["triggered_by"] = triggered_by
        rec["entry_hash"] = chain_hash(rec)
        prev = rec["entry_hash"]
        events.append(rec)
    return events


def _sample_chain():
    return _chain(
        [
            ("user_prompt", {"prompt": "add healthz"}, None),
            ("Read", {"file_path": "src/app.py"}, 1),
            ("Edit", {"file_path": "src/app.py"}, 2),
            ("Bash", {"command": "pytest -q"}, 3),
        ]
    )


# ── derivation ───────────────────────────────────────────────────────────────


def test_derive_summary_is_a_pure_function_of_events():
    events = _sample_chain()
    s = derive_summary(events)
    assert s == {
        "agents": [AGENT],
        "by_tool": {"Bash": 1, "Edit": 1, "Read": 1, "user_prompt": 1},
        "files": ["src/app.py"],  # deduped + sorted
        "seq_first": 1,
        "seq_last": 4,
    }


def test_derive_summary_handles_nested_journalevent_shape():
    """Capture ledgers nest the payload under `event`; derivation must agree."""
    flat = _sample_chain()
    nested = []
    for e in flat:
        n = {k: v for k, v in e.items() if k not in ("source_agent", "tool_name", "args")}
        n["event"] = {
            "source_agent": e["source_agent"],
            "tool_name": e["tool_name"],
            "args": e["args"],
        }
        nested.append(n)
    assert derive_summary(nested) == derive_summary(flat)


# ── structural verification (no crypto) ──────────────────────────────────────


def test_build_envelope_then_structure_is_clean():
    env = build_envelope(events=_sample_chain(), claim="c", issuer_agent=AGENT, issued_at=1)
    assert env["schema"] == SCHEMA
    assert verify_structure(env) == []


def test_structure_rejects_a_lying_summary():
    env = build_envelope(events=_sample_chain(), claim="c", issuer_agent=AGENT, issued_at=1)
    env["summary"]["files"] = []  # hide the touched file
    errs = verify_structure(env)
    assert any("summary does not match" in e for e in errs)


def test_structure_rejects_event_count_lie():
    env = build_envelope(events=_sample_chain(), claim="c", issuer_agent=AGENT, issued_at=1)
    env["event_count"] = 99
    assert any("event_count" in e for e in verify_structure(env))


def test_structure_rejects_tip_mismatch():
    env = build_envelope(events=_sample_chain(), claim="c", issuer_agent=AGENT, issued_at=1)
    env["tip"] = "0" * 64
    assert any("tip" in e for e in verify_structure(env))


def test_structure_rejects_a_tampered_event():
    env = build_envelope(events=_sample_chain(), claim="c", issuer_agent=AGENT, issued_at=1)
    env["events"][1]["args"]["file_path"] = "src/evil.py"
    assert verify_structure(env) != []


def test_seal_header_excludes_events_seal_anchors():
    env = build_envelope(events=_sample_chain(), claim="c", issuer_agent=AGENT, issued_at=1)
    env["seal"] = {"sig": "x"}
    env["anchors"] = [{"seq_id": 1}]
    h = seal_header(env)
    assert "events" not in h and "seal" not in h and "anchors" not in h
    assert h["claim"] == "c" and h["tip"] == env["tip"]


def test_empty_chain_cannot_be_sealed():
    with pytest.raises(ValueError):
        build_envelope(events=[], claim="c", issuer_agent=AGENT, issued_at=1)


# ── the Ed25519 seal (requires cryptography) ─────────────────────────────────
# Gate ONLY the crypto tests — the structural tests above are stdlib-only and
# must run everywhere (including CI, which does not install the [signing] extra).

try:
    import cryptography  # noqa: F401

    from korg_ledger.signing import mint_seal, public_key_hex, verify_seal

    _HAS_CRYPTO = True
except ImportError:  # pragma: no cover
    _HAS_CRYPTO = False

requires_crypto = pytest.mark.skipif(not _HAS_CRYPTO, reason="cryptography not installed")
SEED = bytes([42]) * 32


@requires_crypto
def test_mint_then_verify_roundtrip_and_pin():
    env = mint_seal(
        events=_sample_chain(),
        claim="did the thing",
        issuer_agent=AGENT,
        issued_at=1760000000,
        private_seed=SEED,
    )
    pub = public_key_hex(SEED)
    assert verify_seal(env) == []
    assert verify_seal(env, pin_pubkey=pub) == []
    assert verify_seal(env, pin_pubkey="00" * 32) != []


@requires_crypto
def test_seal_binds_the_claim():
    env = mint_seal(events=_sample_chain(), claim="A", issuer_agent=AGENT, issued_at=1, private_seed=SEED)
    tampered = copy.deepcopy(env)
    tampered["claim"] = "B"
    errs = verify_seal(tampered)
    assert any("seal signature" in e for e in errs)
    # the chain itself is untouched — only the seal fails
    assert not any("chain" in e for e in errs)


@requires_crypto
def test_stripped_seal_is_a_downgrade():
    env = mint_seal(events=_sample_chain(), claim="A", issuer_agent=AGENT, issued_at=1, private_seed=SEED)
    del env["seal"]
    assert any("seal is absent" in e for e in verify_seal(env))


@requires_crypto
def test_frozen_fixture_verifies_under_python():
    """The committed cross-impl fixture (also verified by Rust + JS) round-trips."""
    env = json.loads(FIXTURE.read_text())
    assert verify_seal(env) == []
    # and re-minting the same inputs reproduces it byte-for-byte (deterministic).
    assert env["seal"]["pubkey"] == public_key_hex(SEED)
