"""Atomic, idempotent edits to ~/.claude/settings.json (Claude Code hooks).

Sibling of claude_config.py (which owns ~/.claude.json / MCP servers). This
module owns the *hooks* file: it registers the korg-hook capture command
under hooks.{PostToolUse,Stop,SubagentStop}. Write-precious: read → modify →
atomic-rename, backup to `.korg-backup`, idempotent, preserve every other key.
"""
from __future__ import annotations

import json
import os
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

DEFAULT_SETTINGS_PATH = Path.home() / ".claude" / "settings.json"
HOOK_EVENTS = ("PostToolUse", "Stop", "SubagentStop")

ChangeStatus = Literal["added", "unchanged"]


@dataclass
class HookSpec:
    """A command hook to register across one or more hook events."""

    command: str
    events: tuple[str, ...] = HOOK_EVENTS
    matcher: str = ""  # "" matches all tools (PostToolUse); ignored for Stop/SubagentStop


def load_settings(settings_path: Path = DEFAULT_SETTINGS_PATH) -> dict[str, Any]:
    """Load settings.json. Returns {} if the file doesn't exist."""
    if not settings_path.exists():
        return {}
    return json.loads(settings_path.read_text())


def save_settings_atomic(
    settings: dict[str, Any],
    settings_path: Path = DEFAULT_SETTINGS_PATH,
    *,
    backup_suffix: str = ".korg-backup",
) -> Path | None:
    """Write atomically via tmp-rename; back up any prior file first."""
    settings_path.parent.mkdir(parents=True, exist_ok=True)
    backup_path: Path | None = None
    if settings_path.exists():
        backup_path = Path(str(settings_path) + backup_suffix)
        shutil.copy2(settings_path, backup_path)
    tmp = Path(str(settings_path) + ".tmp")
    tmp.write_text(json.dumps(settings, indent=2) + "\n")
    os.replace(tmp, settings_path)
    return backup_path


def _command_in_groups(groups: list, command: str) -> bool:
    for g in groups:
        for h in (g.get("hooks") or []):
            if h.get("type") == "command" and h.get("command") == command:
                return True
    return False


def ensure_hook_registered(
    spec: HookSpec,
    settings_path: Path = DEFAULT_SETTINGS_PATH,
) -> tuple[ChangeStatus, Path | None]:
    """Idempotently register `spec.command` under each event in `spec.events`.

    ("added", backup)     — at least one event was missing the command; written.
    ("unchanged", None)   — every event already had it; no write.
    """
    settings = load_settings(settings_path)
    hooks = settings.get("hooks") or {}
    changed = False
    for event in spec.events:
        groups = hooks.get(event) or []
        if _command_in_groups(groups, spec.command):
            continue
        groups.append({
            "matcher": spec.matcher,
            "hooks": [{"type": "command", "command": spec.command}],
        })
        hooks[event] = groups
        changed = True
    if not changed:
        return ("unchanged", None)
    settings["hooks"] = hooks
    backup = save_settings_atomic(settings, settings_path)
    return ("added", backup)


def remove_hook(
    command: str,
    settings_path: Path = DEFAULT_SETTINGS_PATH,
) -> tuple[Literal["removed", "absent"], Path | None]:
    """Remove every command-hook matching `command` across all events.

    Prunes emptied groups and emptied event arrays. Idempotent.
    """
    settings = load_settings(settings_path)
    hooks = settings.get("hooks") or {}
    found = False
    for event, groups in list(hooks.items()):
        new_groups = []
        for g in groups:
            original = g.get("hooks") or []
            kept = [h for h in original
                    if not (h.get("type") == "command" and h.get("command") == command)]
            if len(kept) != len(original):
                found = True
            if kept:
                new_groups.append({**g, "hooks": kept})
        if new_groups:
            hooks[event] = new_groups
        else:
            del hooks[event]
    if not found:
        return ("absent", None)
    settings["hooks"] = hooks
    backup = save_settings_atomic(settings, settings_path)
    return ("removed", backup)


def get_registered_hook_events(
    command: str,
    settings_path: Path = DEFAULT_SETTINGS_PATH,
) -> list[str]:
    """Return the hook events that currently carry `command`."""
    settings = load_settings(settings_path)
    hooks = settings.get("hooks") or {}
    return [event for event, groups in hooks.items() if _command_in_groups(groups, command)]
