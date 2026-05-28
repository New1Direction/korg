"""macOS launchd integration — installs `korg-ingest-claude --tail` as a
background service that starts at login and auto-restarts on failure.

The plist lives at `~/Library/LaunchAgents/com.korg.ingest-claude.plist`.
Loading/unloading uses `launchctl` so the OS supervises the process.

This module degrades gracefully on non-macOS platforms — every public
function raises `UnsupportedPlatformError` rather than partially-working.
The setup CLI falls back to `nohup` background on Linux until systemd-user
support lands (TODO).
"""

from __future__ import annotations

import os
import plistlib
import platform
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Literal


LABEL = "com.korg.ingest-claude"
PLIST_PATH = Path.home() / "Library" / "LaunchAgents" / f"{LABEL}.plist"
LOG_DIR = Path.home() / "Library" / "Logs"
STDOUT_LOG = LOG_DIR / "korg-ingest-claude.log"
STDERR_LOG = LOG_DIR / "korg-ingest-claude.err"


class UnsupportedPlatformError(RuntimeError):
    pass


def is_macos() -> bool:
    return platform.system() == "Darwin"


def require_macos() -> None:
    if not is_macos():
        raise UnsupportedPlatformError(
            f"launchd is macOS-only; this is {platform.system()}. "
            f"For Linux, use the --nohup fallback (or write a systemd unit)."
        )


@dataclass
class PlistSpec:
    """The plist payload — separated from disk I/O so tests can assert on shape."""

    label: str
    program_arguments: list[str]
    stdout_path: Path
    stderr_path: Path
    run_at_load: bool = True
    keep_alive: bool = True
    working_directory: Path | None = None

    def to_dict(self) -> dict:
        out: dict = {
            "Label": self.label,
            "ProgramArguments": list(self.program_arguments),
            "RunAtLoad": self.run_at_load,
            "KeepAlive": self.keep_alive,
            "StandardOutPath": str(self.stdout_path),
            "StandardErrorPath": str(self.stderr_path),
        }
        if self.working_directory is not None:
            out["WorkingDirectory"] = str(self.working_directory)
        return out


def build_spec(
    command_path: Path,
    extra_args: list[str] | None = None,
) -> PlistSpec:
    args = [str(command_path)] + (extra_args or [])
    return PlistSpec(
        label=LABEL,
        program_arguments=args,
        stdout_path=STDOUT_LOG,
        stderr_path=STDERR_LOG,
    )


def find_korg_ingest_claude() -> Path:
    """Locate the `korg-ingest-claude` binary that launchd will exec.

    Raises FileNotFoundError with a helpful message if the binary isn't on PATH.
    """
    path = shutil.which("korg-ingest-claude")
    if path is None:
        raise FileNotFoundError(
            "korg-ingest-claude not on PATH. Install the claude-code adapter first:\n"
            "    pip install -e /path/to/Korg/adapters/claude-code"
        )
    return Path(path)


# ── Plist install / uninstall ─────────────────────────────────────────


PlistChangeStatus = Literal["created", "updated", "unchanged"]


def write_plist(
    spec: PlistSpec,
    plist_path: Path = PLIST_PATH,
) -> PlistChangeStatus:
    """Write the plist file atomically. Returns whether the content changed."""
    plist_path.parent.mkdir(parents=True, exist_ok=True)
    LOG_DIR.mkdir(parents=True, exist_ok=True)

    new_bytes = plistlib.dumps(spec.to_dict())
    if plist_path.exists():
        existing = plist_path.read_bytes()
        if existing == new_bytes:
            return "unchanged"

    tmp = plist_path.with_suffix(plist_path.suffix + ".tmp")
    tmp.write_bytes(new_bytes)
    os.replace(tmp, plist_path)
    return "created" if not plist_path.exists() else "updated"


def load_service(plist_path: Path = PLIST_PATH) -> subprocess.CompletedProcess:
    """`launchctl load -w` the plist (starts the service)."""
    require_macos()
    return subprocess.run(
        ["launchctl", "load", "-w", str(plist_path)],
        capture_output=True,
        text=True,
        check=False,
    )


def unload_service(plist_path: Path = PLIST_PATH) -> subprocess.CompletedProcess:
    """`launchctl unload -w` the plist (stops + disables it)."""
    require_macos()
    return subprocess.run(
        ["launchctl", "unload", "-w", str(plist_path)],
        capture_output=True,
        text=True,
        check=False,
    )


def is_loaded(label: str = LABEL) -> bool:
    """True if launchctl knows about our service label right now."""
    if not is_macos():
        return False
    result = subprocess.run(
        ["launchctl", "list"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return False
    for line in result.stdout.splitlines():
        # Format: PID Status Label
        parts = line.split()
        if len(parts) >= 3 and parts[2] == label:
            return True
    return False


def install_service(
    extra_args: list[str] | None = None,
    plist_path: Path = PLIST_PATH,
) -> tuple[PlistChangeStatus, subprocess.CompletedProcess | None]:
    """End-to-end: build → write plist → launchctl load. Idempotent.

    Returns (plist_status, launchctl_result). launchctl_result is None
    when the plist was unchanged and the service was already loaded.
    """
    require_macos()
    binary = find_korg_ingest_claude()
    spec = build_spec(binary, extra_args)
    status = write_plist(spec, plist_path)

    # If the service is already loaded AND the plist didn't change,
    # we don't need to bounce it.
    if status == "unchanged" and is_loaded():
        return status, None

    # If it was loaded with an old plist, unload first to clear the old one.
    if is_loaded():
        unload_service(plist_path)
    return status, load_service(plist_path)


def uninstall_service(
    plist_path: Path = PLIST_PATH,
) -> tuple[Literal["removed", "absent"], subprocess.CompletedProcess | None]:
    """Stop + unload + delete the plist. Idempotent."""
    require_macos()
    unload_result: subprocess.CompletedProcess | None = None
    if is_loaded():
        unload_result = unload_service(plist_path)
    if plist_path.exists():
        plist_path.unlink()
        return "removed", unload_result
    return "absent", unload_result
