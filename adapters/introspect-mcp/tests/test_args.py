"""Tests for the MCP-args → CLI-argv mapper."""

from __future__ import annotations

from pathlib import Path

import pytest

from korg_introspect_mcp.args import build_argv, kebab, value_to_argv


# ── kebab ─────────────────────────────────────────────────────────────


def test_kebab_underscore_to_hyphen():
    assert kebab("top_n") == "top-n"
    assert kebab("file_path") == "file-path"


def test_kebab_idempotent_for_already_kebab():
    assert kebab("top-n") == "top-n"
    assert kebab("file") == "file"


# ── value_to_argv ─────────────────────────────────────────────────────


def test_value_to_argv_string():
    assert value_to_argv("hello") == ["hello"]


def test_value_to_argv_number():
    assert value_to_argv(42) == ["42"]
    assert value_to_argv(3.14) == ["3.14"]


def test_value_to_argv_path():
    assert value_to_argv(Path("/tmp/x")) == ["/tmp/x"]


def test_value_to_argv_bool_raises():
    """Bools are handled at the property level (flag-on-true), so they
    should never reach value_to_argv. If they do, raise loudly."""
    with pytest.raises(TypeError):
        value_to_argv(True)


def test_value_to_argv_object_falls_back_to_json():
    result = value_to_argv({"k": "v"})
    assert result == ['{"k": "v"}']


# ── build_argv: basic shapes ──────────────────────────────────────────


def test_build_argv_simple_string():
    argv = build_argv(
        binary_path=Path("/bin/thump"),
        command_id="thump.echo",
        binary_name="thump",
        arguments={"message": "hi"},
    )
    assert argv == ["/bin/thump", "echo", "--message", "hi"]


def test_build_argv_kebab_case_flag_from_snake_case_arg():
    argv = build_argv(
        binary_path=Path("/bin/x"),
        command_id="x.foo",
        binary_name="x",
        arguments={"top_n": 5},
    )
    assert "--top-n" in argv
    assert "5" in argv


def test_build_argv_bool_true_emits_flag_only():
    argv = build_argv(
        binary_path=Path("/bin/x"),
        command_id="x.foo",
        binary_name="x",
        arguments={"quiet": True},
    )
    assert "--quiet" in argv
    # No trailing value after --quiet
    idx = argv.index("--quiet")
    # Either it's the last arg or the next one is also a flag (not a value)
    assert idx == len(argv) - 1


def test_build_argv_bool_false_omits_flag():
    argv = build_argv(
        binary_path=Path("/bin/x"),
        command_id="x.foo",
        binary_name="x",
        arguments={"quiet": False, "message": "hi"},
    )
    assert "--quiet" not in argv
    assert "--message" in argv


def test_build_argv_array_repeats_flag():
    argv = build_argv(
        binary_path=Path("/bin/x"),
        command_id="x.foo",
        binary_name="x",
        arguments={"tags": ["a", "b", "c"]},
    )
    # Each item gets its own --tags flag
    assert argv.count("--tags") == 3
    assert all(v in argv for v in ("a", "b", "c"))


def test_build_argv_none_value_omitted():
    argv = build_argv(
        binary_path=Path("/bin/x"),
        command_id="x.foo",
        binary_name="x",
        arguments={"optional": None, "message": "hi"},
    )
    assert "--optional" not in argv
    assert "--message" in argv


# ── build_argv: subcommand paths ──────────────────────────────────────


def test_build_argv_naked_binary_no_subcommand():
    """command_id == binary_name (e.g. "thump") → no subcommand path."""
    argv = build_argv(
        binary_path=Path("/bin/thump"),
        command_id="thump",
        binary_name="thump",
        arguments={"flag": "x"},
    )
    assert argv == ["/bin/thump", "--flag", "x"]


def test_build_argv_one_segment_subcommand():
    argv = build_argv(
        binary_path=Path("/bin/thump"),
        command_id="thump.generate",
        binary_name="thump",
        arguments={"name": "x"},
    )
    assert argv == ["/bin/thump", "generate", "--name", "x"]


def test_build_argv_nested_subcommand_path():
    argv = build_argv(
        binary_path=Path("/bin/thump"),
        command_id="thump.bun.script.run",
        binary_name="thump",
        arguments={"name": "build"},
    )
    assert argv[:5] == ["/bin/thump", "bun", "script", "run", "--name"]


def test_build_argv_handles_dashed_subcommand():
    """install-extension stays as one segment, not split on -"""
    argv = build_argv(
        binary_path=Path("/bin/korgex"),
        command_id="korgex.install-extension",
        binary_name="korgex",
        arguments={},
    )
    assert argv == ["/bin/korgex", "install-extension"]


def test_build_argv_command_id_without_binary_prefix_treated_as_subcommand_path():
    """Unusual but supported: if command_id doesn't start with binary_name,
    use the whole thing as a subcommand path."""
    argv = build_argv(
        binary_path=Path("/bin/x"),
        command_id="some.other.path",
        binary_name="x",
        arguments={},
    )
    assert argv == ["/bin/x", "some", "other", "path"]
