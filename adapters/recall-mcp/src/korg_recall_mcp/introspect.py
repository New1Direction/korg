"""Capabilities + introspection — the single source of truth for both the
MCP tools/list response AND the `--introspect` CLI document.

This is the Foundry insight applied here: a CLI's `--introspect` document
and an MCP server's tools descriptor are conceptually the same thing —
a machine-readable description of every callable, with side-effect
metadata, schemas, and stable IDs. We collapse them into one source.

Schema versioning: the document is tagged `korg:introspect@v1`. Any
incompatible change to field names or required-ness bumps the suffix.
Adding new optional fields keeps `@v1`.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Literal


# ── Capability vocabulary ─────────────────────────────────────────────


SideEffects = Literal["none", "fs_read", "fs_write", "network", "ledger_write"]
OutputMode = Literal["none", "stream", "envelope", "session"]


@dataclass(frozen=True)
class Capabilities:
    """Static, declared metadata about how a callable behaves.

    Agents can read this BEFORE invocation to decide whether running is
    safe and how to consume the output. Matches the spirit of Foundry's
    Capabilities struct, narrowed to what korg adapters actually need.
    """

    # How the result is delivered:
    #   "none"      — no machine-readable output
    #   "stream"    — text/NDJSON line stream on stdout
    #   "envelope"  — JsonEnvelope<T> wrapper on stdout
    #   "session"   — long-lived stateful session (e.g. MCP server)
    output_mode: OutputMode = "envelope"

    # Worst-case effects this callable can have. Conservative — declare
    # what it *can* do, not what one invocation might do.
    side_effects: SideEffects = "none"

    requires_project: bool = False  # needs a project / workspace context
    long_running: bool = False      # may run indefinitely
    stateful: bool = False          # mutates persistent state
    reads_stdin: bool = False       # consumes stdin
    supports_output_path: bool = False  # accepts a --out file path

    def to_dict(self) -> dict[str, Any]:
        return {
            "output_mode": self.output_mode,
            "side_effects": self.side_effects,
            "requires_project": self.requires_project,
            "long_running": self.long_running,
            "stateful": self.stateful,
            "reads_stdin": self.reads_stdin,
            "supports_output_path": self.supports_output_path,
        }


@dataclass(frozen=True)
class Callable:
    """One callable surface — exposed as both an MCP tool AND a CLI verb.

    The same instance produces:
      - MCP tools/list entry  via `to_mcp_tool()`
      - --introspect entry    via `to_introspect_entry()`

    The two projections differ only in which fields each surface needs;
    the underlying truth is identical.
    """

    # Stable ID across versions. `<binary>.<callable>`. Agents may pin to this.
    id: str
    # Short human-readable name (what shows up in MCP tools/list and CLI help).
    name: str
    description: str
    # JSON Schema for input arguments. Same shape MCP expects.
    input_schema: dict[str, Any]
    # Which surfaces support this callable.
    surfaces: list[str] = field(default_factory=lambda: ["cli", "mcp"])
    capabilities: Capabilities = field(default_factory=Capabilities)

    def to_mcp_tool(self) -> dict[str, Any]:
        """Project as an MCP tools/list entry."""
        return {
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        }

    def to_introspect_entry(self) -> dict[str, Any]:
        """Project as a `--introspect` document entry."""
        return {
            "command_id": self.id,
            "name": self.name,
            "description": self.description,
            "surfaces": list(self.surfaces),
            "input_schema": self.input_schema,
            "capabilities": self.capabilities.to_dict(),
        }


# ── Stable exit codes (Foundry-style canonical table) ─────────────────


EXIT_CODES: dict[int, str] = {
    0: "success",
    1: "error.generic",
    2: "error.usage",
    3: "error.config",
    4: "error.io",
    5: "error.network",
    6: "error.user_interrupt",
    7: "error.dependency_missing",
}


# ── Registry: the single source of truth for korg-recall-mcp ──────────


def _recall_callable() -> Callable:
    """The one callable this package exposes today.

    Defined as a function so cosmetic edits to defaults stay easy to
    review and the constants stay close to their docstrings.
    """
    return Callable(
        id="korg-recall-mcp.recall",
        name="recall",
        description=(
            "Search across all prior AI sessions recorded in the korg "
            "ledger. Returns relevant past prompts, model replies, and "
            "tool calls/results. Use this BEFORE attempting work that "
            "may have been done before — finding the prior session "
            "saves the cost of rediscovery."
        ),
        input_schema={
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural-language description of what you're looking for.",
                },
                "top_n": {
                    "type": "integer",
                    "description": "Max number of results (default 5).",
                    "default": 5,
                    "minimum": 1,
                    "maximum": 50,
                },
                "min_score": {
                    "type": "number",
                    "description": (
                        "Cosine-similarity floor for semantic matches (default 0.30). "
                        "Ignored for substring mode."
                    ),
                    "default": 0.30,
                    "minimum": 0.0,
                    "maximum": 1.0,
                },
                "mode": {
                    "type": "string",
                    "enum": ["auto", "semantic", "substring"],
                    "description": (
                        "auto: semantic if fastembed installed else substring. "
                        "semantic: require embedding-backed ranking. "
                        "substring: pure keyword AND-of-terms."
                    ),
                    "default": "auto",
                },
                "tool_filter": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": (
                        "Optional list of tool_name values to restrict the "
                        "search to (e.g. ['user_prompt'], ['Read', 'Bash'])."
                    ),
                },
            },
            "required": ["query"],
        },
        surfaces=["cli", "mcp"],
        capabilities=Capabilities(
            output_mode="stream",
            side_effects="fs_read",  # reads .jsonl ledger files
            requires_project=False,
            long_running=False,
            stateful=False,
            reads_stdin=False,
            supports_output_path=False,
        ),
    )


def get_callables() -> list[Callable]:
    """All callables this package exposes, in stable order."""
    return [_recall_callable()]


# ── Document builder ──────────────────────────────────────────────────


INTROSPECT_SCHEMA_ID = "korg:introspect@v1"
BINARY_NAME = "korg-recall-mcp"


def build_introspect_document(version: str) -> dict[str, Any]:
    """Build the full `--introspect` document.

    `version` is plumbed in (rather than imported from `__init__`) to avoid
    a circular import between this module and the top-level package.

    Note: `exit_codes` keys are emitted as **strings** since JSON has no
    integer keys. Agents reading the wire format should do
    `doc["exit_codes"]["0"]`, not `[0]`.
    """
    return {
        "schema": INTROSPECT_SCHEMA_ID,
        "binary": BINARY_NAME,
        "version": version,
        "callables_declared": True,
        "callables": [c.to_introspect_entry() for c in get_callables()],
        "exit_codes": {str(k): v for k, v in EXIT_CODES.items()},
    }
