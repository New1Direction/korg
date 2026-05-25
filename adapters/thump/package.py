"""
Package command group for cli-anything-bun.

Implements the high-level package management commands on top of
Operation + Process:

- package.add(...)
- package.install(...)
- package.remove(...)

These are the transactional counterparts to script_run.
All commands produce rich event streams (raw output + lifecycle)
and return clean result objects.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Union

from .operations import Operation
from .process import Process


@dataclass
class PackageAddResult:
    """Result of `package.add`."""
    packages: List[str]
    returncode: int
    duration: float
    success: bool
    cwd: Optional[str] = None
    pid: Optional[int] = None
    signal: Optional[int] = None
    timed_out: bool = False
    error: Optional[Dict[str, Any]] = None


@dataclass
class PackageInstallResult:
    """Result of `package.install`."""
    packages: List[str]   # empty list means "install from lockfile / package.json"
    returncode: int
    duration: float
    success: bool
    cwd: Optional[str] = None
    pid: Optional[int] = None
    signal: Optional[int] = None
    timed_out: bool = False
    error: Optional[Dict[str, Any]] = None


@dataclass
class PackageRemoveResult:
    """Result of `package.remove`."""
    packages: List[str]
    returncode: int
    duration: float
    success: bool
    cwd: Optional[str] = None
    pid: Optional[int] = None
    signal: Optional[int] = None
    timed_out: bool = False
    error: Optional[Dict[str, Any]] = None


def _build_bun_cmd(base: str, packages: Sequence[str], extra: List[str]) -> List[str]:
    cmd = ["bun", base]
    if packages:
        cmd.extend(packages)
    if extra:
        cmd.extend(extra)
    return cmd


def add(
    packages: Sequence[str],
    cwd: Optional[Union[str, Path]] = None,
    dev: bool = False,
    exact: bool = False,
    peer: bool = False,
    optional: bool = False,
    env: Optional[Dict[str, str]] = None,
    timeout: Optional[float] = None,
    extra_args: Optional[Sequence[str]] = None,
    session_id: Optional[str] = None,
) -> PackageAddResult:
    """
    Add one or more packages (equivalent to `bun add <packages>`).

    Supports the most common flags:
        dev=True      -> --dev
        exact=True    -> --exact
        peer=True     -> --peer
        optional=True -> --optional

    Extra flags can be passed via extra_args if needed.
    """
    if not packages:
        raise ValueError("package.add requires at least one package name")

    packages = list(packages)
    cwd_path = Path(cwd).resolve() if cwd else Path.cwd()
    extra_args = list(extra_args or [])

    flags = []
    if dev:
        flags.append("--dev")
    if exact:
        flags.append("--exact")
    if peer:
        flags.append("--peer")
    if optional:
        flags.append("--optional")

    cmd = _build_bun_cmd("add", packages, flags + extra_args)

    initial_data = {
        "packages": packages,
        "flags": {"dev": dev, "exact": exact, "peer": peer, "optional": optional},
        "cwd": str(cwd_path),
    }

    with Operation("package.add", initial_data, session_id=session_id) as op:
        try:
            with Process(cmd, cwd=cwd_path, env=env, timeout=timeout, op=op) as proc:
                proc.wait()

            res = proc.result
            success = res.returncode == 0

            op.set_result(
                packages=packages,
                returncode=res.returncode,
                duration=round(res.duration, 3),
                success=success,
                pid=res.pid,
            )

            return PackageAddResult(
                packages=packages,
                returncode=res.returncode,
                duration=res.duration,
                success=success,
                cwd=str(cwd_path),
                pid=res.pid,
                signal=res.signal,
                timed_out=res.timed_out,
            )

        except Exception as e:
            op.set_result(success=False, error={"message": str(e)})
            return PackageAddResult(
                packages=packages,
                returncode=-1,
                duration=0.0,
                success=False,
                cwd=str(cwd_path),
                error={"type": "runtime_error", "message": str(e)},
            )


def install(
    packages: Optional[Sequence[str]] = None,
    cwd: Optional[Union[str, Path]] = None,
    frozen_lockfile: bool = False,
    env: Optional[Dict[str, str]] = None,
    timeout: Optional[float] = None,
    extra_args: Optional[Sequence[str]] = None,
    session_id: Optional[str] = None,
) -> PackageInstallResult:
    """
    Run `bun install`.

    - If `packages` is provided (non-empty), they are passed after `bun install`
      (Bun treats this similarly to adding + installing).
    - `frozen_lockfile=True` → `--frozen-lockfile`
    """
    packages = list(packages or [])
    cwd_path = Path(cwd).resolve() if cwd else Path.cwd()
    extra_args = list(extra_args or [])

    flags = []
    if frozen_lockfile:
        flags.append("--frozen-lockfile")

    cmd = _build_bun_cmd("install", packages, flags + extra_args)

    initial_data = {
        "packages": packages,
        "frozen_lockfile": frozen_lockfile,
        "cwd": str(cwd_path),
    }

    op_name = "package.install"
    with Operation(op_name, initial_data, session_id=session_id) as op:
        try:
            with Process(cmd, cwd=cwd_path, env=env, timeout=timeout, op=op) as proc:
                proc.wait()

            res = proc.result
            success = res.returncode == 0

            op.set_result(
                packages=packages,
                returncode=res.returncode,
                duration=round(res.duration, 3),
                success=success,
            )

            return PackageInstallResult(
                packages=packages,
                returncode=res.returncode,
                duration=res.duration,
                success=success,
                cwd=str(cwd_path),
                pid=res.pid,
                signal=res.signal,
                timed_out=res.timed_out,
            )

        except Exception as e:
            op.set_result(success=False, error={"message": str(e)})
            return PackageInstallResult(
                packages=packages,
                returncode=-1,
                duration=0.0,
                success=False,
                cwd=str(cwd_path),
                error={"type": "runtime_error", "message": str(e)},
            )


def remove(
    packages: Sequence[str],
    cwd: Optional[Union[str, Path]] = None,
    env: Optional[Dict[str, str]] = None,
    timeout: Optional[float] = None,
    extra_args: Optional[Sequence[str]] = None,
    session_id: Optional[str] = None,
) -> PackageRemoveResult:
    """
    Remove one or more packages (equivalent to `bun remove <packages>`).
    """
    if not packages:
        raise ValueError("package.remove requires at least one package name")

    packages = list(packages)
    cwd_path = Path(cwd).resolve() if cwd else Path.cwd()
    extra_args = list(extra_args or [])

    cmd = _build_bun_cmd("remove", packages, extra_args)

    initial_data = {
        "packages": packages,
        "cwd": str(cwd_path),
    }

    with Operation("package.remove", initial_data, session_id=session_id) as op:
        try:
            with Process(cmd, cwd=cwd_path, env=env, timeout=timeout, op=op) as proc:
                proc.wait()

            res = proc.result
            success = res.returncode == 0

            op.set_result(
                packages=packages,
                returncode=res.returncode,
                duration=round(res.duration, 3),
                success=success,
            )

            return PackageRemoveResult(
                packages=packages,
                returncode=res.returncode,
                duration=res.duration,
                success=success,
                cwd=str(cwd_path),
                pid=res.pid,
                signal=res.signal,
                timed_out=res.timed_out,
            )

        except Exception as e:
            op.set_result(success=False, error={"message": str(e)})
            return PackageRemoveResult(
                packages=packages,
                returncode=-1,
                duration=0.0,
                success=False,
                cwd=str(cwd_path),
                error={"type": "runtime_error", "message": str(e)},
            )
