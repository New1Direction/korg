# Phase 1 — Plan 4: `korg-setup` Hook Registration + Daemon Demotion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make verifiable capture turn on with one command — `korg-setup` registers the `korg-hook` command in `~/.claude/settings.json`, and the macOS-only launchd daemon becomes opt-in rather than required.

**Architecture:** A new `claude_settings.py` module mirrors the existing `claude_config.py` (atomic, idempotent, backed-up edits) but targets `~/.claude/settings.json` under `hooks.{PostToolUse,Stop,SubagentStop}`. `run_setup` gains an additive, idempotent "hooks" step that resolves `korg-hook` on PATH and registers it; the binaries gate stops hard-requiring the daemon binary (`korg-ingest-claude`), and the daemon defaults to off.

**Tech Stack:** Python 3.9+, stdlib only; the existing `korg_setup` package; `pytest`.

---

## Background facts (verified)

- `~/.claude/settings.json` (hooks) is a **different file** from `~/.claude.json` (MCP servers). The MCP registration in `claude_config.py` stays as-is.
- Claude Code hook schema: `{"hooks": {"<Event>": [{"matcher": "<pattern>", "hooks": [{"type": "command", "command": "<cmd>"}]}]}}`. An empty `matcher` (`""`) matches all tools; it is ignored for `Stop`/`SubagentStop`.
- Every `run_setup(...)` call in `test_setup.py` passes `install_daemon=` **explicitly**, so flipping its default `True → False` does not change any existing test.
- `test_setup_fails_when_binaries_missing` patches `which → None`; keeping `korg-recall-mcp` as the hard requirement preserves that test. `_which_stub` returns `korg-ingest-claude` + `korg-recall-mcp` (not `korg-hook`); the new hooks step must **warn/skip** (not fail) when `korg-hook` is absent, so the existing "binaries present" tests stay `overall_ok`.

---

## File Structure

```
adapters/korg-setup/src/korg_setup/
├── claude_settings.py        # CREATE: idempotent ~/.claude/settings.json hook edits
├── setup.py                  # MODIFY: hooks step; relax binaries gate; daemon default off
└── __main__.py               # MODIFY: --daemon opt-in; uninstall removes the hook
adapters/korg-setup/tests/
├── test_claude_settings.py   # CREATE
└── test_setup.py             # MODIFY: hook-step assertions; _which_stub adds korg-hook
```

---

### Task 1: `claude_settings.py` — idempotent hook registration

**Files:**
- Create: `adapters/korg-setup/src/korg_setup/claude_settings.py`
- Test: `adapters/korg-setup/tests/test_claude_settings.py`

- [ ] **Step 1: Write the failing tests**

```python
# adapters/korg-setup/tests/test_claude_settings.py
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
    s = load_settings(settings_path)
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
```

- [ ] **Step 2: Run them and watch them fail**

Run: `PYTHONPATH=adapters/korg-setup/src python3 -m pytest adapters/korg-setup/tests/test_claude_settings.py -q`
Expected: FAIL — `ModuleNotFoundError: No module named 'korg_setup.claude_settings'`.

- [ ] **Step 3: Write `claude_settings.py`**

```python
# adapters/korg-setup/src/korg_setup/claude_settings.py
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
from dataclasses import dataclass, field
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
            kept = [h for h in (g.get("hooks") or [])
                    if not (h.get("type") == "command" and h.get("command") == command)]
            if len(kept) != len(g.get("hooks") or []):
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
```

- [ ] **Step 4: Run them and watch them pass**

Run: `PYTHONPATH=adapters/korg-setup/src python3 -m pytest adapters/korg-setup/tests/test_claude_settings.py -v`
Expected: PASS (7 passed).

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-setup/src/korg_setup/claude_settings.py adapters/korg-setup/tests/test_claude_settings.py
git commit -m "feat(korg-setup): claude_settings — idempotent ~/.claude/settings.json hook registration"
```

---

### Task 2: Wire hook registration into `run_setup` + relax gate + demote daemon

**Files:**
- Modify: `adapters/korg-setup/src/korg_setup/setup.py`
- Test: `adapters/korg-setup/tests/test_setup.py`

- [ ] **Step 1: Add the new behavior tests** (append to `test_setup.py`)

First update `_which_stub` to also resolve `korg-hook`, and add hook-aware tests:

```python
def _which_with_hook(name):
    return {
        "korg-ingest-claude": "/fake/korg-ingest-claude",
        "korg-recall-mcp": "/fake/korg-recall-mcp",
        "korg-hook": "/fake/korg-hook",
    }.get(name)


def test_setup_registers_hook_when_present(tmp_config, tmp_ledger_dir, tmp_ledger_file, tmp_path):
    settings = tmp_path / ".claude" / "settings.json"
    with patch("korg_setup.setup.shutil.which", side_effect=_which_with_hook):
        report = run_setup(
            ledger_dir=tmp_ledger_dir, ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config, claude_settings_path=settings,
            install_daemon=False,
        )
    assert report.overall_ok
    hooks_step = next(s for s in report.steps if s.name == "hooks")
    assert hooks_step.status == "ok"
    saved = json.loads(settings.read_text())
    assert saved["hooks"]["PostToolUse"][0]["hooks"][0]["command"] == "/fake/korg-hook"


def test_setup_warns_when_hook_binary_absent(tmp_config, tmp_ledger_dir, tmp_ledger_file, tmp_path):
    settings = tmp_path / ".claude" / "settings.json"
    # _which_stub resolves recall + ingest but NOT korg-hook
    with patch("korg_setup.setup.shutil.which", side_effect=_which_stub):
        report = run_setup(
            ledger_dir=tmp_ledger_dir, ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config, claude_settings_path=settings,
            install_daemon=False,
        )
    assert report.overall_ok  # missing hook is a warning, not a failure
    hooks_step = next(s for s in report.steps if s.name == "hooks")
    assert hooks_step.status == "warn"
    assert not settings.exists()


def test_setup_does_not_require_ingest_binary(tmp_config, tmp_ledger_dir, tmp_ledger_file, tmp_path):
    # only recall + hook present; the daemon binary is gone — setup must still succeed
    def which(name):
        return {"korg-recall-mcp": "/fake/korg-recall-mcp", "korg-hook": "/fake/korg-hook"}.get(name)
    with patch("korg_setup.setup.shutil.which", side_effect=which):
        report = run_setup(
            ledger_dir=tmp_ledger_dir, ledger_file=tmp_ledger_file,
            claude_config_path=tmp_config, claude_settings_path=tmp_path / ".claude" / "settings.json",
            install_daemon=False,
        )
    assert report.overall_ok
```

- [ ] **Step 2: Run them and watch them fail**

Run: `PYTHONPATH=adapters/korg-setup/src python3 -m pytest adapters/korg-setup/tests/test_setup.py -q`
Expected: FAIL — `run_setup()` has no `claude_settings_path` kwarg / no "hooks" step.

- [ ] **Step 3: Edit `setup.py`**

(a) Add the import near the other `korg_setup` imports:

```python
from korg_setup.claude_settings import (
    DEFAULT_SETTINGS_PATH,
    HookSpec,
    ensure_hook_registered,
)
```

(b) Add module defaults near `DEFAULT_MCP_SERVER_NAME`:

```python
DEFAULT_HOOK_COMMAND_NAME = "korg-hook"
```

(c) Add `hook_status` to `SetupReport`:

```python
    hook_status: Optional[str] = None
```

(d) Replace the binaries gate (step 1) so `korg-recall-mcp` is the hard requirement and `korg-ingest-claude` is optional:

```python
    # 1. Binaries on PATH
    recall_bin = shutil.which("korg-recall-mcp")
    hook_bin = shutil.which("korg-hook")
    ingest_bin = shutil.which("korg-ingest-claude")  # optional: only for the legacy daemon
    if not recall_bin:
        report.add(
            "binaries",
            "fail",
            "korg-recall-mcp not on PATH. Install the recall-mcp adapter "
            "(pip install -e 'adapters/recall-mcp[semantic]') and re-run.",
        )
        return report
    detail = f"korg-recall-mcp={recall_bin}"
    detail += f"; korg-hook={hook_bin}" if hook_bin else "; korg-hook=NOT FOUND (capture won't auto-register)"
    report.add("binaries", "ok", detail)
```

(e) Add `claude_settings_path` and `register_hook` params to the signature, and flip the daemon default:

```python
    claude_settings_path: Path = DEFAULT_SETTINGS_PATH,
    register_hook: bool = True,
    install_daemon: bool = False,   # was True — the hook is now the default capture
```

(f) Insert the hooks step immediately after the `claude_config` step (before the bridges block, step 3.5):

```python
    # 3.4. ~/.claude/settings.json — register the korg-hook capture command.
    if register_hook:
        if not hook_bin:
            report.add(
                "hooks",
                "warn",
                "korg-hook not on PATH; capture won't auto-register. Install the "
                "claude-code adapter (pip install -e adapters/claude-code) and re-run.",
            )
        elif dry_run:
            report.add("hooks", "ok", f"would register korg-hook in {claude_settings_path}")
            report.hook_status = "added"
        else:
            try:
                status, _ = ensure_hook_registered(HookSpec(command=hook_bin), claude_settings_path)
                report.hook_status = status
                if status == "added":
                    report.add("hooks", "ok", f"registered korg-hook in {claude_settings_path}")
                else:
                    report.add("hooks", "skip", f"korg-hook already registered in {claude_settings_path}")
            except Exception as e:
                report.add("hooks", "fail", f"could not edit {claude_settings_path}: {e}")
                return report
    else:
        report.add("hooks", "skip", "register_hook=False")
```

(g) In the Linux daemon hint and the macOS branch, the `ingest_bin` variable is still referenced. It may now be `None` (optional). Guard the Linux hint:

```python
        hint = (
            f"{ingest_bin or 'korg-ingest-claude'} --tail --state {tail_state} --out {ledger_file}"
        )
```

(h) Update `format_report`'s closing message to mention the hook:

```python
    if report.overall_ok:
        lines.append("")
        lines.append("Setup complete. Restart Claude Code to load the MCP server + capture hook.")
        if report.hook_status in {"added", "unchanged"}:
            lines.append("Verifiable capture is now ON for new Claude Code sessions (PostToolUse hook).")
        if report.plist_status in {"created", "updated"}:
            lines.append("Legacy tail daemon also running (launchd).")
```

- [ ] **Step 4: Run the full setup suite and watch it pass**

Run: `PYTHONPATH=adapters/korg-setup/src python3 -m pytest adapters/korg-setup/tests/test_setup.py -v`
Expected: PASS — the new hook tests plus all pre-existing setup tests (they pass `install_daemon` explicitly and assert `overall_ok`, which the relaxed gate preserves).

- [ ] **Step 5: Commit**

```bash
git add adapters/korg-setup/src/korg_setup/setup.py adapters/korg-setup/tests/test_setup.py
git commit -m "feat(korg-setup): register korg-hook in run_setup; relax gate; daemon opt-in"
```

---

### Task 3: CLI — `--daemon` opt-in + uninstall removes the hook

**Files:**
- Modify: `adapters/korg-setup/src/korg_setup/__main__.py`

- [ ] **Step 1: Make the daemon opt-in and register the hook by default**

In the argument parser, replace the `--no-daemon` flag with an opt-in `--daemon` (keep `--no-daemon` accepted as a deprecated no-op so existing muscle memory doesn't error):

```python
    parser.add_argument("--daemon", action="store_true",
                        help="also install the legacy launchd tail daemon (macOS; off by default)")
    parser.add_argument("--no-daemon", action="store_true",
                        help=argparse.SUPPRESS)  # deprecated: daemon is already off by default
    parser.add_argument("--no-hook", action="store_true",
                        help="don't register the korg-hook capture hook in ~/.claude/settings.json")
    parser.add_argument("--claude-settings", type=Path, default=DEFAULT_SETTINGS_PATH,
                        help=f"Claude Code hooks file (default {DEFAULT_SETTINGS_PATH})")
```

Add the import:

```python
from korg_setup.claude_settings import DEFAULT_SETTINGS_PATH, remove_hook
```

Update the `run_setup(...)` call in `_cmd_setup`:

```python
    report = run_setup(
        ledger_dir=args.ledger_dir,
        ledger_file=args.ledger_file,
        claude_config_path=args.claude_config,
        claude_settings_path=args.claude_settings,
        mcp_server_name=args.mcp_name,
        register_hook=not args.no_hook,
        install_daemon=args.daemon,
        register_introspect_bridges=not args.no_bridges,
        bridge_allow=args.bridge_allow,
        dry_run=args.dry_run,
    )
```

Update the confirmation text in `_cmd_setup` to mention the hook instead of the launchd agent:

```python
        print(
            "korg-setup will:\n"
            f"  · ensure {args.ledger_dir} exists\n"
            f"  · register MCP server '{args.mcp_name}' in {args.claude_config}\n"
            f"  · register the korg-hook capture hook in {args.claude_settings}\n"
            + (f"  · install the launchd agent {LABEL} (macOS)\n" if args.daemon else ""),
            file=sys.stderr,
        )
```

- [ ] **Step 2: Make `uninstall` remove the hook**

In `_cmd_uninstall`, after the MCP removal block, add:

```python
    # Capture hook in ~/.claude/settings.json
    hook_bin = shutil.which("korg-hook")
    removed_any = False
    for cmd in filter(None, [hook_bin, "korg-hook"]):
        status_h, _ = remove_hook(cmd, args.claude_settings)
        removed_any = removed_any or status_h == "removed"
    print(
        f"  {'✓ removed' if removed_any else '·'} korg-hook from {args.claude_settings}",
        file=sys.stderr,
    )
```

Add `import shutil` at the top if not present, and add the `--claude-settings` arg default to the uninstall path (it's a top-level arg, already available on `args`).

- [ ] **Step 3: Smoke-test the CLI end to end**

Run:
```bash
PYTHONPATH=adapters/korg-setup/src python3 -m korg_setup --help
PYTHONPATH=adapters/korg-setup/src python3 -m korg_setup --dry-run --yes \
  --claude-config /tmp/ks-cfg.json --claude-settings /tmp/ks-settings.json \
  --ledger-dir /tmp/ks-korg 2>&1 | tail -20
```
Expected: `--help` shows `--daemon` and `--no-hook`; the dry-run prints a "hooks" step and writes nothing.

- [ ] **Step 4: Full korg-setup suite green + commit**

Run: `PYTHONPATH=adapters/korg-setup/src python3 -m pytest adapters/korg-setup/tests -v`
Expected: PASS — entire korg-setup suite (claude_settings, setup, claude_config, launchd, discovery, status, bridge_registration).

```bash
git add adapters/korg-setup/src/korg_setup/__main__.py
git commit -m "feat(korg-setup): CLI --daemon opt-in, --no-hook, and uninstall removes the hook"
```

---

## Self-Review

**1. Spec coverage (§4.6):** new `claude_settings.py::ensure_hook_registered` writing `~/.claude/settings.json` under `hooks.{PostToolUse,Stop,SubagentStop}` ✓ (Task 1); idempotent + backup + preserves other keys ✓ (Task 1 tests); distinct from the `~/.claude.json` MCP registration ✓ (separate module/file); daemon demoted to optional/opt-in ✓ (Task 2 default flip + Task 3 `--daemon`); registration wired into `korg-setup` ✓ (Task 2); uninstall removes it ✓ (Task 3).

**2. Placeholder scan:** No TBD/TODO; complete code in every code step; exact commands + expected output in every run step.

**3. Type/name consistency:** `HookSpec`, `ensure_hook_registered`, `remove_hook`, `get_registered_hook_events`, `load_settings`, `save_settings_atomic`, `DEFAULT_SETTINGS_PATH`, `HOOK_EVENTS` used identically across the module, its tests, `setup.py`, and `__main__.py`. The group shape `{"matcher","hooks":[{"type":"command","command"}]}` matches the Claude Code hooks schema and is asserted consistently in tests. `run_setup` new kwargs (`claude_settings_path`, `register_hook`, `install_daemon=False`) match their call sites in the new tests and `__main__.py`.

**Note:** `status.py` is left unchanged (reporting hook status in `korg-setup status` is a small follow-up, not required for "capture turns on"). No gaps found.
