"""Tests for the Capabilities/introspect document.

The most important invariant: the MCP tools/list schema and the
--introspect schema for the same callable MUST agree on field names
and types. They're projections of one source of truth; if they drift,
agents that read one and call the other get broken.
"""

from __future__ import annotations

import json
from subprocess import PIPE, Popen
from unittest.mock import patch

import pytest

from korg_recall_mcp.introspect import (
    EXIT_CODES,
    INTROSPECT_SCHEMA_ID,
    Callable,
    Capabilities,
    build_introspect_document,
    get_callables,
)
from korg_recall_mcp.server import _recall_tool_schema


# ── Capabilities ──────────────────────────────────────────────────────


def test_capabilities_defaults_are_safe():
    """Defaults should be the most conservative (zero-effect) values so
    agents don't get surprised by undeclared side effects."""
    c = Capabilities()
    assert c.side_effects == "none"
    assert c.requires_project is False
    assert c.long_running is False
    assert c.stateful is False
    assert c.reads_stdin is False
    assert c.supports_output_path is False


def test_capabilities_to_dict_round_trip():
    c = Capabilities(
        output_mode="stream",
        side_effects="fs_read",
        long_running=True,
    )
    d = c.to_dict()
    assert d["output_mode"] == "stream"
    assert d["side_effects"] == "fs_read"
    assert d["long_running"] is True
    # All declared fields must be present
    for key in (
        "output_mode",
        "side_effects",
        "requires_project",
        "long_running",
        "stateful",
        "reads_stdin",
        "supports_output_path",
    ):
        assert key in d


def test_capabilities_is_frozen():
    c = Capabilities()
    with pytest.raises(Exception):
        c.output_mode = "session"  # type: ignore[misc]


# ── Callable projections ──────────────────────────────────────────────


def test_callable_to_mcp_tool_uses_mcp_field_names():
    """MCP uses camelCase `inputSchema`; the introspect projection uses
    snake_case `input_schema`. Both must work from the same source."""
    [recall] = get_callables()
    mcp_tool = recall.to_mcp_tool()
    assert set(mcp_tool.keys()) == {"name", "description", "inputSchema"}
    assert mcp_tool["name"] == "recall"
    assert mcp_tool["inputSchema"]["type"] == "object"
    assert "query" in mcp_tool["inputSchema"]["properties"]


def test_callable_to_introspect_entry_carries_capabilities_and_id():
    [recall] = get_callables()
    entry = recall.to_introspect_entry()
    assert entry["command_id"] == "korg-recall-mcp.recall"
    assert entry["name"] == "recall"
    assert "input_schema" in entry  # snake_case for the introspect doc
    assert "inputSchema" not in entry
    assert entry["surfaces"] == ["cli", "mcp"]
    caps = entry["capabilities"]
    assert caps["side_effects"] == "fs_read"
    assert caps["stateful"] is False


def test_callable_id_is_stable_format():
    """Agents may pin to command_id; format must be <binary>.<name>."""
    for c in get_callables():
        assert c.id.startswith("korg-recall-mcp.")
        # No spaces, dots OK, lowercase
        assert " " not in c.id
        assert c.id.lower() == c.id


# ── Consistency: MCP vs CLI introspect (the critical invariant) ───────


def test_mcp_tool_schema_matches_introspect_for_recall():
    """The schema that goes out via MCP tools/list must be byte-identical
    to the schema that goes out via --introspect for the same callable.
    If this test fails, agents that read --introspect and call MCP (or
    vice-versa) will see drift."""
    mcp_schema = _recall_tool_schema()["inputSchema"]
    [recall] = [c for c in get_callables() if c.name == "recall"]
    introspect_schema = recall.to_introspect_entry()["input_schema"]
    assert mcp_schema == introspect_schema


def test_mcp_tools_list_name_matches_introspect_name():
    mcp_tool = _recall_tool_schema()
    [recall] = [c for c in get_callables() if c.name == "recall"]
    assert mcp_tool["name"] == recall.name
    assert mcp_tool["description"] == recall.description


# ── build_introspect_document ─────────────────────────────────────────


def test_document_has_schema_tag():
    doc = build_introspect_document("0.1.0")
    assert doc["schema"] == INTROSPECT_SCHEMA_ID
    assert doc["schema"] == "korg:introspect@v1"


def test_document_includes_version_and_binary():
    doc = build_introspect_document("1.2.3")
    assert doc["binary"] == "korg-recall-mcp"
    assert doc["version"] == "1.2.3"


def test_document_declares_truthfulness_flag():
    """Foundry's pattern: `capabilities_declared: true` is the agent's
    signal that this package actually documents its capabilities
    (rather than emitting an empty document)."""
    doc = build_introspect_document("0.1.0")
    assert doc.get("callables_declared") is True


def test_document_includes_exit_codes_table():
    doc = build_introspect_document("0.1.0")
    assert isinstance(doc["exit_codes"], dict)
    # Wire format: keys are strings (JSON has no int keys)
    assert doc["exit_codes"]["0"] == "success"
    assert "error.generic" in doc["exit_codes"].values()
    assert all(isinstance(k, str) for k in doc["exit_codes"].keys())


def test_exit_codes_python_table_uses_int_keys():
    """In-Python the EXIT_CODES table is int-keyed; on the wire it's
    stringified. This decouples Python code paths from the JSON
    serialization quirk."""
    assert all(isinstance(k, int) for k in EXIT_CODES.keys())


def test_document_is_json_serializable():
    doc = build_introspect_document("0.1.0")
    blob = json.dumps(doc, indent=2)
    # And round-trippable
    assert json.loads(blob) == doc


def test_document_callables_section_matches_registry():
    doc = build_introspect_document("0.1.0")
    expected = [c.to_introspect_entry() for c in get_callables()]
    assert doc["callables"] == expected


# ── EXIT_CODES table ──────────────────────────────────────────────────


def test_exit_codes_namespaced_and_lowercase():
    for code, name in EXIT_CODES.items():
        assert isinstance(code, int)
        assert isinstance(name, str)
        assert name == name.lower()
        # Must be `.`-namespaced (e.g. "error.network") or "success"
        assert name == "success" or "." in name


def test_exit_code_zero_is_success():
    assert EXIT_CODES[0] == "success"
