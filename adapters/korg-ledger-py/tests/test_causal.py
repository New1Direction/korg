"""korg-ledger@v1 §9: causal-parent resolution across flat and nested shapes."""
import json

import pytest

from korg_ledger import CausalityError, LedgerWriter, agent_tool_call_event, causal_parent, verify_dag


def test_nested_links_are_checked_not_skipped():
    events = [
        {"seq_id": 1, "metadata": {"triggered_by": None}},
        {"seq_id": 2, "metadata": {"triggered_by": 3}},  # forward reference
        {"seq_id": 3, "metadata": {"triggered_by": 7}},  # dangling
    ]
    assert verify_dag(events) == [
        "seq 2: triggered_by 3 is not strictly earlier",
        "seq 3: triggered_by 7 does not exist",
    ]


@pytest.mark.parametrize(
    "value, parent, error",
    [
        (None, None, None),
        (1, 1, None),
        (1.0, 1, None),  # JS cannot tell 1.0 from 1, so every impl accepts it
        (True, None, "triggered_by is not an integer"),  # bool is not an int (Python quirk)
        (1.5, None, "triggered_by is not an integer"),
        ("1", None, "triggered_by is not an integer"),
        (2**53, None, "triggered_by is not an integer"),
        ([1], None, "triggered_by is not an integer"),
    ],
)
def test_integer_domain(value, parent, error):
    assert causal_parent({"triggered_by": value}) == (parent, error)
    assert causal_parent({"metadata": {"triggered_by": value}}) == (parent, error)


def test_both_shapes_must_agree():
    assert causal_parent({"triggered_by": 2, "metadata": {"triggered_by": 2}}) == (2, None)
    assert causal_parent({"triggered_by": 2, "metadata": {"triggered_by": None}}) == (2, None)
    assert causal_parent({"triggered_by": 2, "metadata": {"triggered_by": 1}}) == (
        None, "conflicting triggered_by (top-level 2 vs metadata 1)")


def test_bool_seq_ids_are_not_counted():
    # previously Python counted True as seq 1 and Rust/JS did not — a verdict split
    assert verify_dag([{"seq_id": True}, {"seq_id": 1}]) == []
    assert verify_dag([{"seq_id": 1}, {"seq_id": 2, "triggered_by": 1}, {"seq_id": "x", "triggered_by": 1}]) == [
        "seq ?: triggered_by 1 cannot be ordered (seq_id is not an integer)"
    ]


def _evt():
    return agent_tool_call_event(source_agent="agent:t", tool_name="t", args={}, result={}, success=True, duration_ms=0)


@pytest.mark.parametrize("bad", [True, "1", 1.0])
def test_writer_rejects_non_integer_triggered_by(tmp_path, bad):
    w = LedgerWriter(tmp_path / "l.jsonl")
    w.append(event=_evt(), actor_id="korg:x")
    with pytest.raises(CausalityError):
        w.append(event=_evt(), actor_id="korg:x", triggered_by=bad)


def test_writer_output_passes_the_nested_dag_check(tmp_path):
    led = tmp_path / "l.jsonl"
    w = LedgerWriter(led)
    a = w.append(event=_evt(), actor_id="korg:x")
    b = w.append(event=_evt(), actor_id="korg:x", triggered_by=a)
    w.append(event=_evt(), actor_id="korg:x", triggered_by=b)
    events = [json.loads(l) for l in led.read_text().splitlines()]
    assert verify_dag(events) == []
    # and a hand-edited forward link in metadata is now caught
    events[1]["metadata"]["triggered_by"] = 3
    assert verify_dag(events) == ["seq 2: triggered_by 3 is not strictly earlier"]
