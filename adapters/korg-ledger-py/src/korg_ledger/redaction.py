"""korg-redact@v1 — redaction at persistence (stdlib only).

Normative text: spec/korg-ledger-v1/REDACTION.md. Byte-for-byte equivalent to
the Rust ``korg_ledger::redact`` module; both are pinned by the shared cases in
spec/korg-ledger-v1/vectors/redaction/korg-redact-v1.json.

A chained event cannot be redacted after it is hashed (that is tampering), so a
producer MUST run ``redact`` on the record *before* ``chain_hash`` and before
anything touches disk. The policy is deterministic and idempotent:
``redact(redact(x)) == redact(x)``, and a value is redaction-clean iff
``scan(value) == []``.

Two rule families:

* **sensitive keys** — an object member whose normalized key is a credential
  name (``authorization``, ``api_key``, ``client_secret``, ``*_token`` …) has its
  whole value replaced by ``[REDACTED:sensitive_key]`` (null, booleans and "" are
  kept: they carry no secret).
* **value patterns** — every string is scanned for credential shapes (PEM private
  keys, URL userinfo, bearer tokens, provider API keys, JWTs, ``NAME_TOKEN=…``
  assignments, ``"password": "…"`` JSON fields) and the secret part is replaced
  in place by ``[REDACTED:<rule>]``.

Markers never carry any function of the secret (no hash, prefix, or length), so
a persisted ledger gives no oracle for guessing low-entropy secrets.

The patterns deliberately use ASCII classes only (no ``\\s``, ``\\b``, ``\\w`` or
case-insensitive flags): Python ``re`` and the Rust ``regex`` crate disagree on
their Unicode semantics, and a split would make one implementation persist what
the other would redact.
"""
from __future__ import annotations

import re
from typing import Any, Callable

from ._hash import json_int

#: Policy identifier recorded on every redacted event (``metadata.tags`` on a
#: nested JournalEvent, top-level ``redaction`` on a flat record).
POLICY = "korg-redact@v1"

#: Tag key under ``metadata.tags`` that declares the policy a producer applied.
TAG = "korg.redaction"


def marker(rule: str) -> str:
    return f"[REDACTED:{rule}]"


_MARKER_RE = re.compile(r"\[REDACTED:[a-z_]+\]")

#: Exact normalized key names whose values are credentials.
SENSITIVE_KEYS = frozenset(
    {
        "authorization",
        "proxy_authorization",
        "cookie",
        "set_cookie",
        "api_key",
        "apikey",
        "password",
        "passwd",
        "passphrase",
        "secret",
        "token",
        "private_key",
        "secret_key",
        "aws_secret_access_key",
        "credential",
        "credentials",
    }
)

#: Normalized key suffixes whose values are credentials (``openai_api_key``,
#: ``client_secret``, ``access_token``, ``x_amz_security_token`` …). Note
#: ``prompt_tokens`` / ``max_tokens`` do NOT match (plural).
SENSITIVE_KEY_SUFFIXES = (
    "_api_key",
    "_apikey",
    "_secret",
    "_secret_key",
    "_private_key",
    "_password",
    "_passwd",
    "_passphrase",
    "_token",
)


def normalize_key(key: str) -> str:
    """camelCase / kebab-case / spaced → snake_case, ASCII-lowercased.

    ``accessToken`` → ``access_token``, ``X-Api-Key`` → ``x_api_key``,
    ``APIKey`` → ``apikey``. Only ASCII letters change case."""
    out = []
    prev = ""
    for c in key:
        if "A" <= c <= "Z":
            if prev and ("a" <= prev <= "z" or "0" <= prev <= "9"):
                out.append("_")
            out.append(chr(ord(c) + 32))
        elif c in "- ":
            out.append("_")
        else:
            out.append(c)
        prev = c
    return "".join(out)


def is_sensitive_key(key: str) -> bool:
    k = normalize_key(key)
    return k in SENSITIVE_KEYS or k.endswith(SENSITIVE_KEY_SUFFIXES)


# Leading "boundary": start of string or a non-[A-Za-z0-9_] character, captured
# and re-emitted (Rust regex has no look-behind).
_B = r"(^|[^A-Za-z0-9_])"

_SECRET_JSON_KEYS = (
    "password|passwd|passphrase|secret|client_secret|api_key|apikey|"
    "access_token|refresh_token|auth_token|id_token|token|private_key|authorization"
)

#: Ordered (rule_id, compiled pattern, replacement). Order is normative.
_RULES: list[tuple[str, "re.Pattern[str]", Callable[["re.Match[str]"], str]]] = [
    (
        "private_key_block",
        re.compile(
            r"-----BEGIN[A-Z ]*PRIVATE KEY-----"
            r"(?:[\s\S]*?-----END[A-Z ]*PRIVATE KEY-----|[\s\S]*)"
        ),
        lambda m: marker("private_key_block"),
    ),
    (
        "url_credentials",
        re.compile(r"([A-Za-z][A-Za-z0-9+.\-]*://)[^ \t\r\n/:@\[\]]+:[^ \t\r\n/@\[\]]+@"),
        lambda m: m.group(1) + marker("url_credentials") + "@",
    ),
    (
        "bearer_token",
        re.compile(r"((?:Bearer|bearer|BEARER)[ \t]+)[A-Za-z0-9._~+/\-]{16,}=*"),
        lambda m: m.group(1) + marker("bearer_token"),
    ),
    (
        "provider_api_key",
        re.compile(_B + r"sk-[A-Za-z0-9_\-]{20,}"),
        lambda m: m.group(1) + marker("provider_api_key"),
    ),
    (
        "github_token",
        re.compile(_B + r"(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,})"),
        lambda m: m.group(1) + marker("github_token"),
    ),
    (
        "slack_token",
        re.compile(_B + r"xox[abposr]-[A-Za-z0-9\-]{10,}"),
        lambda m: m.group(1) + marker("slack_token"),
    ),
    (
        "aws_access_key_id",
        re.compile(_B + r"(?:AKIA|ASIA)[0-9A-Z]{16}"),
        lambda m: m.group(1) + marker("aws_access_key_id"),
    ),
    (
        "google_api_key",
        re.compile(_B + r"AIza[0-9A-Za-z_\-]{35}"),
        lambda m: m.group(1) + marker("google_api_key"),
    ),
    (
        "jwt",
        re.compile(_B + r"eyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}"),
        lambda m: m.group(1) + marker("jwt"),
    ),
    (
        "env_assignment",
        re.compile(
            _B
            + r"([A-Z][A-Z0-9_]*(?:API_KEY|APIKEY|SECRET|SECRET_KEY|PASSWORD|PASSWD|TOKEN|PRIVATE_KEY|ACCESS_KEY))"
            r"([ \t]*[=:][ \t]*)"
            r"(?:\"[^\"\n]*\"|'[^'\n]*'|[^ \t\r\n\"',;&]+)"
        ),
        lambda m: m.group(1) + m.group(2) + m.group(3) + marker("env_assignment"),
    ),
    (
        "json_secret_field",
        re.compile(r"(\"(?:" + _SECRET_JSON_KEYS + r")\"[ \t]*:[ \t]*)\"(?:[^\"\\\n]|\\.)*\""),
        lambda m: m.group(1) + '"' + marker("json_secret_field") + '"',
    ),
]

#: Rule ids in normative order (also the key-rule id).
RULE_IDS = ("sensitive_key",) + tuple(r[0] for r in _RULES)


#: Upper bound on rule-list passes. Real input reaches its fixed point in one or
#: two passes; hitting the bound leaves the string unclean, which the writer's
#: fail-closed gate (R4) then refuses to persist.
MAX_PASSES = 8


def redact_string(s: str) -> tuple[str, list[str]]:
    """Apply the value-pattern rules, in order, repeatedly until the string stops
    changing (a fixed point — so redaction is idempotent by construction even
    where one replacement exposes a new match, e.g. two keys back to back).
    Returns (new, ids of the rules that fired, in normative order)."""
    fired: set[str] = set()
    for _ in range(MAX_PASSES):
        changed = False
        for rule_id, pat, repl in _RULES:
            new = pat.sub(repl, s)
            if new != s:
                fired.add(rule_id)
                s = new
                changed = True
        if not changed:
            break
    return s, [r for r in RULE_IDS if r in fired]


def _pointer_escape(token: str) -> str:
    return token.replace("~", "~0").replace("/", "~1")


def _redact(value: Any, path: str, findings: list[dict]) -> Any:
    if isinstance(value, str):
        new, fired = redact_string(value)
        for r in fired:
            findings.append({"path": path, "rule": r})
        return new
    if isinstance(value, list):
        return [_redact(v, f"{path}/{i}", findings) for i, v in enumerate(value)]
    if isinstance(value, dict):
        out = {}
        for k, v in value.items():
            p = f"{path}/{_pointer_escape(k)}"
            if is_sensitive_key(k) and not _keeps_under_sensitive_key(v):
                findings.append({"path": p, "rule": "sensitive_key"})
                out[k] = marker("sensitive_key")
            else:
                out[k] = _redact(v, p, findings)
        return out
    return value


def _keeps_under_sensitive_key(v: Any) -> bool:
    # null / booleans / "" carry no secret; an existing marker is already redacted.
    if v is None or isinstance(v, bool) or v == "":
        return True
    return isinstance(v, str) and _MARKER_RE.fullmatch(v) is not None


def redact(value: Any) -> tuple[Any, list[dict]]:
    """Redact a JSON value under korg-redact@v1.

    Returns ``(redacted_copy, findings)`` where each finding is
    ``{"path": <RFC 6901 JSON pointer>, "rule": <rule id>}``. The input is not
    mutated. Object keys are never rewritten (they are structure, not data)."""
    findings: list[dict] = []
    return _redact(value, "", findings), findings


def scan(value: Any) -> list[dict]:
    """Findings that ``redact`` would produce — empty iff ``value`` is clean."""
    return redact(value)[1]


#: Fields that are chain structure, never payload: excluded from redaction and
#: from the cleanliness lint (they are hex digests / integers by construction).
STRUCTURAL_FIELDS = ("prev_hash", "entry_hash", "event_sig")


def finding_order(f: dict) -> tuple:
    """Canonical finding order: JSON pointer (code point), then normative rule
    order. Implementations iterate object members in different orders, so every
    reported list is sorted by this key."""
    return (f["path"], RULE_IDS.index(f["rule"]))


def scan_record(record: dict) -> list[dict]:
    """Lint one persisted ledger record (flat or nested) for unredacted secrets."""
    body = {k: v for k, v in record.items() if k not in STRUCTURAL_FIELDS}
    return scan(body)


def verify_redaction(events: list) -> list[str]:
    """Journal-level lint: one error per finding, localized to ``seq N`` and a
    JSON pointer. ``[]`` iff every event is redaction-clean under korg-redact@v1."""
    errors: list[str] = []
    if not isinstance(events, list):
        return ["events is not a list"]
    for e in events:
        if not isinstance(e, dict):
            continue
        sid = json_int(e.get("seq_id"))
        label = "?" if sid is None else str(sid)
        for f in sorted(scan_record(e), key=finding_order):
            errors.append(f"seq {label}: unredacted {f['rule']} at {f['path'] or '/'}")
    return errors


def rechain_redacted(events: list, key: bytes | None = None) -> list:
    """Migration helper (REDACTION.md R6): a NEW journal holding a redacted copy
    of ``events``, re-chained from genesis.

    Each record body is redacted and declares the policy (``metadata.tags`` on a
    nested record, top-level ``redaction`` on a flat one); ``seq_id`` and causal
    links are preserved; ``prev_hash``/``entry_hash`` are recomputed (HMAC when
    ``key`` is given). ``event_sig`` is dropped — the old signature covered the
    unredacted bytes, so re-sign if you need per-event attribution. The result is
    a different chain: record the old tip next to the new one (and anchor both)
    so the provenance of the copy is auditable."""
    from ._hash import GENESIS, chain_hash  # local: avoid widening module imports

    out: list = []
    prev = GENESIS
    for e in events:
        if not isinstance(e, dict):
            raise ValueError("rechain_redacted: every event must be a JSON object")
        body = {k: v for k, v in e.items() if k not in STRUCTURAL_FIELDS}
        md = body.get("metadata")
        if isinstance(md, dict) and isinstance(body.get("event"), dict):
            tags = md.get("tags") if isinstance(md.get("tags"), dict) else {}
            body["metadata"] = {**md, "tags": {**tags, TAG: POLICY}}
        else:
            body["redaction"] = POLICY
        rec, _ = redact(body)
        rec["prev_hash"] = prev
        rec["entry_hash"] = chain_hash(rec, key)
        prev = rec["entry_hash"]
        out.append(rec)
    return out
