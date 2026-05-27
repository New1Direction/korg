"""Tests for the MCP JSON-RPC server surface.

We don't spawn a subprocess; we test the Server.handle() unit and the
serve_stdio() loop with StringIO streams. That exercises the protocol
without involving real I/O.
"""

from __future__ import annotations

import io
import json
from pathlib import Path

import pytest

from korg_recall_mcp.index import EventIndex
from korg_recall_mcp.search import RecallEngine
from korg_recall_mcp.server import (
    PROTOCOL_VERSION,
    SERVER_NAME,
    Server,
    format_matches_for_llm,
    serve_stdio,
)


def _write_event(path: Path, seq: int, tool_name: str, args: dict, **extra) -> None:
    record = {
        "seq": seq,
        "source_agent": "agent:test",
        "tool_name": tool_name,
        "args": args,
        "result": extra.get("result", {}),
        "success": True,
        "duration_ms": 0,
    }
    with path.open("a") as f:
        f.write(json.dumps(record) + "\n")


@pytest.fixture
def server(tmp_path):
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "rust borrow checker question"})
    _write_event(f, 2, "llm_inference", {}, result={"text": "rust enforces ownership"})
    _write_event(f, 3, "user_prompt", {"prompt": "css flexbox tips"})
    idx = EventIndex.from_paths(f)
    return Server(engine=RecallEngine(idx))


# ── Protocol ──────────────────────────────────────────────────────────


def test_initialize_returns_protocol_handshake(server):
    msg = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}
    response = server.handle(msg)
    assert response["jsonrpc"] == "2.0"
    assert response["id"] == 1
    result = response["result"]
    assert result["protocolVersion"] == PROTOCOL_VERSION
    assert result["serverInfo"]["name"] == SERVER_NAME
    assert "tools" in result["capabilities"]


def test_initialized_notification_returns_none(server):
    msg = {"jsonrpc": "2.0", "method": "notifications/initialized"}
    response = server.handle(msg)
    assert response is None


def test_tools_list_returns_recall(server):
    msg = {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}
    response = server.handle(msg)
    tools = response["result"]["tools"]
    assert len(tools) == 1
    assert tools[0]["name"] == "recall"
    assert "query" in tools[0]["inputSchema"]["properties"]
    assert tools[0]["inputSchema"]["required"] == ["query"]


def test_recall_call_returns_text_content(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {"name": "recall", "arguments": {"query": "rust", "mode": "substring"}},
    }
    response = server.handle(msg)
    content = response["result"]["content"]
    assert content[0]["type"] == "text"
    text = content[0]["text"]
    assert "recall" in text
    assert "rust" in text.lower() or "borrow" in text.lower()


def test_recall_with_no_matches(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "recall",
            "arguments": {"query": "quantum chromodynamics", "mode": "substring"},
        },
    }
    response = server.handle(msg)
    text = response["result"]["content"][0]["text"]
    assert "no relevant matches" in text


def test_recall_empty_query(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {"name": "recall", "arguments": {"query": ""}},
    }
    response = server.handle(msg)
    text = response["result"]["content"][0]["text"]
    assert "empty query" in text.lower()


def test_unknown_tool_returns_error(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {"name": "not-a-real-tool", "arguments": {}},
    }
    response = server.handle(msg)
    assert "error" in response
    assert response["error"]["code"] == -32602


def test_unknown_method_returns_error(server):
    msg = {"jsonrpc": "2.0", "id": 7, "method": "wat"}
    response = server.handle(msg)
    assert response["error"]["code"] == -32601


def test_unknown_method_notification_returns_none(server):
    msg = {"jsonrpc": "2.0", "method": "wat-notification"}
    response = server.handle(msg)
    assert response is None


def test_ping(server):
    msg = {"jsonrpc": "2.0", "id": 8, "method": "ping"}
    response = server.handle(msg)
    assert response["result"] == {}


def test_recall_tool_filter(server):
    msg = {
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": {
            "name": "recall",
            "arguments": {
                "query": "rust",
                "mode": "substring",
                "tool_filter": ["user_prompt"],
            },
        },
    }
    response = server.handle(msg)
    text = response["result"]["content"][0]["text"]
    # llm_inference seq=2 also matches "rust" but should be excluded by the filter
    assert "tool=user_prompt" in text
    assert "tool=llm_inference" not in text


# ── serve_stdio loop ──────────────────────────────────────────────────


def test_serve_stdio_handles_one_round(tmp_path):
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "rust borrow checker"})
    idx = EventIndex.from_paths(f)
    eng = RecallEngine(idx)

    stdin = io.StringIO()
    stdin.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}) + "\n")
    stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    stdin.write(
        json.dumps(
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "recall",
                    "arguments": {"query": "rust", "mode": "substring"},
                },
            }
        )
        + "\n"
    )
    stdin.seek(0)

    stdout = io.StringIO()
    stderr = io.StringIO()
    rc = serve_stdio(eng, stdin=stdin, stdout=stdout, stderr=stderr)
    assert rc == 0

    out_lines = [json.loads(ln) for ln in stdout.getvalue().splitlines() if ln.strip()]
    # 2 responses (initialize, tools/call); the notification produced no output
    assert len(out_lines) == 2
    assert out_lines[0]["id"] == 1
    assert out_lines[1]["id"] == 2
    assert "rust" in out_lines[1]["result"]["content"][0]["text"].lower()


def test_serve_stdio_skips_malformed_lines(tmp_path):
    f = tmp_path / "ledger.jsonl"
    _write_event(f, 1, "user_prompt", {"prompt": "ok"})
    idx = EventIndex.from_paths(f)
    eng = RecallEngine(idx)

    stdin = io.StringIO("not json\n\n")
    stdout = io.StringIO()
    stderr = io.StringIO()
    rc = serve_stdio(eng, stdin=stdin, stdout=stdout, stderr=stderr)
    assert rc == 0
    assert stdout.getvalue() == ""  # no response for garbage
    assert "bad json" in stderr.getvalue()


# ── Formatter ─────────────────────────────────────────────────────────


def test_format_matches_empty():
    from korg_recall_mcp.search import Match
    assert "no relevant matches" in format_matches_for_llm([], "semantic")


def test_format_matches_includes_seq_score_tool(server):
    from korg_recall_mcp.index import IndexedEvent
    from korg_recall_mcp.search import Match

    ev = IndexedEvent(
        source_file="/tmp/x.jsonl",
        seq=42,
        source_agent="agent:test#abc",
        tool_name="user_prompt",
        args={"prompt": "test"},
        result={},
        embed_text="test prompt",
    )
    formatted = format_matches_for_llm([Match(event=ev, score=0.87, via="semantic")], "semantic")
    assert "seq=42" in formatted
    assert "score=0.87" in formatted
    assert "tool=user_prompt" in formatted
