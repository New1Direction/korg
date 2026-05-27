"""
Tail-mode ingester — watches Claude Code's projects dir and streams new
JSONL lines into a korg ledger as they're written.

Design:
  - Discover .jsonl files under `projects_dir` (recursive).
  - Track byte offset per file in a persistent state file. Offsets survive
    restart so we resume exactly where we left off.
  - Each pollable file is bound to its own ClaudeCodeAdapter — the adapter
    holds chain state (prompt_seq, llm_seq) and the parser holds tool-result
    bookkeeping, so partial-turn batches across polls still produce the
    correct causal chain.
  - Only read up to the last `\\n` in the file. A line being mid-write at
    poll time is held back until the next poll, when its terminating `\\n`
    has been flushed.

Polling vs filesystem watching:
  - inotify / FSEvents are platform-specific and require extra deps.
  - Claude Code writes batches, not character streams, so a 0.5-2s poll
    interval is more than fast enough for an audit-grade ledger.
  - We KISS and use polling.

This module is intentionally self-contained — no network, no korg
dependency. The `emit` callable is the only seam to a real ledger.
"""

from __future__ import annotations

import json
import os
import signal
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Optional

from claude_code_adapter.adapter import ClaudeCodeAdapter, EmitFn, IngestStats


DEFAULT_PROJECTS_DIR = Path.home() / ".claude" / "projects"
DEFAULT_STATE_PATH = Path.home() / ".korg" / "claude-tail-state.json"
DEFAULT_POLL_INTERVAL_S = 1.0


@dataclass
class TailState:
    """Persistent {file_path: byte_offset} across runs.

    Atomic write via tmp-rename so a crash mid-write doesn't corrupt the file.
    """

    path: Path
    offsets: dict[str, int] = field(default_factory=dict)

    @classmethod
    def load(cls, path: Path) -> "TailState":
        if path.exists():
            try:
                data = json.loads(path.read_text())
                if isinstance(data, dict):
                    # Filter to int values only to defend against corruption
                    offsets = {
                        k: int(v) for k, v in data.items() if isinstance(v, (int, float))
                    }
                    return cls(path=path, offsets=offsets)
            except (json.JSONDecodeError, OSError, ValueError):
                pass
        return cls(path=path, offsets={})

    def get(self, key: str) -> int:
        return self.offsets.get(key, 0)

    def set(self, key: str, value: int) -> None:
        self.offsets[key] = value

    def save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(self.path.suffix + ".tmp")
        tmp.write_text(json.dumps(self.offsets, indent=2, sort_keys=True))
        os.replace(tmp, self.path)


@dataclass
class PollStats:
    """One poll cycle's aggregate."""

    files_active: int = 0
    new_events: int = 0
    new_user_prompts: int = 0
    new_llm_rounds: int = 0
    new_tool_calls: int = 0
    new_dropped: int = 0


class TailIngester:
    """Watches a Claude Code projects directory and ingests new JSONL events.

    Construct, then call `poll_once()` to do one pass, or `run()` to loop
    forever (Ctrl-C-safe). All chain state lives in per-file adapters that
    persist for the life of the ingester instance.
    """

    def __init__(
        self,
        emit: EmitFn,
        state_path: Path | None = None,
        projects_dir: Path | None = None,
        source_agent_prefix: str = "agent:claude-code",
    ) -> None:
        self.emit = emit
        self.state = TailState.load(state_path or DEFAULT_STATE_PATH)
        self.projects_dir = (projects_dir or DEFAULT_PROJECTS_DIR).expanduser()
        self.source_agent_prefix = source_agent_prefix
        # session_key (str) → adapter instance
        self._adapters: dict[str, ClaudeCodeAdapter] = {}
        # Stop signal for run()
        self._stop = False

    # ── Adapters ──────────────────────────────────────────────────────

    def _session_id_from_path(self, path: Path) -> str:
        """Each .jsonl file is named after a session UUID. Use that so the
        source_agent string is stable for a given session across restarts."""
        return path.stem

    def _adapter_for(self, path: Path) -> ClaudeCodeAdapter:
        key = str(path)
        if key not in self._adapters:
            session_id = self._session_id_from_path(path)
            source_agent = f"{self.source_agent_prefix}#{session_id}"
            self._adapters[key] = ClaudeCodeAdapter(
                emit=self.emit, source_agent=source_agent
            )
        return self._adapters[key]

    # ── Polling ───────────────────────────────────────────────────────

    def discover_files(self) -> list[Path]:
        """Return all .jsonl files under projects_dir, sorted for determinism."""
        if not self.projects_dir.exists():
            return []
        return sorted(self.projects_dir.glob("**/*.jsonl"))

    def poll_once(self) -> PollStats:
        """Read new bytes from every known .jsonl, parse, emit. Persist offsets."""
        agg = PollStats()
        any_progress = False

        for path in self.discover_files():
            key = str(path)
            try:
                size = path.stat().st_size
            except OSError:
                continue

            offset = self.state.get(key)
            if size <= offset:
                continue  # nothing new since last poll

            try:
                with path.open("r", encoding="utf-8", errors="replace") as f:
                    f.seek(offset)
                    chunk = f.read(size - offset)
            except OSError:
                continue

            if not chunk:
                continue

            # Only read complete lines. A line being mid-write at poll
            # time is held back: we rsplit at the last \n and advance
            # offset only past complete lines.
            if "\n" not in chunk:
                continue
            complete, _partial = chunk.rsplit("\n", 1)
            consumed_bytes = len(complete.encode("utf-8")) + 1  # +1 for the final \n
            new_offset = offset + consumed_bytes

            lines = [ln for ln in complete.split("\n") if ln.strip()]
            if not lines:
                # Just whitespace progress — advance offset, no events.
                self.state.set(key, new_offset)
                continue

            adapter = self._adapter_for(path)
            stats = adapter.ingest(lines)

            agg.files_active += 1
            agg.new_user_prompts += stats.user_prompts
            agg.new_llm_rounds += stats.llm_rounds
            agg.new_tool_calls += stats.tool_calls
            agg.new_dropped += stats.dropped
            agg.new_events += (
                stats.user_prompts + stats.llm_rounds + stats.tool_calls
            )

            self.state.set(key, new_offset)
            any_progress = True

        if any_progress:
            self.state.save()

        return agg

    # ── Run loop ──────────────────────────────────────────────────────

    def stop(self) -> None:
        """Signal the run loop to exit at the next poll boundary."""
        self._stop = True

    def run(
        self,
        poll_interval_s: float = DEFAULT_POLL_INTERVAL_S,
        on_poll: Optional[Callable[[PollStats], None]] = None,
        install_signal_handlers: bool = True,
    ) -> IngestStats:
        """Block until stop() is called or SIGINT/SIGTERM received.

        Returns aggregate IngestStats across all polls.
        """
        if install_signal_handlers:
            def _handler(_signo, _frame):
                self._stop = True

            try:
                signal.signal(signal.SIGINT, _handler)
                signal.signal(signal.SIGTERM, _handler)
            except (ValueError, OSError):
                # Signals can only be set from the main thread; ignore in tests.
                pass

        total = IngestStats()
        while not self._stop:
            poll = self.poll_once()
            if on_poll is not None:
                try:
                    on_poll(poll)
                except Exception:
                    # Never let a callback bug stop the loop.
                    pass
            total.user_prompts += poll.new_user_prompts
            total.llm_rounds += poll.new_llm_rounds
            total.tool_calls += poll.new_tool_calls
            total.dropped += poll.new_dropped
            # Sleep in small chunks so stop() responds quickly.
            slept = 0.0
            while slept < poll_interval_s and not self._stop:
                time.sleep(min(0.1, poll_interval_s - slept))
                slept += 0.1
        return total


# ── Convenience emit() implementations ────────────────────────────────


def make_jsonl_emit(path: Path) -> EmitFn:
    """Append each event body as a JSON line to `path`. Returns a 1-based seq id.

    This is the simplest viable ledger backend — a flat file you can grep,
    jq, or feed into korg-tui's offline-replay tools.
    """
    state = {"seq": 0}
    path.parent.mkdir(parents=True, exist_ok=True)

    def emit(body: dict[str, Any]) -> int:
        state["seq"] += 1
        record = {"seq": state["seq"], **body}
        with path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(record) + "\n")
        return state["seq"]

    return emit


def make_stub_emit() -> EmitFn:
    """In-memory emit that prints + returns ascending seq ids. For dev/CLI demos."""
    state = {"seq": 0}

    def emit(body: dict[str, Any]) -> int:
        state["seq"] += 1
        line = json.dumps({"seq": state["seq"], **body}, default=str)
        print(line, flush=True)
        return state["seq"]

    return emit
