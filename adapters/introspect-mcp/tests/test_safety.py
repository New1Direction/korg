"""Tests for the safety / side-effects gating policy."""

from __future__ import annotations

import pytest

from korg_introspect_mcp.safety import (
    ALL_EFFECTS,
    ALWAYS_ALLOWED,
    Policy,
)


# ── Defaults ──────────────────────────────────────────────────────────


def test_default_policy_allows_none_and_fs_read():
    p = Policy.from_env(env={})
    assert p.allows("none")
    assert p.allows("fs_read")


def test_default_policy_denies_writes_and_network():
    p = Policy.from_env(env={})
    assert not p.allows("fs_write")
    assert not p.allows("network")
    assert not p.allows("ledger_write")


def test_read_only_factory():
    p = Policy.read_only()
    assert p.allowed == ALWAYS_ALLOWED


def test_all_factory():
    p = Policy.all()
    assert p.allowed == ALL_EFFECTS
    for effect in ALL_EFFECTS:
        assert p.allows(effect)


# ── Env-var parsing ───────────────────────────────────────────────────


def test_env_var_single_value():
    p = Policy.from_env(env={"KORG_INTROSPECT_MCP_ALLOW": "fs_write"})
    assert p.allows("fs_write")
    assert p.allows("fs_read")  # still allowed from defaults
    assert not p.allows("network")


def test_env_var_comma_separated():
    p = Policy.from_env(env={"KORG_INTROSPECT_MCP_ALLOW": "fs_write,network,ledger_write"})
    assert p.allows("fs_write")
    assert p.allows("network")
    assert p.allows("ledger_write")


def test_env_var_whitespace_tolerant():
    p = Policy.from_env(env={"KORG_INTROSPECT_MCP_ALLOW": "  fs_write , network  "})
    assert p.allows("fs_write")
    assert p.allows("network")


def test_env_var_all_keyword():
    p = Policy.from_env(env={"KORG_INTROSPECT_MCP_ALLOW": "all"})
    assert p.allowed == ALL_EFFECTS


def test_env_var_star_keyword():
    p = Policy.from_env(env={"KORG_INTROSPECT_MCP_ALLOW": "*"})
    assert p.allowed == ALL_EFFECTS


def test_env_var_case_insensitive():
    p = Policy.from_env(env={"KORG_INTROSPECT_MCP_ALLOW": "ALL"})
    assert p.allowed == ALL_EFFECTS


def test_empty_env_var_keeps_defaults():
    p = Policy.from_env(env={"KORG_INTROSPECT_MCP_ALLOW": ""})
    assert p.allowed == ALWAYS_ALLOWED


def test_missing_env_var_keeps_defaults():
    p = Policy.from_env(env={})
    assert p.allowed == ALWAYS_ALLOWED


# ── explain_denial ─────────────────────────────────────────────────────


def test_explain_denial_mentions_env_var_and_effect():
    p = Policy.read_only()
    msg = p.explain_denial("fs_write")
    assert "fs_write" in msg
    assert "KORG_INTROSPECT_MCP_ALLOW" in msg


def test_policy_is_frozen():
    p = Policy.read_only()
    with pytest.raises(Exception):
        p.allowed = frozenset({"fs_write"})  # type: ignore[misc]
