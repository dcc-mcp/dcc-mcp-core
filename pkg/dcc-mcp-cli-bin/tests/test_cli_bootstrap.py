"""Unit tests for the ``dcc-mcp-cli`` PyPI wrapper bootstrap.

Run with the wrapper package importable::

    PYTHONPATH=pkg/dcc-mcp-cli-bin/python pytest pkg/dcc-mcp-cli-bin/tests

The tests never execute a real binary: the "executable" inside the synthetic
payload archive is a text file, and :func:`dcc_mcp_cli._bootstrap._execute` is
stubbed where the tests care about the process boundary.
"""

from __future__ import annotations

import hashlib
import json
import os
import zipfile

from dcc_mcp_cli import _bootstrap
import pytest


@pytest.fixture
def payload_dir_path(tmp_path):
    """Return the directory the synthetic payload is staged into."""
    return tmp_path / "payload"


@pytest.fixture
def payload(payload_dir_path, monkeypatch):
    """Stage a synthetic payload and point the bootstrap at it."""
    payload_dir = payload_dir_path
    payload_dir.mkdir(exist_ok=True)
    archive = payload_dir / "dcc-mcp-cli-9.9.9-linux-x86_64.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as archive_file:
        archive_file.writestr("dcc-mcp-cli", b"#!/bin/sh\nexit 0\n")
    metadata = {
        "schema_version": 1,
        "distribution": "dcc-mcp-cli",
        "version": "9.9.9",
        "platform": "linux-x86_64",
        "archive": archive.name,
        "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        "member": "dcc-mcp-cli",
        "wheel_platform_tag": "manylinux_2_17_x86_64",
    }
    (payload_dir / "payload.json").write_text(json.dumps(metadata), encoding="utf-8")

    monkeypatch.setattr(_bootstrap, "payload_dir", lambda: payload_dir)
    monkeypatch.delenv("DCC_MCP_CLI_BIN", raising=False)
    monkeypatch.delenv("DCC_MCP_CLI_BIN_DIR", raising=False)
    return metadata


def rewrite_payload(payload_dir_path, metadata):
    """Persist a modified manifest the way the bootstrap reads it."""
    (payload_dir_path / "payload.json").write_text(json.dumps(metadata), encoding="utf-8")


@pytest.fixture
def bin_dir(tmp_path, monkeypatch):
    """Restrict candidate directories to a writable temporary directory."""
    target = tmp_path / "scripts"
    target.mkdir()
    monkeypatch.setattr(_bootstrap, "candidate_dirs", lambda: [target])
    return target


def test_payload_dir_missing_raises(tmp_path, monkeypatch):
    """A wheel built without a payload fails with an actionable message."""
    monkeypatch.setattr(_bootstrap, "payload_dir", lambda: tmp_path / "absent")
    with pytest.raises(_bootstrap.PayloadError, match="missing"):
        _bootstrap.binary_path()


def test_resolve_binary_unpacks_into_the_scripts_directory(payload, bin_dir):
    """The archive is unpacked into the environment's scripts directory."""
    binary = _bootstrap.resolve_binary()

    assert binary.parent == bin_dir
    assert binary.name == _bootstrap._binary_names()[0]
    assert binary.is_file()


def test_unpacked_binary_is_executable(payload, bin_dir):
    """The unpacked binary carries the executable bit."""
    if os.name == "nt":
        pytest.skip("POSIX mode bits are not meaningful on Windows")

    binary = _bootstrap.resolve_binary()

    assert binary.stat().st_mode & 0o111


def test_marker_is_written_next_to_the_binary(payload, bin_dir):
    """A package-manager marker lands beside the binary, in the same directory."""
    binary = _bootstrap.resolve_binary()

    marker = json.loads(_bootstrap.marker_path(binary.parent).read_text(encoding="utf-8"))
    assert marker["schema_version"] == 1
    assert marker["distribution"] == "dcc-mcp-cli"
    assert marker["manager"] == _bootstrap.PACKAGE_MANAGER_NAME
    assert marker["version"] == "9.9.9"
    assert marker["platform"] == "linux-x86_64"
    assert marker["binary"] == binary.name


def test_second_resolution_reuses_the_unpacked_binary(payload, bin_dir, monkeypatch):
    """A marker with a matching version short-circuits the unpack."""
    first = _bootstrap.resolve_binary()

    def explode(*args, **kwargs):  # pragma: no cover - must never be reached
        raise AssertionError("the payload must not be unpacked twice")

    monkeypatch.setattr(_bootstrap, "_extract", explode)
    assert _bootstrap.resolve_binary() == first


def test_version_bump_forces_a_reunpack(payload, payload_dir_path, bin_dir):
    """Upgrading the wheel replaces the binary instead of keeping the old one."""
    first = _bootstrap.resolve_binary()
    first.write_text("stale", encoding="utf-8")

    payload["version"] = "9.9.10"
    rewrite_payload(payload_dir_path, payload)

    second = _bootstrap.resolve_binary()
    assert second == first
    assert second.read_bytes() != b"stale"


def test_archive_digest_mismatch_is_rejected(payload, payload_dir_path, bin_dir):
    """A corrupted archive is refused instead of being unpacked."""
    payload["archive_sha256"] = "0" * 64
    rewrite_payload(payload_dir_path, payload)

    with pytest.raises(_bootstrap.PayloadError, match="sha256 mismatch"):
        _bootstrap.resolve_binary()


def test_missing_archive_member_is_reported(payload, payload_dir_path, bin_dir):
    """An archive without the expected member fails with a clear error."""
    payload["member"] = "dcc-mcp-cli-missing"
    rewrite_payload(payload_dir_path, payload)

    with pytest.raises(_bootstrap.PayloadError, match="does not contain"):
        _bootstrap.resolve_binary()


def test_dcc_mcp_cli_bin_override_wins(payload, bin_dir, tmp_path, monkeypatch):
    """``DCC_MCP_CLI_BIN`` pins an existing binary and skips the payload."""
    pinned = tmp_path / "pinned-cli"
    pinned.write_text("pinned", encoding="utf-8")
    monkeypatch.setenv("DCC_MCP_CLI_BIN", str(pinned))

    assert _bootstrap.binary_path() == pinned


def test_windows_never_replaces_the_console_launcher(payload, bin_dir, monkeypatch):
    """On Windows the unpacked binary uses a distinct name.

    The pip-generated ``dcc-mcp-cli.exe`` launcher is the running process image
    and cannot be replaced while mapped, so the unpack must target another name.
    """
    monkeypatch.setattr(_bootstrap.os, "name", "nt")

    names = _bootstrap._binary_names()

    assert names == ("dcc-mcp-cli-bin.exe",)
    assert "dcc-mcp-cli.exe" not in names


def test_posix_prefers_the_canonical_name(payload, bin_dir, monkeypatch):
    """On POSIX the binary may take the canonical ``dcc-mcp-cli`` name."""
    monkeypatch.setattr(_bootstrap.os, "name", "posix")

    assert _bootstrap._binary_names() == ("dcc-mcp-cli", "dcc-mcp-cli-bin")


def test_binary_path_override_missing_file_falls_back(payload, bin_dir, tmp_path, monkeypatch):
    """An override pointing at a missing file is ignored, not fatal."""
    monkeypatch.setenv("DCC_MCP_CLI_BIN", str(tmp_path / "absent"))

    assert _bootstrap.binary_path().parent == bin_dir


def test_main_forwards_arguments_and_exit_code(payload, bin_dir, monkeypatch):
    """``main`` runs the binary with the caller's arguments."""
    captured = {}

    def fake_execute(binary, argv):
        captured["binary"] = binary
        captured["argv"] = argv
        return 7

    monkeypatch.setattr(_bootstrap, "_execute", fake_execute)

    assert _bootstrap.main(["--version"]) == 7
    assert captured["argv"] == ["--version"]
    assert captured["binary"].parent == bin_dir


def test_main_reports_payload_errors_without_a_traceback(payload, bin_dir, monkeypatch, capsys):
    """A broken payload prints one stderr line and exits non-zero."""
    monkeypatch.setattr(_bootstrap, "load_payload", lambda: (_ for _ in ()).throw(_bootstrap.PayloadError("boom")))

    assert _bootstrap.main([]) == 1
    assert "boom" in capsys.readouterr().err


def test_candidate_dirs_honour_the_env_override(tmp_path, monkeypatch):
    """``DCC_MCP_CLI_BIN_DIR`` is the first candidate directory."""
    pinned = tmp_path / "pinned-dir"
    monkeypatch.setenv("DCC_MCP_CLI_BIN_DIR", str(pinned))

    assert _bootstrap.candidate_dirs()[0] == pinned


def test_candidate_dirs_are_deduplicated(tmp_path, monkeypatch):
    """Repeated directories collapse to one entry."""
    monkeypatch.setenv("DCC_MCP_CLI_BIN_DIR", str(_bootstrap._user_fallback_dir()))

    directories = _bootstrap.candidate_dirs()

    assert len(directories) == len(set(directories))


def test_payload_requires_a_known_schema(tmp_path, monkeypatch):
    """A payload from an unknown schema version is refused."""
    payload_dir = tmp_path / "payload"
    payload_dir.mkdir()
    (payload_dir / "payload.json").write_text(json.dumps({"schema_version": 99}), encoding="utf-8")
    monkeypatch.setattr(_bootstrap, "payload_dir", lambda: payload_dir)

    with pytest.raises(_bootstrap.PayloadError, match="schema_version"):
        _bootstrap.load_payload()


def test_payload_missing_archive_is_reported(tmp_path, monkeypatch):
    """A manifest pointing at an absent archive is refused."""
    payload_dir = tmp_path / "payload"
    payload_dir.mkdir()
    (payload_dir / "payload.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "version": "9.9.9",
                "platform": "linux-x86_64",
                "archive": "absent.zip",
                "member": "dcc-mcp-cli",
            }
        ),
        encoding="utf-8",
    )
    monkeypatch.setattr(_bootstrap, "payload_dir", lambda: payload_dir)

    with pytest.raises(_bootstrap.PayloadError, match=r"absent\.zip"):
        _bootstrap.load_payload()


def test_execute_flushes_before_handing_over(payload, bin_dir, monkeypatch):
    """POSIX ``execv`` replaces the process, so buffers are flushed first."""
    flushed = []
    monkeypatch.setattr(_bootstrap.sys.stdout, "flush", lambda: flushed.append("stdout"))
    monkeypatch.setattr(_bootstrap.sys.stderr, "flush", lambda: flushed.append("stderr"))

    if os.name == "nt":
        monkeypatch.setattr(_bootstrap.subprocess, "run", lambda command: _Result(0))
        _bootstrap._execute(bin_dir / "dcc-mcp-cli", ["--version"])
    else:
        monkeypatch.setattr(_bootstrap.os, "execv", lambda path, argv: None)
        _bootstrap._execute(bin_dir / "dcc-mcp-cli", ["--version"])

    assert flushed == ["stdout", "stderr"]


class _Result:
    """Minimal stand-in for ``subprocess.CompletedProcess``."""

    def __init__(self, returncode):
        self.returncode = returncode


def test_execute_returns_the_child_exit_code_on_windows(payload, bin_dir, monkeypatch):
    """Windows propagates the child exit code because it cannot ``execv``."""
    monkeypatch.setattr(_bootstrap.os, "name", "nt")
    monkeypatch.setattr(_bootstrap.subprocess, "run", lambda command: _Result(3))

    assert _bootstrap._execute(bin_dir / "dcc-mcp-cli-bin.exe", []) == 3


def test_execute_maps_ctrl_c_to_130_on_windows(payload, bin_dir, monkeypatch):
    """An interrupted child exits 130, the conventional SIGINT code."""

    def interrupt(command):
        raise KeyboardInterrupt

    monkeypatch.setattr(_bootstrap.os, "name", "nt")
    monkeypatch.setattr(_bootstrap.subprocess, "run", interrupt)

    assert _bootstrap._execute(bin_dir / "dcc-mcp-cli-bin.exe", []) == 130
