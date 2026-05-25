"""
CLI entry point for thump (Thumper's Python Bun semantic harness).

This module turns the Python semantic harness into a real, runnable CLI
that can be invoked directly or as a subprocess from the Rust frontend
(thump / thumper / bunny / thump-cli).

It provides:

- Clean subcommand structure (script run, package add/install/remove)
- Global flags (--session-id, --cwd, --timeout)
- Proper dispatch to the library functions
- Correct exit codes
- Full NDJSON event stream passthrough (the library already emits everything)

Usage examples:
    python -m thump script run dev
    python -m thump package add hono zod --dev
    python -m thump package remove zod
"""

from __future__ import annotations

import argparse
import sys
import uuid
from pathlib import Path
from typing import List, Optional

from .package import add as package_add, install as package_install, remove as package_remove
from .script import run as script_run


def _generate_session_id() -> str:
    return f"cli-{uuid.uuid4().hex[:12]}"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="cli-anything-bun",
        description="Agent-native semantic harness for Bun",
    )

    # Global options
    parser.add_argument(
        "--session-id",
        dest="session_id",
        default=None,
        help="Correlation ID for the entire session (auto-generated if omitted)",
    )
    parser.add_argument(
        "--cwd",
        default=None,
        help="Working directory for the command (defaults to current directory)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=None,
        help="Timeout in seconds for the underlying process",
    )

    subparsers = parser.add_subparsers(dest="command", required=True)

    # === script group ===
    script_parser = subparsers.add_parser("script", help="Script-related commands")
    script_sub = script_parser.add_subparsers(dest="script_command", required=True)

    # script run
    run_parser = script_sub.add_parser("run", help="Run a script from package.json")
    run_parser.add_argument("name", help="Name of the script to run")
    run_parser.add_argument(
        "args",
        nargs=argparse.REMAINDER,
        help="Arguments to pass to the script",
    )

    # === package group ===
    package_parser = subparsers.add_parser("package", help="Package management commands")
    package_sub = package_parser.add_subparsers(dest="package_command", required=True)

    # package add
    add_parser = package_sub.add_parser("add", help="Add packages")
    add_parser.add_argument("packages", nargs="+", help="Package names to add")
    add_parser.add_argument("--dev", action="store_true", help="Add as dev dependency")
    add_parser.add_argument("--exact", action="store_true", help="Install exact version")
    add_parser.add_argument("--peer", action="store_true", help="Add as peer dependency")
    add_parser.add_argument("--optional", action="store_true", help="Add as optional dependency")

    # package install
    install_parser = package_sub.add_parser("install", help="Install dependencies")
    install_parser.add_argument(
        "packages",
        nargs="*",
        help="Optional packages to install (if empty, runs plain 'bun install')",
    )
    install_parser.add_argument(
        "--frozen-lockfile",
        action="store_true",
        help="Fail if lockfile is not up to date",
    )

    # package remove
    remove_parser = package_sub.add_parser("remove", help="Remove packages")
    remove_parser.add_argument("packages", nargs="+", help="Package names to remove")

    return parser


def main(argv: Optional[List[str]] = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    # Normalize common globals
    session_id = args.session_id or _generate_session_id()
    cwd = Path(args.cwd).resolve() if args.cwd else None
    timeout = args.timeout

    try:
        if args.command == "script":
            if args.script_command == "run":
                result = script_run(
                    name=args.name,
                    args=args.args,
                    cwd=cwd,
                    timeout=timeout,
                    session_id=session_id,
                )
                return 0 if result.success else 1

        elif args.command == "package":
            if args.package_command == "add":
                result = package_add(
                    packages=args.packages,
                    cwd=cwd,
                    dev=args.dev,
                    exact=args.exact,
                    peer=args.peer,
                    optional=args.optional,
                    timeout=timeout,
                    session_id=session_id,
                )
                return 0 if result.success else 1

            elif args.package_command == "install":
                result = package_install(
                    packages=args.packages or None,
                    cwd=cwd,
                    frozen_lockfile=args.frozen_lockfile,
                    timeout=timeout,
                    session_id=session_id,
                )
                return 0 if result.success else 1

            elif args.package_command == "remove":
                result = package_remove(
                    packages=args.packages,
                    cwd=cwd,
                    timeout=timeout,
                    session_id=session_id,
                )
                return 0 if result.success else 1

        # Fallback (should not happen due to argparse required=True)
        parser.print_help()
        return 2

    except KeyboardInterrupt:
        print("Interrupted by user", file=sys.stderr)
        return 130
    except Exception as exc:
        # Top-level safety net — the library functions already emit good events,
        # but we still want a clean non-zero exit and a last-resort message.
        print(f"Fatal error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
