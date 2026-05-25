"""
Process runner abstraction for thump (Thumper's Bun semantic layer).

Provides live, observable execution of child processes (Bun commands,
scripts, dev servers, etc.) while preserving raw output and emitting
structured events through the stable envelope.

Supports transparent delegation to the native Rust Bun executor (thump internal run-bun).

The decision is made by `should_prefer_native_bun()` which checks:
- Explicit env var `THUMP_PREFER_NATIVE_BUN` (preferred) or legacy `API_ANYTHING_PREFER_NATIVE_BUN`
- Heuristic parent-process detection (are we running under `thump`, `thumper`, `bunny`, `thump-cli`, or legacy `api-anything`?)

This is the official thin shim for making the native execution
layer the default during absorb and generation flows while remaining
fully reversible and dependency-free.

Core guarantees:
- Raw stdout/stderr is never lost (emitted via raw_stdout / raw_stderr)
- Every stream event is correlated via op_id when an Operation is provided
- Clean context-manager lifetime
- Timeout + signal handling with normalized errors
- Exit code and timing are always available in the result

Designed to be used inside an Operation:

    with Operation("script.run", {"cmd": ["bun", "run", "dev"]}) as op:
        with Process(["bun", "run", "dev"], cwd=..., op=op) as proc:
            ...
        op.set_result(returncode=proc.result.returncode, duration=proc.result.duration)
"""

from __future__ import annotations

import os
import shutil
import signal
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Union

from .events import EventLevel, emit, raw_stdout, raw_stderr
from .operations import Operation


# ---------------------------------------------------------------------------
# Thin compatibility shim for preferring the native Rust Bun executor
# (Chunk 15 - Native Bun Execution Layer v2)
#
# This shim allows the absorb/generation flows (and any other Python code
# using cli_anything_bun) to automatically prefer the fast native Rust runner
# (`api-anything internal run-bun`) when running under the api-anything TUI/CLI.
#
# It is designed to be:
#   - Low risk and fully reversible
#   - Dependency-free
#   - Easy to disable
#
# Control:
#   THUMP_PREFER_NATIVE_BUN=1            → Force native (new canonical)
#   API_ANYTHING_PREFER_NATIVE_BUN=1     → Force native (legacy fallback)
#   THUMP_PREFER_NATIVE_BUN=0            → Force system `bun`
#   Unset                                → Use heuristic (parent process detection)
# ---------------------------------------------------------------------------

def should_prefer_native_bun() -> bool:
    """
    Returns True if the native Rust Bun executor should be preferred over
    the system `bun` binary.

    Detection order (highest priority first):
    1. Explicit environment variable override (THUMP_PREFER_NATIVE_BUN preferred, falls back to legacy API_ANYTHING_*)
    2. Environmental active indicator: THUMP_PARENT_ACTIVE or legacy API_ANYTHING_PARENT_ACTIVE env var is set to "1"
    3. Default: False (stay on system `bun`)
    """
    # 1. New canonical env var first
    override = os.environ.get("THUMP_PREFER_NATIVE_BUN")
    if override is None:
        # Fallback to old env var name if users have it cached from earlier builds
        override = os.environ.get("API_ANYTHING_PREFER_NATIVE_BUN")

    if override is not None:
        val = override.lower().strip()
        if val in ("1", "true", "yes", "on"):
            return True
        if val in ("0", "false", "no", "off"):
            return False

    # 2. Environmental parent indicator
    if os.environ.get("THUMP_PARENT_ACTIVE") == "1":
        return True
    if os.environ.get("API_ANYTHING_PARENT_ACTIVE") == "1":
        return True
    if os.environ.get("THUMP_FORCE_NATIVE", "").lower() in ("1", "true", "yes"):
        return True

    return False


def get_bun_executor() -> str:
    """
    Public helper: returns the executable that should be used to run Bun commands.

    Respects `should_prefer_native_bun()` (env var + parent process detection).
    When native is preferred, prefers `thump` (or any of its aliases) in PATH,
    falling back to legacy `api-anything` during transition.
    """
    if should_prefer_native_bun():
        # Try the new primary binary first, then aliases, then legacy name
        for candidate in ["thump", "thumper", "bunny", "thump-cli", "api-anything"]:
            exe = shutil.which(candidate)
            if exe:
                return exe
    return "bun"


def rewrite_bun_command(cmd: List[str]) -> List[str]:
    """
    Public helper that rewrites a logical Bun command to use the native
    Rust runner when appropriate.

    This is the main entry point used by the Process class.
    The rewrite is only performed when `should_prefer_native_bun()` returns True.

    The change is fully reversible via environment variable.
    """
    if not cmd or cmd[0] != "bun":
        return cmd

    if not should_prefer_native_bun():
        return cmd

    executor = get_bun_executor()
    # Any of the Thumper-family binaries (or legacy) should trigger the internal delegation
    if executor != "bun" and any(alias in executor for alias in ["thump", "thumper", "bunny", "thump-cli", "api-anything"]):
        # Rewrite to the internal native entry point
        return [executor, "internal", "run-bun"] + cmd[1:]
    return cmd


@dataclass
class ProcessResult:
    """Structured outcome of a completed process (source of truth lives in events too)."""
    cmd: List[str]
    returncode: int
    pid: int
    duration: float  # seconds
    cwd: Optional[str] = None
    signal: Optional[int] = None
    timed_out: bool = False
    error: Optional[Dict[str, Any]] = None   # normalized error if we killed it ourselves


class Process:
    """
    Context manager that runs a subprocess and streams its output as events.

    When `should_prefer_native_bun()` returns True (controlled by the
    `THUMP_PREFER_NATIVE_BUN` environment variable or automatic
    parent-process detection), commands starting with `bun` are transparently
    rewritten (via `rewrite_bun_command`) to use the native Rust execution layer
    (`thump internal run-bun ...`).

    Raw output is emitted immediately using the raw_* helpers (process.stdout / process.stderr
    events with "raw" payload). Structured lifecycle events are also emitted.

    Example (recommended pattern):

        with Operation("script.run", {"script": "dev"}) as op:
            with Process(
                ["bun", "run", "dev"],
                cwd=Path.cwd(),
                op=op,
                timeout=300,
            ) as proc:
                # stdout/stderr are already being emitted live
                pass
            op.set_result(returncode=proc.result.returncode)
    """

    def __init__(
        self,
        cmd: List[str],
        cwd: Optional[Union[str, Path]] = None,
        env: Optional[Dict[str, str]] = None,
        timeout: Optional[float] = None,
        op: Optional[Operation] = None,
        stdin_data: Optional[Union[str, bytes]] = None,
        # Future: shell=False by default (safer)
    ):
        # Apply thin native shim (Chunk 14)
        self.cmd = rewrite_bun_command(list(cmd))
        self.cwd = str(cwd) if cwd else None
        self.env = env or os.environ.copy()
        self.timeout = timeout
        self.op = op
        self.stdin_data = stdin_data

        self._proc: Optional[subprocess.Popen] = None
        self._result: Optional[ProcessResult] = None
        self._start_time: Optional[float] = None
        self._threads: List[threading.Thread] = []
        self._stdout_thread: Optional[threading.Thread] = None
        self._stderr_thread: Optional[threading.Thread] = None
        self._timeout_triggered = False

    @property
    def result(self) -> Optional[ProcessResult]:
        return self._result

    @property
    def pid(self) -> Optional[int]:
        return self._proc.pid if self._proc else None

    @property
    def returncode(self) -> Optional[int]:
        return self._proc.returncode if self._proc else None

    def __enter__(self) -> "Process":
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> bool:
        self.wait()
        return False

    def start(self) -> "Process":
        """Spawn the child process and begin live streaming of stdout/stderr."""
        if self._proc is not None:
            return self

        self._start_time = time.time()

        # Prepare stdin
        stdin = subprocess.PIPE if self.stdin_data is not None else None

        try:
            self._proc = subprocess.Popen(
                self.cmd,
                cwd=self.cwd,
                env=self.env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                stdin=stdin,
                text=True,
                bufsize=1,  # line buffered
            )
        except FileNotFoundError as e:
            self._emit_error("process_not_found", str(e))
            raise

        # Emit structured start event (correlated if we have an op)
        self._emit(
            "process.started",
            {
                "pid": self._proc.pid,
                "cmd": self.cmd,
                "cwd": self.cwd,
            },
            level=EventLevel.INFO,
        )

        # Start reader threads (they emit raw events as data arrives)
        self._stdout_thread = threading.Thread(
            target=self._read_stream,
            args=(self._proc.stdout, "stdout"),
            daemon=True,
            name=f"bun-proc-stdout-{self._proc.pid}",
        )
        self._stderr_thread = threading.Thread(
            target=self._read_stream,
            args=(self._proc.stderr, "stderr"),
            daemon=True,
            name=f"bun-proc-stderr-{self._proc.pid}",
        )

        self._stdout_thread.start()
        self._stderr_thread.start()
        self._threads = [self._stdout_thread, self._stderr_thread]

        # If we have stdin data, write it and close
        if self.stdin_data is not None and self._proc.stdin:
            if isinstance(self.stdin_data, str):
                self._proc.stdin.write(self.stdin_data)
            else:
                self._proc.stdin.write(self.stdin_data.decode("utf-8", errors="replace"))
            self._proc.stdin.close()

        return self

    def wait(self, timeout: Optional[float] = None) -> ProcessResult:
        """
        Wait for the process to exit (or timeout).

        Returns the final ProcessResult. If a timeout occurs, the process
        is terminated and a normalized error is recorded.
        """
        if self._result is not None:
            return self._result

        effective_timeout = timeout or self.timeout
        killed_by_timeout = False

        try:
            if effective_timeout is not None:
                try:
                    self._proc.wait(timeout=effective_timeout)
                except subprocess.TimeoutExpired:
                    killed_by_timeout = True
                    self._timeout_triggered = True
                    self._terminate()
            else:
                self._proc.wait()
        finally:
            # Make sure reader threads see EOF
            self._join_readers()

        duration = time.time() - self._start_time if self._start_time else 0.0

        # Determine signal if any
        sig = None
        if self._proc.returncode is not None and self._proc.returncode < 0:
            sig = -self._proc.returncode

        self._result = ProcessResult(
            cmd=self.cmd,
            returncode=self._proc.returncode,
            pid=self._proc.pid,
            duration=duration,
            cwd=self.cwd,
            signal=sig,
            timed_out=killed_by_timeout,
        )

        # Emit structured exit event
        exit_data: Dict[str, Any] = {
            "returncode": self._proc.returncode,
            "duration": round(duration, 3),
            "pid": self._proc.pid,
        }
        if sig is not None:
            exit_data["signal"] = sig
        if killed_by_timeout:
            exit_data["timed_out"] = True

        self._emit("process.exited", exit_data, level=EventLevel.INFO)

        return self._result

    def _terminate(self) -> None:
        """Best-effort graceful then forceful termination."""
        if not self._proc or self._proc.poll() is not None:
            return

        try:
            self._proc.send_signal(signal.SIGTERM)
            try:
                self._proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                self._proc.kill()
                self._proc.wait(timeout=1.0)
        except Exception:
            # Last resort
            try:
                self._proc.kill()
            except Exception:
                pass

    def _read_stream(self, stream: IO[str], stream_name: str) -> None:
        """Read lines from stdout or stderr and emit them as raw events immediately."""
        if stream is None:
            return

        emit_fn = raw_stdout if stream_name == "stdout" else raw_stderr

        try:
            for line in iter(stream.readline, ""):
                if line:
                    # Preserve the line exactly (including trailing newline if present)
                    emit_fn(
                        line,
                        op_id=self.op.op_id if self.op else None,
                        session_id=self.op.session_id if self.op else None,
                    )
        except Exception:
            # Never let a reader thread crash the whole runner
            pass
        finally:
            try:
                stream.close()
            except Exception:
                pass

    def _join_readers(self) -> None:
        for t in self._threads:
            if t.is_alive():
                t.join(timeout=1.0)

    def _emit(self, event: str, data: Dict[str, Any], level: EventLevel = EventLevel.INFO) -> None:
        """Emit a structured event, correlated with the Operation if present."""
        if self.op:
            self.op.emit(event, data, level=level)
        else:
            emit(
                event,
                data,
                level=level,
                op_id=None,
                session_id=None,
            )

    def _emit_error(self, error_type: str, message: str) -> None:
        """Emit a normalized error (used for early failures like 'command not found')."""
        err = {
            "type": error_type,
            "recoverable": False,
            "retryable": False,
            "message": message,
        }
        self._emit("process.error", err, level=EventLevel.ERROR)


# ---------------------------------------------------------------------------
# Convenience function (very useful for simple one-shot commands)
# ---------------------------------------------------------------------------

def run_process(
    cmd: List[str],
    cwd: Optional[Union[str, Path]] = None,
    env: Optional[Dict[str, str]] = None,
    timeout: Optional[float] = None,
    op: Optional[Operation] = None,
    stdin_data: Optional[Union[str, bytes]] = None,
) -> ProcessResult:
    """
    Run a command to completion and return the ProcessResult.

    This is the simplest way to execute a one-shot Bun command inside an Operation.
    """
    with Process(cmd, cwd=cwd, env=env, timeout=timeout, op=op, stdin_data=stdin_data) as proc:
        return proc.wait()
