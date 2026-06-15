"""
korg MCP server tests — Phase A/B/C (resources).

Covers the capability declaration, URI parser error paths, the five
fixed resources, Phase B session/event/agent variable-segment resources,
and Phase C blob resources.

The MCP server is a thin translator over Korg's HTTP API. Tests mock
the HTTP layer via patching `_korg_journal` and `_korg_blob` so they
run without a live Korg server.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from unittest.mock import patch

import pytest
import requests

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

import mcp_server  # noqa: E402


def call(method: str, params: dict | None = None, req_id: int = 1) -> dict:
    """Round-trip one JSON-RPC request through process_message."""
    msg = {"jsonrpc": "2.0", "id": req_id, "method": method}
    if params is not None:
        msg["params"] = params
    return mcp_server.process_message(json.dumps(msg))


# Sample journal with one root, three downstream events, and one
# convention-violating actor. Ordered newest-first to match _korg_journal.
SAMPLE_EVENTS = [
    {
        "seq_id": 5,
        "schema_version": "1.0",
        "event": {
            "event_type": "AgentToolCall", "tool_name": "Edit",
            "source_agent": "agent:korgex@0.2.2",
        },
        "metadata": {"triggered_by": 4},
    },
    {
        "seq_id": 4,
        "schema_version": "1.0",
        "event": {
            "event_type": "AgentToolCall", "tool_name": "Read",
            "source_agent": "agent:korgex@0.2.2",
        },
        "metadata": {"triggered_by": 3},
    },
    {
        "seq_id": 3,
        "schema_version": "1.0",
        "event": {
            "event_type": "AgentToolCall", "tool_name": "llm_inference",
            "source_agent": "agent:korgex@0.2.2",
        },
        "metadata": {"triggered_by": 2},
    },
    {
        "seq_id": 2,
        "schema_version": "1.0",
        "event": {
            "event_type": "AgentToolCall", "tool_name": "user_prompt",
            "source_agent": "agent:korgex@0.2.2",
            "args": {"prompt": "fix the bug in math_utils.py"},
        },
        "metadata": {"triggered_by": None},
    },
    {
        "seq_id": 1,
        "schema_version": "1.0",
        "event": {
            "event_type": "AgentToolCall", "tool_name": "user_prompt",
            "source_agent": "BAD_PREFIX",     # violates §1.1
            "args": {"prompt": "earlier session"},
        },
        "metadata": {"triggered_by": None},
    },
]


# ── 1. initialize — capability declaration ──────────────────────────────


def test_initialize_declares_resources_capability():
    resp = call("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "0"},
    })
    caps = resp["result"]["capabilities"]
    assert "tools" in caps
    assert "resources" in caps
    # Phase D: subscribe is now True; listChanged remains False (static resource list)
    assert caps["resources"]["subscribe"] is True
    assert caps["resources"]["listChanged"] is False


# ── 2. resources/list and templates/list ────────────────────────────────


def test_resources_list_returns_five_fixed_resources():
    resp = call("resources/list")
    resources = resp["result"]["resources"]
    assert len(resources) == 5
    uris = {r["uri"] for r in resources}
    assert uris == {
        "korg://local/ledger/recent",
        "korg://local/ledger/heads",
        "korg://local/schema/event",
        "korg://local/schema/spec",
        "korg://local/stats/integrity",
    }


def test_resources_list_all_under_local_ledger():
    resp = call("resources/list")
    for r in resp["result"]["resources"]:
        assert r["uri"].startswith("korg://local/"), r["uri"]
        assert "mimeType" in r
        assert "name" in r


def test_resources_templates_list_returns_six_after_phase_c():
    """Phase B ships 5 + Phase C adds blob = 6 URI templates total."""
    resp = call("resources/templates/list")
    templates = resp["result"]["resourceTemplates"]
    assert len(templates) == 6
    uri_templates = {t["uriTemplate"] for t in templates}
    assert uri_templates == {
        "korg://local/session/{root_seq}",
        "korg://local/session/{root_seq}/summary",
        "korg://local/session/{root_seq}/events",
        "korg://local/event/{seq_id}",
        "korg://local/agent/{source_agent}/recent",
        "korg://local/blob/{sha256}",
    }
    for t in templates:
        assert "name" in t
        assert "mimeType" in t


# ── 3. URI parser error paths (§8.6) ────────────────────────────────────


def test_parser_rejects_wrong_scheme():
    resp = call("resources/read", {"uri": "https://local/schema/event"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "bad_scheme"


def test_parser_rejects_unknown_ledger():
    """§8.1: only 'local' is valid in v1."""
    resp = call("resources/read", {"uri": "korg://prod/schema/event"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "unknown_ledger"
    assert "local" in resp["error"]["message"]


def test_parser_rejects_missing_ledger():
    resp = call("resources/read", {"uri": "korg:///schema/event"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "missing_ledger"


def test_parser_rejects_empty_path():
    resp = call("resources/read", {"uri": "korg://local/"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "empty_path"


def test_parser_rejects_unknown_resource_path():
    resp = call("resources/read", {"uri": "korg://local/no/such/thing"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "no_such_resource"
    assert resp["error"]["data"]["uri"] == "korg://local/no/such/thing"


# ── 4. schema/event resource (no Korg dependency) ───────────────────────


def test_read_schema_event_returns_json_schema():
    resp = call("resources/read", {"uri": "korg://local/schema/event"})
    content = resp["result"]["contents"][0]
    assert content["mimeType"] == "application/json"
    schema = json.loads(content["text"])
    assert schema["title"] == "AgentToolCall"
    assert "source_agent" in schema["properties"]
    assert "triggered_by" in schema["properties"]
    # The actor prefix pattern must be enforced
    assert schema["properties"]["source_agent"]["pattern"] == "^(agent|human|korg|mcp):"


# ── 5. schema/spec resource (reads file from disk) ──────────────────────


def test_read_schema_spec_returns_markdown():
    resp = call("resources/read", {"uri": "korg://local/schema/spec"})
    content = resp["result"]["contents"][0]
    assert content["mimeType"] == "text/markdown"
    text = content["text"]
    # Stable anchors in the actual spec
    assert "# Korg Agent Event Spec" in text
    assert "§2a" in text
    assert "§2b" in text
    assert "PROPOSED" in text


def test_read_schema_spec_handles_missing_file():
    """If the spec file is unexpectedly missing, the error must be clean."""
    with patch.object(mcp_server, "SPEC_PATH", Path("/nonexistent/spec.md")):
        resp = call("resources/read", {"uri": "korg://local/schema/spec"})
        assert "error" in resp
        assert resp["error"]["data"]["reason"] == "spec_not_found"


# ── 6. ledger/recent — pagination semantics (§8.3 cursor = seq_id) ──────


def test_read_ledger_recent_returns_events():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/ledger/recent?limit=3"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert len(data["events"]) == 3


def test_read_ledger_recent_cursor_paginates_by_seq_id():
    """§8.3: cursor is the seq_id of the last event returned; next page
    requests cursor=N and returns events with seq < N."""
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        # Page 1: newest 2
        resp1 = call("resources/read", {"uri": "korg://local/ledger/recent?limit=2"})
        page1 = json.loads(resp1["result"]["contents"][0]["text"])
        assert [e["seq_id"] for e in page1["events"]] == [5, 4]
        assert page1["has_more"] is True
        assert page1["next_cursor"] == 4

        # Page 2: cursor=4 returns events with seq < 4
        resp2 = call("resources/read", {
            "uri": f"korg://local/ledger/recent?limit=2&cursor={page1['next_cursor']}",
        })
        page2 = json.loads(resp2["result"]["contents"][0]["text"])
        assert [e["seq_id"] for e in page2["events"]] == [3, 2]
        assert page2["has_more"] is True


def test_read_ledger_recent_last_page_has_no_more():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/ledger/recent?limit=100"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["has_more"] is False
        assert data["next_cursor"] is None


# ── 7. ledger/heads — root sessions only ────────────────────────────────


def test_read_ledger_heads_returns_only_root_user_prompts():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/ledger/heads"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        # SAMPLE_EVENTS has two roots at seq=1 and seq=2
        assert data["count"] == 2
        seqs = {h["seq_id"] for h in data["heads"]}
        assert seqs == {1, 2}


def test_read_ledger_heads_includes_prompt_preview():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/ledger/heads"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        previews = {h["prompt_preview"] for h in data["heads"]}
        assert "fix the bug in math_utils.py" in previews


# ── 8. stats/integrity — actor convention check ─────────────────────────


def test_read_stats_integrity_detects_actor_convention_violation():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/stats/integrity"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["actor_convention_ok"] is False
        assert "BAD_PREFIX" in data["actor_convention_violations"]


def test_read_stats_integrity_passes_when_all_actors_conform():
    clean = [e for e in SAMPLE_EVENTS if e["event"]["source_agent"] != "BAD_PREFIX"]
    with patch.object(mcp_server, "_korg_journal", return_value=clean):
        resp = call("resources/read", {"uri": "korg://local/stats/integrity"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["actor_convention_ok"] is True
        assert data["actor_convention_violations"] == []


def test_read_stats_integrity_counts_root_sessions():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/stats/integrity"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["root_sessions_in_sample"] == 2
        assert data["sample_size"] == 5


# ── 9. Korg unreachable — clean structured error ────────────────────────


def test_read_ledger_recent_returns_clean_error_when_korg_down():
    def raise_conn(*_args, **_kwargs):
        raise requests.ConnectionError("connection refused")

    with patch.object(mcp_server, "_korg_journal", side_effect=raise_conn):
        resp = call("resources/read", {"uri": "korg://local/ledger/recent"})
        assert "error" in resp
        assert resp["error"]["data"]["reason"] == "korg_unreachable"
        assert "korg" in resp["error"]["message"].lower()


# ── 10. process_message dispatch (regression) ──────────────────────────


def test_existing_tools_methods_still_work_after_resources_added():
    """Make sure adding resources/* dispatch didn't break tools/list."""
    resp = call("tools/list")
    tools = resp["result"]["tools"]
    assert len(tools) == 2
    names = {t["name"] for t in tools}
    assert names == {"korg_append_event", "korg_query_events"}
    # Annotations from the earlier work still present
    for t in tools:
        assert "annotations" in t


def test_unknown_method_still_returns_method_not_found():
    resp = call("notmethod/notreal")
    assert "error" in resp
    assert resp["error"]["code"] == -32601


# ── Phase B fixtures ────────────────────────────────────────────────────────
#
# Six-event multi-agent session rooted at seq=1.
# Main agent (korgex@0.2.2): seqs 1,2,3,6
#   1 — user_prompt (root)
#   2 — llm_inference round 1  (triggered_by=1)
#   3 — Agent spawn call        (triggered_by=2)
#   6 — llm_inference round 2  (triggered_by=2, per §2a)
# Sub-agent (korgex-sub@0.2.2): seqs 4,5
#   4 — llm_inference           (triggered_by=3, per §2b)
#   5 — Read tool               (triggered_by=4)
#
MULTI_AGENT_EVENTS = [
    # newest-first, as returned by _korg_journal
    {
        "seq_id": 6, "schema_version": "1.0",
        "event": {"event_type": "AgentToolCall", "tool_name": "llm_inference",
                  "source_agent": "agent:korgex@0.2.2"},
        "metadata": {"triggered_by": 2},
    },
    {
        "seq_id": 5, "schema_version": "1.0",
        "event": {"event_type": "AgentToolCall", "tool_name": "Read",
                  "source_agent": "agent:korgex-sub@0.2.2"},
        "metadata": {"triggered_by": 4},
    },
    {
        "seq_id": 4, "schema_version": "1.0",
        "event": {"event_type": "AgentToolCall", "tool_name": "llm_inference",
                  "source_agent": "agent:korgex-sub@0.2.2"},
        "metadata": {"triggered_by": 3},
    },
    {
        "seq_id": 3, "schema_version": "1.0",
        "event": {"event_type": "AgentToolCall", "tool_name": "Agent",
                  "source_agent": "agent:korgex@0.2.2"},
        "metadata": {"triggered_by": 2},
    },
    {
        "seq_id": 2, "schema_version": "1.0",
        "event": {"event_type": "AgentToolCall", "tool_name": "llm_inference",
                  "source_agent": "agent:korgex@0.2.2"},
        "metadata": {"triggered_by": 1},
    },
    {
        "seq_id": 1, "schema_version": "1.0",
        "event": {"event_type": "AgentToolCall", "tool_name": "user_prompt",
                  "source_agent": "agent:korgex@0.2.2",
                  "args": {"prompt": "implement the feature"}},
        "metadata": {"triggered_by": None},
    },
]


# ── 11. resources/templates/list — Phase B ──────────────────────────────────
# (test is in section 3 above, updated from Phase A assertion)


# ── 12. event/{seq_id} ──────────────────────────────────────────────────────


def test_read_event_by_seq_id_returns_full_body():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/event/3"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["seq_id"] == 3
        assert data["event"]["tool_name"] == "llm_inference"
        assert "metadata" in data


def test_read_event_not_found():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/event/999"})
        assert "error" in resp
        assert resp["error"]["data"]["reason"] == "not_found"


def test_read_event_bad_seq_id():
    resp = call("resources/read", {"uri": "korg://local/event/notanint"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "bad_seq_id"


# ── 13. session/{root_seq} — metadata ───────────────────────────────────────


def test_read_session_meta_counts_all_events_in_subtree():
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/session/1"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["root_seq"] == 1
        # Session rooted at 1 spans all 6 events
        assert data["total_events"] == 6
        assert data["first_seq"] == 1
        assert data["last_seq"] == 6


def test_read_session_meta_lists_both_agents():
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/session/1"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["agent_count"] == 2
        assert set(data["agents"]) == {
            "agent:korgex@0.2.2",
            "agent:korgex-sub@0.2.2",
        }


def test_read_session_meta_not_found():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/session/999"})
        assert "error" in resp
        assert resp["error"]["data"]["reason"] == "not_found"


def test_read_session_meta_bad_seq_id():
    resp = call("resources/read", {"uri": "korg://local/session/notanint"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "bad_seq_id"


# ── 14. session/{root_seq}/summary — skeleton, oldest→newest ────────────────


def test_read_session_summary_is_flat_chronological():
    """§8.3: events returned oldest→newest by seq_id, all spines mixed."""
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/session/1/summary"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        seqs = [e["seq_id"] for e in data["events"]]
        assert seqs == sorted(seqs), "summary must be flat chronological (ascending seq_id)"
        assert seqs == [1, 2, 3, 4, 5, 6]


def test_read_session_summary_skeleton_fields():
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/session/1/summary"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        ev = data["events"][0]
        for field in ("seq_id", "source_agent", "tool_name", "triggered_by",
                      "success", "duration_ms", "has_payload_refs"):
            assert field in ev, f"summary skeleton missing field {field!r}"
        assert "args" not in ev, "summary must not include full args (use /events for that)"


def test_read_session_summary_source_agent_filter_sub_spine():
    """§2b: sub-agent spine walk via ?source_agent filter."""
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {
            "uri": "korg://local/session/1/summary?source_agent=agent:korgex-sub@0.2.2",
        })
        data = json.loads(resp["result"]["contents"][0]["text"])
        agents = {e["source_agent"] for e in data["events"]}
        assert agents == {"agent:korgex-sub@0.2.2"}
        assert [e["seq_id"] for e in data["events"]] == [4, 5]


def test_read_session_summary_cursor_oldest_to_newest():
    """§8.3.1: head-surface cursor — cursor=N returns events with seq_id > N."""
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        # Page 1: oldest 3
        resp1 = call("resources/read", {"uri": "korg://local/session/1/summary?limit=3"})
        page1 = json.loads(resp1["result"]["contents"][0]["text"])
        assert [e["seq_id"] for e in page1["events"]] == [1, 2, 3]
        assert page1["has_more"] is True
        assert page1["next_cursor"] == 3

        # Page 2: cursor=3 returns events with seq_id > 3
        resp2 = call("resources/read", {
            "uri": f"korg://local/session/1/summary?limit=3&cursor={page1['next_cursor']}",
        })
        page2 = json.loads(resp2["result"]["contents"][0]["text"])
        assert [e["seq_id"] for e in page2["events"]] == [4, 5, 6]
        assert page2["has_more"] is False
        assert page2["next_cursor"] is None


# ── 15. session/{root_seq}/events — full bodies, oldest→newest ──────────────


def test_read_session_events_includes_full_body():
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/session/1/events"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert len(data["events"]) == 6
        first = data["events"][0]
        # Full journal envelope fields present
        assert "event" in first
        assert "metadata" in first
        assert first["seq_id"] == 1


def test_read_session_events_main_spine():
    """§2b: main-agent spine walk via ?source_agent filter."""
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {
            "uri": "korg://local/session/1/events?source_agent=agent:korgex@0.2.2",
        })
        data = json.loads(resp["result"]["contents"][0]["text"])
        seqs = [e["seq_id"] for e in data["events"]]
        assert seqs == [1, 2, 3, 6]


def test_read_session_events_sub_spine():
    """§2b: sub-agent spine walk via ?source_agent filter."""
    with patch.object(mcp_server, "_korg_journal", return_value=MULTI_AGENT_EVENTS):
        resp = call("resources/read", {
            "uri": "korg://local/session/1/events?source_agent=agent:korgex-sub@0.2.2",
        })
        data = json.loads(resp["result"]["contents"][0]["text"])
        seqs = [e["seq_id"] for e in data["events"]]
        assert seqs == [4, 5]


# ── 16. agent/{source_agent}/recent ─────────────────────────────────────────


def test_read_agent_recent_returns_only_that_agent():
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/agent/agent:korgex@0.2.2/recent"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        assert data["source_agent"] == "agent:korgex@0.2.2"
        agents = {e["event"]["source_agent"] for e in data["events"]}
        assert agents == {"agent:korgex@0.2.2"}


def test_read_agent_recent_is_newest_first():
    """§8.3.1: agent/recent is a tail surface — newest→oldest."""
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        resp = call("resources/read", {"uri": "korg://local/agent/agent:korgex@0.2.2/recent"})
        data = json.loads(resp["result"]["contents"][0]["text"])
        seqs = [e["seq_id"] for e in data["events"]]
        assert seqs == sorted(seqs, reverse=True), "agent/recent must be newest-first"


def test_read_agent_recent_cursor_tail_direction():
    """cursor=N returns events with seq_id < N (tail-surface semantics)."""
    with patch.object(mcp_server, "_korg_journal", return_value=SAMPLE_EVENTS):
        # korgex events: seqs 5,4,3,2 (newest-first after filter)
        resp1 = call("resources/read", {
            "uri": "korg://local/agent/agent:korgex@0.2.2/recent?limit=2",
        })
        page1 = json.loads(resp1["result"]["contents"][0]["text"])
        assert [e["seq_id"] for e in page1["events"]] == [5, 4]
        assert page1["has_more"] is True
        assert page1["next_cursor"] == 4

        resp2 = call("resources/read", {
            "uri": f"korg://local/agent/agent:korgex@0.2.2/recent?limit=2&cursor={page1['next_cursor']}",
        })
        page2 = json.loads(resp2["result"]["contents"][0]["text"])
        assert [e["seq_id"] for e in page2["events"]] == [3, 2]
        assert page2["has_more"] is False


# ── 17. variable-route unknown sub-resource ──────────────────────────────────


def test_unknown_session_sub_resource_returns_no_such_resource():
    resp = call("resources/read", {"uri": "korg://local/session/1/nosuchpath"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "no_such_resource"


# ── 18. blob/{sha256} — Phase C ─────────────────────────────────────────────

# Canonical 64-char sha256 used across blob tests.
FAKE_SHA256 = "a" * 64

# Well-formed JSON blob content (UTF-8 text that parses as JSON).
BLOB_JSON_BYTES = b'{"answer": 42}'

# Plain text blob (UTF-8, not JSON).
BLOB_TEXT_BYTES = b"hello from blob store"

# Binary blob (non-UTF-8 bytes).
BLOB_BINARY_BYTES = bytes(range(256))


def _blob_mock(content: bytes, content_type: str = "application/octet-stream"):
    """Return a patch.object side_effect that returns (content, content_type)."""
    return (content, content_type)


def test_blob_template_advertised_in_templates_list():
    resp = call("resources/templates/list")
    templates = resp["result"]["resourceTemplates"]
    uri_templates = {t["uriTemplate"] for t in templates}
    assert f"korg://local/blob/{{sha256}}" in uri_templates
    assert len(templates) == 6  # 5 Phase B + 1 Phase C


def test_read_blob_json_returns_text_content():
    with patch.object(mcp_server, "_korg_blob", return_value=_blob_mock(BLOB_JSON_BYTES)):
        resp = call("resources/read", {"uri": f"korg://local/blob/{FAKE_SHA256}"})
        content = resp["result"]["contents"][0]
        assert content["mimeType"] == "application/json"
        assert "text" in content
        assert json.loads(content["text"]) == {"answer": 42}


def test_read_blob_plain_text_returns_text_content():
    with patch.object(mcp_server, "_korg_blob", return_value=(BLOB_TEXT_BYTES, "text/plain")):
        resp = call("resources/read", {"uri": f"korg://local/blob/{FAKE_SHA256}"})
        content = resp["result"]["contents"][0]
        assert content["mimeType"] == "text/plain"
        assert content["text"] == "hello from blob store"


def test_read_blob_binary_returns_blob_content():
    with patch.object(mcp_server, "_korg_blob", return_value=(BLOB_BINARY_BYTES, "application/octet-stream")):
        resp = call("resources/read", {"uri": f"korg://local/blob/{FAKE_SHA256}"})
        content = resp["result"]["contents"][0]
        assert content["mimeType"] == "application/octet-stream"
        assert "blob" in content, "binary blobs must use MCP blob content type (base64 in 'blob' field)"
        assert "text" not in content
        import base64
        assert base64.b64decode(content["blob"]) == BLOB_BINARY_BYTES


def test_read_blob_not_found_returns_not_found_error():
    def raise_404(*_args, **_kwargs):
        mock_resp = type("R", (), {"status_code": 404, "text": "not found"})()
        raise requests.HTTPError(response=mock_resp)

    with patch.object(mcp_server, "_korg_blob", side_effect=raise_404):
        resp = call("resources/read", {"uri": f"korg://local/blob/{FAKE_SHA256}"})
        assert "error" in resp
        assert resp["error"]["data"]["reason"] == "not_found"


def test_read_blob_too_large_returns_structured_error():
    """§8.4.2: blobs over 10MB return blob_too_large with http_url escape hatch."""
    big_bytes = b"x" * (11 * 1024 * 1024)  # 11MB

    with patch.object(mcp_server, "_korg_blob", return_value=(big_bytes, "application/octet-stream")):
        resp = call("resources/read", {"uri": f"korg://local/blob/{FAKE_SHA256}"})
        assert "error" in resp
        err = resp["error"]
        assert err["data"]["reason"] == "blob_too_large"
        assert err["data"]["sha256"] == FAKE_SHA256
        assert err["data"]["size_bytes"] == len(big_bytes)
        assert err["data"]["http_url"] == f"/api/blob/{FAKE_SHA256}"
        # blob_too_large uses -32603 (transport limit, not bad request)
        assert err["code"] == -32603


def test_read_blob_bad_sha256_returns_error():
    resp = call("resources/read", {"uri": "korg://local/blob/notasha256"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "bad_sha256"


def test_read_blob_uri_field_present_in_content():
    with patch.object(mcp_server, "_korg_blob", return_value=(BLOB_JSON_BYTES, "application/json")):
        uri = f"korg://local/blob/{FAKE_SHA256}"
        resp = call("resources/read", {"uri": uri})
        assert resp["result"]["contents"][0]["uri"] == uri


# ── 19. Phase D — subscriptions ─────────────────────────────────────────────
#
# Tests call _dispatch_notifications() directly (no real threading) and patch
# _send_notification to capture what would be written to stdout. This keeps
# tests deterministic and fast — the daemon thread is a deployment concern,
# not a unit-test concern. The _seq_to_root table and _subscriptions dict are
# manipulated directly; each test clears them to avoid state leakage.


def _clear_sub_state():
    """Reset all Phase D module state between tests."""
    with mcp_server._subscription_lock:
        mcp_server._subscriptions.clear()
    with mcp_server._seq_to_root_lock:
        mcp_server._seq_to_root.clear()


# Common events used across Phase D tests.
_D_ROOT = {
    "seq_id": 1, "schema_version": "1.0",
    "event": {
        "event_type": "AgentToolCall", "tool_name": "user_prompt",
        "source_agent": "agent:korgex@0.2.2",
        "args": {"prompt": "go"},
    },
    "metadata": {"triggered_by": None},
}
_D_CHILD = {
    "seq_id": 2, "schema_version": "1.0",
    "event": {
        "event_type": "AgentToolCall", "tool_name": "Read",
        "source_agent": "agent:korgex@0.2.2",
    },
    "metadata": {"triggered_by": 1},
}
_D_OTHER = {
    "seq_id": 3, "schema_version": "1.0",
    "event": {
        "event_type": "AgentToolCall", "tool_name": "Edit",
        "source_agent": "agent:other@1.0",
    },
    "metadata": {"triggered_by": None},
}


def test_initialize_declares_subscribe_true_in_phase_d():
    """Phase D flips subscribe capability to True."""
    resp = call("initialize", {
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "0"},
    })
    caps = resp["result"]["capabilities"]
    assert caps["resources"]["subscribe"] is True
    assert caps["resources"]["listChanged"] is False


def test_subscribe_returns_empty_confirmation():
    """§8.5.3: subscribe returns empty result (no snapshot)."""
    _clear_sub_state()
    resp = call("resources/subscribe", {"uri": "korg://local/ledger/recent"})
    assert resp["result"] == {}


def test_subscribe_fires_notification_for_ledger_recent():
    """ledger/recent predicate: any new event triggers a notification."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})

    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_CHILD])

    assert len(captured) == 1
    assert captured[0]["method"] == "notifications/resources/updated"
    assert captured[0]["params"]["uri"] == "korg://local/ledger/recent"


def test_ledger_heads_predicate_fires_only_on_root_events():
    """ledger/heads predicate: user_prompt with triggered_by=None only."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/heads"})

    # _D_CHILD has triggered_by=1 — should NOT fire
    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_CHILD])
    assert captured == []

    # _D_ROOT has triggered_by=None — SHOULD fire
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_ROOT])
    assert len(captured) == 1


def test_session_summary_predicate_uses_seq_to_root():
    """session/{root}/summary predicate: fires when new event's root matches."""
    _clear_sub_state()
    # Pre-populate: root=1, child=2
    mcp_server._update_seq_to_root([_D_ROOT, _D_CHILD])
    call("resources/subscribe", {"uri": "korg://local/session/1/summary"})

    # _D_CHILD (seq=2, root=1) — matches
    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_CHILD])
    assert len(captured) == 1
    assert captured[0]["params"]["uri"] == "korg://local/session/1/summary"

    # _D_OTHER (seq=3, not in _seq_to_root → root=3) — no match
    captured.clear()
    mcp_server._update_seq_to_root([_D_OTHER])
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_OTHER])
    assert captured == []


def test_agent_recent_predicate_filters_by_source_agent():
    """agent/{source_agent}/recent predicate: fires only for that agent."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/agent/agent:korgex@0.2.2/recent"})

    # _D_CHILD is from agent:korgex@0.2.2 — matches
    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_CHILD])
    assert len(captured) == 1

    # _D_OTHER is from agent:other@1.0 — no match
    captured.clear()
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_OTHER])
    assert captured == []


def test_two_subscriptions_only_matching_fires():
    """With two subscriptions, only the one whose predicate matches fires."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})
    call("resources/subscribe", {"uri": "korg://local/ledger/heads"})

    captured = []
    # _D_CHILD is not a root, so ledger/heads should NOT fire
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_CHILD])

    assert len(captured) == 1
    assert captured[0]["params"]["uri"] == "korg://local/ledger/recent"


def test_at_most_one_notification_per_uri_per_dispatch():
    """Multiple matching events in one tick fire exactly one notification per URI."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})

    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_ROOT, _D_CHILD, _D_OTHER])

    assert len(captured) == 1


def test_unsubscribe_stops_notifications():
    """After unsubscribe, matching events produce no notification."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})
    resp = call("resources/unsubscribe", {"uri": "korg://local/ledger/recent"})
    assert resp["result"] == {}

    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_CHILD])
    assert captured == []


def test_unsubscribe_nonexistent_is_noop():
    """Unsubscribing a URI that was never subscribed returns empty success."""
    _clear_sub_state()
    resp = call("resources/unsubscribe", {"uri": "korg://local/ledger/recent"})
    assert resp["result"] == {}


def test_subscribe_idempotent_single_entry():
    """Subscribing the same URI twice results in exactly one subscription entry."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})

    with mcp_server._subscription_lock:
        count = len(mcp_server._subscriptions.get("korg://local/ledger/recent", []))
    assert count == 1


def test_subscribe_non_subscribable_uri_returns_error():
    """event/{seq_id} is immutable — subscribing it returns not_subscribable."""
    _clear_sub_state()
    resp = call("resources/subscribe", {"uri": "korg://local/event/42"})
    assert "error" in resp
    assert resp["error"]["data"]["reason"] == "not_subscribable"


def test_predicate_exception_does_not_crash_dispatch():
    """A predicate that raises must not kill dispatch for other subscriptions."""
    _clear_sub_state()

    # Install a broken subscription directly
    broken = mcp_server.Subscription(
        uri="korg://local/ledger/recent",
        predicate=lambda _e: 1 / 0,
    )
    with mcp_server._subscription_lock:
        mcp_server._subscriptions["korg://local/ledger/recent"] = [broken]

    # Also subscribe a good one
    call("resources/subscribe", {"uri": "korg://local/ledger/heads"})

    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([_D_ROOT])

    # ledger/heads matched; ledger/recent raised — but dispatch completed
    assert len(captured) == 1
    assert captured[0]["params"]["uri"] == "korg://local/ledger/heads"


def test_dispatch_notifications_empty_events_is_noop():
    """_dispatch_notifications([]) returns without touching subscriptions."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})

    captured = []
    with patch.object(mcp_server, "_send_notification", side_effect=captured.append):
        mcp_server._dispatch_notifications([])
    assert captured == []


def test_update_seq_to_root_builds_lookup():
    """_update_seq_to_root correctly maps root and child events."""
    _clear_sub_state()
    mcp_server._update_seq_to_root([_D_ROOT, _D_CHILD])

    with mcp_server._seq_to_root_lock:
        assert mcp_server._seq_to_root[1] == 1   # root maps to itself
        assert mcp_server._seq_to_root[2] == 1   # child inherits root=1


# ── Invariant tests (from property-test spec) ────────────────────────────────


def test_exactly_once_across_consecutive_ticks():
    """Invariant 1+2: each _dispatch_notifications call fires exactly one notification
    per matching URI — no merging across ticks, no drops between ticks."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})

    d4 = {"seq_id": 4, "schema_version": "1.0",
          "event": {"event_type": "AgentToolCall", "tool_name": "Write",
                    "source_agent": "agent:korgex@0.2.2"},
          "metadata": {"triggered_by": 1}}
    d5 = {"seq_id": 5, "schema_version": "1.0",
          "event": {"event_type": "AgentToolCall", "tool_name": "Bash",
                    "source_agent": "agent:korgex@0.2.2"},
          "metadata": {"triggered_by": 1}}

    tick1, tick2 = [], []

    with patch.object(mcp_server, "_send_notification", side_effect=tick1.append):
        mcp_server._dispatch_notifications([_D_ROOT, _D_CHILD, _D_OTHER])  # 3 matching events

    with patch.object(mcp_server, "_send_notification", side_effect=tick2.append):
        mcp_server._dispatch_notifications([d4, d5])  # 2 matching events

    # Each tick: exactly one notification regardless of batch size
    assert len(tick1) == 1
    assert len(tick2) == 1
    assert tick1[0]["params"]["uri"] == "korg://local/ledger/recent"
    assert tick2[0]["params"]["uri"] == "korg://local/ledger/recent"


def test_unsubscribe_post_snapshot_fires_once_more():
    """Invariant 3 edge case: if unsubscribe runs after _dispatch_notifications has
    already snapshotted the registry, the subscriber receives one final notification.
    Events arriving after unsubscribe is fully committed produce no notification."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})

    # Simulate the snapshot _dispatch_notifications takes internally
    with mcp_server._subscription_lock:
        pre_unsub_snapshot = {uri: list(subs)
                              for uri, subs in mcp_server._subscriptions.items()}

    # Unsubscribe runs after snapshot — registry is now empty
    call("resources/unsubscribe", {"uri": "korg://local/ledger/recent"})

    # Dispatch from the stale snapshot still fires (snapshot-race: one more notification)
    fired_from_stale = []
    for uri, subs in pre_unsub_snapshot.items():
        for sub in subs:
            try:
                if sub.predicate(_D_CHILD):
                    fired_from_stale.append(uri)
                    break
            except Exception:
                pass
    assert len(fired_from_stale) == 1  # documented: one final notification possible

    # But the next real dispatch (fresh snapshot) produces nothing
    post_unsub = []
    with patch.object(mcp_server, "_send_notification", side_effect=post_unsub.append):
        mcp_server._dispatch_notifications([_D_CHILD])
    assert post_unsub == []


def test_restart_empties_registry_and_silences_notifications():
    """Invariant 5 (§8.5.4 ephemeral-state): subscriptions are in-memory only.
    After server restart (module state cleared), all subscriptions are gone and
    no notifications fire for events arriving during or after downtime."""
    _clear_sub_state()
    call("resources/subscribe", {"uri": "korg://local/ledger/recent"})
    call("resources/subscribe", {"uri": "korg://local/ledger/heads"})

    with mcp_server._subscription_lock:
        assert len(mcp_server._subscriptions) == 2

    # Simulate restart: module-level dicts re-initialize to empty (same as process start)
    _clear_sub_state()

    with mcp_server._subscription_lock:
        assert len(mcp_server._subscriptions) == 0

    # Events that arrived during downtime produce no notifications after restart
    post_restart = []
    with patch.object(mcp_server, "_send_notification", side_effect=post_restart.append):
        mcp_server._dispatch_notifications([_D_ROOT, _D_CHILD, _D_OTHER])
    assert post_restart == []
