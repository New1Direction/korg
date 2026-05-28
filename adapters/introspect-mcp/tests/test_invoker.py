"""Tests for the invoker — argv → subprocess → MCP-formatted result."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from korg_introspect_mcp.discovery import discover
from korg_introspect_mcp.invoker import (
    SESSION_NOT_SUPPORTED,
    InvocationResult,
    invoke,
)


def _get(callables, command_id):
    for c in callables:
        if c.command_id == command_id:
            return c
    raise KeyError(command_id)


# ── Envelope mode ─────────────────────────────────────────────────────


def test_invoke_envelope_returns_pretty_json(fixture_binary):
    d = discover(str(fixture_binary))
    echo = _get(d.callables, "fixture-bin.echo")
    result = invoke(
        echo,
        arguments={"message": "hello"},
        binary_path=d.binary_path,
        binary_name=d.binary_name,
    )
    assert not result.is_error
    # The fixture echoes argv as JSON; we should get pretty-printed JSON back.
    parsed = json.loads(result.text)
    assert "argv" in parsed
    assert "--message" in parsed["argv"]
    assert "hello" in parsed["argv"]


def test_invoke_passes_kebab_flags_for_snake_case_args(fixture_binary):
    d = discover(str(fixture_binary))
    echo = _get(d.callables, "fixture-bin.echo")
    result = invoke(
        echo,
        arguments={"message": "hi", "count": 7, "loud": True},
        binary_path=d.binary_path,
        binary_name=d.binary_name,
    )
    parsed = json.loads(result.text)
    argv = parsed["argv"]
    assert "--message" in argv
    assert "--count" in argv
    assert "--loud" in argv
    # Verify positional order: echo subcommand first
    assert argv[0] == "echo"


def test_invoke_array_args_repeat_flag(fixture_binary):
    d = discover(str(fixture_binary))
    echo = _get(d.callables, "fixture-bin.echo")
    result = invoke(
        echo,
        arguments={"message": "x", "tags": ["a", "b"]},
        binary_path=d.binary_path,
        binary_name=d.binary_name,
    )
    parsed = json.loads(result.text)
    argv = parsed["argv"]
    assert argv.count("--tags") == 2


# ── Session mode ──────────────────────────────────────────────────────


def test_invoke_session_returns_unsupported_error(fixture_binary):
    d = discover(str(fixture_binary))
    shell = _get(d.callables, "fixture-bin.shell")
    result = invoke(
        shell,
        arguments={},
        binary_path=d.binary_path,
        binary_name=d.binary_name,
    )
    assert result.is_error
    assert "session" in result.text.lower()


# ── Failure cases ─────────────────────────────────────────────────────


def test_invoke_non_zero_exit_reports_error(fixture_binary):
    d = discover(str(fixture_binary))
    fail = _get(d.callables, "fixture-bin.fail")
    result = invoke(
        fail,
        arguments={},
        binary_path=d.binary_path,
        binary_name=d.binary_name,
    )
    assert result.is_error
    assert "exit_code" in result.text
    assert "1" in result.text


def test_invoke_timeout(fixture_binary, tmp_path):
    # Patch the binary to sleep longer than the timeout
    import sys, stat, textwrap
    slow_bin = tmp_path / "slow-bin"
    slow_bin.write_text(
        f"#!{sys.executable}\nimport time\ntime.sleep(5)\nprint('done')\n"
    )
    slow_bin.chmod(slow_bin.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    # Synthesize a callable that points at the slow binary
    d = discover(str(fixture_binary))
    echo = _get(d.callables, "fixture-bin.echo")

    result = invoke(
        echo,
        arguments={"message": "x"},
        binary_path=slow_bin,
        binary_name="slow-bin",
        timeout_s=0.5,
    )
    assert result.is_error
    assert "timed out" in result.text.lower()


def test_invoke_uses_correct_binary_path(fixture_binary):
    """Sanity: the binary actually exec'd is the one we passed in."""
    d = discover(str(fixture_binary))
    echo = _get(d.callables, "fixture-bin.echo")
    result = invoke(
        echo,
        arguments={"message": "trace"},
        binary_path=d.binary_path,
        binary_name=d.binary_name,
    )
    assert "trace" in result.text
