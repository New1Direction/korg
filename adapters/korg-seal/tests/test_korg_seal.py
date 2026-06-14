"""korg-seal — key management, minting, and CLI roundtrip.

Minting needs cryptography (it signs), so the whole suite skips without it —
mirroring korg-ledger-py's signing tests. CI installs only pytest, so these
skip there; the cross-impl agreement (a korg-seal mint verifies under the Rust
binary) is exercised by the end-to-end smoke in the workflow.
"""
from __future__ import annotations

import json
import os
import stat
import sys

import pytest

pytest.importorskip("cryptography")

from korg_ledger import chain_hash  # noqa: E402
from korg_seal import keys, minter as mint_mod  # noqa: E402
from korg_seal.cli import main as cli_main  # noqa: E402

AGENT = "agent:korgex@0.14.1"


def _chain(steps):
    events, prev = [], "0" * 64
    for i, (tool, args, tb) in enumerate(steps, start=1):
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
        if tb is not None:
            rec["triggered_by"] = tb
        rec["entry_hash"] = chain_hash(rec)
        prev = rec["entry_hash"]
        events.append(rec)
    return events


def _sample(tmp_path):
    events = _chain(
        [
            ("user_prompt", {"prompt": "add healthz"}, None),
            ("Edit", {"file_path": "src/app.py"}, 1),
            ("Bash", {"command": "pytest -q"}, 2),
        ]
    )
    p = tmp_path / "session.jsonl"
    p.write_text("\n".join(json.dumps(e) for e in events) + "\n", encoding="utf-8")
    return p, events


# ── keys ─────────────────────────────────────────────────────────────────────


def test_key_is_generated_once_with_0600_and_is_stable(tmp_path):
    kp = tmp_path / "issuer.ed25519"
    seed1 = keys.load_or_create_seed(kp)
    assert len(seed1) == 32
    assert stat.S_IMODE(os.stat(kp).st_mode) == 0o600
    seed2 = keys.load_or_create_seed(kp)  # idempotent
    assert seed1 == seed2
    assert len(keys.public_key_hex(seed1)) == 64


def test_corrupt_key_file_is_rejected(tmp_path):
    kp = tmp_path / "issuer.ed25519"
    kp.write_bytes(b"too short")
    with pytest.raises(ValueError):
        keys.load_or_create_seed(kp)


# ── mint ─────────────────────────────────────────────────────────────────────


def test_mint_produces_a_seal_that_verifies(tmp_path):
    from korg_ledger.signing import verify_seal

    ledger, _ = _sample(tmp_path)
    seed = bytes([7]) * 32
    seal = mint_mod.mint(ledger_path=ledger, claim="added healthz", seed=seed, issued_at=1)
    assert seal["schema"] == "goldseal@v1"
    assert verify_seal(seal) == []
    # issuer label is derived from the key when not given
    assert seal["issuer"]["agent"].startswith("agent:korg-seal#")
    # summary was derived from the events, not asserted
    assert seal["summary"]["files"] == ["src/app.py"]


def test_mint_refuses_a_broken_chain(tmp_path):
    ledger, events = _sample(tmp_path)
    events[1]["args"]["file_path"] = "src/evil.py"  # break the hash chain
    ledger.write_text("\n".join(json.dumps(e) for e in events) + "\n", encoding="utf-8")
    with pytest.raises(ValueError, match="does not verify"):
        mint_mod.mint(ledger_path=ledger, claim="x", seed=bytes([7]) * 32, issued_at=1)


def test_mint_is_deterministic(tmp_path):
    ledger, _ = _sample(tmp_path)
    seed = bytes([7]) * 32
    a = mint_mod.mint(ledger_path=ledger, claim="c", seed=seed, issued_at=1)
    b = mint_mod.mint(ledger_path=ledger, claim="c", seed=seed, issued_at=1)
    assert a == b  # same inputs → byte-identical seal


# ── CLI ──────────────────────────────────────────────────────────────────────


def test_cli_mint_then_verify_roundtrip(tmp_path, capsys):
    ledger, _ = _sample(tmp_path)
    kp = tmp_path / "issuer.ed25519"
    out = tmp_path / "seal.json"

    rc = cli_main(["mint", str(ledger), "--claim", "added a healthz endpoint", "--key", str(kp), "-o", str(out)])
    assert rc == 0
    assert out.exists()

    rc = cli_main(["verify", str(out)])
    assert rc == 0
    assert "VALID" in capsys.readouterr().out

    # pin the actual issuer → still valid; a wrong pin → invalid
    pub = keys.public_key_hex(keys.load_or_create_seed(kp))
    assert cli_main(["verify", str(out), "--pin", pub]) == 0
    assert cli_main(["verify", str(out), "--pin", "00" * 32]) == 1


def test_cli_verify_rejects_a_tampered_seal(tmp_path):
    ledger, _ = _sample(tmp_path)
    kp = tmp_path / "issuer.ed25519"
    out = tmp_path / "seal.json"
    cli_main(["mint", str(ledger), "--claim", "c", "--key", str(kp), "-o", str(out)])

    env = json.loads(out.read_text())
    env["claim"] = "something the issuer never signed"
    out.write_text(json.dumps(env))
    assert cli_main(["verify", str(out)]) == 1


def test_cli_key_prints_pubkey(tmp_path, capsys):
    kp = tmp_path / "issuer.ed25519"
    rc = cli_main(["key", "--key", str(kp)])
    assert rc == 0
    assert len(capsys.readouterr().out.strip()) == 64


def test_cli_mint_missing_ledger_is_usage_error(tmp_path):
    assert cli_main(["mint", str(tmp_path / "nope.jsonl"), "--claim", "c", "--key", str(tmp_path / "k")]) == 2


# ── git-tip anchor resolution (the "when" step) ──────────────────────────────

from korg_seal import resolve as resolver  # noqa: E402


def test_parse_github_repo_forms():
    for url in [
        "https://github.com/New1Direction/korg",
        "https://github.com/New1Direction/korg.git",
        "github.com/New1Direction/korg/",
        "New1Direction/korg",
    ]:
        assert resolver.parse_github_repo(url) == ("New1Direction", "korg")
    with pytest.raises(ValueError):
        resolver.parse_github_repo("not a repo")


def _fake_commit(patch_contains, date="2026-06-14T06:01:28Z"):
    patch = f"+  \"tip\": \"{patch_contains}\"\n" if patch_contains else "+ unrelated\n"
    return lambda owner, name, sha: {
        "commit": {"committer": {"date": date}},
        "files": [{"filename": "seal.json", "patch": patch}],
    }


def test_resolve_anchor_witnessed():
    anchor = {
        "seq_id": 5,
        "entry_hash": "deadbeef" * 8,
        "anchor_kind": "git-tip",
        "anchor_proof": {"repo": "github.com/New1Direction/korg", "commit": "0e566b0"},
    }
    r = resolver.resolve_anchor(anchor, fetch=_fake_commit("deadbeef" * 8))
    assert r.witnessed is True
    assert r.committed_at == "2026-06-14T06:01:28Z"


def test_resolve_anchor_not_witnessed_when_hash_absent():
    anchor = {
        "seq_id": 5,
        "entry_hash": "deadbeef" * 8,
        "anchor_kind": "git-tip",
        "anchor_proof": {"repo": "New1Direction/korg", "commit": "abc"},
    }
    r = resolver.resolve_anchor(anchor, fetch=_fake_commit(None))
    assert r.witnessed is False
    assert "does not introduce" in r.detail


def test_resolve_anchor_handles_missing_commit():
    import urllib.error

    def boom(owner, name, sha):
        raise urllib.error.HTTPError("u", 404, "Not Found", {}, None)

    anchor = {
        "seq_id": 1,
        "entry_hash": "ab" * 32,
        "anchor_kind": "git-tip",
        "anchor_proof": {"repo": "New1Direction/korg", "commit": "missing"},
    }
    r = resolver.resolve_anchor(anchor, fetch=boom)
    assert r.witnessed is False
    assert "404" in r.detail


def test_anchor_rebinds_and_stays_verifiable(tmp_path):
    from korg_ledger.signing import verify_seal

    ledger, _ = _sample(tmp_path)
    seed = bytes([7]) * 32
    seal = mint_mod.mint(ledger_path=ledger, claim="did the thing", seed=seed, issued_at=1)
    assert "anchors" not in seal

    anchored = mint_mod.anchor(
        seal=seal, repo="github.com/New1Direction/korg", commit="0e566b0", seed=seed
    )
    # the anchor is present, the claim/issuer/tip are preserved, and it verifies
    assert anchored["anchors"][0]["anchor_proof"]["commit"] == "0e566b0"
    assert anchored["claim"] == "did the thing"
    assert anchored["tip"] == seal["tip"]
    assert verify_seal(anchored) == []

    # the anchor is BOUND: stripping it breaks the seal (re-mint re-signed it in)
    import copy

    stripped = copy.deepcopy(anchored)
    del stripped["anchors"]
    assert verify_seal(stripped) != []
