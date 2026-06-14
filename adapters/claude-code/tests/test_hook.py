import json
import sys
from pathlib import Path

# korg-ledger-py is a sibling under adapters/
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "korg-ledger-py" / "src"))

from claude_code_adapter.hook import run_hook
from korg_ledger import verify_chain

SESSION = [
    {"type": "user", "message": {"content": "fix the bug"}},
    {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 10, "output_tokens": 5},
        "content": [{"type": "tool_use", "id": "tu1", "name": "Read", "input": {"file": "a.py"}}]}},
    {"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "tu1", "content": "body", "is_error": False}]}},
]


def _write_transcript(path, records):
    path.write_text("".join(json.dumps(r) + "\n" for r in records))


def _ledger(korg_home, sid):
    p = korg_home / "sessions" / f"{sid}.jsonl"
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def test_hook_captures_a_verifiable_session(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    events = _ledger(korg_home, "sess-abc")
    assert [e["event"]["tool_name"] for e in events] == ["user_prompt", "llm_inference", "Read"]
    assert events[2]["event"]["result"] == {"output": "body"}
    assert verify_chain(events) == []


def test_second_firing_with_no_new_content_is_idempotent(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    run_hook(payload, korg_home=korg_home)  # same transcript, no growth
    events = _ledger(korg_home, "sess-abc")
    assert [e["seq_id"] for e in events] == [1, 2, 3]  # no duplicates


def test_incremental_firing_appends_only_new_events(tmp_path):
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abc.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-abc", "transcript_path": str(transcript),
               "hook_event_name": "PostToolUse"}
    run_hook(payload, korg_home=korg_home)
    # a follow-up turn lands, then the hook fires again
    _write_transcript(transcript, SESSION + [
        {"type": "user", "message": {"content": "now add a test"}},
        {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 3, "output_tokens": 2},
            "content": [{"type": "text", "text": "ok"}]}},
    ])
    run_hook(payload, korg_home=korg_home)
    events = _ledger(korg_home, "sess-abc")
    names = [e["event"]["tool_name"] for e in events]
    assert names == ["user_prompt", "llm_inference", "Read", "user_prompt", "llm_inference"]
    # follow-up user_prompt chains to the prior llm_inference (spec §2a)
    assert events[3]["metadata"]["triggered_by"] == 2
    assert events[4]["metadata"]["triggered_by"] == 2
    assert verify_chain(events) == []


def test_run_hook_never_raises_on_bad_input(tmp_path):
    # missing transcript_path / unreadable file must not raise (hook must exit 0)
    run_hook({"session_id": "x", "hook_event_name": "Stop"}, korg_home=tmp_path / ".korg")
    run_hook({"session_id": "y", "transcript_path": str(tmp_path / "nope.jsonl")},
             korg_home=tmp_path / ".korg")


def test_captured_ledger_verifies_under_spec_oracle(tmp_path):
    # the spec's INDEPENDENT reference verifier (not our own verify_chain)
    sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "spec" / "korg-ledger-v1"))
    import conformance as oracle

    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-z.jsonl"
    _write_transcript(transcript, SESSION)
    run_hook({"session_id": "sess-z", "transcript_path": str(transcript),
              "hook_event_name": "PostToolUse"}, korg_home=korg_home)
    events = _ledger(korg_home, "sess-z")
    assert oracle.verify_chain(events, None) == []
    assert oracle.chain_hash(events[-1]) == events[-1]["entry_hash"]


def test_parallel_tools_results_not_dropped_across_firings(tmp_path):
    """Two tool_use blocks in one round whose results land in a LATER firing must
    NOT be emitted with result={} (append-only can't patch them). They are held
    back until their results arrive, then captured with output."""
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-par.jsonl"
    payload = {"session_id": "sess-par", "transcript_path": str(transcript)}

    # firing 1: assistant fires Read + Bash in parallel; NO results yet
    base = [
        {"type": "user", "message": {"content": "go"}},
        {"type": "assistant", "message": {"model": "claude", "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [
                {"type": "tool_use", "id": "r1", "name": "Read", "input": {"file": "a.py"}},
                {"type": "tool_use", "id": "b1", "name": "Bash", "input": {"command": "ls"}},
            ]}},
    ]
    _write_transcript(transcript, base)
    run_hook(payload, korg_home=korg_home)

    # firing 2: both results land (batched into one user message)
    _write_transcript(transcript, base + [
        {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "r1", "content": "READ_OUT", "is_error": False},
            {"type": "tool_result", "tool_use_id": "b1", "content": "BASH_OUT", "is_error": False},
        ]}},
    ])
    run_hook(payload, korg_home=korg_home)

    events = _ledger(korg_home, "sess-par")
    assert verify_chain(events) == []
    by_tool = {e["event"]["tool_name"]: e["event"]["result"] for e in events if e["event"]["tool_name"] in ("Read", "Bash")}
    assert by_tool.get("Read", {}).get("output") == "READ_OUT", "Read result must be captured, not dropped"
    assert by_tool.get("Bash", {}).get("output") == "BASH_OUT", "Bash result must be captured, not dropped"


def test_oversized_int_arg_is_recorded_not_crashing(tmp_path):
    """A tool arg beyond ±(2^53-1) (e.g. a nanosecond timestamp) must be recorded
    (coerced canon-safe), not crash the firing and re-emit everything on the next."""
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-big.jsonl"
    payload = {"session_id": "sess-big", "transcript_path": str(transcript)}
    recs = [
        {"type": "user", "message": {"content": "go"}},
        {"type": "assistant", "message": {"model": "c", "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"offset": 2**53, "cmd": "x"}}]}},
        {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "ok", "is_error": False}]}},
    ]
    _write_transcript(transcript, recs)
    for _ in range(3):  # repeated firings must not duplicate
        run_hook(payload, korg_home=korg_home)
    events = _ledger(korg_home, "sess-big")
    assert verify_chain(events) == []
    bash = [e for e in events if e["event"]["tool_name"] == "Bash"]
    assert len(bash) == 1, "must not duplicate on re-firing"
    assert bash[0]["event"]["args"]["offset"] == str(2**53), "big int coerced to string + recorded"


def test_nonfinite_float_arg_is_recorded_not_dropped(tmp_path):
    """NaN/Infinity (json.loads accepts these literals) in a tool arg must be
    coerced canon-safe and RECORDED, not silently dropped."""
    from claude_code_adapter.canonical_emit import make_canonical_emit
    led = tmp_path / "l.jsonl"
    emit = make_canonical_emit(led, actor_id="a")
    seq = emit({"source_agent": "a", "tool_name": "T",
                "args": {"x": float("inf"), "y": float("nan")}, "result": {}})
    assert seq is not None, "event must be recorded, not dropped"
    events = [json.loads(l) for l in led.read_text().splitlines() if l.strip()]
    assert events[0]["event"]["args"] == {"x": "inf", "y": "nan"}


def test_stop_flushes_incomplete_tool_with_abort_marker(tmp_path):
    """A session that ends mid-tool (Stop fires, no tool_result) must record the
    in-flight tool with an abort marker, not silently omit it."""
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-abort.jsonl"
    recs = [
        {"type": "user", "message": {"content": "go"}},
        {"type": "assistant", "message": {"model": "c", "usage": {"input_tokens": 1, "output_tokens": 1},
            "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "sleep 999"}}]}},
    ]
    _write_transcript(transcript, recs)
    # PostToolUse holds the incomplete tool back; Stop must flush it
    run_hook({"session_id": "sess-abort", "transcript_path": str(transcript), "hook_event_name": "PostToolUse"}, korg_home=korg_home)
    run_hook({"session_id": "sess-abort", "transcript_path": str(transcript), "hook_event_name": "Stop"}, korg_home=korg_home)
    events = _ledger(korg_home, "sess-abort")
    assert verify_chain(events) == []
    bash = [e for e in events if e["event"]["tool_name"] == "Bash"]
    assert len(bash) == 1 and bash[0]["event"]["result"] == {"aborted": True}
    assert bash[0]["event"]["success"] is False


def test_concurrent_firings_do_not_duplicate_the_session(tmp_path):
    """Two firings racing on the same session must not each emit the whole tail
    (the per-session lock serializes the read-modify-write)."""
    import threading
    korg_home = tmp_path / ".korg"
    transcript = tmp_path / "sess-race.jsonl"
    _write_transcript(transcript, SESSION)
    payload = {"session_id": "sess-race", "transcript_path": str(transcript), "hook_event_name": "PostToolUse"}

    barrier = threading.Barrier(2)

    def fire():
        barrier.wait()
        run_hook(payload, korg_home=korg_home)

    threads = [threading.Thread(target=fire) for _ in range(2)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    events = _ledger(korg_home, "sess-race")
    assert verify_chain(events) == []
    # the 3-record SESSION yields a fixed set of events — never doubled
    seqs = [e["seq_id"] for e in events]
    assert len(seqs) == len(set(seqs)), f"duplicate seq_ids → session was duplicated: {seqs}"
