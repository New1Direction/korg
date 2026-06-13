"""CLI for korg-backfill: retroactively capture history, optionally migrate a flat ledger."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from claude_code_adapter.backfill import DEFAULT_PROJECTS_DIR, backfill_all
from claude_code_adapter.migrate import migrate_flat_file


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="korg-backfill",
        description="Retroactively capture historical Claude Code sessions into verifiable per-session ledgers.",
    )
    parser.add_argument("--projects-dir", type=Path, default=DEFAULT_PROJECTS_DIR,
                        help=f"Claude Code projects dir (default {DEFAULT_PROJECTS_DIR})")
    parser.add_argument("--korg-home", type=Path, default=Path.home() / ".korg",
                        help="korg home (default ~/.korg)")
    parser.add_argument("--migrate-flat", type=Path, default=None,
                        help="also migrate this legacy flat claude-events.jsonl (guarded; backfill is preferred)")
    args = parser.parse_args(argv)

    report = backfill_all(projects_dir=args.projects_dir, korg_home=args.korg_home)
    print(f"backfill: {report.events_written} events across {report.sessions} session(s) "
          f"→ {args.korg_home}/sessions/", file=sys.stderr)

    if args.migrate_flat is not None:
        m = migrate_flat_file(args.migrate_flat, korg_home=args.korg_home)
        print(f"migrate: {m.events_written} events across {m.sessions} session(s); "
              f"skipped {len(m.skipped)} already-present", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
