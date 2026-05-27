"""CLI entry point for the Claude Code adapter.

Usage:

    # One-shot replay of all sessions under ~/.claude/projects/, write to JSONL:
    python -m claude_code_adapter --once --out ~/.korg/claude-replay.jsonl

    # Live-tail mode — keep running, ingest new events as they're written:
    python -m claude_code_adapter --tail --out ~/.korg/claude-tail.jsonl

    # Dev mode — print events to stdout instead of writing anywhere:
    python -m claude_code_adapter --tail --stub

    # Custom projects dir / state file:
    python -m claude_code_adapter --tail \\
        --projects-dir ~/.claude/projects \\
        --state ~/.korg/my-tail-state.json
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from claude_code_adapter.tail import (
    DEFAULT_POLL_INTERVAL_S,
    DEFAULT_PROJECTS_DIR,
    DEFAULT_STATE_PATH,
    TailIngester,
    make_jsonl_emit,
    make_stub_emit,
)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="korg-ingest-claude",
        description=(
            "Stream Claude Code session JSONL events into a korg ledger. "
            "Run with --tail for continuous capture, or --once to backfill."
        ),
    )
    mode = p.add_mutually_exclusive_group(required=True)
    mode.add_argument(
        "--tail",
        action="store_true",
        help="Watch the projects dir and ingest new events as they're written.",
    )
    mode.add_argument(
        "--once",
        action="store_true",
        help="Do one pass: ingest everything that hasn't been ingested yet, then exit.",
    )

    sink = p.add_mutually_exclusive_group()
    sink.add_argument(
        "--out",
        type=Path,
        help=(
            "Append each event as a JSON line to this file. Default if neither "
            "--out nor --stub is given: ~/.korg/claude-events.jsonl"
        ),
    )
    sink.add_argument(
        "--stub",
        action="store_true",
        help="Print each event to stdout (no file write).",
    )

    p.add_argument(
        "--projects-dir",
        type=Path,
        default=DEFAULT_PROJECTS_DIR,
        help=f"Directory to scan recursively for *.jsonl (default: {DEFAULT_PROJECTS_DIR}).",
    )
    p.add_argument(
        "--state",
        type=Path,
        default=DEFAULT_STATE_PATH,
        help=f"Persistent byte-offset state file (default: {DEFAULT_STATE_PATH}).",
    )
    p.add_argument(
        "--poll-interval",
        type=float,
        default=DEFAULT_POLL_INTERVAL_S,
        help=f"Tail poll interval in seconds (default: {DEFAULT_POLL_INTERVAL_S}).",
    )
    p.add_argument(
        "--source-agent",
        type=str,
        default="agent:claude-code",
        help="Prefix for the source_agent string written to each event.",
    )

    args = p.parse_args(argv)

    # Wire up the emit() backend.
    if args.stub:
        emit = make_stub_emit()
        out_label = "stdout"
    else:
        out_path = args.out or (Path.home() / ".korg" / "claude-events.jsonl")
        emit = make_jsonl_emit(out_path)
        out_label = str(out_path)

    ingester = TailIngester(
        emit=emit,
        state_path=args.state,
        projects_dir=args.projects_dir,
        source_agent_prefix=args.source_agent,
    )

    if args.once:
        stats = ingester.poll_once()
        print(
            f"[korg-ingest-claude] one-shot pass complete · "
            f"files_active={stats.files_active} "
            f"events={stats.new_events} "
            f"(user={stats.new_user_prompts}, llm={stats.new_llm_rounds}, "
            f"tools={stats.new_tool_calls}, dropped={stats.new_dropped}) → {out_label}",
            file=sys.stderr,
        )
        return 0

    # Tail mode
    print(
        f"[korg-ingest-claude] tail mode active · "
        f"watching {args.projects_dir} → {out_label} "
        f"(poll every {args.poll_interval}s; state at {args.state}). "
        f"Ctrl-C to stop.",
        file=sys.stderr,
    )

    def announce(poll):
        if poll.new_events:
            print(
                f"[korg-ingest-claude] +{poll.new_events} events "
                f"across {poll.files_active} session(s)",
                file=sys.stderr,
            )

    total = ingester.run(
        poll_interval_s=args.poll_interval,
        on_poll=announce,
    )
    print(
        f"[korg-ingest-claude] stopped. total this run: "
        f"user={total.user_prompts} llm={total.llm_rounds} "
        f"tools={total.tool_calls} dropped={total.dropped}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
