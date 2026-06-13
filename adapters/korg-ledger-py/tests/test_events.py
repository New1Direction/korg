from korg_ledger import agent_tool_call_event


def test_builds_agent_tool_call_event_shape():
    ev = agent_tool_call_event(
        source_agent="agent:claude-code@0.2.29",
        tool_name="Read",
        args={"file_path": "math_utils.py"},
        result={"lines": 7},
        success=True,
        duration_ms=50,
        timestamp="2026-05-25T02:50:37.077539Z",
    )
    assert ev == {
        "event_type": "AgentToolCall",
        "source_agent": "agent:claude-code@0.2.29",
        "tool_name": "Read",
        "args": {"file_path": "math_utils.py"},
        "result": {"lines": 7},
        "payload_refs": [],
        "success": True,
        "duration_ms": 50,
        "timestamp": "2026-05-25T02:50:37.077539Z",
    }


def test_timestamp_defaults_to_utc_z():
    ev = agent_tool_call_event(
        source_agent="a", tool_name="t", args={}, result={},
        success=True, duration_ms=0,
    )
    assert ev["timestamp"].endswith("Z")
