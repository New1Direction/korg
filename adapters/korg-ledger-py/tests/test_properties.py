import json
import tempfile
from pathlib import Path

from hypothesis import given, settings, strategies as st

from korg_ledger import (
    CausalityError,
    LedgerWriter,
    agent_tool_call_event,
    verify_chain,
)

# small JSON-object payloads for args (canon-safe integer range — values beyond
# ±(2^53-1) are out of korg-ledger@v1 scope and rejected by canonicalize)
_payloads = st.lists(
    st.dictionaries(
        st.text(min_size=1, max_size=5),
        st.integers(min_value=-(2**53 - 1), max_value=2**53 - 1),
        max_size=4,
    ),
    max_size=10,
)


def _write(led, payloads):
    w = LedgerWriter(led)
    for p in payloads:
        w.append(
            event=agent_tool_call_event(
                source_agent="a", tool_name="t", args=p, result={},
                success=True, duration_ms=0),
            actor_id="korg:test",
        )
    return [json.loads(l) for l in led.read_text().splitlines() if l.strip()]


@settings(max_examples=150, deadline=None)
@given(payloads=_payloads)
def test_any_sequence_of_appends_verifies_clean(payloads):
    with tempfile.TemporaryDirectory() as d:
        events = _write(Path(d) / "l.jsonl", payloads)
        assert len(events) == len(payloads)
        assert verify_chain(events) == []


@settings(max_examples=150, deadline=None)
@given(payloads=_payloads.filter(lambda p: len(p) >= 1), idx=st.integers(min_value=0))
def test_tampering_any_event_breaks_verification(payloads, idx):
    with tempfile.TemporaryDirectory() as d:
        events = _write(Path(d) / "l.jsonl", payloads)
        i = idx % len(events)
        events[i]["event"]["args"]["__tamper__"] = 1  # mutate without rehashing
        assert verify_chain(events) != []


@settings(max_examples=100, deadline=None)
@given(payloads=_payloads)
def test_resume_preserves_the_chain(payloads):
    with tempfile.TemporaryDirectory() as d:
        led = Path(d) / "l.jsonl"
        _write(led, payloads)
        # a fresh writer resumes and appends one more; the whole chain stays valid
        w2 = LedgerWriter(led)
        w2.append(
            event=agent_tool_call_event(source_agent="a", tool_name="t2", args={}, result={},
                                        success=True, duration_ms=0),
            actor_id="korg:test",
        )
        events = [json.loads(l) for l in led.read_text().splitlines() if l.strip()]
        assert len(events) == len(payloads) + 1
        assert verify_chain(events) == []


@settings(max_examples=50, deadline=None)
@given(bad=st.integers(min_value=1))
def test_causality_gate_rejects_non_earlier_triggered_by(bad):
    with tempfile.TemporaryDirectory() as d:
        w = LedgerWriter(Path(d) / "l.jsonl")
        # first event is seq 1; any triggered_by >= the next seq must be rejected
        s1 = w.append(event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={},
                                                  success=True, duration_ms=0), actor_id="korg:test")
        try:
            w.append(event=agent_tool_call_event(source_agent="a", tool_name="t", args={}, result={},
                                                 success=True, duration_ms=0),
                     actor_id="korg:test", triggered_by=s1 + 1 + bad)
            raised = False
        except CausalityError:
            raised = True
        assert raised
