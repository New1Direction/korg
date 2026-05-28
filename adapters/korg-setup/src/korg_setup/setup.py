"""End-to-end setup orchestration.

run_setup() does the following, idempotently:
  1. Verify that `korg-ingest-claude` and `korg-recall-mcp` are on PATH.
  2. Create `~/.korg/` (the ledger directory).
  3. Register the `korg-recall` MCP server in `~/.claude.json`.
  4. On macOS, install + start the `com.korg.ingest-claude` launchd agent.
     On Linux/other, print a one-liner the user can paste into a tmux
     session or systemd unit (TODO: native systemd-user support).

Returns a structured `SetupReport` so the CLI layer can print + exit
with a meaningful code, and tests can assert on what was done.
"""

from __future__ import annotations

import shutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from korg_setup.claude_config import (
    DEFAULT_CONFIG_PATH,
    ChangeStatus,
    McpServerSpec,
    ensure_mcp_server_registered,
)
from korg_setup.launchd import (
    LABEL,
    PLIST_PATH,
    PlistChangeStatus,
    UnsupportedPlatformError,
    install_service,
    is_macos,
)


DEFAULT_LEDGER_DIR = Path.home() / ".korg"
DEFAULT_LEDGER_FILE = DEFAULT_LEDGER_DIR / "claude-events.jsonl"
DEFAULT_TAIL_STATE = DEFAULT_LEDGER_DIR / "claude-tail-state.json"
DEFAULT_MCP_SERVER_NAME = "korg-recall"


@dataclass
class SetupStep:
    """One step's outcome — what was done and a human-readable summary."""

    name: str
    status: str  # "ok", "skip", "warn", "fail"
    detail: str = ""


@dataclass
class SetupReport:
    steps: list[SetupStep] = field(default_factory=list)
    backup_path: Optional[Path] = None
    plist_status: Optional[PlistChangeStatus] = None
    config_status: Optional[ChangeStatus] = None
    overall_ok: bool = True

    def add(self, name: str, status: str, detail: str = "") -> None:
        self.steps.append(SetupStep(name=name, status=status, detail=detail))
        if status == "fail":
            self.overall_ok = False


# ── Public entry point ────────────────────────────────────────────────


def run_setup(
    *,
    ledger_dir: Path = DEFAULT_LEDGER_DIR,
    ledger_file: Path = DEFAULT_LEDGER_FILE,
    tail_state: Path = DEFAULT_TAIL_STATE,
    claude_config_path: Path = DEFAULT_CONFIG_PATH,
    mcp_server_name: str = DEFAULT_MCP_SERVER_NAME,
    install_daemon: bool = True,
    dry_run: bool = False,
) -> SetupReport:
    """Run the full setup. Each step is idempotent; safe to re-run."""
    report = SetupReport()

    # 1. Binaries on PATH
    ingest_bin = shutil.which("korg-ingest-claude")
    recall_bin = shutil.which("korg-recall-mcp")
    if not ingest_bin:
        report.add(
            "binaries",
            "fail",
            "korg-ingest-claude not on PATH. Install the claude-code adapter "
            "(pip install -e adapters/claude-code) and re-run.",
        )
    if not recall_bin:
        report.add(
            "binaries",
            "fail",
            "korg-recall-mcp not on PATH. Install the recall-mcp adapter "
            "(pip install -e 'adapters/recall-mcp[semantic]') and re-run.",
        )
    if ingest_bin and recall_bin:
        report.add(
            "binaries",
            "ok",
            f"korg-ingest-claude={ingest_bin}; korg-recall-mcp={recall_bin}",
        )
    if not report.overall_ok:
        # Bail early — none of the next steps make sense without binaries.
        return report

    # 2. Ledger dir
    if dry_run:
        report.add("ledger_dir", "ok", f"would create {ledger_dir} (dry-run)")
    else:
        ledger_dir.mkdir(parents=True, exist_ok=True)
        report.add("ledger_dir", "ok", f"ensured {ledger_dir}")

    # 3. ~/.claude.json — register the MCP server
    spec = McpServerSpec(
        name=mcp_server_name,
        command=recall_bin,  # absolute path so launchd / Claude both find it
        args=["--ledger", str(ledger_file)],
    )
    if dry_run:
        report.add(
            "claude_config",
            "ok",
            f"would register MCP server '{mcp_server_name}' in {claude_config_path}",
        )
        report.config_status = "added"
    else:
        try:
            status, backup = ensure_mcp_server_registered(spec, claude_config_path)
            report.config_status = status
            report.backup_path = backup
            if status == "added":
                report.add(
                    "claude_config",
                    "ok",
                    f"added '{mcp_server_name}' to {claude_config_path}",
                )
            elif status == "updated":
                report.add(
                    "claude_config",
                    "ok",
                    f"updated existing '{mcp_server_name}' in {claude_config_path}",
                )
            else:
                report.add(
                    "claude_config",
                    "skip",
                    f"'{mcp_server_name}' already registered identically in {claude_config_path}",
                )
        except Exception as e:
            report.add(
                "claude_config",
                "fail",
                f"could not edit {claude_config_path}: {e}",
            )
            return report

    # 4. Background service
    if not install_daemon:
        report.add("daemon", "skip", "install_daemon=False; skipping launchd")
        return report

    if is_macos():
        if dry_run:
            report.add(
                "daemon",
                "ok",
                f"would install launchd agent {LABEL} (plist at {PLIST_PATH})",
            )
        else:
            try:
                tail_args = ["--tail", "--state", str(tail_state), "--out", str(ledger_file)]
                plist_status, launchctl_result = install_service(extra_args=tail_args)
                report.plist_status = plist_status
                if launchctl_result is not None and launchctl_result.returncode != 0:
                    report.add(
                        "daemon",
                        "warn",
                        f"plist {plist_status}; launchctl load said: "
                        f"{launchctl_result.stderr.strip() or launchctl_result.stdout.strip()}",
                    )
                else:
                    report.add(
                        "daemon",
                        "ok",
                        f"launchd agent {LABEL} {plist_status} and started",
                    )
            except UnsupportedPlatformError as e:
                report.add("daemon", "warn", str(e))
            except Exception as e:
                report.add("daemon", "fail", f"launchd install failed: {e}")
    else:
        # Linux + other: print the one-liner the user should run.
        # systemd-user support is a follow-up.
        hint = (
            f"{ingest_bin} --tail --state {tail_state} --out {ledger_file}"
        )
        report.add(
            "daemon",
            "warn",
            "Auto-restart service install not yet supported on this platform.\n"
            f"  Run this in a tmux / screen session:\n    {hint}\n"
            "  Or wire it into a systemd-user unit.",
        )

    return report


# ── Pretty-printing ───────────────────────────────────────────────────


def format_report(report: SetupReport) -> str:
    lines = []
    icons = {"ok": "✓", "skip": "·", "warn": "!", "fail": "✗"}
    for step in report.steps:
        icon = icons.get(step.status, "?")
        first, *rest = step.detail.split("\n") if step.detail else [""]
        lines.append(f"  {icon} {step.name:<14} {first}")
        for r in rest:
            lines.append(f"                   {r}")
    if report.overall_ok:
        lines.append("")
        lines.append("Setup complete. Restart Claude Code to load the new MCP server.")
        if report.plist_status in {"created", "updated"}:
            lines.append(
                "Tail capture is now running in the background (launchd). "
                "Check status with: korg-setup status"
            )
    else:
        lines.append("")
        lines.append("Setup did not complete cleanly. Fix the issues above and re-run.")
    return "\n".join(lines)
