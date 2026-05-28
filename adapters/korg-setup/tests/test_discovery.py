"""Tests for the auto-discovery probe used by korg-setup."""

from __future__ import annotations

import json
import stat
import sys
import textwrap
from pathlib import Path

import pytest

from korg_setup.discovery import (
    DEFAULT_CANDIDATES,
    SUPPORTED_SCHEMA,
    IntrospectableBinary,
    discover_all,
    probe,
)


def _make_introspect_bin(
    tmp_path: Path,
    name: str,
    schema: str = SUPPORTED_SCHEMA,
    version: str = "0.0.1",
    callables: list | None = None,
    exit_code: int = 0,
    stdout_override: str | None = None,
) -> Path:
    """Build a fake binary at tmp_path/<name> that responds to --introspect."""
    doc = {
        "schema": schema,
        "binary": name,
        "version": version,
        "callables_declared": True,
        "callables": callables or [
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
    if stdout_override is None:
        doc_path.write_text(json.dumps(doc))
        body = f"with open({json.dumps(str(doc_path))}) as f: sys.stdout.write(f.read())"
    else:
        body = f"sys.stdout.write({json.dumps(stdout_override)})"

    script = textwrap.dedent(f"""\
        #!{sys.executable}
        import sys
        if "--introspect" in sys.argv[1:]:
            {body}
            sys.exit({exit_code})
        sys.exit(0)
    """)
    p = tmp_path / name
    p.write_text(script)
    p.chmod(p.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return p


# ── probe ─────────────────────────────────────────────────────────────


def test_probe_missing_binary_returns_none(monkeypatch):
    """Bare name not on PATH → None (no exception)."""
    monkeypatch.setenv("PATH", "/nonexistent")
    assert probe("definitely-not-there") is None


def test_probe_valid_binary_returns_introspectable(tmp_path, monkeypatch):
    bin_path = _make_introspect_bin(tmp_path, "fake-tool")
    monkeypatch.setenv("PATH", str(tmp_path))
    result = probe("fake-tool")
    assert result is not None
    assert isinstance(result, IntrospectableBinary)
    assert result.binary_name == "fake-tool"
    assert result.binary_path == bin_path
    assert result.callable_count == 1


def test_probe_wrong_schema_returns_none(tmp_path, monkeypatch):
    _make_introspect_bin(tmp_path, "wrong-schema-tool", schema="other:schema@v1")
    monkeypatch.setenv("PATH", str(tmp_path))
    assert probe("wrong-schema-tool") is None


def test_probe_non_json_stdout_returns_none(tmp_path, monkeypatch):
    _make_introspect_bin(tmp_path, "garbage-tool", stdout_override="not json at all")
    monkeypatch.setenv("PATH", str(tmp_path))
    assert probe("garbage-tool") is None


def test_probe_empty_stdout_returns_none(tmp_path, monkeypatch):
    _make_introspect_bin(tmp_path, "empty-tool", stdout_override="", exit_code=1)
    monkeypatch.setenv("PATH", str(tmp_path))
    assert probe("empty-tool") is None


def test_probe_missing_callables_returns_none(tmp_path, monkeypatch):
    name = "no-callables-tool"
    doc = {"schema": SUPPORTED_SCHEMA, "binary": name, "version": "0.0.1"}
    bad_json_doc = json.dumps(doc)  # no `callables` key
    _make_introspect_bin(tmp_path, name, stdout_override=bad_json_doc)
    monkeypatch.setenv("PATH", str(tmp_path))
    assert probe(name) is None


def test_probe_non_zero_exit_with_valid_stdout(tmp_path, monkeypatch):
    """Some clap-based binaries print the document but exit nonzero
    (e.g. when --introspect is parsed before missing-required-arg checks).
    The probe should still accept these."""
    _make_introspect_bin(tmp_path, "nonzero-but-valid", exit_code=2)
    monkeypatch.setenv("PATH", str(tmp_path))
    result = probe("nonzero-but-valid")
    assert result is not None


# ── discover_all ──────────────────────────────────────────────────────


def test_discover_all_finds_every_present_candidate(tmp_path, monkeypatch):
    _make_introspect_bin(tmp_path, "thump")
    _make_introspect_bin(tmp_path, "korg")
    _make_introspect_bin(tmp_path, "korgex")
    monkeypatch.setenv("PATH", str(tmp_path))
    discovered = discover_all(candidates=("thump", "korg", "korgex"))
    names = {b.binary_name for b in discovered}
    assert names == {"thump", "korg", "korgex"}


def test_discover_all_skips_missing(tmp_path, monkeypatch):
    _make_introspect_bin(tmp_path, "thump")
    monkeypatch.setenv("PATH", str(tmp_path))
    discovered = discover_all(candidates=("thump", "not-installed-1", "not-installed-2"))
    assert {b.binary_name for b in discovered} == {"thump"}


def test_discover_all_empty_path_returns_empty(monkeypatch):
    monkeypatch.setenv("PATH", "/nonexistent")
    assert discover_all() == []


# ── mcp_server_name convention ────────────────────────────────────────


def test_mcp_server_name_for_korg_prefixed_binary_keeps_name(tmp_path):
    """korg-recall-mcp stays as 'korg-recall-mcp' — no double prefix."""
    b = IntrospectableBinary(
        binary_name="korg-recall-mcp",
        binary_path=tmp_path / "korg-recall-mcp",
        version="0.1.0",
        callable_count=1,
    )
    assert b.mcp_server_name == "korg-recall-mcp"


def test_mcp_server_name_for_bare_binary_gets_korg_prefix(tmp_path):
    """thump becomes korg-thump for namespacing in ~/.claude.json."""
    b = IntrospectableBinary(
        binary_name="thump",
        binary_path=tmp_path / "thump",
        version="0.2.0",
        callable_count=9,
    )
    assert b.mcp_server_name == "korg-thump"


# ── DEFAULT_CANDIDATES ────────────────────────────────────────────────


def test_default_candidates_includes_all_ecosystem_binaries():
    assert "thump" in DEFAULT_CANDIDATES
    assert "korg" in DEFAULT_CANDIDATES
    assert "korgex" in DEFAULT_CANDIDATES
    assert "korg-recall-mcp" in DEFAULT_CANDIDATES
