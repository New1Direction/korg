"""korg-import@v1: Okto diagnostics + ningen-shikkaku lifecycle envelopes."""
import json
from pathlib import Path

import pytest

from korg_ledger import LedgerWriter, verify_chain, verify_dag, verify_redaction
from korg_ledger.__main__ import main as cli
from korg_ledger.imports import (
    ImportRejected,
    dazai_log_to_envelope,
    envelope_digest,
    import_envelope,
    okto_mutation_record,
    okto_session_to_envelope,
    validate_envelope,
)

IMPORTS = Path(__file__).resolve().parents[3] / "spec" / "korg-ledger-v1" / "imports"
SECRET = "sk-ant-api03-REALSECRETREALSECRET0000"


def _load(name):
    return json.loads((IMPORTS / name).read_text(encoding="utf-8"))


def _events(path):
    return [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]


@pytest.mark.parametrize("name", ["okto-diagnostics.example.json", "ningen-shikkaku-lifecycle.example.json"])
def test_spec_examples_import_into_a_verifiable_ledger(tmp_path, name):
    env = _load(name)
    assert validate_envelope(env) == []
    led = tmp_path / "l.jsonl"
    w = LedgerWriter(led)
    root = w.append(event={"event_type": "AgentToolCall", "tool_name": "user_prompt"}, actor_id="korg:x")
    seqs = import_envelope(w, env, triggered_by=root)
    events = _events(led)
    assert seqs == list(range(2, 2 + 1 + len(env["records"])))
    assert verify_chain(events) == [] and verify_dag(events) == [] and verify_redaction(events) == []
    batch, first = events[1], events[2]
    assert batch["event"]["tool_name"] == "korg.import"
    assert batch["event"]["args"]["envelope_sha256"] == envelope_digest(env)
    assert batch["metadata"]["triggered_by"] == root
    src = env["source"]
    assert first["event"]["source_agent"] == f"import:{src['name']}@{src['version']}"
    assert first["event"]["tool_name"] == f"{src['name']}.{env['records'][0]['kind']}"
    assert all(e["metadata"]["triggered_by"] == seqs[0] for e in events[2:])


@pytest.mark.parametrize(
    "name, expected",
    [
        ("okto-diagnostics.rejected.json", [
            "records[0]: forbidden field 'request_headers' (may carry secrets or raw traffic)",
            "records[1]: forbidden field 'description' (may carry secrets or raw traffic)",
        ]),
        ("ningen-shikkaku-lifecycle.rejected.json", [
            "records[0]: forbidden field 'value' (may carry secrets or raw traffic)",
            "records[1]: field 'reason' has an invalid value",
        ]),
    ],
)
def test_spec_rejections_are_atomic(tmp_path, name, expected):
    env = _load(name)
    errors = validate_envelope(env)
    for want in expected:
        assert any(e.startswith(want) for e in errors), (want, errors)
    led = tmp_path / "l.jsonl"
    with pytest.raises(ImportRejected):
        import_envelope(LedgerWriter(led), env)
    assert led.read_text() == ""  # nothing appended, not even the batch event


def test_errors_never_echo_values():
    env = _load("okto-diagnostics.example.json")
    env["records"][0]["client"] = SECRET
    env["records"][1]["kind"] = SECRET
    env["profile"] = "okto.diagnostics@v1"
    assert not any(SECRET in e for e in validate_envelope(env))


@pytest.mark.parametrize(
    "mutate, needle",
    [
        (lambda e: e.update(schema="korg-import@v2"), "schema must be"),
        (lambda e: e.update(profile="okto.raw@v1"), "unknown profile"),
        (lambda e: e["source"].update(name="ningen-shikkaku"), "source.name must be 'okto'"),
        (lambda e: e["source"].update(extra=1), "source must be exactly"),
        (lambda e: e.update(records=[]), "non-empty"),
        (lambda e: e.update(note="hi"), "unknown field 'note'"),
        (lambda e: e["records"][1].update(duration_ms=1840.5), "'duration_ms' has an invalid value"),
        (lambda e: e["records"][1].update(status_code=True), "'status_code' has an invalid value"),
        (lambda e: e["records"][0].pop("client"), "missing required field 'client'"),
        (lambda e: e["records"][0].update(session_id="has spaces/and slashes"), "'session_id' has an invalid value"),
        (lambda e: e["records"][3].update(mutation_id=SECRET), "secret-shaped value at /mutation_id"),
    ],
)
def test_allowlist_rejections(mutate, needle):
    env = _load("okto-diagnostics.example.json")
    mutate(env)
    errors = validate_envelope(env)
    assert any(needle in e for e in errors), errors


def _okto_session():
    """Shaped like okto's selfhswarm.models.Session — full of secret-bearing fields."""
    return {
        "id": "3f2b9c1e-7a44-4d1b-9a0e-5c6d7e8f9a01",
        "client": "claude",
        "node_id": "node-1",
        "started_at_hlc": {"wall_ms": 1781395200000, "counter": 0, "node_id": "node-1"},
        "connect_target": "internal-gateway.corp:443",
        "tls_established": True,
        "provider": {"vendor": "Anthropic", "product": "Messages API", "hostname": "api.anthropic.com"},
        "turns": [{
            "index": 0,
            "request_headers": {"authorization": f"Bearer {SECRET}", "x-api-key": SECRET},
            "request_body": {"messages": [{"role": "user", "content": f"my key is {SECRET}"}]},
            "response_headers": {"set-cookie": "session=abc"},
            "response_status": 200,
            "stream_chunks": [
                {"index": 0, "raw_hex": SECRET.encode().hex(), "parsed": {"t": SECRET}, "anomalous": False},
                {"index": 1, "raw_hex": "7b", "anomalous": True, "anomaly_type": "malformed_json_in_sse_frame",
                 "applied_mutation_id": "m-1", "expected_hex": SECRET.encode().hex()},
                {"index": 2, "raw_hex": "", "anomalous": True, "anomaly_type": "brand_new_label"},
            ],
            "token_usage": {"input_tokens": 12, "output_tokens": 3},
            "duration_ms": 1840.6,
            "anomalies_detected": 2,
            "mutations_applied": ["m-1"],
        }],
    }


def test_okto_session_projection_never_copies_secret_bearing_fields(tmp_path):
    env = okto_session_to_envelope(_okto_session(), okto_version="0.1.0")
    blob = json.dumps(env)
    for leaked in (SECRET, SECRET.encode().hex(), "internal-gateway", "api.anthropic.com", "session=abc"):
        assert leaked not in blob
    assert validate_envelope(env) == []
    kinds = [r["kind"] for r in env["records"]]
    assert kinds == ["session", "turn", "anomaly", "anomaly"]
    session, turn, a1, a2 = env["records"]
    assert session["provider_vendor"] == "anthropic" and session["provider_product"] == "messages-api"
    assert (session["turn_count"], session["anomaly_count"], session["mutation_count"]) == (1, 2, 1)
    assert turn["duration_ms"] == 1841 and turn["status_code"] == 200 and turn["chunk_count"] == 3
    assert (a1["chunk_index"], a1["repaired"], a2["anomaly_type"], a2["repaired"]) == (1, True, "other", False)
    led = tmp_path / "l.jsonl"
    import_envelope(LedgerWriter(led), env)
    assert SECRET not in led.read_text()


def test_okto_mutation_projection_drops_traffic_bytes():
    rec = okto_mutation_record({
        "id": "9d8c7b6a-5e4f-4a3b-8c2d-1e0f9a8b7c6d", "hlc": {"wall_ms": 5, "counter": 0, "node_id": "n"},
        "client": "codex", "scope": "stream_chunk", "pattern_hex": SECRET.encode().hex(),
        "patch_hex": "00", "patch_type": "literal", "description": f"JSON parse failure: {SECRET}",
        "confidence": 0.92345, "source_node": "n", "session_id": "s-1", "reinforcement_count": 2,
    })
    assert rec == {"kind": "mutation", "mutation_id": "9d8c7b6a-5e4f-4a3b-8c2d-1e0f9a8b7c6d", "scope": "stream_chunk",
                   "patch_type": "literal", "at_ms": 5, "session_id": "s-1", "client": "codex",
                   "confidence_bp": 9234, "reinforcement_count": 2}


DAZAI_LOG = f"""\
[dazai] watchdog pid=4242 mode=ARMED — real wipe + SIGKILL on trigger
[dazai] socket=/run/user/1000/dazai.sock
[dazai] allocated 2 secret buffer(s), 2 mlock'd
[dazai] registered pid 4300 (1 total)
[dazai] ARMED trigger (heartbeat connection dropped): SIGKILL in 5s unless a client reconnects or sends CANCEL
[dazai] panic CANCELLED (client reconnected)
[dazai] DRY-RUN HARD trigger (SIGUSR2 hard panic): zeroizing buffers; WOULD SIGKILL registered PIDs and self
[dazai] PANIC (grace window expired): SIGKILL registered PIDs, zeroize buffers, then SIGKILL self
[dazai] SIGKILL registered pid 4300: already gone
[dazai] wiped 2 secret buffer(s)
motokano: parsing --tool "name=key,kind=static,value={SECRET}"
12:00:01 [dazai.deadman] DRY-RUN: zeroized 2 buffer(s); standing down (no kill)
[dazai] client QUIT: intentional clean stand-down
"""


def test_dazai_log_projection_extracts_lifecycle_and_drops_everything_else(tmp_path):
    env = dazai_log_to_envelope(DAZAI_LOG.splitlines(), version="0.1.0")
    assert SECRET not in json.dumps(env) and "/run/user" not in json.dumps(env)
    assert env["dropped"] == 2  # the socket line and the motokano error that echoes a tool spec
    assert env["records"] == [
        {"kind": "daemon_start", "pid": 4242, "mode": "armed"},
        {"kind": "buffers", "allocated": 2, "locked": 2},
        {"kind": "pid_registered", "pid": 4300, "total": 1},
        {"kind": "trigger", "mode": "armed", "severity": "graceful", "reason": "heartbeat_dropped"},
        {"kind": "cancelled", "reason": "client_reconnected"},
        {"kind": "trigger", "mode": "dry_run", "severity": "hard", "reason": "sigusr2"},
        {"kind": "panic", "reason": "grace_expired"},
        {"kind": "pid_killed", "pid": 4300, "outcome": "already_gone"},
        {"kind": "wiped", "buffers": 2},
        {"kind": "wiped", "buffers": 2},
        {"kind": "shutdown", "how": "quit"},
    ]
    assert validate_envelope(env) == []


def test_cli_round_trip(tmp_path, capsys):
    log = tmp_path / "daemon.log"
    log.write_text(DAZAI_LOG)
    assert cli(["envelope", "dazai-log", str(log), "--source-version", "0.1.0"]) == 0
    env_path = tmp_path / "env.json"
    env_path.write_text(capsys.readouterr().out)
    led = tmp_path / "ledger.jsonl"
    assert cli(["import", str(env_path), "--ledger", str(led)]) == 0
    assert cli(["lint", str(led)]) == 0
    bad = tmp_path / "bad.json"
    bad.write_text((IMPORTS / "ningen-shikkaku-lifecycle.rejected.json").read_text())
    before = led.read_text()
    assert cli(["import", str(bad), "--ledger", str(led)]) == 1
    assert led.read_text() == before
