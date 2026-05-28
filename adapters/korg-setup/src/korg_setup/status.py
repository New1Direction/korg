"""Status report — what's installed, what's running, where the ledger is."""

from __future__ import annotations

import shutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from korg_setup.claude_config import DEFAULT_CONFIG_PATH, get_registered_server
from korg_setup.launchd import LABEL, PLIST_PATH, is_loaded, is_macos
from korg_setup.setup import (
    DEFAULT_LEDGER_DIR,
    DEFAULT_LEDGER_FILE,
    DEFAULT_MCP_SERVER_NAME,
)


@dataclass
class StatusReport:
    binaries: dict[str, Optional[str]] = field(default_factory=dict)
    mcp_registered: bool = False
    mcp_command: Optional[str] = None
    ledger_path: Optional[Path] = None
    ledger_size_bytes: int = 0
    ledger_lines: int = 0
    launchd_plist_exists: bool = False
    launchd_loaded: bool = False
    platform_supports_daemon: bool = False
    notes: list[str] = field(default_factory=list)


def gather_status(
    *,
    ledger_file: Path = DEFAULT_LEDGER_FILE,
    claude_config_path: Path = DEFAULT_CONFIG_PATH,
    mcp_server_name: str = DEFAULT_MCP_SERVER_NAME,
) -> StatusReport:
    report = StatusReport()

    # Binary discovery
    for name in ("korg-ingest-claude", "korg-recall-mcp"):
        path = shutil.which(name)
        report.binaries[name] = path

    # Claude config
    entry = get_registered_server(mcp_server_name, claude_config_path)
    if entry:
        report.mcp_registered = True
        report.mcp_command = entry.get("command")

    # Ledger inspection — best-effort line count without slurping the whole file.
    report.ledger_path = ledger_file
    if ledger_file.exists():
        report.ledger_size_bytes = ledger_file.stat().st_size
        # Count newlines — for the largest realistic ledger (~MBs), this is fast.
        # For multi-GB ledgers, we'd switch to a byte-counting estimate.
        try:
            with ledger_file.open("rb") as f:
                report.ledger_lines = sum(1 for _ in f)
        except OSError:
            report.notes.append(f"could not read {ledger_file} for line count")

    # Launchd
    report.platform_supports_daemon = is_macos()
    if report.platform_supports_daemon:
        report.launchd_plist_exists = PLIST_PATH.exists()
        report.launchd_loaded = is_loaded(LABEL)

    return report


def format_status(report: StatusReport) -> str:
    lines: list[str] = []

    # Binaries
    lines.append("Binaries:")
    for name, path in report.binaries.items():
        if path:
            lines.append(f"  ✓ {name:<22} {path}")
        else:
            lines.append(f"  ✗ {name:<22} not on PATH")

    # MCP registration
    lines.append("")
    lines.append("Claude Code MCP registration:")
    if report.mcp_registered:
        lines.append(f"  ✓ korg-recall registered with command {report.mcp_command}")
    else:
        lines.append("  ✗ korg-recall NOT registered. Run: korg-setup")

    # Ledger
    lines.append("")
    lines.append("Ledger:")
    if report.ledger_path and report.ledger_path.exists():
        kb = report.ledger_size_bytes / 1024
        lines.append(
            f"  ✓ {report.ledger_path}"
        )
        lines.append(
            f"      {report.ledger_lines} event(s), {kb:.1f} KiB"
        )
    else:
        lines.append(f"  · {report.ledger_path} (empty / not yet written)")

    # Daemon
    lines.append("")
    lines.append("Tail capture service:")
    if not report.platform_supports_daemon:
        lines.append("  · not supported on this platform; run --tail manually")
    elif report.launchd_loaded:
        lines.append(f"  ✓ launchd agent {LABEL} is RUNNING")
    elif report.launchd_plist_exists:
        lines.append(
            f"  ! launchd plist exists at {PLIST_PATH} but service is not loaded. "
            f"Run: launchctl load -w {PLIST_PATH}"
        )
    else:
        lines.append("  ✗ launchd agent not installed. Run: korg-setup")

    for note in report.notes:
        lines.append("")
        lines.append(f"Note: {note}")

    return "\n".join(lines)
