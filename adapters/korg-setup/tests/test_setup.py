"""Tests for the setup orchestrator.

We mock `shutil.which` to control whether the binaries appear to be on
PATH, and mock `install_service` so the launchd path doesn't try to
touch the user's real launchd.
"""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from korg_setup.setup import (
    DEFAULT_MCP_SERVER_NAME,
    format_report,
    run_setup,
)


@pytest.fixture
def tmp_config(tmp_path: Path) -> Path:
    return tmp_path / ".claude.json"


@pytest.fixture
def tmp_ledger_dir(tmp_path: Path) -> Path:
    return tmp_path / ".korg"


@pytest.fixture
def tmp_ledger_file(tmp_ledger_dir: Path) -> Path:
    return tmp_ledger_dir / "claude-events.jsonl"


# ── run_setup ──────────────────────────────────────────────────────────


def test_setup_fails_when_binaries_missing(tmp_config, tmp_ledger_dir, tmp_ledger_file):
    with patch("korg_setup.setup.shutil.which", return_value=None):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
    assert report.overall_ok is False
    fails = [s for s in report.steps if s.status == "fail"]
    assert any("not on PATH" in s.detail for s in fails)


def _which_stub(name):
    """Pretend both binaries are at fixed paths."""
    return {
        "korg-ingest-claude": "/fake/korg-ingest-claude",
        "korg-recall-mcp": "/fake/korg-recall-mcp",
    }.get(name)


def test_setup_registers_mcp_when_binaries_present(
    tmp_config, tmp_ledger_dir, tmp_ledger_file
):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
    assert report.overall_ok
    assert tmp_config.exists()
    saved = json.loads(tmp_config.read_text())
    entry = saved["mcpServers"][DEFAULT_MCP_SERVER_NAME]
    assert entry["command"] == "/fake/korg-recall-mcp"
    assert str(tmp_ledger_file) in entry["args"]


def test_setup_dry_run_writes_nothing(tmp_config, tmp_ledger_dir, tmp_ledger_file):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
            dry_run=True,
        )
    assert report.overall_ok
    assert not tmp_config.exists()
    assert not tmp_ledger_dir.exists()


def test_setup_is_idempotent(tmp_config, tmp_ledger_dir, tmp_ledger_file):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        r1 = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
        r2 = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
    assert r1.config_status == "added"
    assert r2.config_status == "unchanged"


def test_setup_creates_ledger_dir(tmp_config, tmp_ledger_dir, tmp_ledger_file):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
    assert tmp_ledger_dir.exists()
    assert tmp_ledger_dir.is_dir()


def test_setup_preserves_existing_claude_config_keys(
    tmp_config, tmp_ledger_dir, tmp_ledger_file
):
    # Pre-populate the config with unrelated keys
    tmp_config.write_text(json.dumps({"userID": "u1", "oauthAccount": {"x": 1}}))
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
    saved = json.loads(tmp_config.read_text())
    assert saved["userID"] == "u1"
    assert saved["oauthAccount"] == {"x": 1}
    assert "mcpServers" in saved


@patch("korg_setup.setup.is_macos", return_value=True)
@patch("korg_setup.setup.install_service")
def test_setup_installs_launchd_on_macos(
    mock_install, mock_is_macos, tmp_config, tmp_ledger_dir, tmp_ledger_file
):
    mock_install.return_value = (
        "created",
        MagicMock(returncode=0, stdout="", stderr=""),
    )
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=True,
        )
    assert mock_install.called
    args, kwargs = mock_install.call_args
    extra_args = kwargs.get("extra_args") or args[0]
    assert "--tail" in extra_args
    assert any("--out" == a or a == "--out" for a in extra_args)
    daemon_step = next(s for s in report.steps if s.name == "daemon")
    assert daemon_step.status == "ok"


@patch("korg_setup.setup.is_macos", return_value=False)
def test_setup_warns_on_non_macos_daemon(
    mock_is_macos, tmp_config, tmp_ledger_dir, tmp_ledger_file
):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=True,
        )
    daemon_step = next(s for s in report.steps if s.name == "daemon")
    assert daemon_step.status == "warn"
    assert "Auto-restart service install not yet supported" in daemon_step.detail


@patch("korg_setup.setup.is_macos", return_value=True)
@patch(
    "korg_setup.setup.install_service",
    return_value=("created", MagicMock(returncode=1, stdout="", stderr="boom")),
)
def test_setup_records_launchctl_failure_as_warn(
    mock_install, mock_is_macos, tmp_config, tmp_ledger_dir, tmp_ledger_file
):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=True,
        )
    daemon_step = next(s for s in report.steps if s.name == "daemon")
    assert daemon_step.status == "warn"
    assert "boom" in daemon_step.detail


def test_no_daemon_skips_launchd(tmp_config, tmp_ledger_dir, tmp_ledger_file):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
    daemon_step = next(s for s in report.steps if s.name == "daemon")
    assert daemon_step.status == "skip"


def _which_with_hook(name):
    return {
        "korg-ingest-claude": "/fake/korg-ingest-claude",
        "korg-recall-mcp": "/fake/korg-recall-mcp",
        "korg-hook": "/fake/korg-hook",
    }.get(name)


def test_setup_registers_hook_when_present(tmp_config, tmp_ledger_dir, tmp_ledger_file, tmp_path):
    settings = tmp_path / ".claude" / "settings.json"
    with patch("korg_setup.setup.shutil.which", side_effect=_which_with_hook):
        report = run_setup(
            ledger_dir=tmp_ledger_dir, ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config, claude_settings_path=settings,
            install_daemon=False,
        )
    assert report.overall_ok
    hooks_step = next(s for s in report.steps if s.name == "hooks")
    assert hooks_step.status == "ok"
    saved = json.loads(settings.read_text())
    assert saved["hooks"]["PostToolUse"][0]["hooks"][0]["command"] == "/fake/korg-hook"


def test_setup_warns_when_hook_binary_absent(tmp_config, tmp_ledger_dir, tmp_ledger_file, tmp_path):
    settings = tmp_path / ".claude" / "settings.json"
    # _which_stub resolves recall + ingest but NOT korg-hook
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir, ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config, claude_settings_path=settings,
            install_daemon=False,
        )
    assert report.overall_ok  # missing hook is a warning, not a failure
    hooks_step = next(s for s in report.steps if s.name == "hooks")
    assert hooks_step.status == "warn"
    assert not settings.exists()


def test_setup_does_not_require_ingest_binary(tmp_config, tmp_ledger_dir, tmp_ledger_file, tmp_path):
    # only recall + hook present; the daemon binary is gone — setup must still succeed
    def which(name):
        return {"korg-recall-mcp": "/fake/korg-recall-mcp", "korg-hook": "/fake/korg-hook"}.get(name)
    with patch("korg_setup.setup.shutil.which", side_effect=which):
        report = run_setup(
            ledger_dir=tmp_ledger_dir, ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config, claude_settings_path=tmp_path / ".claude" / "settings.json",
            install_daemon=False,
        )
    assert report.overall_ok


# ── format_report ──────────────────────────────────────────────────────


def test_format_report_includes_step_names(tmp_config, tmp_ledger_dir, tmp_ledger_file):
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir,
            ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config,
            install_daemon=False,
        )
    out = format_report(report)
    assert "binaries" in out
    assert "claude_config" in out
    assert "Setup complete" in out
