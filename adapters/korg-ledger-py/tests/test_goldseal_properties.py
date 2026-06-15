"""Property-based hardening for the Gold Seal verifiers.

Two security invariants, fuzzed:
  1. The verifier NEVER crashes on hostile input — arbitrary JSON, malformed
     events, wrong types — it returns errors, it does not raise.
  2. The verifier NEVER accepts something it shouldn't — arbitrary junk is never
     valid, and any single-character flip in a hash or the seal signature is
     rejected.

Needs hypothesis + cryptography; skips cleanly without them (CI installs both).
"""
import json
from pathlib import Path

import pytest

pytest.importorskip("hypothesis")
pytest.importorskip("cryptography")

from hypothesis import HealthCheck, given, settings  # noqa: E402
from hypothesis import strategies as st  # noqa: E402

from korg_ledger.goldseal import derive_summary, verify_structure  # noqa: E402
from korg_ledger.signing import verify_seal  # noqa: E402

FIXTURE = Path(__file__).resolve().parents[3] / "crates/korg-verify/tests/fixtures/goldseal-v1.json"

# arbitrary JSON-shaped values (incl. floats/nan/inf, deep nesting, junk keys)
json_values = st.recursive(
    st.none()
    | st.booleans()
    | st.integers(min_value=-(2**60), max_value=2**60)
    | st.floats(allow_nan=True, allow_infinity=True)
    | st.text(max_size=24),
    lambda c: st.lists(c, max_size=6) | st.dictionaries(st.text(max_size=12), c, max_size=6),
    max_leaves=25,
)

_SLOW = [HealthCheck.too_slow]


@given(blob=json_values)
@settings(max_examples=500, deadline=None, suppress_health_check=_SLOW)
def test_verify_structure_never_crashes_and_junk_is_invalid(blob):
    errs = verify_structure(blob)
    assert isinstance(errs, list)
    assert errs != [], "arbitrary input must never verify as a valid structure"


@given(blob=json_values)
@settings(max_examples=400, deadline=None, suppress_health_check=_SLOW)
def test_verify_seal_never_crashes_and_junk_is_invalid(blob):
    errs = verify_seal(blob)
    assert isinstance(errs, list)
    assert errs != [], "arbitrary input must never verify as a valid Gold Seal"


@given(blob=json_values)
@settings(max_examples=300, deadline=None, suppress_health_check=_SLOW)
def test_derive_summary_never_crashes_and_stays_canon_safe(blob):
    events = blob if isinstance(blob, list) else [blob]
    s = derive_summary(events)
    assert set(s) == {"agents", "by_tool", "files", "seq_first", "seq_last"}
    assert isinstance(s["agents"], list) and isinstance(s["files"], list)
    assert isinstance(s["seq_first"], int) and isinstance(s["seq_last"], int)


def _fixture():
    return json.loads(FIXTURE.read_text())


@given(i=st.integers(min_value=0, max_value=63), c=st.sampled_from("0123456789abcdef"))
@settings(max_examples=80, deadline=None)
def test_flipping_any_tip_char_is_rejected(i, c):
    env = _fixture()
    chars = list(env["tip"])
    if chars[i] == c:  # ensure it's actually a change
        c = "f" if c != "f" else "0"
    chars[i] = c
    env["tip"] = "".join(chars)
    assert verify_seal(env) != []


@given(i=st.integers(min_value=0, max_value=127), c=st.sampled_from("0123456789abcdef"))
@settings(max_examples=80, deadline=None)
def test_flipping_any_seal_signature_char_is_rejected(i, c):
    env = _fixture()
    chars = list(env["seal"]["sig"])
    if chars[i] == c:
        c = "f" if c != "f" else "0"
    chars[i] = c
    env["seal"]["sig"] = "".join(chars)
    assert verify_seal(env) != []


@given(data=st.data())
@settings(max_examples=120, deadline=None, suppress_health_check=_SLOW)
def test_replacing_any_event_with_junk_is_rejected(data):
    env = _fixture()
    idx = data.draw(st.integers(min_value=0, max_value=len(env["events"]) - 1))
    env["events"][idx] = data.draw(json_values)
    assert verify_seal(env) != [], "a junk event must always break verification"
