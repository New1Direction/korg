"""Tests for ~/.claude.json idempotent editing."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from korg_setup.claude_config import (
    McpServerSpec,
    ensure_mcp_server_registered,
    get_registered_server,
    load_config,
    remove_mcp_server,
    save_config_atomic,
)


@pytest.fixture
def empty_config_path(tmp_path: Path) -> Path:
    return tmp_path / ".claude.json"


@pytest.fixture
def populated_config_path(tmp_path: Path) -> Path:
    """A config file that already has unrelated keys + an unrelated MCP server."""
    p = tmp_path / ".claude.json"
    p.write_text(
        json.dumps(
            {
                "userID": "user-abc",
                "oauthAccount": {"token": "secret"},
                "projects": {"/x/y": {"foo": 1}},
                "mcpServers": {
                    "other-server": {"command": "other", "args": []},
                },
            },
            indent=2,
        )
    )
    return p


# ── load + atomic save ────────────────────────────────────────────────


def test_load_missing_returns_empty(tmp_path):
    assert load_config(tmp_path / "nope.json") == {}


def test_save_atomic_creates_file(tmp_path):
    p = tmp_path / "out.json"
    backup = save_config_atomic({"hello": 1}, p)
    assert backup is None
    assert json.loads(p.read_text()) == {"hello": 1}


def test_save_atomic_backs_up_existing(tmp_path):
    p = tmp_path / "out.json"
    p.write_text(json.dumps({"old": True}))
    backup = save_config_atomic({"new": True}, p)
    assert backup is not None
    assert backup.exists()
    assert json.loads(backup.read_text()) == {"old": True}
    assert json.loads(p.read_text()) == {"new": True}


def test_save_atomic_leaves_no_tmp_file(tmp_path):
    p = tmp_path / "out.json"
    save_config_atomic({"x": 1}, p)
    assert not list(tmp_path.glob("*.tmp"))


# ── ensure_mcp_server_registered ──────────────────────────────────────


def test_register_into_empty_file(empty_config_path):
    spec = McpServerSpec(name="korg-recall", command="/usr/bin/korg-recall-mcp", args=["--ledger", "/x"])
    status, backup = ensure_mcp_server_registered(spec, empty_config_path)
    assert status == "added"
    assert backup is None  # file didn't exist before
    saved = load_config(empty_config_path)
    assert saved["mcpServers"]["korg-recall"]["command"] == "/usr/bin/korg-recall-mcp"


def test_register_alongside_existing_servers(populated_config_path):
    spec = McpServerSpec(name="korg-recall", command="/cr/korg-recall-mcp", args=[])
    status, backup = ensure_mcp_server_registered(spec, populated_config_path)
    assert status == "added"
    assert backup is not None and backup.exists()
    saved = load_config(populated_config_path)
    # Original unrelated keys untouched
    assert saved["userID"] == "user-abc"
    assert saved["oauthAccount"] == {"token": "secret"}
    assert saved["projects"] == {"/x/y": {"foo": 1}}
    # Original MCP server still present
    assert saved["mcpServers"]["other-server"] == {"command": "other", "args": []}
    # Plus the new one
    assert saved["mcpServers"]["korg-recall"]["command"] == "/cr/korg-recall-mcp"


def test_register_same_spec_twice_is_idempotent(empty_config_path):
    spec = McpServerSpec(name="korg-recall", command="/x/korg-recall-mcp", args=["--ledger", "/y"])
    ensure_mcp_server_registered(spec, empty_config_path)
    status, backup = ensure_mcp_server_registered(spec, empty_config_path)
    assert status == "unchanged"
    assert backup is None  # no write happened


def test_register_different_spec_updates(empty_config_path):
    old = McpServerSpec(name="korg-recall", command="/old", args=["--ledger", "/a"])
    new = McpServerSpec(name="korg-recall", command="/new", args=["--ledger", "/b"])
    ensure_mcp_server_registered(old, empty_config_path)
    status, backup = ensure_mcp_server_registered(new, empty_config_path)
    assert status == "updated"
    assert backup is not None and backup.exists()
    saved = load_config(empty_config_path)
    assert saved["mcpServers"]["korg-recall"]["command"] == "/new"


def test_register_with_env_var(empty_config_path):
    spec = McpServerSpec(
        name="korg-recall",
        command="/x",
        args=[],
        env={"KORG_DEBUG": "1"},
    )
    ensure_mcp_server_registered(spec, empty_config_path)
    saved = load_config(empty_config_path)
    assert saved["mcpServers"]["korg-recall"]["env"] == {"KORG_DEBUG": "1"}


# ── remove_mcp_server ─────────────────────────────────────────────────


def test_remove_absent_is_noop(empty_config_path):
    empty_config_path.write_text(json.dumps({"userID": "x"}))
    status, backup = remove_mcp_server("nope", empty_config_path)
    assert status == "absent"
    assert backup is None
    # File untouched
    assert load_config(empty_config_path) == {"userID": "x"}


def test_remove_existing(populated_config_path):
    status, backup = remove_mcp_server("other-server", populated_config_path)
    assert status == "removed"
    assert backup is not None and backup.exists()
    saved = load_config(populated_config_path)
    assert saved["mcpServers"] == {}
    # Other keys preserved
    assert saved["userID"] == "user-abc"


# ── get_registered_server ─────────────────────────────────────────────


def test_get_registered_server_present(populated_config_path):
    entry = get_registered_server("other-server", populated_config_path)
    assert entry == {"command": "other", "args": []}


def test_get_registered_server_absent(populated_config_path):
    assert get_registered_server("never", populated_config_path) is None


def test_get_registered_server_missing_file(tmp_path):
    assert get_registered_server("anything", tmp_path / "nope.json") is None
