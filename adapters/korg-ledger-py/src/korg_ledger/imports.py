"""korg-import@v1 — import envelopes for evidence korg did not observe itself.

Normative text: spec/korg-ledger-v1/IMPORT.md. Two profiles ship:

* ``okto.diagnostics@v1`` — sanitized diagnostics from the Okto agent-traffic
  proxy (session/turn/anomaly/mutation/replay *metadata*; never headers, bodies,
  raw stream bytes, or anomaly text).
* ``ningen-shikkaku.lifecycle@v1`` — lifecycle metadata from the
  ningen-shikkaku dead-man's switch (armed/trigger/panic/wipe/pid events; never a
  secret value, tool spec, argv, or free-text message).

Safety is by **allowlist**, not by scrubbing: every record kind declares the only
fields it may carry and their types; anything else rejects the WHOLE envelope
before a single event is appended (fail closed, atomic). Strings are restricted
to identifiers and enums — there is no free-text field in either profile — and
the korg-redact@v1 scanner runs over every record as a second gate.

The converters (``okto_session_to_envelope``, ``okto_mutation_record``,
``dazai_log_to_envelope``) build envelopes by *projection*: they read only the
allowlisted fields from the source's native output, so secret-bearing fields are
never even copied.
"""
from __future__ import annotations

import hashlib
import json
import re
from typing import Any, Iterable

from ._events import agent_tool_call_event
from ._hash import canonicalize
from .redaction import scan

SCHEMA = "korg-import@v1"
ACTOR_ID = "korg:import"

_SAFE_INT_MAX = 2**53 - 1
_ID_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:\-]{0,127}")
_SLUG_RE = re.compile(r"[a-z0-9][a-z0-9_.\-]{0,63}")
_VERSION_RE = re.compile(r"[0-9A-Za-z][0-9A-Za-z.+\-]{0,31}")


class ImportRejected(ValueError):
    """The envelope violates korg-import@v1; nothing was appended."""

    def __init__(self, errors: list[str]):
        super().__init__("; ".join(errors))
        self.errors = errors


# ── field types ──────────────────────────────────────────────────────────────


def _nonneg_int(v, _):
    return isinstance(v, int) and not isinstance(v, bool) and 0 <= v <= _SAFE_INT_MAX


def _bool(v, _):
    return isinstance(v, bool)


def _id(v, _):
    return isinstance(v, str) and _ID_RE.fullmatch(v) is not None


def _slug(v, _):
    return isinstance(v, str) and _SLUG_RE.fullmatch(v) is not None


def _enum(*allowed):
    def check(v, _):
        return isinstance(v, str) and v in allowed

    check.allowed = allowed  # type: ignore[attr-defined]
    return check


def _int_range(lo, hi):
    def check(v, _):
        return isinstance(v, int) and not isinstance(v, bool) and lo <= v <= hi

    return check


def _slug_count_map(v, _):
    return (
        isinstance(v, dict)
        and len(v) <= 16
        and all(_slug(k, None) and _nonneg_int(n, None) for k, n in v.items())
    )


# ── profiles ─────────────────────────────────────────────────────────────────
# kind → (required fields, optional fields); each field → type check.

_OKTO_CLIENTS = _enum(
    "claude", "gemini", "codex", "agy", "kimi", "kimi-code", "cursor", "opencode", "pi", "hermes", "unknown"
)
_OKTO_ANOMALY = _enum(
    "bare_json_without_data_prefix",
    "malformed_json_in_sse_frame",
    "empty_response_body",
    "line_ending_mismatch",
    "protocol_anomaly",
    "other",
)

OKTO_PROFILE = "okto.diagnostics@v1"
NINGEN_PROFILE = "ningen-shikkaku.lifecycle@v1"

_AT = {"at_ms": _nonneg_int}

PROFILES: dict[str, dict[str, Any]] = {
    OKTO_PROFILE: {
        "source": "okto",
        "kinds": {
            "session": (
                {"session_id": _id, "client": _OKTO_CLIENTS},
                {**_AT, "tls_established": _bool, "synthetic": _bool, "provider_vendor": _slug,
                 "provider_product": _slug, "turn_count": _nonneg_int, "anomaly_count": _nonneg_int,
                 "mutation_count": _nonneg_int},
            ),
            "turn": (
                {"session_id": _id, "turn_index": _nonneg_int},
                {**_AT, "status_code": _int_range(100, 599), "duration_ms": _nonneg_int,
                 "chunk_count": _nonneg_int, "anomaly_count": _nonneg_int,
                 "mutations_applied": _nonneg_int, "token_usage": _slug_count_map},
            ),
            "anomaly": (
                {"session_id": _id, "turn_index": _nonneg_int, "chunk_index": _nonneg_int,
                 "anomaly_type": _OKTO_ANOMALY},
                {**_AT, "repaired": _bool},
            ),
            "mutation": (
                {"mutation_id": _id, "scope": _enum("request_header", "request_body", "response_header",
                                                      "response_body", "stream_chunk"),
                 "patch_type": _enum("literal", "regex_sub", "json_patch", "header_set")},
                {**_AT, "session_id": _id, "client": _OKTO_CLIENTS, "confidence_bp": _int_range(0, 10000),
                 "reinforcement_count": _nonneg_int},
            ),
            "replay": (
                {"session_id": _id, "resolved": _bool},
                {**_AT, "total_anomalies": _nonneg_int, "total_mutations_applied": _nonneg_int},
            ),
        },
        # Named so a rejection explains *why* (these carry secrets or raw traffic).
        "forbidden": (
            "request_headers", "response_headers", "headers", "request_body", "response_body", "body",
            "raw_hex", "expected_hex", "patched_hex", "pattern_hex", "patch_hex", "description",
            "parsed", "chunks", "stream_chunks", "path", "url", "connect_target", "hostname", "node_id",
        ),
    },
    NINGEN_PROFILE: {
        "source": "ningen-shikkaku",
        "kinds": {
            "daemon_start": (
                {"mode": _enum("armed", "dry_run")},
                {**_AT, "pid": _nonneg_int, "grace_ms": _nonneg_int, "ping_timeout_ms": _nonneg_int,
                 "seccomp": _bool},
            ),
            "buffers": ({"allocated": _nonneg_int}, {**_AT, "locked": _nonneg_int}),
            "status": (
                {"alive": _bool, "armed": _bool},
                {**_AT, "grace_seconds": _nonneg_int, "registered_pids": _nonneg_int},
            ),
            "armed": ({"via": _enum("flag", "control", "mcp")}, dict(_AT)),
            "trigger": (
                {"reason": _enum("heartbeat_dropped", "ping_deadline_missed", "sigusr1", "sigusr2",
                                 "grace_expired", "other"),
                 "mode": _enum("armed", "dry_run")},
                {**_AT, "severity": _enum("graceful", "hard")},
            ),
            "cancelled": ({"reason": _enum("client_reconnected", "client_cancel", "other")}, dict(_AT)),
            "panic": (
                {"reason": _enum("heartbeat_dropped", "ping_deadline_missed", "sigusr1", "sigusr2",
                                 "grace_expired", "other")},
                dict(_AT),
            ),
            "wiped": ({"buffers": _nonneg_int}, dict(_AT)),
            "pid_registered": ({"pid": _nonneg_int}, {**_AT, "total": _nonneg_int}),
            "pid_unregistered": ({"pid": _nonneg_int}, dict(_AT)),
            "pid_killed": ({"pid": _nonneg_int, "outcome": _enum("sent", "already_gone")}, dict(_AT)),
            "shutdown": ({"how": _enum("quit", "signal")}, dict(_AT)),
            "exit_condition_fired": (
                {"counter_mode": _enum("calls", "session", "either")},
                {**_AT, "calls": _nonneg_int},
            ),
        },
        "forbidden": (
            "value", "secret", "secret_value", "plaintext", "data", "bytes", "cmd", "command", "spec",
            "tool_spec", "stdout", "stderr", "output", "argv", "args", "env", "message", "reply",
            "buffer_address", "address",
        ),
    },
}

_ENVELOPE_FIELDS = {"schema", "profile", "source", "records", "exported_at_ms", "dropped"}


def validate_envelope(env: Any) -> list[str]:
    """All korg-import@v1 violations in ``env`` (``[]`` iff importable)."""
    if not isinstance(env, dict):
        return ["envelope is not a JSON object"]
    errors: list[str] = []
    for k in sorted(set(env) - _ENVELOPE_FIELDS):
        errors.append(f"envelope: unknown field {k!r}")
    if env.get("schema") != SCHEMA:
        errors.append(f"envelope: schema must be {SCHEMA!r}")
    profile = PROFILES.get(env.get("profile"))
    if profile is None:
        errors.append(f"envelope: unknown profile (known: {sorted(PROFILES)})")
        return errors
    src = env.get("source")
    if not (isinstance(src, dict) and set(src) == {"name", "version"}):
        errors.append("envelope: source must be exactly {name, version}")
    else:
        if src["name"] != profile["source"]:
            errors.append(f"envelope: source.name must be {profile['source']!r} for {env['profile']}")
        if not (isinstance(src["version"], str) and _VERSION_RE.fullmatch(src["version"])):
            errors.append("envelope: source.version must match [0-9A-Za-z][0-9A-Za-z.+-]{0,31}")
    for k in ("exported_at_ms", "dropped"):
        if k in env and not _nonneg_int(env[k], None):
            errors.append(f"envelope: {k} must be a non-negative integer")
    records = env.get("records")
    if not isinstance(records, list) or not records:
        errors.append("envelope: records must be a non-empty array")
        return errors
    if len(records) > 10_000:
        errors.append("envelope: more than 10000 records")
        return errors
    kinds, forbidden = profile["kinds"], profile["forbidden"]
    for i, rec in enumerate(records):
        where = f"records[{i}]"
        if not isinstance(rec, dict):
            errors.append(f"{where}: not an object")
            continue
        kind = rec.get("kind")
        if kind not in kinds:
            errors.append(f"{where}: unknown kind (known: {sorted(kinds)})")
            continue
        required, optional = kinds[kind]
        for f in sorted(set(rec) - {"kind"}):
            if f in forbidden:
                errors.append(f"{where}: forbidden field {f!r} (may carry secrets or raw traffic)")
            elif f not in required and f not in optional:
                errors.append(f"{where}: field {f!r} is not allowed for kind {kind!r}")
            elif not (required.get(f) or optional[f])(rec[f], f):
                allowed = getattr(required.get(f) or optional[f], "allowed", None)
                hint = f" (one of {list(allowed)})" if allowed else ""
                errors.append(f"{where}: field {f!r} has an invalid value{hint}")
        for f in sorted(set(required) - set(rec)):
            errors.append(f"{where}: missing required field {f!r}")
        for finding in scan(rec):
            errors.append(f"{where}: secret-shaped value at {finding['path']} ({finding['rule']})")
    return errors


def envelope_digest(env: dict) -> str:
    """sha256 over the canonical envelope bytes — lets a holder of the original
    sanitized envelope prove it is exactly what was imported."""
    return hashlib.sha256(canonicalize(env)).hexdigest()


def import_envelope(writer, env: dict, *, triggered_by: int | None = None, actor_id: str = ACTOR_ID) -> list[int]:
    """Validate, then append one ``korg.import`` batch event plus one event per
    record (each ``triggered_by`` the batch). Returns the appended seq_ids.

    Atomic with respect to validation: an invalid envelope raises
    ``ImportRejected`` before anything is written."""
    errors = validate_envelope(env)
    if errors:
        raise ImportRejected(errors)
    src = env["source"]
    agent = f"import:{src['name']}@{src['version']}"
    digest = envelope_digest(env)
    batch_args = {
        "profile": env["profile"],
        "record_count": len(env["records"]),
        "envelope_sha256": digest,
    }
    for k in ("exported_at_ms", "dropped"):
        if k in env:
            batch_args[k] = env[k]
    seqs = [
        writer.append(
            event=agent_tool_call_event(
                source_agent=agent, tool_name="korg.import", args=batch_args, result={}, success=True, duration_ms=0
            ),
            actor_id=actor_id,
            triggered_by=triggered_by,
        )
    ]
    for i, rec in enumerate(env["records"]):
        args = {k: v for k, v in rec.items() if k != "kind"}
        seqs.append(
            writer.append(
                event=agent_tool_call_event(
                    source_agent=agent,
                    tool_name=f"{src['name']}.{rec['kind']}",
                    args=args,
                    result={"import_index": i},
                    success=True,
                    duration_ms=0,
                ),
                actor_id=actor_id,
                triggered_by=seqs[0],
            )
        )
    return seqs


# ── converters: native sibling output → envelope (projection only) ──────────


def _slugify(s: Any) -> str | None:
    if not isinstance(s, str):
        return None
    out = re.sub(r"[^a-z0-9_.\-]+", "-", s.lower()).strip("-")[:64]
    return out if out and _SLUG_RE.fullmatch(out) else None


def _ms(v: Any) -> int | None:
    if isinstance(v, bool) or not isinstance(v, (int, float)) or v != v or v < 0:
        return None
    n = int(round(v))
    return n if n <= _SAFE_INT_MAX else None


def okto_session_to_envelope(session: dict, *, okto_version: str) -> dict:
    """Project an Okto ``Session`` (``~/.okto/sessions/<id>.jsonl``) onto
    ``okto.diagnostics@v1``. Reads ONLY: id, client, started_at_hlc.wall_ms,
    tls_established, synthetic, provider.{vendor,product}, and per turn: index,
    response_status, duration_ms, token_usage, anomalies_detected,
    mutations_applied (counted), and each stream chunk's index/anomalous/
    anomaly_type/applied_mutation_id. Headers, bodies, raw_hex, parsed chunks,
    connect_target and hostname are never read."""
    sid = session.get("id")
    client = session.get("client") if session.get("client") in _OKTO_CLIENTS.allowed else "unknown"
    rec: dict[str, Any] = {"kind": "session", "session_id": sid, "client": client}
    started = _ms((session.get("started_at_hlc") or {}).get("wall_ms"))
    if started is not None:
        rec["at_ms"] = started
    for k in ("tls_established", "synthetic"):
        if isinstance(session.get(k), bool):
            rec[k] = session[k]
    provider = session.get("provider") or {}
    for src_k, dst_k in (("vendor", "provider_vendor"), ("product", "provider_product")):
        slug = _slugify(provider.get(src_k)) if isinstance(provider, dict) else None
        if slug:
            rec[dst_k] = slug
    records = [rec]
    dropped = 0
    turns = session.get("turns") or []
    total_anomalies = total_mutations = 0
    for t in turns:
        if not isinstance(t, dict):
            dropped += 1
            continue
        idx = t.get("index")
        chunks = t.get("stream_chunks") or []
        mutations = t.get("mutations_applied") or []
        turn = {"kind": "turn", "session_id": sid, "turn_index": idx, "chunk_count": len(chunks),
                "mutations_applied": len(mutations) if isinstance(mutations, list) else 0}
        status = t.get("response_status")
        if isinstance(status, int) and not isinstance(status, bool) and 100 <= status <= 599:
            turn["status_code"] = status
        dur = _ms(t.get("duration_ms"))
        if dur is not None:
            turn["duration_ms"] = dur
        usage = t.get("token_usage")
        if isinstance(usage, dict):
            clean = {_slugify(k): v for k, v in usage.items() if _slugify(k) and _nonneg_int(v, None)}
            if clean:
                turn["token_usage"] = dict(list(sorted(clean.items()))[:16])
        anomalies = 0
        chunk_records = []
        for c in chunks:
            if isinstance(c, dict) and c.get("anomalous") is True:
                anomalies += 1
                atype = c.get("anomaly_type")
                chunk_records.append({
                    "kind": "anomaly", "session_id": sid, "turn_index": idx, "chunk_index": c.get("index"),
                    "anomaly_type": atype if atype in _OKTO_ANOMALY.allowed else "other",
                    "repaired": c.get("applied_mutation_id") is not None,
                })
        turn["anomaly_count"] = anomalies
        total_anomalies += anomalies
        total_mutations += turn["mutations_applied"]
        records.append(turn)
        records.extend(chunk_records)
    rec["turn_count"] = len(turns)
    rec["anomaly_count"] = total_anomalies
    rec["mutation_count"] = total_mutations
    env = {"schema": SCHEMA, "profile": OKTO_PROFILE, "source": {"name": "okto", "version": okto_version},
           "records": records}
    if dropped:
        env["dropped"] = dropped
    return env


def okto_mutation_record(mutation: dict) -> dict:
    """Project one Okto genome ``Mutation`` (``~/.okto/genome.jsonl``) onto a
    ``mutation`` record. ``pattern_hex``, ``patch_hex`` and ``description`` (which
    embed real traffic bytes) are never read; ``confidence`` becomes integer
    basis points because ledger numbers are integers."""
    rec: dict[str, Any] = {
        "kind": "mutation",
        "mutation_id": mutation.get("id"),
        "scope": mutation.get("scope"),
        "patch_type": mutation.get("patch_type"),
    }
    at = _ms((mutation.get("hlc") or {}).get("wall_ms"))
    if at is not None:
        rec["at_ms"] = at
    if isinstance(mutation.get("session_id"), str):
        rec["session_id"] = mutation["session_id"]
    if mutation.get("client") in _OKTO_CLIENTS.allowed:
        rec["client"] = mutation["client"]
    conf = mutation.get("confidence")
    if isinstance(conf, (int, float)) and not isinstance(conf, bool) and 0 <= conf <= 1:
        rec["confidence_bp"] = int(round(conf * 10000))
    rc = mutation.get("reinforcement_count")
    if _nonneg_int(rc, None):
        rec["reinforcement_count"] = rc
    return rec


_DAZAI_REASONS = {
    "client reconnected": "client_reconnected",
    "client CANCEL": "client_cancel",
    "heartbeat connection dropped": "heartbeat_dropped",
    "ping deadline missed": "ping_deadline_missed",
    "SIGUSR1 panic signal": "sigusr1",
    "SIGUSR2 hard panic": "sigusr2",
    "grace window expired": "grace_expired",
}

_TRIGGER_REASONS = {"heartbeat_dropped", "ping_deadline_missed", "sigusr1", "sigusr2", "grace_expired"}


def _reason(text: str, allowed: set[str]) -> str:
    r = _DAZAI_REASONS.get(text.strip(), "other")
    return r if r in allowed else "other"


# (pattern, builder) — matched with re.search so both the Rust "[dazai] …" and
# the Python reference "HH:MM:SS [dazai.deadman] …" log formats are accepted.
_DAZAI_LINES: list[tuple[re.Pattern[str], Any]] = [
    (re.compile(r"watchdog pid=(\d+) mode=(ARMED|DRY-RUN)"),
     lambda m: {"kind": "daemon_start", "pid": int(m[1]), "mode": "armed" if m[2] == "ARMED" else "dry_run"}),
    (re.compile(r"allocated (\d+) secret buffer\(s\), (\d+) mlock'd"),
     lambda m: {"kind": "buffers", "allocated": int(m[1]), "locked": int(m[2])}),
    (re.compile(r"ARMED at runtime via control message"), lambda m: {"kind": "armed", "via": "control"}),
    (re.compile(r"DRY-RUN (HARD )?trigger \((.+?)\)"),
     lambda m: {"kind": "trigger", "mode": "dry_run", "severity": "hard" if m[1] else "graceful",
                "reason": _reason(m[2], _TRIGGER_REASONS)}),
    (re.compile(r"ARMED trigger \((.+?)\)"),
     lambda m: {"kind": "trigger", "mode": "armed", "severity": "graceful",
                "reason": _reason(m[1], _TRIGGER_REASONS)}),
    (re.compile(r"panic CANCELLED \((.+?)\)"),
     lambda m: {"kind": "cancelled", "reason": _reason(m[1], {"client_reconnected", "client_cancel"})}),
    (re.compile(r"PANIC \((.+?)\)"), lambda m: {"kind": "panic", "reason": _reason(m[1], _TRIGGER_REASONS)}),
    (re.compile(r"SIGKILL registered pid (\d+): (sent|already gone)"),
     lambda m: {"kind": "pid_killed", "pid": int(m[1]), "outcome": m[2].replace(" ", "_")}),
    (re.compile(r"wiped (\d+) secret buffer\(s\)"), lambda m: {"kind": "wiped", "buffers": int(m[1])}),
    (re.compile(r"zeroized (\d+) buffer\(s\)"), lambda m: {"kind": "wiped", "buffers": int(m[1])}),
    (re.compile(r"registered pid (\d+) \((\d+) total\)"),
     lambda m: {"kind": "pid_registered", "pid": int(m[1]), "total": int(m[2])}),
    (re.compile(r"client QUIT: intentional clean stand-down"), lambda m: {"kind": "shutdown", "how": "quit"}),
    (re.compile(r"SIGTERM/SIGINT: clean shutdown"), lambda m: {"kind": "shutdown", "how": "signal"}),
]


def dazai_log_to_envelope(lines: Iterable[str], *, version: str) -> dict:
    """Project ningen-shikkaku daemon stderr (``[dazai] …`` lines) onto
    ``ningen-shikkaku.lifecycle@v1``. Only the fixed lifecycle lines are
    recognised; their numeric/enum parts are extracted and everything else on the
    line is discarded. Unrecognised lines (including any ``error: …`` text, which
    can echo a tool spec) are dropped and counted in ``dropped``."""
    records: list[dict] = []
    dropped = 0
    for line in lines:
        if not line.strip():
            continue
        for pat, build in _DAZAI_LINES:
            m = pat.search(line)
            if m:
                records.append(build(m))
                break
        else:
            dropped += 1
    env = {"schema": SCHEMA, "profile": NINGEN_PROFILE,
           "source": {"name": "ningen-shikkaku", "version": version}, "records": records}
    if dropped:
        env["dropped"] = dropped
    return env


def load_envelope(text: str) -> dict:
    return json.loads(text)
