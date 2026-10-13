"""korg-redact@v1: the shared hand-authored cases + redaction-at-persistence."""
import json
from pathlib import Path

import pytest
from hypothesis import given, settings, strategies as st

from korg_ledger import (
    LedgerWriter,
    agent_tool_call_event,
    redact,
    scan,
    verify_chain,
    verify_dag,
    verify_redaction,
)
from korg_ledger.redaction import finding_order

SPEC = Path(__file__).resolve().parents[3] / "spec" / "korg-ledger-v1"
CASES = json.loads((SPEC / "vectors" / "redaction" / "korg-redact-v1.json").read_text(encoding="utf-8"))


@pytest.mark.parametrize("case", CASES["cases"], ids=lambda c: c["name"])
def test_shared_redaction_cases(case):
    want = case["input"] if case["output"] is None else case["output"]
    got, findings = redact(case["input"])
    assert got == want
    assert sorted(findings, key=finding_order) == case["findings"]
    # idempotent: the output is clean and a second pass changes nothing
    assert redact(got) == (got, [])


# Fragments that can splice into secrets or near-secrets; idempotence must hold
# for any concatenation, including ones that create overlaps between rules.
_frag = st.sampled_from([
    "sk-", "abcdefghijklmnopqrstuvwxyz", "Bearer ", "API_KEY", "=", ":", " ", "\n", "'", '"',
    "https://", "u:p@", "@", "AKIA", "IOSFODNN7EXAMPLE", "-----BEGIN RSA PRIVATE KEY-----",
    "-----END RSA PRIVATE KEY-----", "eyJ", "abcdefgh.", '"password": ', "[REDACTED:jwt]", "é", "ghp_",
    "0123456789" * 4, "xoxb-", "AIza",
])


@settings(max_examples=3000, deadline=None)
@given(st.lists(_frag, max_size=12).map("".join))
def test_redaction_is_idempotent_on_spliced_strings(s):
    once, _ = redact(s)
    assert redact(once) == (once, [])


def _evt(args, result=None):
    return agent_tool_call_event(
        source_agent="agent:claude-code@2.1.0", tool_name="Bash", args=args,
        result=result or {}, success=True, duration_ms=1,
    )


SECRET = "sk-proj-THISISASECRETVALUE0123456789"


def test_writer_redacts_before_hashing_and_never_writes_the_secret(tmp_path):
    led = tmp_path / "ledger.jsonl"
    w = LedgerWriter(led)
    s1 = w.append(event=_evt({"command": f"export OPENAI_API_KEY={SECRET}"}), actor_id="korg:claude-hook")
    w.append(
        event=_evt({"url": "https://bob:hunter2@example.com"}, {"headers": {"Authorization": f"Bearer {SECRET}"}}),
        actor_id="korg:claude-hook", triggered_by=s1,
    )
    raw = led.read_text()
    assert SECRET not in raw and "hunter2" not in raw
    events = [json.loads(l) for l in raw.splitlines()]
    # the chain is over the redacted bytes, so it verifies as-is
    assert verify_chain(events) == [] and verify_dag(events) == []
    assert verify_redaction(events) == []
    assert events[0]["event"]["args"]["command"] == "export OPENAI_API_KEY=[REDACTED:env_assignment]"
    assert events[1]["event"]["result"]["headers"]["Authorization"] == "[REDACTED:sensitive_key]"
    assert all(e["metadata"]["tags"]["korg.redaction"] == "korg-redact@v1" for e in events)


def test_writer_does_not_mutate_the_callers_event(tmp_path):
    ev = _evt({"api_key": "k"})
    LedgerWriter(tmp_path / "l.jsonl").append(event=ev, actor_id="korg:x")
    assert ev["args"]["api_key"] == "k"


def test_opting_out_is_visible_to_the_lint(tmp_path):
    led = tmp_path / "l.jsonl"
    LedgerWriter(led, redaction=None).append(event=_evt({"command": f"X={SECRET}"}), actor_id="korg:x")
    events = [json.loads(l) for l in led.read_text().splitlines()]
    assert "korg.redaction" not in events[0]["metadata"]["tags"]
    assert verify_redaction(events) == ["seq 1: unredacted provider_api_key at /event/args/command"]


def test_unknown_policy_is_rejected(tmp_path):
    with pytest.raises(ValueError):
        LedgerWriter(tmp_path / "l.jsonl", redaction="korg-redact@v9")


def test_lint_matches_the_conformance_manifest():
    manifest = json.loads((SPEC / "conformance.json").read_text(encoding="utf-8"))
    checked = 0
    for v in manifest["vectors"]:
        if "redaction" not in v:
            continue
        lines = (SPEC / "vectors" / v["file"]).read_text(encoding="utf-8").splitlines()
        events = [json.loads(l) for l in lines if l.strip()]
        want = v.get("redaction_errors", []) if v["redaction"] == "unclean" else []
        assert verify_redaction(events) == want, v["file"]
        checked += 1
    assert checked >= 2


def test_rechain_redacted_migrates_an_unredacted_journal(tmp_path):
    from korg_ledger.__main__ import main as cli
    from korg_ledger.redaction import rechain_redacted

    old = tmp_path / "old.jsonl"
    w = LedgerWriter(old, redaction=None)
    a = w.append(event=_evt({"command": f"X_API_KEY={SECRET}"}), actor_id="korg:x")
    w.append(event=_evt({"ok": 1}), actor_id="korg:x", triggered_by=a)
    events = [json.loads(l) for l in old.read_text().splitlines()]
    assert verify_redaction(events)  # the old journal leaks

    new = rechain_redacted(events)
    assert SECRET not in json.dumps(new)
    assert verify_chain(new) == [] and verify_dag(new) == [] and verify_redaction(new) == []
    assert [e["seq_id"] for e in new] == [1, 2] and new[1]["metadata"]["triggered_by"] == 1
    assert new[-1]["entry_hash"] != events[-1]["entry_hash"]  # a new chain, by design

    flat = rechain_redacted([{"seq_id": 1, "args": {"token": "t"}}])
    assert flat[0]["redaction"] == "korg-redact@v1" and flat[0]["args"]["token"] == "[REDACTED:sensitive_key]"

    out = tmp_path / "new.jsonl"
    assert cli(["rechain-redacted", str(old), str(out)]) == 0
    assert cli(["lint", str(out)]) == 0
    assert cli(["lint", str(old)]) == 1
