"""CLI entry point — runs the MCP server, or executes a one-shot recall query.

Default behavior (no flags besides ledger paths) is to run the MCP server
on stdio. This is what Claude Code launches when it spawns the configured
MCP server subprocess.

For local testing without an MCP client:

    korg-recall-mcp --query "rust borrow checker" \\
                    --ledger ~/.korg/claude-events.jsonl

For the live "follow my ledger" setup, pair this with
`korg-ingest-claude --tail` running in another shell — they share the
same JSONL file and recall picks up new events on every search.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from korg_recall_mcp.index import EventIndex
from korg_recall_mcp.introspect import build_introspect_document
from korg_recall_mcp.search import (
    DEFAULT_MIN_SCORE,
    DEFAULT_TOP_N,
    RecallEngine,
)
from korg_recall_mcp.server import SERVER_VERSION, format_matches_for_llm, serve_stdio


DEFAULT_LEDGER = Path.home() / ".korg" / "claude-events.jsonl"


def main(argv: list[str] | None = None) -> int:
    # Transparent-accept a leading "recall" token so that invocations via
    # the korg-introspect-mcp bridge work without per-binary special-casing
    # there. The bridge converts command_id `korg-recall-mcp.recall` →
    # `[korg-recall-mcp, recall, --query, ...]` per its uniform argv
    # convention; we strip the redundant subcommand here.
    raw = list(sys.argv[1:]) if argv is None else list(argv)
    if raw and raw[0] == "recall":
        raw = raw[1:]
    argv = raw

    p = argparse.ArgumentParser(
        prog="korg-recall-mcp",
        description=(
            "Cross-session semantic recall over the korg ledger. Runs as an "
            "MCP server by default; use --query for one-shot CLI search."
        ),
    )
    p.add_argument(
        "--ledger",
        action="append",
        type=Path,
        default=None,
        help=(
            "Path to a ledger .jsonl file (the output of `korg-ingest-claude`). "
            "Can be a file or a directory containing *.jsonl. Repeatable. "
            f"Default: {DEFAULT_LEDGER}"
        ),
    )
    p.add_argument(
        "--query",
        type=str,
        default=None,
        help="One-shot CLI search instead of running the MCP server.",
    )
    p.add_argument(
        "--top-n",
        type=int,
        default=DEFAULT_TOP_N,
        help=f"Max number of matches (default {DEFAULT_TOP_N}).",
    )
    p.add_argument(
        "--min-score",
        type=float,
        default=DEFAULT_MIN_SCORE,
        help=f"Cosine-similarity floor for semantic mode (default {DEFAULT_MIN_SCORE}).",
    )
    p.add_argument(
        "--mode",
        choices=["auto", "semantic", "substring"],
        default="auto",
    )
    p.add_argument(
        "--introspect",
        action="store_true",
        help=(
            "Print the korg:introspect@v1 document (callables, capabilities, "
            "exit codes) as JSON on stdout and exit. The same source of truth "
            "the MCP tools/list endpoint serves from."
        ),
    )

    args = p.parse_args(argv)

    # Introspect short-circuit — works before any I/O so agents can discover
    # the tool without having a ledger at all.
    if args.introspect:
        import json as _json
        print(_json.dumps(build_introspect_document(SERVER_VERSION), indent=2))
        return 0

    ledger_paths = args.ledger or [DEFAULT_LEDGER]
    index = EventIndex(ledger_paths=[Path(p).expanduser() for p in ledger_paths])
    initial_count = index.refresh()
    engine = RecallEngine(index=index)

    if args.query is not None:
        matches = engine.search(
            args.query,
            mode=args.mode,
            top_n=args.top_n,
            min_score=args.min_score,
        )
        print(format_matches_for_llm(matches, engine.last_mode or args.mode))
        return 0

    print(
        f"[korg-recall-mcp] starting MCP server on stdio. "
        f"ledger paths: {[str(p) for p in ledger_paths]}. "
        f"loaded {initial_count} event(s) at startup.",
        file=sys.stderr,
    )
    return serve_stdio(engine)


if __name__ == "__main__":
    raise SystemExit(main())
