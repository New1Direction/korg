from korg_ledger import GENESIS, chain_hash, verify_anchors


def _chain(n):
    out, prev = [], GENESIS
    for i in range(1, n + 1):
        ev = {"seq_id": i, "prev_hash": prev, "payload": {"i": i}}
        ev["entry_hash"] = chain_hash(ev)
        prev = ev["entry_hash"]
        out.append(ev)
    return out


def test_verify_anchors_accepts_correct_anchor():
    chain = _chain(3)
    anchors = [{"seq_id": 3, "entry_hash": chain[2]["entry_hash"], "anchor_kind": "git-tip"}]
    assert verify_anchors(chain, anchors) == []


def test_verify_anchors_flags_wrong_entry_hash():
    chain = _chain(3)
    errs = verify_anchors(chain, [{"seq_id": 3, "entry_hash": "deadbeef"}])
    assert any("seq 3" in e for e in errs)


def test_verify_anchors_flags_missing_seq():
    chain = _chain(3)
    errs = verify_anchors(chain, [{"seq_id": 99, "entry_hash": chain[2]["entry_hash"]}])
    assert errs


def test_verify_anchors_flags_malformed_record():
    chain = _chain(2)
    assert verify_anchors(chain, [{"seq_id": 1}]) != []


def test_verify_anchors_matches_negative_and_zero_seq_ids():
    # Cross-language parity regression: seq_id is a signed integer, so a chain
    # with a negative or zero seq_id is a fully valid, in-domain artifact. Python
    # matches anchors by raw integer equality and accepts these; the Rust verifier
    # must too (it previously used as_u64, silently rejecting negatives — a
    # same-bytes verdict split). Confirm both verdicts here on this side.
    for sid in (-5, 0):
        ev = {"seq_id": sid, "prev_hash": GENESIS, "x": 1}
        ev["entry_hash"] = chain_hash(ev)
        chain = [ev]
        good = [{"seq_id": sid, "entry_hash": ev["entry_hash"], "anchor_kind": "git-tip"}]
        assert verify_anchors(chain, good) == []
        assert verify_anchors(chain, [{"seq_id": sid, "entry_hash": "deadbeef"}]) != []
