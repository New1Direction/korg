"""Safety gating via declared Capabilities.side_effects.

A callable's `capabilities.side_effects` declares the worst-case effect
class. Without explicit operator approval, the bridge refuses to
invoke callables that can write files, mutate the ledger, or hit the
network. This makes auto-exposing the entire ecosystem to Claude Code
safe-by-default — the agent can DISCOVER everything but only INVOKE
read-only operations until the user opts in to broader access.

Approval mechanism: the `KORG_INTROSPECT_MCP_ALLOW` env var lists
side_effects values that may be invoked. Comma-separated.

Examples:
  (unset)                                 — none + fs_read only
  KORG_INTROSPECT_MCP_ALLOW=fs_write       — also allow file writes
  KORG_INTROSPECT_MCP_ALLOW=fs_write,network,ledger_write
                                          — full access
  KORG_INTROSPECT_MCP_ALLOW=all            — alias for everything

The user makes the call explicitly per their threat model. Anything
short of `KORG_INTROSPECT_MCP_ALLOW=all` is more conservative than
running `<binary> --query ...` in a shell directly, which is the
baseline an agent could do anyway via Bash.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Iterable


# Effect classes in increasing privilege order. Agents can call anything
# at-or-below the declared allow level.
ALWAYS_ALLOWED = frozenset({"none", "fs_read"})
ALL_EFFECTS = frozenset({"none", "fs_read", "fs_write", "network", "ledger_write"})


def _parse_allow_env(raw: str | None) -> frozenset[str]:
    if not raw:
        return frozenset()
    raw = raw.strip().lower()
    if raw in ("all", "*"):
        return ALL_EFFECTS
    parts = {p.strip() for p in raw.split(",") if p.strip()}
    return frozenset(parts)


@dataclass(frozen=True)
class Policy:
    """The effective gating policy for one bridge instance."""

    # The set of side_effects values that may be invoked.
    allowed: frozenset[str]

    @classmethod
    def from_env(cls, env: dict[str, str] | None = None) -> "Policy":
        env = env if env is not None else dict(os.environ)
        extra = _parse_allow_env(env.get("KORG_INTROSPECT_MCP_ALLOW"))
        return cls(allowed=ALWAYS_ALLOWED | extra)

    @classmethod
    def all(cls) -> "Policy":
        """Allow everything. Useful for trusted/internal deployments."""
        return cls(allowed=ALL_EFFECTS)

    @classmethod
    def read_only(cls) -> "Policy":
        """Default — only side-effect-free + read-only."""
        return cls(allowed=ALWAYS_ALLOWED)

    def allows(self, side_effects: str) -> bool:
        return side_effects in self.allowed

    def explain_denial(self, side_effects: str) -> str:
        return (
            f"refused: callable declares side_effects={side_effects!r}, which is "
            f"not in the allow list {sorted(self.allowed)}. "
            f"To enable, set KORG_INTROSPECT_MCP_ALLOW={side_effects} "
            f"(or KORG_INTROSPECT_MCP_ALLOW=all)."
        )
