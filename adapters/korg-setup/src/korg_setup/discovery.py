"""Discover `--introspect`-aware binaries on PATH and prepare MCP server entries.

Strategy: try a small list of known candidates (thump, korg, korgex, ...) +
any user-supplied additions. For each candidate found on PATH, run
`<binary> --introspect` and check it emits a valid `korg:introspect@v1`
document. The candidate set is hardcoded for simplicity — adding a new
binary is a one-line PR here, not a config file.

The probing is conservative: a binary that doesn't support --introspect,
times out, or emits a non-matching schema is silently skipped (logged at
debug). We never auto-register something that doesn't declare itself as
korg-ecosystem-compatible.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional


SUPPORTED_SCHEMA = "korg:introspect@v1"
PROBE_TIMEOUT_S = 5.0

# Hardcoded candidate list. Order matters only for stable iteration in tests.
# Adding a new binary to the ecosystem = one line here.
DEFAULT_CANDIDATES: tuple[str, ...] = (
    "thump",
    "korg",
    "korgex",
    "korg-recall-mcp",
)


@dataclass(frozen=True)
class IntrospectableBinary:
    """A binary that successfully responded to --introspect."""

    binary_name: str       # The name the binary declares (`thump`, `korg`, ...)
    binary_path: Path      # Absolute path the probe found
    version: str
    callable_count: int

    @property
    def mcp_server_name(self) -> str:
        """Name to register under `mcpServers` in ~/.claude.json.

        Convention: `korg-<binary>`. Two exceptions to avoid awkward names:
          - `korg` itself stays as `korg` (don't double-name).
          - Binaries already starting with `korg-` (e.g. `korg-recall-mcp`,
            `korg-introspect-mcp`) stay as-is.

        Examples:
          thump            → korg-thump
          korgex           → korg-korgex
          korg             → korg
          korg-recall-mcp  → korg-recall-mcp
        """
        if self.binary_name == "korg":
            return "korg"
        if self.binary_name.startswith("korg-"):
            return self.binary_name
        return f"korg-{self.binary_name}"


def probe(spec: str, timeout_s: float = PROBE_TIMEOUT_S) -> Optional[IntrospectableBinary]:
    """Try to discover an introspect-aware binary.

    Returns None on any failure (not found on PATH, doesn't accept
    --introspect, malformed document, wrong schema, timeout). Never
    raises — discovery is a best-effort probe.
    """
    path_str = shutil.which(spec)
    if path_str is None:
        return None
    binary_path = Path(path_str)

    try:
        result = subprocess.run(
            [str(binary_path), "--introspect"],
            capture_output=True,
            text=True,
            timeout=timeout_s,
            check=False,
        )
    except (subprocess.TimeoutExpired, OSError):
        return None

    if result.returncode != 0 and not result.stdout.strip():
        # Some binaries can still emit the document on non-zero (e.g. clap
        # complaining about missing args first); only bail if stdout is also empty.
        return None

    try:
        doc = json.loads(result.stdout)
    except json.JSONDecodeError:
        return None
    if not isinstance(doc, dict):
        return None
    if doc.get("schema") != SUPPORTED_SCHEMA:
        return None

    callables = doc.get("callables")
    if not isinstance(callables, list):
        return None

    return IntrospectableBinary(
        binary_name=str(doc.get("binary") or spec),
        binary_path=binary_path,
        version=str(doc.get("version") or "0.0.0"),
        callable_count=len(callables),
    )


def discover_all(
    candidates: Iterable[str] = DEFAULT_CANDIDATES,
    timeout_s: float = PROBE_TIMEOUT_S,
) -> list[IntrospectableBinary]:
    """Probe every candidate; return the ones that responded successfully.

    Order matches the candidate order so output is deterministic.
    Duplicates (same binary name) are collapsed (last one wins).
    """
    seen: dict[str, IntrospectableBinary] = {}
    for spec in candidates:
        result = probe(spec, timeout_s=timeout_s)
        if result is None:
            continue
        seen[result.binary_name] = result
    return list(seen.values())
