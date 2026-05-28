"""Tests for the discovery layer — running --introspect + validating the doc."""

from __future__ import annotations

import json
import os
import stat
import sys
import textwrap
from pathlib import Path

import pytest

from korg_introspect_mcp.discovery import (
    DiscoveryError,
    discover,
    resolve_binary,
    run_introspect,
    validate_document,
)


def _make_script(tmp_path: Path, body: str, name: str = "fake-bin") -> Path:
    p = tmp_path / name
    p.write_text(f"#!{sys.executable}\n{body}")
    p.chmod(p.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return p


# ── resolve_binary ────────────────────────────────────────────────────


def test_resolve_absolute_path(fixture_binary):
    p = resolve_binary(str(fixture_binary))
    assert p == fixture_binary


def test_resolve_missing_absolute_path(tmp_path):
    with pytest.raises(DiscoveryError, match="not found at absolute path"):
        resolve_binary(str(tmp_path / "does-not-exist"))


def test_resolve_missing_bare_name_raises():
    with pytest.raises(DiscoveryError, match="not found on PATH"):
        resolve_binary("definitely-not-a-real-binary-9876")


# ── run_introspect ────────────────────────────────────────────────────


def test_run_introspect_returns_dict(fixture_binary):
    doc = run_introspect(fixture_binary)
    assert isinstance(doc, dict)
    assert doc["schema"] == "korg:introspect@v1"
    assert doc["binary"] == "fixture-bin"


def test_run_introspect_raises_on_invalid_json(tmp_path):
    script = _make_script(tmp_path, "import sys\nprint('not json at all')\nsys.exit(0)")
    with pytest.raises(DiscoveryError, match="did not return valid JSON"):
        run_introspect(script)


def test_run_introspect_raises_on_timeout(tmp_path):
    script = _make_script(
        tmp_path,
        "import time, sys\ntime.sleep(5)\nsys.exit(0)",
    )
    with pytest.raises(DiscoveryError, match="timed out"):
        run_introspect(script, timeout_s=0.5)


# ── validate_document ─────────────────────────────────────────────────


def test_validate_rejects_wrong_schema(tmp_path):
    doc = {"schema": "some:other@v1", "callables": []}
    with pytest.raises(DiscoveryError, match="unsupported introspect schema"):
        validate_document(doc, tmp_path / "x")


def test_validate_rejects_missing_callables(tmp_path):
    doc = {"schema": "korg:introspect@v1"}
    with pytest.raises(DiscoveryError, match="no 'callables' array"):
        validate_document(doc, tmp_path / "x")


def test_validate_rejects_duplicate_command_ids(tmp_path):
    doc = {
        "schema": "korg:introspect@v1",
        "callables": [
            {"command_id": "x.a", "name": "a", "input_schema": {}, "capabilities": {}},
            {"command_id": "x.a", "name": "a", "input_schema": {}, "capabilities": {}},
        ],
    }
    with pytest.raises(DiscoveryError, match="duplicate command_id"):
        validate_document(doc, tmp_path / "x")


def test_validate_rejects_missing_required_fields(tmp_path):
    doc = {
        "schema": "korg:introspect@v1",
        "callables": [{"command_id": "x.a", "name": "a"}],  # missing input_schema, capabilities
    }
    with pytest.raises(DiscoveryError, match="missing required field"):
        validate_document(doc, tmp_path / "x")


def test_validate_accepts_minimal_valid_doc(tmp_path):
    doc = {
        "schema": "korg:introspect@v1",
        "binary": "x",
        "version": "1.2.3",
        "callables": [
            {
                "command_id": "x.a",
                "name": "a",
                "description": "",
                "input_schema": {"type": "object"},
                "capabilities": {"side_effects": "none"},
            }
        ],
    }
    result = validate_document(doc, tmp_path / "x")
    assert result.binary_name == "x"
    assert result.version == "1.2.3"
    assert len(result.callables) == 1


# ── discover (end-to-end) ─────────────────────────────────────────────


def test_discover_end_to_end(fixture_binary):
    d = discover(str(fixture_binary))
    assert d.binary_name == "fixture-bin"
    assert d.callables_declared is True
    assert len(d.callables) == 4
    command_ids = {c.command_id for c in d.callables}
    assert command_ids == {
        "fixture-bin.echo",
        "fixture-bin.write",
        "fixture-bin.shell",
        "fixture-bin.fail",
    }


def test_discover_by_command_id_lookup(fixture_binary):
    d = discover(str(fixture_binary))
    echo = d.by_command_id("fixture-bin.echo")
    assert echo is not None
    assert echo.name == "echo"
    assert d.by_command_id("nonexistent") is None
