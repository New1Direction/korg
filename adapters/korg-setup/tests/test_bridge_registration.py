"""Tests for the bridge auto-registration step inside run_setup()."""

from __future__ import annotations

import json
import stat
import sys
import textwrap
from pathlib import Path
from unittest.mock import patch

import pytest

from korg_setup.setup import DEFAULT_BRIDGE_ALLOW, run_setup


def _make_fake_introspect_bin(tmp_path: Path, name: str) -> Path:
    """Build a minimal introspect-aware binary."""
    doc = {
        "schema": "korg:introspect@v1",
        "binary": name,
        "version": "0.0.1",
        "callables_declared": True,
        "callables": [
            {
                "command_id": f"{name}.echo",
                "name": "echo",
                "description": "echo",
                "input_schema": {"type": "object"},
                "capabilities": {"side_effects": "none"},
            }
        ],
    }
    doc_path = tmp_path / f"{name}.introspect.json"
    doc_path.write_text(json.dumps(doc))
    script = textwrap.dedent(f"""\
        #!{sys.executable}
        import sys
        if "--introspect" in sys.argv[1:]:
            with open({json.dumps(str(doc_path))}) as f:
                sys.stdout.write(f.read())
            sys.exit(0)
        sys.exit(0)
    """)
    p = tmp_path / name
    p.write_text(script)
    p.chmod(p.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return p


def _which_stub_factory(installed: dict[str, str]):
    """Build a shutil.which stub that returns paths only for `installed` names."""
    def _stub(name: str):
        return installed.get(name)
    return _stub


@pytest.fixture
def setup_env(tmp_path, monkeypatch):
    """Common setup: tmp config file, tmp ledger dir, fake binaries on PATH."""
    # Pre-install the required core binaries (fakes — they don't need to actually work)
    ingest_bin = tmp_path / "korg-ingest-claude"
    ingest_bin.write_text("#!/bin/sh\nexit 0\n")
    ingest_bin.chmod(ingest_bin.stat().st_mode | stat.S_IXUSR)
    recall_bin = tmp_path / "korg-recall-mcp"
    recall_bin.write_text("#!/bin/sh\nexit 0\n")
    recall_bin.chmod(recall_bin.stat().st_mode | stat.S_IXUSR)
    bridge_bin = tmp_path / "korg-introspect-mcp"
    bridge_bin.write_text("#!/bin/sh\nexit 0\n")
    bridge_bin.chmod(bridge_bin.stat().st_mode | stat.S_IXUSR)

    monkeypatch.setenv("PATH", str(tmp_path))
    return {
        "config": tmp_path / ".claude.json",
        "ledger_dir": tmp_path / ".korg",
        "ledger_file": tmp_path / ".korg" / "events.jsonl",
        "ingest_bin": ingest_bin,
        "recall_bin": recall_bin,
        "bridge_bin": bridge_bin,
        "tmp_path": tmp_path,
    }


def test_bridge_step_registers_discovered_binaries(setup_env, monkeypatch):
    _make_fake_introspect_bin(setup_env["tmp_path"], "thump")
    _make_fake_introspect_bin(setup_env["tmp_path"], "korgex")

    report = run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        install_daemon=False,
        introspect_candidates=("thump", "korgex"),
    )
    assert report.overall_ok
    saved = json.loads(setup_env["config"].read_text())
    servers = saved["mcpServers"]
    # The canonical recall server is registered natively
    assert "korg-recall" in servers
    # Bridges for thump + korgex
    assert "korg-thump" in servers
    assert "korg-korgex" in servers
    # Bridge command points at korg-introspect-mcp
    assert servers["korg-thump"]["command"] == str(setup_env["bridge_bin"])
    # Args carry the binary path
    assert any(a.endswith("/thump") for a in servers["korg-thump"]["args"])


def test_bridge_step_skips_unsupported_binaries(setup_env):
    # Only thump is introspect-aware; korg-no-introspect just exits.
    _make_fake_introspect_bin(setup_env["tmp_path"], "thump")
    bad = setup_env["tmp_path"] / "korg-no-introspect"
    bad.write_text("#!/bin/sh\nexit 1\n")
    bad.chmod(bad.stat().st_mode | stat.S_IXUSR)

    report = run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        install_daemon=False,
        introspect_candidates=("thump", "korg-no-introspect"),
    )
    saved = json.loads(setup_env["config"].read_text())
    assert "korg-thump" in saved["mcpServers"]
    # The non-introspect-aware binary was not registered
    assert "korg-no-introspect" not in saved["mcpServers"]


def test_bridge_step_idempotent(setup_env):
    _make_fake_introspect_bin(setup_env["tmp_path"], "thump")

    r1 = run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        install_daemon=False,
        introspect_candidates=("thump",),
    )
    r2 = run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        install_daemon=False,
        introspect_candidates=("thump",),
    )
    # Second run shouldn't add anything new
    assert r1.bridge_entries["korg-thump"][1] == "added"
    assert r2.bridge_entries["korg-thump"][1] == "unchanged"


def test_bridge_step_carries_allow_env(setup_env):
    _make_fake_introspect_bin(setup_env["tmp_path"], "thump")

    run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        install_daemon=False,
        introspect_candidates=("thump",),
        bridge_allow="all",
    )
    saved = json.loads(setup_env["config"].read_text())
    entry = saved["mcpServers"]["korg-thump"]
    assert entry["env"] == {"KORG_INTROSPECT_MCP_ALLOW": "all"}


def test_bridge_step_disabled_by_flag(setup_env):
    _make_fake_introspect_bin(setup_env["tmp_path"], "thump")

    report = run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        install_daemon=False,
        register_introspect_bridges=False,
        introspect_candidates=("thump",),
    )
    saved = json.loads(setup_env["config"].read_text())
    assert "korg-thump" not in saved["mcpServers"]
    step = next(s for s in report.steps if s.name == "bridges")
    assert step.status == "skip"


def test_bridge_step_dry_run_writes_nothing(setup_env):
    _make_fake_introspect_bin(setup_env["tmp_path"], "thump")

    report = run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        install_daemon=False,
        introspect_candidates=("thump",),
        dry_run=True,
    )
    assert not setup_env["config"].exists()
    step = next(s for s in report.steps if s.name == "bridges")
    assert "would register" in step.detail


def test_bridge_step_skips_when_bridge_bin_missing(tmp_path, monkeypatch):
    """If korg-introspect-mcp isn't on PATH, skip cleanly with a helpful message."""
    # Only the core binaries are present
    ingest_bin = tmp_path / "korg-ingest-claude"
    ingest_bin.write_text("#!/bin/sh\nexit 0\n")
    ingest_bin.chmod(ingest_bin.stat().st_mode | stat.S_IXUSR)
    recall_bin = tmp_path / "korg-recall-mcp"
    recall_bin.write_text("#!/bin/sh\nexit 0\n")
    recall_bin.chmod(recall_bin.stat().st_mode | stat.S_IXUSR)
    # Also drop a fake introspect-aware binary
    _make_fake_introspect_bin(tmp_path, "thump")

    monkeypatch.setenv("PATH", str(tmp_path))

    config = tmp_path / ".claude.json"
    report = run_setup(
        ledger_dir=tmp_path / ".korg",
        ledger_file=tmp_path / ".korg" / "events.jsonl",
        claude_config_path=config,
        install_daemon=False,
        introspect_candidates=("thump",),
    )
    step = next(s for s in report.steps if s.name == "bridges")
    assert step.status == "skip"
    assert "korg-introspect-mcp not on PATH" in step.detail


def test_bridge_step_does_not_double_register_recall(setup_env):
    """If the canonical 'korg-recall' is the same name a bridge would use,
    don't overwrite the native MCP entry with a bridge entry."""
    _make_fake_introspect_bin(setup_env["tmp_path"], "korg-recall-mcp")

    run_setup(
        ledger_dir=setup_env["ledger_dir"],
        ledger_file=setup_env["ledger_file"],
        claude_config_path=setup_env["config"],
        mcp_server_name="korg-recall-mcp",
        install_daemon=False,
        introspect_candidates=("korg-recall-mcp",),
    )
    saved = json.loads(setup_env["config"].read_text())
    # The native registration wins — the command should be korg-recall-mcp itself,
    # NOT korg-introspect-mcp (which would mean the bridge clobbered it).
    entry = saved["mcpServers"]["korg-recall-mcp"]
    assert entry["command"] == str(setup_env["recall_bin"])
