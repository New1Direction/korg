from __future__ import annotations

import json
from pathlib import Path

import pytest

from korg_setup.claude_settings import (
    DEFAULT_SETTINGS_PATH,
    HookSpec,
    ensure_hook_registered,
    get_registered_hook_events,
    load_settings,
    remove_hook,
)

CMD = "/usr/local/bin/korg-hook"


@pytest.fixture
def settings_path(tmp_path: Path) -> Path:
    return tmp_path / ".claude" / "settings.json"


def test_register_into_missing_file(settings_path):
    status, backup = ensure_hook_registered(HookSpec(command=CMD), settings_path)
    assert status == "added"
    assert backup is None
    s = load_settings(settings_path)
    for event in ("PostToolUse", "Stop", "SubagentStop"):
        groups = s["hooks"][event]
        assert groups[0]["hooks"][0]["command"] == CMD
        assert groups[0]["hooks"][0]["type"] == "command"


def test_register_preserves_existing_settings_and_hooks(settings_path):
    settings_path.parent.mkdir(parents=True)
    settings_path.write_text(json.dumps({
        "model": "opus",
        "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "other"}]}]},
    }))
    status, backup = ensure_hook_registered(HookSpec(command=CMD), settings_path)
    assert status == "added"
    assert backup is not None and backup.exists()
    s = load_settings(settings_path)
    assert s["model"] == "opus"                                   # unrelated key preserved
    assert s["hooks"]["PreToolUse"][0]["hooks"][0]["command"] == "other"  # other hook preserved
    assert s["hooks"]["PostToolUse"][0]["hooks"][0]["command"] == CMD


def test_register_is_idempotent(settings_path):
    ensure_hook_registered(HookSpec(command=CMD), settings_path)
    status, backup = ensure_hook_registered(HookSpec(command=CMD), settings_path)
    assert status == "unchanged"
    assert backup is None
    # exactly one group per event (no duplicates)
    s = load_settings(settings_path)
    assert len(s["hooks"]["PostToolUse"]) == 1


def test_register_adds_missing_events_only(settings_path):
    # pre-existing korg-hook on PostToolUse only; ensure fills Stop + SubagentStop
    settings_path.parent.mkdir(parents=True)
    settings_path.write_text(json.dumps({
        "hooks": {"PostToolUse": [{"matcher": "", "hooks": [{"type": "command", "command": CMD}]}]},
    }))
    status, _ = ensure_hook_registered(HookSpec(command=CMD), settings_path)
    assert status == "added"  # Stop + SubagentStop were missing
    s = load_settings(settings_path)
    assert len(s["hooks"]["PostToolUse"]) == 1  # not duplicated
    assert s["hooks"]["Stop"][0]["hooks"][0]["command"] == CMD


def test_remove_hook(settings_path):
    ensure_hook_registered(HookSpec(command=CMD), settings_path)
    status, backup = remove_hook(CMD, settings_path)
    assert status == "removed"
    assert backup is not None
    # our command is gone from every event; empty event arrays pruned
    assert get_registered_hook_events(CMD, settings_path) == []


def test_remove_absent_is_noop(settings_path):
    settings_path.parent.mkdir(parents=True)
    settings_path.write_text(json.dumps({"model": "opus"}))
    status, backup = remove_hook(CMD, settings_path)
    assert status == "absent"
    assert backup is None


def test_remove_preserves_other_hooks(settings_path):
    settings_path.parent.mkdir(parents=True)
    settings_path.write_text(json.dumps({
        "hooks": {"PostToolUse": [
            {"matcher": "", "hooks": [{"type": "command", "command": CMD}]},
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "keep-me"}]},
        ]},
    }))
    remove_hook(CMD, settings_path)
    s = load_settings(settings_path)
    cmds = [h["command"] for g in s["hooks"]["PostToolUse"] for h in g["hooks"]]
    assert cmds == ["keep-me"]
