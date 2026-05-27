"""Tests for tail-mode ingestion.

These tests don't run a real `run()` loop — they exercise `poll_once()`
directly, which is the unit that does all the real work. The loop is just a
sleep+poll wrapper around it.

The critical invariants tested:

  - **No duplicate emission.** Calling poll_once() multiple times against
    the same file emits each line exactly once, even if the file grew
    between polls.
  - **Cross-poll causal coherence.** A tool_use in poll 1 and its
    tool_result in poll 2 must still attach correctly (parser + adapter
    state must persist across polls).
  - **Mid-write tolerance.** A line being written without a trailing \\n
    must be held back until the next poll.
  - **Restart safety.** A fresh TailIngester with the same state_path
    resumes exactly where the prior one stopped.
  - **Multiple sessions.** Different .jsonl files get independent adapters
    (so their seq chains don't interfere).
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from claude_code_adapter import (
    ClaudeCodeAdapter,
    TailIngester,
    TailState,
    make_jsonl_emit,
    make_stub_emit,
)


def _write(path: Path, *lines: str) -> None:
    """Append complete lines (each terminated with \\n) to a file."""
    with path.open("a") as f:
        for line in lines:
            f.write(line + "\n")


def _append_raw(path: Path, text: str) -> None:
    """Append raw bytes (no auto-newline). For mid-write tests."""
    with path.open("a") as f:
        f.write(text)


def _user(content: str) -> str:
    return json.dumps({"type": "user", "message": {"role": "user", "content": content}})


def _assistant_text(text: str, output_tokens: int = 5) -> str:
    return json.dumps(
        {
            "type": "assistant",
            "message": {
                "role": "assistant",
                "model": "claude-opus-4-7",
                "content": [{"type": "text", "text": text}],
                "usage": {"input_tokens": 1, "output_tokens": output_tokens},
            },
        }
    )


def _assistant_tool_use(tool_name: str, args: dict, tool_use_id: str) -> str:
    return json.dumps(
        {
            "type": "assistant",
            "message": {
                "role": "assistant",
                "model": "claude-opus-4-7",
                "content": [
                    {"type": "tool_use", "id": tool_use_id, "name": tool_name, "input": args}
                ],
                "usage": {"input_tokens": 1, "output_tokens": 1},
            },
        }
    )


def _tool_result(tool_use_id: str, output: str, is_error: bool = False) -> str:
    return json.dumps(
        {
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": output,
                        "is_error": is_error,
                    }
                ],
            },
        }
    )


# ── TailState ─────────────────────────────────────────────────────────


def test_tailstate_persists_and_reloads(tmp_path):
    state_file = tmp_path / "tail-state.json"
    s = TailState.load(state_file)
    assert s.offsets == {}
    s.set("/foo.jsonl", 100)
    s.set("/bar.jsonl", 250)
    s.save()

    reloaded = TailState.load(state_file)
    assert reloaded.offsets == {"/foo.jsonl": 100, "/bar.jsonl": 250}


def test_tailstate_handles_corrupt_file(tmp_path):
    state_file = tmp_path / "tail-state.json"
    state_file.write_text("{ not valid json")
    s = TailState.load(state_file)
    # Falls back to empty offsets instead of crashing
    assert s.offsets == {}


def test_tailstate_atomic_save_via_tmp_rename(tmp_path):
    """The .tmp file should not linger after save()."""
    state_file = tmp_path / "tail-state.json"
    s = TailState.load(state_file)
    s.set("/x.jsonl", 1)
    s.save()
    assert state_file.exists()
    assert not list(tmp_path.glob("*.tmp"))


# ── poll_once: basic ──────────────────────────────────────────────────


def test_poll_once_emits_events_from_a_new_file(tmp_path):
    projects = tmp_path / "projects" / "session-abc"
    projects.mkdir(parents=True)
    f = projects / "abc.jsonl"
    _write(f, _user("hello"), _assistant_text("hi back"))

    state_file = tmp_path / "state.json"
    out_file = tmp_path / "out.jsonl"

    ing = TailIngester(
        emit=make_jsonl_emit(out_file),
        state_path=state_file,
        projects_dir=tmp_path / "projects",
    )
    stats = ing.poll_once()

    # 1 user_prompt + 1 llm_inference
    assert stats.new_events == 2
    assert stats.new_user_prompts == 1
    assert stats.new_llm_rounds == 1
    assert out_file.exists()
    written = [json.loads(ln) for ln in out_file.read_text().splitlines() if ln]
    assert len(written) == 2


def test_poll_once_idempotent_when_file_unchanged(tmp_path):
    projects = tmp_path / "projects"
    projects.mkdir()
    f = projects / "abc.jsonl"
    _write(f, _user("hello"), _assistant_text("hi"))

    state_file = tmp_path / "state.json"
    out_file = tmp_path / "out.jsonl"

    ing = TailIngester(
        emit=make_jsonl_emit(out_file),
        state_path=state_file,
        projects_dir=projects,
    )
    s1 = ing.poll_once()
    s2 = ing.poll_once()  # second poll — file unchanged
    assert s1.new_events == 2
    assert s2.new_events == 0  # no duplication


def test_poll_once_picks_up_only_new_lines_when_file_grows(tmp_path):
    projects = tmp_path / "projects"
    projects.mkdir()
    f = projects / "abc.jsonl"
    _write(f, _user("first"))

    state_file = tmp_path / "state.json"
    out_file = tmp_path / "out.jsonl"

    ing = TailIngester(
        emit=make_jsonl_emit(out_file),
        state_path=state_file,
        projects_dir=projects,
    )
    s1 = ing.poll_once()
    assert s1.new_events == 1

    # File grows
    _write(f, _assistant_text("answer 1"), _user("second"))
    s2 = ing.poll_once()
    assert s2.new_events == 2  # only the new ones

    # Verify cumulative output
    written = [json.loads(ln) for ln in out_file.read_text().splitlines() if ln]
    assert len(written) == 3


# ── poll_once: causal coherence across polls ──────────────────────────


def test_tool_use_in_poll1_and_tool_result_in_poll2_still_attach(tmp_path):
    """The most important tail-mode invariant: parser state must persist."""
    projects = tmp_path / "projects"
    projects.mkdir()
    f = projects / "abc.jsonl"

    # Poll 1: user + assistant-with-tool_use (no result yet)
    _write(
        f,
        _user("read /tmp/foo"),
        _assistant_tool_use("Read", {"file_path": "/tmp/foo"}, "tu1"),
    )
    state_file = tmp_path / "state.json"
    out_file = tmp_path / "out.jsonl"
    ing = TailIngester(
        emit=make_jsonl_emit(out_file),
        state_path=state_file,
        projects_dir=projects,
    )
    s1 = ing.poll_once()
    assert s1.new_events == 3  # user_prompt + llm_inference + Read tool_call

    # Poll 2: the tool_result block arrives
    _write(f, _tool_result("tu1", "hello world"))
    s2 = ing.poll_once()
    # The tool_result doesn't emit a new event — it just back-attaches.
    assert s2.new_events == 0

    # Poll 3: a follow-up assistant text
    _write(f, _assistant_text("got: hello world"))
    s3 = ing.poll_once()
    assert s3.new_events == 1


def test_subsequent_llm_chains_to_prior_llm_across_polls(tmp_path):
    """Spec §2a held across an inter-poll boundary."""
    projects = tmp_path / "projects"
    projects.mkdir()
    f = projects / "abc.jsonl"

    bodies = []

    def capture(body):
        bodies.append(dict(body))
        return len(bodies)

    state_file = tmp_path / "state.json"
    ing = TailIngester(
        emit=capture, state_path=state_file, projects_dir=projects
    )

    _write(
        f,
        _user("hi"),
        _assistant_tool_use("Read", {"file_path": "/x"}, "tu1"),
    )
    ing.poll_once()
    # 3 emits: user_prompt(1), llm_inference(2), Read(3)

    _write(f, _tool_result("tu1", "ok"))
    ing.poll_once()
    # No emit — tool_result back-attaches

    _write(f, _assistant_text("done"))
    ing.poll_once()
    # 1 emit: second llm_inference — should triggered_by=2 (prior llm), NOT 3 (tool)

    assert len(bodies) == 4
    assert bodies[3]["tool_name"] == "llm_inference"
    assert bodies[3]["triggered_by"] == 2  # spec §2a


# ── poll_once: mid-write line ─────────────────────────────────────────


def test_partial_line_without_newline_is_held_back(tmp_path):
    projects = tmp_path / "projects"
    projects.mkdir()
    f = projects / "abc.jsonl"
    _write(f, _user("ok"))
    _append_raw(f, '{"type":"user","message":{"role":"user","content":"partial')  # no \n

    state_file = tmp_path / "state.json"
    out_file = tmp_path / "out.jsonl"
    ing = TailIngester(
        emit=make_jsonl_emit(out_file),
        state_path=state_file,
        projects_dir=projects,
    )
    s1 = ing.poll_once()
    assert s1.new_events == 1  # only the complete line

    # Now finish the partial line
    _append_raw(f, '"}}\n')
    s2 = ing.poll_once()
    assert s2.new_events == 1  # the previously-partial line


# ── restart safety ────────────────────────────────────────────────────


def test_restart_with_same_state_resumes_at_correct_offset(tmp_path):
    projects = tmp_path / "projects"
    projects.mkdir()
    f = projects / "abc.jsonl"
    _write(f, _user("first"), _assistant_text("answer"), _user("second"))

    state_file = tmp_path / "state.json"

    # First ingester does one poll
    bodies_1 = []
    ing1 = TailIngester(
        emit=lambda b: (bodies_1.append(dict(b)), len(bodies_1))[1],
        state_path=state_file,
        projects_dir=projects,
    )
    ing1.poll_once()
    assert len(bodies_1) == 3

    # Append more, then restart with the SAME state file
    _write(f, _assistant_text("final"))

    bodies_2 = []
    ing2 = TailIngester(
        emit=lambda b: (bodies_2.append(dict(b)), len(bodies_2))[1],
        state_path=state_file,
        projects_dir=projects,
    )
    s = ing2.poll_once()
    assert s.new_events == 1
    assert len(bodies_2) == 1  # only the new event, not duplicates


def test_restart_with_fresh_state_redoes_everything(tmp_path):
    projects = tmp_path / "projects"
    projects.mkdir()
    f = projects / "abc.jsonl"
    _write(f, _user("hi"), _assistant_text("ok"))

    state_file_1 = tmp_path / "state-1.json"
    state_file_2 = tmp_path / "state-2.json"

    bodies = []
    TailIngester(
        emit=lambda b: (bodies.append(dict(b)), len(bodies))[1],
        state_path=state_file_1,
        projects_dir=projects,
    ).poll_once()
    n_first = len(bodies)
    assert n_first == 2

    # New ingester, fresh state — redoes everything
    TailIngester(
        emit=lambda b: (bodies.append(dict(b)), len(bodies))[1],
        state_path=state_file_2,
        projects_dir=projects,
    ).poll_once()
    assert len(bodies) == n_first + 2  # everything re-emitted


# ── multi-file (multi-session) ────────────────────────────────────────


def test_multiple_session_files_get_independent_adapters(tmp_path):
    projects = tmp_path / "projects"
    projects.mkdir()
    a = projects / "session-a"
    b = projects / "session-b"
    a.mkdir()
    b.mkdir()
    fa = a / "a.jsonl"
    fb = b / "b.jsonl"
    _write(fa, _user("from a"), _assistant_text("answer a"))
    _write(fb, _user("from b"), _assistant_text("answer b"))

    bodies = []
    state_file = tmp_path / "state.json"
    ing = TailIngester(
        emit=lambda body: (bodies.append(dict(body)), len(bodies))[1],
        state_path=state_file,
        projects_dir=projects,
    )
    s = ing.poll_once()
    assert s.files_active == 2
    assert s.new_events == 4

    # Each session's prompts/llm should be present
    prompts = [b for b in bodies if b["tool_name"] == "user_prompt"]
    assert sorted([p["args"]["prompt"] for p in prompts]) == ["from a", "from b"]

    # source_agent should include the session id to disambiguate
    sources = {b["source_agent"] for b in bodies}
    assert any("a" in s for s in sources)
    assert any("b" in s for s in sources)


def test_new_file_appearing_between_polls_is_picked_up(tmp_path):
    projects = tmp_path / "projects"
    projects.mkdir()

    bodies = []
    state_file = tmp_path / "state.json"
    ing = TailIngester(
        emit=lambda body: (bodies.append(dict(body)), len(bodies))[1],
        state_path=state_file,
        projects_dir=projects,
    )
    s0 = ing.poll_once()
    assert s0.files_active == 0

    f = projects / "new.jsonl"
    _write(f, _user("hi"), _assistant_text("hello"))
    s1 = ing.poll_once()
    assert s1.files_active == 1
    assert s1.new_events == 2


# ── projects_dir missing ──────────────────────────────────────────────


def test_missing_projects_dir_yields_no_events(tmp_path):
    state_file = tmp_path / "state.json"
    ing = TailIngester(
        emit=make_stub_emit(),
        state_path=state_file,
        projects_dir=tmp_path / "definitely_does_not_exist",
    )
    s = ing.poll_once()
    assert s.new_events == 0
    assert s.files_active == 0


# ── run() loop with manual stop ───────────────────────────────────────


def test_run_loop_responds_to_stop(tmp_path):
    """Just make sure run() doesn't hang and respects stop()."""
    projects = tmp_path / "projects"
    projects.mkdir()
    state_file = tmp_path / "state.json"

    ing = TailIngester(
        emit=make_stub_emit(),
        state_path=state_file,
        projects_dir=projects,
    )

    # No signals (we're not on the main thread in pytest's perspective for some runners),
    # rely on stop() flag being checked between polls.
    poll_calls = {"n": 0}

    def stop_after_one(_poll):
        poll_calls["n"] += 1
        if poll_calls["n"] >= 1:
            ing.stop()

    # Use a tiny poll interval so the test finishes quickly.
    ing.run(poll_interval_s=0.05, on_poll=stop_after_one, install_signal_handlers=False)
    assert poll_calls["n"] >= 1


def test_jsonl_emit_assigns_increasing_seq_ids(tmp_path):
    out = tmp_path / "out.jsonl"
    emit = make_jsonl_emit(out)
    seq_a = emit({"tool_name": "user_prompt", "args": {"prompt": "a"}})
    seq_b = emit({"tool_name": "llm_inference"})
    assert seq_a == 1
    assert seq_b == 2
    records = [json.loads(ln) for ln in out.read_text().splitlines() if ln]
    assert records[0]["seq"] == 1
    assert records[1]["seq"] == 2
