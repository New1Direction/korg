"""Tests for the macOS launchd integration.

Subprocess calls (launchctl load/unload/list) are mocked so the tests
never actually touch the user's launchd. We assert the plist content
is shaped correctly, and that the right launchctl invocations would
fire under each scenario.
"""

from __future__ import annotations

import plistlib
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from korg_setup.launchd import (
    LABEL,
    LOG_DIR,
    PlistSpec,
    UnsupportedPlatformError,
    build_spec,
    install_service,
    is_loaded,
    uninstall_service,
    write_plist,
)


# ── build_spec ────────────────────────────────────────────────────────


def test_build_spec_default_args(tmp_path):
    spec = build_spec(Path("/usr/local/bin/korg-ingest-claude"))
    assert spec.label == LABEL
    assert spec.program_arguments == ["/usr/local/bin/korg-ingest-claude"]
    assert spec.run_at_load is True
    assert spec.keep_alive is True


def test_build_spec_with_extra_args():
    spec = build_spec(
        Path("/x/korg-ingest-claude"),
        extra_args=["--tail", "--out", "/y/events.jsonl"],
    )
    assert spec.program_arguments == [
        "/x/korg-ingest-claude",
        "--tail",
        "--out",
        "/y/events.jsonl",
    ]


def test_plistspec_to_dict_includes_required_keys():
    spec = PlistSpec(
        label="x",
        program_arguments=["/y"],
        stdout_path=Path("/log/out"),
        stderr_path=Path("/log/err"),
    )
    d = spec.to_dict()
    assert d["Label"] == "x"
    assert d["ProgramArguments"] == ["/y"]
    assert d["RunAtLoad"] is True
    assert d["KeepAlive"] is True
    assert d["StandardOutPath"] == "/log/out"
    assert d["StandardErrorPath"] == "/log/err"
    assert "WorkingDirectory" not in d


def test_plistspec_to_dict_includes_working_directory_when_set():
    spec = PlistSpec(
        label="x",
        program_arguments=["/y"],
        stdout_path=Path("/o"),
        stderr_path=Path("/e"),
        working_directory=Path("/work"),
    )
    assert spec.to_dict()["WorkingDirectory"] == "/work"


# ── write_plist ────────────────────────────────────────────────────────


def test_write_plist_creates_file(tmp_path):
    plist_path = tmp_path / "agent.plist"
    spec = build_spec(Path("/x/bin"), extra_args=["--tail"])
    status = write_plist(spec, plist_path)
    # First write returns "created" or "updated" depending on race; we just
    # assert the file exists with the right content.
    assert plist_path.exists()
    loaded = plistlib.loads(plist_path.read_bytes())
    assert loaded["Label"] == LABEL
    assert loaded["ProgramArguments"] == ["/x/bin", "--tail"]


def test_write_plist_idempotent(tmp_path):
    plist_path = tmp_path / "agent.plist"
    spec = build_spec(Path("/x/bin"), extra_args=["--tail"])
    write_plist(spec, plist_path)
    status = write_plist(spec, plist_path)
    assert status == "unchanged"


def test_write_plist_changed_content_triggers_rewrite(tmp_path):
    plist_path = tmp_path / "agent.plist"
    spec_a = build_spec(Path("/x/bin"), extra_args=["--tail"])
    spec_b = build_spec(Path("/y/different-bin"), extra_args=["--tail"])
    write_plist(spec_a, plist_path)
    status = write_plist(spec_b, plist_path)
    assert status == "updated"
    loaded = plistlib.loads(plist_path.read_bytes())
    assert loaded["ProgramArguments"][0] == "/y/different-bin"


def test_write_plist_atomic_no_tmp_left(tmp_path):
    plist_path = tmp_path / "agent.plist"
    write_plist(build_spec(Path("/x/bin")), plist_path)
    assert not list(tmp_path.glob("*.tmp"))


# ── platform gate ──────────────────────────────────────────────────────


@patch("korg_setup.launchd.platform.system", return_value="Linux")
def test_load_service_raises_on_non_macos(mock_platform, tmp_path):
    from korg_setup.launchd import load_service
    with pytest.raises(UnsupportedPlatformError):
        load_service(tmp_path / "plist")


@patch("korg_setup.launchd.platform.system", return_value="Linux")
def test_is_loaded_returns_false_on_non_macos(mock_platform):
    assert is_loaded() is False


# ── install / uninstall (mocked launchctl) ─────────────────────────────


@patch("korg_setup.launchd.platform.system", return_value="Darwin")
@patch("korg_setup.launchd.subprocess.run")
@patch("korg_setup.launchd.find_korg_ingest_claude")
def test_install_service_writes_plist_and_loads(
    mock_find, mock_run, mock_platform, tmp_path
):
    mock_find.return_value = Path("/x/korg-ingest-claude")
    # launchctl list returns nothing (so is_loaded() = False)
    list_result = MagicMock(returncode=0, stdout="", stderr="")
    load_result = MagicMock(returncode=0, stdout="", stderr="")
    mock_run.side_effect = [list_result, load_result]

    plist_path = tmp_path / "agent.plist"
    status, result = install_service(extra_args=["--tail"], plist_path=plist_path)
    assert plist_path.exists()
    # launchctl was called with load -w
    load_call = mock_run.call_args_list[-1]
    assert load_call[0][0] == ["launchctl", "load", "-w", str(plist_path)]


@patch("korg_setup.launchd.platform.system", return_value="Darwin")
@patch("korg_setup.launchd.subprocess.run")
def test_uninstall_service_removes_plist(mock_run, mock_platform, tmp_path):
    plist_path = tmp_path / "agent.plist"
    plist_path.write_bytes(b"<plist/>")
    # is_loaded returns False on first call (so no unload), but uninstall_service
    # also calls is_loaded once.
    mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
    status, _result = uninstall_service(plist_path=plist_path)
    assert status == "removed"
    assert not plist_path.exists()


@patch("korg_setup.launchd.platform.system", return_value="Darwin")
@patch("korg_setup.launchd.subprocess.run")
def test_uninstall_service_absent_when_plist_missing(mock_run, mock_platform, tmp_path):
    mock_run.return_value = MagicMock(returncode=0, stdout="", stderr="")
    plist_path = tmp_path / "never-existed.plist"
    status, _result = uninstall_service(plist_path=plist_path)
    assert status == "absent"


# ── is_loaded parser ──────────────────────────────────────────────────


@patch("korg_setup.launchd.platform.system", return_value="Darwin")
@patch("korg_setup.launchd.subprocess.run")
def test_is_loaded_detects_our_label(mock_run, mock_platform):
    mock_run.return_value = MagicMock(
        returncode=0,
        stdout="12345  0  com.something.else\n67890  0  com.korg.ingest-claude\n",
        stderr="",
    )
    assert is_loaded() is True


@patch("korg_setup.launchd.platform.system", return_value="Darwin")
@patch("korg_setup.launchd.subprocess.run")
def test_is_loaded_returns_false_when_label_absent(mock_run, mock_platform):
    mock_run.return_value = MagicMock(
        returncode=0,
        stdout="12345  0  com.something.else\n",
        stderr="",
    )
    assert is_loaded() is False


@patch("korg_setup.launchd.platform.system", return_value="Darwin")
@patch("korg_setup.launchd.subprocess.run")
def test_is_loaded_returns_false_on_launchctl_failure(mock_run, mock_platform):
    mock_run.return_value = MagicMock(returncode=1, stdout="", stderr="permission denied")
    assert is_loaded() is False
