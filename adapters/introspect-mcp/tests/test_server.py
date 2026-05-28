"""End-to-end MCP server tests against the fixture binary."""

from __future__ import annotations

import io
import json
from pathlib import Path

import pytest

from korg_introspect_mcp.discovery import discover
from korg_introspect_mcp.safety import Policy
from korg_introspect_mcp.server import PROTOCOL_VERSION, Server, serve_stdio


@pytest.fixture
def server(fixture_binary):
    return Server(discovery=discover(str(fixture_binary)), policy=Policy.all())


@pytest.fixture
def restricted_server(fixture_binary):
    """Server with the default read-only policy — fs_write will be denied."""
    return Server(discovery=discover(str(fixture_binary)), policy=Policy.read_only())


# ── Protocol ──────────────────────────────────────────────────────────


def test_initialize_handshake(server):
    msg = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}
    resp = server.handle(msg)
    assert resp["result"]["protocolVersion"] == PROTOCOL_VERSION
    assert "fixture-bin" in resp["result"]["serverInfo"]["name"]


def test_initialized_notification(server):
    msg = {"jsonrpc": "2.0", "method": "notifications/initialized"}
    assert server.handle(msg) is None


# ── tools/list ────────────────────────────────────────────────────────


def test_tools_list_exposes_all_callables(server):
    msg = {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}
    resp = server.handle(msg)
    tools = resp["result"]["tools"]
    names = {t["name"] for t in tools}
    assert names == {
        "fixture-bin.echo",
        "fixture-bin.write",
        "fixture-bin.shell",
        "fixture-bin.fail",
    }


def test_tools_list_tags_capabilities_in_description(server):
    msg = {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}
    resp = server.handle(msg)
    tools = resp["result"]["tools"]
    write_tool = [t for t in tools if t["name"] == "fixture-bin.write"][0]
    assert "side_effects: fs_write" in write_tool["description"]


def test_tools_list_carries_input_schema_verbatim(server):
    """The input_schema must be byte-identical to what --introspect emitted —
    that's the whole point of the bridge."""
    msg = {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}
    resp = server.handle(msg)
    tools = resp["result"]["tools"]
    echo_tool = [t for t in tools if t["name"] == "fixture-bin.echo"][0]
    assert echo_tool["inputSchema"]["required"] == ["message"]
    assert "tags" in echo_tool["inputSchema"]["properties"]


# ── tools/call ────────────────────────────────────────────────────────


def test_tools_call_echo_returns_argv(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "fixture-bin.echo",
            "arguments": {"message": "hello world"},
        },
    }
    resp = server.handle(msg)
    assert resp["result"]["isError"] is False
    text = resp["result"]["content"][0]["text"]
    parsed = json.loads(text)
    assert "argv" in parsed
    assert "hello world" in parsed["argv"]


def test_tools_call_unknown_tool_returns_error(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {"name": "fixture-bin.not-real", "arguments": {}},
    }
    resp = server.handle(msg)
    assert "error" in resp
    assert resp["error"]["code"] == -32602


def test_tools_call_fail_returns_error_content(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {"name": "fixture-bin.fail", "arguments": {}},
    }
    resp = server.handle(msg)
    assert resp["result"]["isError"] is True
    text = resp["result"]["content"][0]["text"]
    assert "exit_code" in text


# ── Safety gating ─────────────────────────────────────────────────────


def test_restricted_policy_denies_fs_write(restricted_server):
    msg = {
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {"name": "fixture-bin.write", "arguments": {"path": "/tmp/x"}},
    }
    resp = restricted_server.handle(msg)
    assert resp["result"]["isError"] is True
    text = resp["result"]["content"][0]["text"]
    assert "fs_write" in text
    assert "KORG_INTROSPECT_MCP_ALLOW" in text


def test_restricted_policy_allows_fs_read_and_none(restricted_server):
    # echo has side_effects=none → should pass policy check
    msg = {
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {"name": "fixture-bin.echo", "arguments": {"message": "ok"}},
    }
    resp = restricted_server.handle(msg)
    # Either runs successfully or fails for non-policy reasons
    # The denial message specifically mentions KORG_INTROSPECT_MCP_ALLOW
    text = resp["result"]["content"][0]["text"]
    assert "KORG_INTROSPECT_MCP_ALLOW" not in text


def test_session_mode_returns_unsupported_even_with_all_policy(server):
    """Even with full permissions, session mode is unsupported in v1."""
    msg = {
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/call",
        "params": {"name": "fixture-bin.shell", "arguments": {}},
    }
    resp = server.handle(msg)
    text = resp["result"]["content"][0]["text"]
    assert resp["result"]["isError"] is True
    assert "session" in text.lower()


# ── Unknown methods + notifications ───────────────────────────────────


def test_unknown_method(server):
    msg = {"jsonrpc": "2.0", "id": 9, "method": "wat"}
    resp = server.handle(msg)
    assert resp["error"]["code"] == -32601


def test_unknown_notification_returns_none(server):
    msg = {"jsonrpc": "2.0", "method": "wat-notification"}
    assert server.handle(msg) is None


def test_ping(server):
    msg = {"jsonrpc": "2.0", "id": 10, "method": "ping"}
    resp = server.handle(msg)
    assert resp["result"] == {}


# ── serve_stdio loop ──────────────────────────────────────────────────


def test_serve_stdio_full_handshake_and_call(fixture_binary):
    """End-to-end via the loop: initialize → tools/list → tools/call."""
    discovery = discover(str(fixture_binary))
    policy = Policy.all()

    stdin = io.StringIO()
    stdin.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}) + "\n")
    stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    stdin.write(json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}) + "\n")
    stdin.write(
        json.dumps({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "fixture-bin.echo", "arguments": {"message": "loop"}},
        }) + "\n"
    )
    stdin.seek(0)

    stdout = io.StringIO()
    stderr = io.StringIO()
    rc = serve_stdio(discovery, policy, stdin=stdin, stdout=stdout, stderr=stderr)
    assert rc == 0

    lines = [json.loads(l) for l in stdout.getvalue().splitlines() if l.strip()]
    # 3 responses (initialize, tools/list, tools/call); the notification has no response
    assert len(lines) == 3
    assert lines[0]["id"] == 1
    assert lines[1]["id"] == 2
    assert lines[2]["id"] == 3
    # The call's text should include "loop"
    call_text = lines[2]["result"]["content"][0]["text"]
    assert "loop" in call_text
