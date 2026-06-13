from korg_ledger import Hlc


def test_same_physical_increments_logical():
    clock = Hlc(physical=1000, logical=0, actor_id=1)
    nxt = clock.tick(wall_clock_ms=1000)
    assert (nxt.physical, nxt.logical) == (1000, 1)


def test_advancing_physical_resets_logical():
    clock = Hlc(physical=1000, logical=5, actor_id=1)
    nxt = clock.tick(wall_clock_ms=2000)
    assert (nxt.physical, nxt.logical) == (2000, 0)


def test_clock_never_moves_backward():
    clock = Hlc(physical=5000, logical=3, actor_id=1)
    nxt = clock.tick(wall_clock_ms=1000)  # wall clock behind logical time
    assert nxt.physical == 5000
    assert nxt.logical == 4


def test_as_dict_shape():
    assert Hlc(7, 2, 1).as_dict() == {"physical": 7, "logical": 2, "actor_id": 1}
