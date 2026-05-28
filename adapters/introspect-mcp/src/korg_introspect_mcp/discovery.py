"""Discover a binary's callables by running `<binary> --introspect`
and parsing the `korg:introspect@v1` document.

Validation:
  - The document must declare `schema: "korg:introspect@v1"`.
  - It must include a `callables` array.
  - Each callable must carry `command_id`, `name`, `input_schema`, `capabilities`.

Discovery failures (binary not found, --introspect not supported, malformed
document) raise `DiscoveryError` with a clear message instead of partially-
populating the server. Better to fail loud at startup than serve broken tools.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SUPPORTED_SCHEMA = "korg:introspect@v1"
DISCOVERY_TIMEOUT_S = 15.0


class DiscoveryError(RuntimeError):
    """Raised when the introspect document is missing, invalid, or unparseable."""


@dataclass
class DiscoveredCallable:
    """One callable, validated and ready to register as an MCP tool."""

    command_id: str
    name: str
    description: str
    input_schema: dict[str, Any]
    capabilities: dict[str, Any]
    surfaces: list[str]


@dataclass
class DiscoveredBinary:
    """The full discovery result for one binary."""

    binary_path: Path
    schema: str
    binary_name: str
    version: str
    callables: list[DiscoveredCallable]
    exit_codes: dict[str, str]
    callables_declared: bool

    def by_command_id(self, command_id: str) -> DiscoveredCallable | None:
        for c in self.callables:
            if c.command_id == command_id:
                return c
        return None


def resolve_binary(spec: str) -> Path:
    """Resolve a binary spec to an absolute path.

    Accepts: absolute path, relative path, or bare name (PATH lookup).
    """
    p = Path(spec).expanduser()
    if p.is_absolute():
        if not p.exists():
            raise DiscoveryError(f"binary not found at absolute path: {p}")
        return p
    # Relative or bare name → PATH lookup
    found = shutil.which(spec)
    if found is None:
        raise DiscoveryError(
            f"binary not found on PATH: {spec!r}. "
            f"If you meant a relative path, pass it as ./{spec}"
        )
    return Path(found)


def run_introspect(binary_path: Path, timeout_s: float = DISCOVERY_TIMEOUT_S) -> dict[str, Any]:
    """Exec `<binary> --introspect`, capture stdout, parse as JSON.

    stderr is captured but only surfaced on failure (matches the Foundry
    output contract: stdout = result, stderr = diagnostics).
    """
    try:
        result = subprocess.run(
            [str(binary_path), "--introspect"],
            capture_output=True,
            text=True,
            timeout=timeout_s,
            check=False,
        )
    except subprocess.TimeoutExpired as e:
        raise DiscoveryError(
            f"{binary_path} --introspect timed out after {timeout_s}s"
        ) from e
    except OSError as e:
        raise DiscoveryError(f"could not exec {binary_path}: {e}") from e

    if result.returncode != 0:
        # Some binaries print --introspect even on non-zero (e.g. clap's
        # missing-subcommand error). Try to parse anyway; only fail if
        # the parse also fails.
        try:
            return json.loads(result.stdout)
        except json.JSONDecodeError:
            raise DiscoveryError(
                f"{binary_path} --introspect exited {result.returncode}: "
                f"{result.stderr.strip() or result.stdout.strip()}"
            )

    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as e:
        raise DiscoveryError(
            f"{binary_path} --introspect did not return valid JSON: {e}. "
            f"First 200 chars: {result.stdout[:200]!r}"
        ) from e


def validate_document(doc: dict[str, Any], binary_path: Path) -> DiscoveredBinary:
    """Validate the document shape and return a typed DiscoveredBinary."""
    schema = doc.get("schema")
    if schema != SUPPORTED_SCHEMA:
        raise DiscoveryError(
            f"{binary_path}: unsupported introspect schema {schema!r}. "
            f"This version of korg-introspect-mcp supports {SUPPORTED_SCHEMA}."
        )

    raw_callables = doc.get("callables")
    if not isinstance(raw_callables, list):
        raise DiscoveryError(
            f"{binary_path}: introspect document has no 'callables' array."
        )

    discovered: list[DiscoveredCallable] = []
    seen_ids: set[str] = set()
    for i, c in enumerate(raw_callables):
        if not isinstance(c, dict):
            raise DiscoveryError(f"{binary_path}: callables[{i}] is not an object.")
        for required in ("command_id", "name", "input_schema", "capabilities"):
            if required not in c:
                raise DiscoveryError(
                    f"{binary_path}: callables[{i}] missing required field {required!r}"
                )
        cid = c["command_id"]
        if cid in seen_ids:
            raise DiscoveryError(f"{binary_path}: duplicate command_id {cid!r}")
        seen_ids.add(cid)

        discovered.append(
            DiscoveredCallable(
                command_id=cid,
                name=c["name"],
                description=c.get("description", ""),
                input_schema=c["input_schema"],
                capabilities=c["capabilities"],
                surfaces=list(c.get("surfaces") or []),
            )
        )

    return DiscoveredBinary(
        binary_path=binary_path,
        schema=schema,
        binary_name=doc.get("binary", binary_path.name),
        version=doc.get("version", "0.0.0"),
        callables=discovered,
        exit_codes=dict(doc.get("exit_codes") or {}),
        callables_declared=bool(doc.get("callables_declared", False)),
    )


def discover(spec: str, timeout_s: float = DISCOVERY_TIMEOUT_S) -> DiscoveredBinary:
    """Top-level: resolve binary spec, run --introspect, validate, return typed result."""
    binary_path = resolve_binary(spec)
    raw_doc = run_introspect(binary_path, timeout_s=timeout_s)
    return validate_document(raw_doc, binary_path)
