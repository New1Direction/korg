"""Atomic, idempotent edits to ~/.claude.json (Claude Code's MCP config).

The file is the user's personal config — it carries oauth state, project
list, onboarding flags, growthbook caches, etc. We treat it as
write-precious:

  - Always read → modify → atomic-rename, never partial writes.
  - Backup the prior copy to `~/.claude.json.korg-backup` before each edit.
  - Idempotent: registering the same server twice is a no-op.
  - Preserve every other key the file already had.
"""

from __future__ import annotations

import json
import os
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal


DEFAULT_CONFIG_PATH = Path.home() / ".claude.json"


@dataclass
class McpServerSpec:
    """One MCP server entry, matching Claude Code's mcpServers schema."""

    name: str
    command: str
    args: list[str]
    env: dict[str, str] | None = None

    def to_config_value(self) -> dict[str, Any]:
        out: dict[str, Any] = {"command": self.command, "args": list(self.args)}
        if self.env:
            out["env"] = dict(self.env)
        return out


ChangeStatus = Literal["added", "updated", "unchanged"]


def load_config(config_path: Path = DEFAULT_CONFIG_PATH) -> dict[str, Any]:
    """Load ~/.claude.json. Returns {} if the file doesn't exist."""
    if not config_path.exists():
        return {}
    return json.loads(config_path.read_text())


def save_config_atomic(
    config: dict[str, Any],
    config_path: Path = DEFAULT_CONFIG_PATH,
    *,
    backup_suffix: str = ".korg-backup",
) -> Path | None:
    """Write `config` to `config_path` atomically via tmp-rename.

    If a file already exists at `config_path`, it's first copied to
    `config_path.with_suffix(config_path.suffix + backup_suffix)`.
    Returns the backup path (or None if no prior file existed).
    """
    config_path.parent.mkdir(parents=True, exist_ok=True)

    backup_path: Path | None = None
    if config_path.exists():
        backup_path = Path(str(config_path) + backup_suffix)
        shutil.copy2(config_path, backup_path)

    tmp = Path(str(config_path) + ".tmp")
    tmp.write_text(json.dumps(config, indent=2) + "\n")
    os.replace(tmp, config_path)
    return backup_path


def ensure_mcp_server_registered(
    spec: McpServerSpec,
    config_path: Path = DEFAULT_CONFIG_PATH,
) -> tuple[ChangeStatus, Path | None]:
    """Idempotently register `spec` in `config_path` under the `mcpServers` key.

    Returns:
        ("added", backup_path)     — server name was not present; added.
        ("updated", backup_path)   — server name existed but with different
                                     command/args/env; overwritten.
        ("unchanged", None)        — already registered with identical config;
                                     no write performed.
    """
    config = load_config(config_path)
    existing_servers = config.get("mcpServers") or {}
    desired = spec.to_config_value()

    if spec.name in existing_servers:
        if existing_servers[spec.name] == desired:
            return ("unchanged", None)
        existing_servers[spec.name] = desired
        config["mcpServers"] = existing_servers
        backup = save_config_atomic(config, config_path)
        return ("updated", backup)

    existing_servers[spec.name] = desired
    config["mcpServers"] = existing_servers
    backup = save_config_atomic(config, config_path)
    return ("added", backup)


def remove_mcp_server(
    name: str,
    config_path: Path = DEFAULT_CONFIG_PATH,
) -> tuple[Literal["removed", "absent"], Path | None]:
    """Remove the named MCP server from `config_path`. Idempotent."""
    config = load_config(config_path)
    servers = config.get("mcpServers") or {}
    if name not in servers:
        return ("absent", None)
    del servers[name]
    # If we just emptied mcpServers, leave an empty dict — Claude Code is
    # tolerant of it, and dropping the key would be more invasive than
    # this function should be.
    config["mcpServers"] = servers
    backup = save_config_atomic(config, config_path)
    return ("removed", backup)


def get_registered_server(
    name: str,
    config_path: Path = DEFAULT_CONFIG_PATH,
) -> dict[str, Any] | None:
    """Return the current spec for the named MCP server, or None if absent."""
    config = load_config(config_path)
    servers = config.get("mcpServers") or {}
    return servers.get(name)
