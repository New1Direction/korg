"""CLI dispatch for `korg-setup`.

Usage:

    korg-setup                  # run the full setup (interactive: prompts on edit)
    korg-setup --yes            # non-interactive setup
    korg-setup --dry-run        # show what would change, write nothing
    korg-setup status           # report on what's installed
    korg-setup uninstall        # remove the launchd agent + MCP server entry
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from korg_setup.claude_config import (
    DEFAULT_CONFIG_PATH,
    remove_mcp_server,
)
from korg_setup.launchd import (
    LABEL,
    UnsupportedPlatformError,
    is_macos,
    uninstall_service,
)
from korg_setup.setup import (
    DEFAULT_BRIDGE_ALLOW,
    DEFAULT_LEDGER_DIR,
    DEFAULT_LEDGER_FILE,
    DEFAULT_MCP_SERVER_NAME,
    format_report,
    run_setup,
)
from korg_setup.status import format_status, gather_status


def _cmd_setup(args: argparse.Namespace) -> int:
    # Interactive confirmation unless --yes or --dry-run.
    if not args.yes and not args.dry_run:
        print(
            "korg-setup will:\n"
            f"  · ensure {args.ledger_dir} exists\n"
            f"  · register MCP server '{args.mcp_name}' in {args.claude_config}\n"
            f"  · install the launchd agent {LABEL} (macOS only)\n",
            file=sys.stderr,
        )
        confirm = input("Proceed? [y/N] ").strip().lower()
        if confirm not in {"y", "yes"}:
            print("Aborted.", file=sys.stderr)
            return 1

    report = run_setup(
        ledger_dir=args.ledger_dir,
        ledger_file=args.ledger_file,
        claude_config_path=args.claude_config,
        mcp_server_name=args.mcp_name,
        install_daemon=not args.no_daemon,
        register_introspect_bridges=not args.no_bridges,
        bridge_allow=args.bridge_allow,
        dry_run=args.dry_run,
    )
    print(format_report(report), file=sys.stderr)
    if report.backup_path is not None:
        print(f"\nA backup of your prior Claude config was saved to: {report.backup_path}", file=sys.stderr)
    return 0 if report.overall_ok else 1


def _cmd_status(args: argparse.Namespace) -> int:
    report = gather_status(
        ledger_file=args.ledger_file,
        claude_config_path=args.claude_config,
        mcp_server_name=args.mcp_name,
    )
    print(format_status(report))
    return 0


def _cmd_uninstall(args: argparse.Namespace) -> int:
    if not args.yes:
        print(
            f"korg-setup uninstall will:\n"
            f"  · remove MCP server '{args.mcp_name}' from {args.claude_config}\n"
            f"  · remove any auto-registered korg-introspect-mcp bridge entries\n"
            f"  · stop + delete the launchd agent {LABEL} (macOS only)\n"
            f"  · NOT delete the ledger at {args.ledger_file}\n",
            file=sys.stderr,
        )
        confirm = input("Proceed? [y/N] ").strip().lower()
        if confirm not in {"y", "yes"}:
            print("Aborted.", file=sys.stderr)
            return 1

    # Native MCP entry (recall)
    config_status, backup = remove_mcp_server(args.mcp_name, args.claude_config)
    if config_status == "removed":
        print(f"  ✓ removed MCP server '{args.mcp_name}' from {args.claude_config}", file=sys.stderr)
        if backup is not None:
            print(f"  · backup saved to {backup}", file=sys.stderr)
    else:
        print(f"  · '{args.mcp_name}' was not registered in {args.claude_config}", file=sys.stderr)

    # Introspect-bridge entries auto-registered earlier
    from korg_setup.discovery import DEFAULT_CANDIDATES, discover_all
    try:
        discovered = discover_all(candidates=DEFAULT_CANDIDATES)
    except Exception:
        discovered = []
    bridge_names = []
    for b in discovered:
        name = b.mcp_server_name
        if name == args.mcp_name:
            continue  # already removed above
        status_b, _ = remove_mcp_server(name, args.claude_config)
        if status_b == "removed":
            bridge_names.append(name)
    if bridge_names:
        print(
            f"  ✓ removed {len(bridge_names)} bridge entr"
            f"{'y' if len(bridge_names) == 1 else 'ies'}: {', '.join(bridge_names)}",
            file=sys.stderr,
        )
    else:
        print(f"  · no bridge entries to remove", file=sys.stderr)

    # launchd
    if is_macos():
        try:
            svc_status, _result = uninstall_service()
            if svc_status == "removed":
                print(f"  ✓ stopped + removed launchd agent {LABEL}", file=sys.stderr)
            else:
                print(f"  · launchd agent {LABEL} was not installed", file=sys.stderr)
        except UnsupportedPlatformError:
            pass
    else:
        print("  · skipping launchd (not macOS)", file=sys.stderr)

    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="korg-setup",
        description=(
            "One-command setup for the korg ecosystem: register the recall "
            "MCP server with Claude Code and install the live-tail capture "
            "as a launchd background agent (macOS)."
        ),
    )
    # Top-level flags also accepted at the setup subcommand for convenience.
    parser.add_argument("--yes", action="store_true", help="non-interactive (skip confirmation prompt)")
    parser.add_argument("--dry-run", action="store_true", help="show what would change, write nothing")
    parser.add_argument("--no-daemon", action="store_true", help="don't install/start the launchd agent")
    parser.add_argument(
        "--no-bridges",
        action="store_true",
        help="don't auto-register MCP entries for --introspect-aware binaries",
    )
    parser.add_argument(
        "--bridge-allow",
        type=str,
        default=DEFAULT_BRIDGE_ALLOW,
        help=(
            f"KORG_INTROSPECT_MCP_ALLOW value passed to every bridge entry "
            f"(default {DEFAULT_BRIDGE_ALLOW!r}; use 'all' to allow everything)."
        ),
    )
    parser.add_argument(
        "--ledger-dir",
        type=Path,
        default=DEFAULT_LEDGER_DIR,
        help=f"Where to keep the ledger + state files (default {DEFAULT_LEDGER_DIR})",
    )
    parser.add_argument(
        "--ledger-file",
        type=Path,
        default=DEFAULT_LEDGER_FILE,
        help=f"Ledger JSONL path (default {DEFAULT_LEDGER_FILE})",
    )
    parser.add_argument(
        "--claude-config",
        type=Path,
        default=DEFAULT_CONFIG_PATH,
        help=f"Claude Code config file (default {DEFAULT_CONFIG_PATH})",
    )
    parser.add_argument(
        "--mcp-name",
        type=str,
        default=DEFAULT_MCP_SERVER_NAME,
        help=f"Name to register under mcpServers (default '{DEFAULT_MCP_SERVER_NAME}')",
    )

    subparsers = parser.add_subparsers(dest="command")
    subparsers.add_parser("status", help="show what's installed / running")
    subparsers.add_parser("uninstall", help="remove the launchd agent + MCP server entry")

    args = parser.parse_args(argv)

    if args.command == "status":
        return _cmd_status(args)
    if args.command == "uninstall":
        return _cmd_uninstall(args)
    # Default: setup
    return _cmd_setup(args)


if __name__ == "__main__":
    raise SystemExit(main())
