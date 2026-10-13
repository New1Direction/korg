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
    causal_parent,
    chain_hash,
    json_int,
    verify_anchors,
    verify_chain,
    verify_dag,
)
from ._hlc import Hlc
from .redaction import POLICY as REDACTION_POLICY
from .redaction import redact, scan, verify_redaction
from .writer import CausalityError, LedgerWriter, RedactionError

__all__ = [
    "GENESIS",
    "HASH_FIELDS",
    "NIL_UUID",
    "Hlc",
    "LedgerWriter",
    "CausalityError",
    "RedactionError",
    "REDACTION_POLICY",
    "redact",
    "scan",
    "verify_redaction",
    "causal_parent",
    "json_int",
    "canonicalize",
    "chain_hash",
    "verify_chain",
    "verify_dag",
    "verify_anchors",
    "agent_tool_call_event",
]
