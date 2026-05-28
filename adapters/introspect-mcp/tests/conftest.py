"""Shared fixtures: a synthetic 'binary' for hermetic tests.

The fixture writes a small Python script to tmp_path that responds to
`--introspect` with a known document AND echoes back its argv on any
other invocation. Lets us exercise discovery → server → invocation
end-to-end without depending on thumper/korg/korgex being built.
"""

from __future__ import annotations

import json
import os
import stat
import sys
import textwrap
from pathlib import Path

import pytest


def _make_fixture_binary(tmp_path: Path, name: str = "fixture-bin") -> Path:
    """Create a small Python script that mimics a korg:introspect@v1 binary."""
    doc = {
        "schema": "korg:introspect@v1",
        "binary": name,
        "version": "0.0.1",
        "callables_declared": True,
        "callables": [
            {
                "command_id": f"{name}.echo",
                "name": "echo",
                "description": "Echo args back as JSON.",
                "surfaces": ["cli"],
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"},
                        "count": {"type": "integer"},
                        "loud": {"type": "boolean"},
                        "tags": {"type": "array", "items": {"type": "string"}},
                    },
                    "required": ["message"],
                },
                "capabilities": {
                    "output_mode": "envelope",
                    "side_effects": "none",
                    "requires_project": False,
                    "long_running": False,
                    "stateful": False,
                    "reads_stdin": False,
                    "supports_output_path": False,
                },
            },
            {
                "command_id": f"{name}.write",
                "name": "write",
                "description": "Pretend to write a file.",
                "surfaces": ["cli"],
                "input_schema": {
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                },
                "capabilities": {
                    "output_mode": "envelope",
                    "side_effects": "fs_write",
                    "requires_project": False,
                    "long_running": False,
                    "stateful": False,
                    "reads_stdin": False,
                    "supports_output_path": True,
                },
            },
            {
                "command_id": f"{name}.shell",
                "name": "shell",
                "description": "Open a stateful session.",
                "surfaces": ["cli"],
                "input_schema": {"type": "object"},
                "capabilities": {
                    "output_mode": "session",
                    "side_effects": "ledger_write",
                    "requires_project": False,
                    "long_running": True,
                    "stateful": True,
                    "reads_stdin": True,
                    "supports_output_path": False,
                },
            },
            {
                "command_id": f"{name}.fail",
                "name": "fail",
                "description": "Always exits 1.",
                "surfaces": ["cli"],
                "input_schema": {"type": "object"},
                "capabilities": {
                    "output_mode": "envelope",
                    "side_effects": "none",
                    "requires_project": False,
                    "long_running": False,
                    "stateful": False,
                    "reads_stdin": False,
                    "supports_output_path": False,
                },
            },
        ],
        "exit_codes": {
            "0": "success",
            "1": "error.generic",
            "2": "error.usage",
        },
    }

    # Write the introspect doc to a sidecar JSON file so we don't have to
    # worry about Python vs JSON literal syntax (true/True, false/False, null/None).
    doc_path = tmp_path / f"{name}.introspect.json"
    doc_path.write_text(json.dumps(doc))

    script = textwrap.dedent(f"""\
        #!{sys.executable}
        import json, sys
        DOC_PATH = {json.dumps(str(doc_path))}
        if "--introspect" in sys.argv[1:]:
            with open(DOC_PATH) as f:
                sys.stdout.write(f.read())
            sys.exit(0)
        # Subcommand dispatch
        args = sys.argv[1:]
        if args and args[0] == "fail":
            print(json.dumps({{"failed": True}}))
            sys.exit(1)
        # Default: echo argv as JSON envelope
        print(json.dumps({{"argv": args}}))
        sys.exit(0)
    """)

    p = tmp_path / name
    p.write_text(script)
    p.chmod(p.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return p


@pytest.fixture
def fixture_binary(tmp_path: Path) -> Path:
    return _make_fixture_binary(tmp_path, name="fixture-bin")
