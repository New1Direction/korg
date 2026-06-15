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
