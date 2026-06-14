"""korg-ledger@v1 — pure-Python conformant producer + verifier.

Third independent implementation of the frozen korg-ledger@v1 spec
(alongside the Rust `korg-ledger` crate and the JS `verify.mjs`). Produces
JournalEvent JSONL that the Rust `korg-verify` validates.
"""
from __future__ import annotations

from ._events import NIL_UUID, agent_tool_call_event
from ._hash import (
    GENESIS,
    HASH_FIELDS,
    canonicalize,
    chain_hash,
    verify_anchors,
    verify_chain,
    verify_dag,
)
from ._hlc import Hlc
from .writer import CausalityError, LedgerWriter

__all__ = [
    "GENESIS",
    "HASH_FIELDS",
    "NIL_UUID",
    "Hlc",
    "LedgerWriter",
    "CausalityError",
    "canonicalize",
    "chain_hash",
    "verify_chain",
    "verify_dag",
    "verify_anchors",
    "agent_tool_call_event",
]
