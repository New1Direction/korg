"""Tests for the status report."""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import patch

import pytest

from korg_setup.status import format_status, gather_status


def _which_stub(name):
    return {
        "korg-ingest-claude": "/fake/korg-ingest-claude",
        "korg-recall-mcp": "/fake/korg-recall-mcp",
    }.get(name)


@pytest.fixture
def empty_config(tmp_path) -> Path:
    return tmp_path / ".claude.json"


def test_status_when_nothing_installed(tmp_path, empty_config):
    ledger = tmp_path / ".korg" / "claude-events.jsonl"
    with patch("korg_setup.status.shutil.which", return_value=None), \
         patch("korg_setup.status.is_macos", return_value=False):
        report = gather_status(ledger_file=ledger, claude_config_path=empty_config)
    assert report.binaries["korg-ingest-claude"] is None
    assert report.binaries["korg-recall-mcp"] is None
    assert report.mcp_registered is False
    assert report.ledger_lines == 0


def test_status_with_binaries_and_mcp_registered(tmp_path, empty_config):
    empty_config.write_text(
        json.dumps(
            {
                "mcpServers": {
                    "korg-recall": {"command": "/fake/korg-recall-mcp", "args": []}
                }
            }
        )
    )
    ledger = tmp_path / "events.jsonl"
    ledger.write_text("{}\n{}\n{}\n")  # 3 lines
    with patch("korg_setup.status.shutil.which", side_effect=_which_stub), \
         patch("korg_setup.status.is_macos", return_value=False):
        report = gather_status(ledger_file=ledger, claude_config_path=empty_config)
    assert report.binaries["korg-ingest-claude"] == "/fake/korg-ingest-claude"
    assert report.mcp_registered is True
    assert report.mcp_command == "/fake/korg-recall-mcp"
    assert report.ledger_lines == 3
    assert report.ledger_size_bytes > 0


@patch("korg_setup.status.is_macos", return_value=True)
@patch("korg_setup.status.is_loaded", return_value=True)
def test_status_detects_launchd_loaded_on_macos(
    mock_is_loaded, mock_is_macos, tmp_path, empty_config
):
    ledger = tmp_path / "events.jsonl"
    with patch("korg_setup.status.shutil.which", side_effect=_which_stub):
        report = gather_status(ledger_file=ledger, claude_config_path=empty_config)
    assert report.platform_supports_daemon is True
    assert report.launchd_loaded is True


def test_format_status_renders_each_section(tmp_path, empty_config):
    empty_config.write_text(
        json.dumps({"mcpServers": {"korg-recall": {"command": "/x", "args": []}}})
    )
    ledger = tmp_path / "events.jsonl"
    ledger.write_text('{"seq":1}\n')
    with patch("korg_setup.status.shutil.which", side_effect=_which_stub), \
         patch("korg_setup.status.is_macos", return_value=True), \
         patch("korg_setup.status.is_loaded", return_value=False):
        report = gather_status(ledger_file=ledger, claude_config_path=empty_config)
    out = format_status(report)
    assert "Binaries:" in out
    assert "Claude Code MCP registration:" in out
    assert "Ledger:" in out
    assert "Tail capture service:" in out
    # Plist not loaded → instruction to fix it
    assert "is RUNNING" not in out
