"""CLI entry — `korg-introspect-mcp <binary> [--allow ...]`.

Default behavior: launch the MCP server on stdio.
With `--list-tools`: print the discovered tools as a table to stderr
and exit (useful for sanity-checking what an agent will see).
"""

from __future__ import annotations

import argparse
import sys

from korg_introspect_mcp.discovery import DiscoveryError, discover
from korg_introspect_mcp.safety import ALL_EFFECTS, Policy
from korg_introspect_mcp.server import SERVER_VERSION, serve_stdio


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="korg-introspect-mcp",
        description=(
            "Bridge any --introspect-aware binary to MCP. Runs the binary "
            "with --introspect to discover its callables, then serves them "
            "as MCP tools over stdio."
        ),
    )
    p.add_argument(
        "binary",
        help=(
            "Path or name of the binary to wrap. Must support --introspect "
            "and emit a korg:introspect@v1 document."
        ),
    )
    p.add_argument(
        "--allow",
        type=str,
        default=None,
        help=(
            f"Comma-separated side_effects to allow invoking (in addition to "
            f"the always-allowed 'none' and 'fs_read'). Recognized: "
            f"{sorted(ALL_EFFECTS)}. Use 'all' to allow everything. "
            f"Overrides KORG_INTROSPECT_MCP_ALLOW."
        ),
    )
    p.add_argument(
        "--list-tools",
        action="store_true",
        help="Print discovered tools to stderr and exit (no MCP server).",
    )
    p.add_argument(
        "--version",
        action="version",
        version=f"%(prog)s {SERVER_VERSION}",
    )

    args = p.parse_args(argv)

    try:
        discovery = discover(args.binary)
    except DiscoveryError as e:
        print(f"[korg-introspect-mcp] {e}", file=sys.stderr)
        return 1

    # Resolve safety policy: CLI flag wins over env var.
    if args.allow is not None:
        # Build a Policy with the explicit allow set.
        import os
        os.environ["KORG_INTROSPECT_MCP_ALLOW"] = args.allow
    policy = Policy.from_env()

    if args.list_tools:
        print(
            f"[korg-introspect-mcp] {discovery.binary_name} "
            f"v{discovery.version} — {len(discovery.callables)} tool(s):",
            file=sys.stderr,
        )
        for c in discovery.callables:
            cap = c.capabilities
            print(
                f"  {c.command_id:<32} "
                f"side_effects={cap.get('side_effects', '?'):<14} "
                f"output_mode={cap.get('output_mode', '?'):<10} "
                f"long_running={str(cap.get('long_running', False)):<5} "
                f"{'(would be denied)' if not policy.allows(cap.get('side_effects', 'none')) else ''}",
                file=sys.stderr,
            )
        print(
            f"[korg-introspect-mcp] policy: allowed side_effects = "
            f"{sorted(policy.allowed)}",
            file=sys.stderr,
        )
        return 0

    print(
        f"[korg-introspect-mcp] serving {discovery.binary_name} "
        f"v{discovery.version} — {len(discovery.callables)} callables, "
        f"policy={sorted(policy.allowed)}",
        file=sys.stderr,
    )
    return serve_stdio(discovery=discovery, policy=policy)


if __name__ == "__main__":
    raise SystemExit(main())
