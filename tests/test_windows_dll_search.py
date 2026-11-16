"""Embedded DLL directory registration must be bounded and keep handles alive."""

from __future__ import annotations

import os

import pytest

from dcc_mcp_core import _windows_dll_search as dll_search


@pytest.fixture
def windows(monkeypatch, tmp_path):
    monkeypatch.setattr(dll_search.sys, "platform", "win32")
    monkeypatch.setattr(dll_search.sys, "prefix", str(tmp_path))
    monkeypatch.setattr(dll_search.sys, "base_prefix", str(tmp_path))
    monkeypatch.setattr(dll_search.sys, "executable", str(tmp_path / "mobupy.exe"))
    monkeypatch.setattr(dll_search, "_DLL_DIRECTORY_HANDLES", {})
    monkeypatch.delenv("DCC_MCP_PYTHON3_DLL_DIR", raising=False)
    return tmp_path


def test_adds_interpreter_directories_and_retains_handles_once(windows, monkeypatch):
    base = windows / "base"
    base.mkdir()
    explicit = windows / "compatible-runtime"
    explicit.mkdir()
    executable = windows / "host"
    executable.mkdir()
    monkeypatch.setattr(dll_search.sys, "base_prefix", str(base))
    monkeypatch.setattr(dll_search.sys, "executable", str(executable / "motionbuilder.exe"))
    monkeypatch.setenv("DCC_MCP_PYTHON3_DLL_DIR", str(explicit))
    added = {}

    def add(directory):
        handle = object()
        added[directory] = handle
        return handle

    monkeypatch.setattr(dll_search.os, "add_dll_directory", add, raising=False)
    expected = tuple(str(path.resolve()) for path in (explicit, windows, base, executable))
    assert dll_search.prepare_embedded_python_dll_search() == expected
    assert dll_search.prepare_embedded_python_dll_search() == expected
    assert len(added) == 4
    assert {os.path.normcase(path): value for path, value in added.items()} == dll_search._DLL_DIRECTORY_HANDLES


@pytest.mark.parametrize("override", ["relative-runtime", "missing-runtime"])
def test_invalid_override_does_not_expand_to_cwd_or_path(windows, monkeypatch, override):
    value = override if override.startswith("relative") else str(windows / override)
    monkeypatch.setenv("DCC_MCP_PYTHON3_DLL_DIR", value)
    monkeypatch.setenv("PATH", str(windows / "untrusted-path"))
    added = []
    monkeypatch.setattr(dll_search.os, "add_dll_directory", lambda path: added.append(path), raising=False)
    assert dll_search.prepare_embedded_python_dll_search() == (str(windows.resolve()),)
    assert added == [str(windows.resolve())]


def test_rejected_directory_does_not_hide_remaining_interpreter_paths(windows, monkeypatch):
    base = windows / "base"
    base.mkdir()
    monkeypatch.setattr(dll_search.sys, "base_prefix", str(base))

    def add(directory):
        if directory == str(windows.resolve()):
            raise OSError("directory unavailable")
        return object()

    monkeypatch.setattr(dll_search.os, "add_dll_directory", add, raising=False)
    assert dll_search.prepare_embedded_python_dll_search() == (str(base.resolve()),)


def test_skips_dll_search_setup_outside_windows(monkeypatch):
    monkeypatch.setattr(dll_search.sys, "platform", "linux")
    assert dll_search.prepare_embedded_python_dll_search() == ()
