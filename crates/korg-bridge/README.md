# korg-bridge

In-process Python ↔ Korg WAL bridge.

`korg_bridge.Bridge` is a thin PyO3 wrapper around `korg_registry::CapabilityJournal`.
It lets Python agent runtimes (`korgex`, future KorgChat) write AgentToolCall
events directly to the same journal.json that the korg HTTP server reads —
without the HTTP round-trip.

## Tamper-evident by construction (korg-ledger@v1)

Because `CapabilityJournal::append` now hash-chains every event
(`prev_hash`/`entry_hash`, see `korg_registry::ledger_chain`), **anything that
writes through this bridge inherits a tamper-evident, cross-verifiable journal
for free** — no per-app crypto. A journal written by the Rust bridge verifies
byte-for-byte under both `korg_registry::ledger_chain::verify_chain` (Rust) and
korgex's `src/ledger_spec.verify_chain` (Python): the two ledger paths
(korgchat-via-bridge, korgex) collapse onto one chained substrate.

Build the wheel and verify:

```bash
maturin build -m crates/korg-bridge/Cargo.toml          # → target/wheels/korg_bridge-*.whl
pip install --force-reinstall --no-deps target/wheels/korg_bridge-*.whl
pytest crates/korg-bridge/tests/test_bridge_chain.py    # recomputes the chain from stdlib; must match
```

## Why

Before v0.3.0, `korgex/src/korg_ledger.py` POSTed every event to a running
korg server over HTTP. That works, but it requires the server to be live and
adds ~milliseconds of latency per event.

`korg_bridge` cuts the HTTP path: Python calls a Rust function, the Rust side
appends to the journal under the same file lock the server uses, and returns
the assigned `seq_id`. The on-disk format is identical, so a server can be
launched against the same journal afterwards for browsing / MCP serving.

## Build

```bash
cd crates/korg-bridge
python3 -m maturin develop
```

## Quick example

```python
from korg_bridge import Bridge

bridge = Bridge(
    journal_path=".korg/journal.json",
    snapshot_path=".korg/snapshot.json",
    lock_path=".korg/journal.lock",
)
root = bridge.record_user_prompt("add a /healthz endpoint")
seq = bridge.record_tool_call(
    source_agent="agent:korgex@0.3.0",
    tool_name="Edit",
    args={"file_path": "src/routes.py"},
    result={"success": True},
    success=True,
    duration_ms=142,
    triggered_by=root,
)
```
