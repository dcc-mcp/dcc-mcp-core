from pathlib import Path


def test_adds_embedded_python_prefix_to_windows_dll_search(monkeypatch, tmp_path):
    from dcc_mcp_core import _windows_dll_search as windows_dll_search

    added = []
    handle = object()
    monkeypatch.setattr(windows_dll_search.sys, "platform", "win32")
    monkeypatch.setattr(windows_dll_search.sys, "prefix", str(tmp_path))
    monkeypatch.setattr(windows_dll_search.os, "add_dll_directory", lambda path: added.append(path) or handle, raising=False)
    windows_dll_search._DLL_DIRECTORY_HANDLES.clear()

    windows_dll_search.prepare_embedded_python_dll_search()

    assert added == [str(Path(tmp_path).resolve())]
    assert [handle] == windows_dll_search._DLL_DIRECTORY_HANDLES


def test_skips_dll_search_setup_outside_windows(monkeypatch):
    from dcc_mcp_core import _windows_dll_search as windows_dll_search

    monkeypatch.setattr(windows_dll_search.sys, "platform", "linux")
    windows_dll_search._DLL_DIRECTORY_HANDLES.clear()

    windows_dll_search.prepare_embedded_python_dll_search()

    assert windows_dll_search._DLL_DIRECTORY_HANDLES == []
