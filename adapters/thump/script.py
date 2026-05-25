"""
Script command group for cli-anything-bun.

Implements the high-level "script run" capability on top of the
Operation + Process foundation.

This is the first real user-facing command implemented in Phase 1.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Union

from .operations import Operation
from .process import Process, ProcessResult


@dataclass
class ScriptRunResult:
    """Clean, agent-friendly result of running a script."""
    name: str
    args: List[str]
    returncode: int
    duration: float
    success: bool
    cwd: Optional[str] = None
    pid: Optional[int] = None
    signal: Optional[int] = None
    timed_out: bool = False
    error: Optional[Dict[str, Any]] = None


def run(
    name: str,
    args: Optional[Sequence[str]] = None,
    cwd: Optional[Union[str, Path]] = None,
    env: Optional[Dict[str, str]] = None,
    timeout: Optional[float] = None,
    session_id: Optional[str] = None,
) -> ScriptRunResult:
    """
    Run a script defined in package.json (equivalent to `bun run <name>`).

    This is a transactional Operation that uses the live Process runner
    under the hood. All output is streamed as events (process.stdout / stderr)
    with full op_id correlation.

    Example:
        result = script.run("dev", cwd=Path("my-app"))
        # or inside a larger flow with an existing session:
        result = script.run("build", cwd=..., session_id="sess-123")

    Returns a ScriptRunResult. The full event stream (including raw output)
    is the primary observable artifact for agents and UIs.
    """
    args = list(args or [])
    cwd_path = Path(cwd).resolve() if cwd else Path.cwd()

    initial_data = {
        "name": name,
        "args": args,
        "cwd": str(cwd_path),
    }

    with Operation("script.run", initial_data, session_id=session_id) as op:
        cmd = ["bun", "run", name, *args]

        try:
            with Process(
                cmd,
                cwd=cwd_path,
                env=env,
                timeout=timeout,
                op=op,
            ) as proc:
                proc.wait()

            result = proc.result

            success = result.returncode == 0

            run_result = ScriptRunResult(
                name=name,
                args=args,
                returncode=result.returncode,
                duration=result.duration,
                success=success,
                cwd=str(cwd_path),
                pid=result.pid,
                signal=result.signal,
                timed_out=result.timed_out,
            )

            op.set_result(
                returncode=result.returncode,
                duration=round(result.duration, 3),
                success=success,
                pid=result.pid,
            )

            return run_result

        except FileNotFoundError as e:
            # "bun" binary not found or other spawn failure
            op.set_result(success=False, error={"type": "command_not_found", "message": str(e)})
            return ScriptRunResult(
                name=name,
                args=args,
                returncode=127,
                duration=0.0,
                success=False,
                cwd=str(cwd_path),
                error={"type": "command_not_found", "message": str(e)},
            )
        except Exception as e:
            op.set_result(success=False, error={"type": "runtime_error", "message": str(e)})
            return ScriptRunResult(
                name=name,
                args=args,
                returncode=-1,
                duration=0.0,
                success=False,
                cwd=str(cwd_path),
                error={"type": "runtime_error", "message": str(e)},
            )
