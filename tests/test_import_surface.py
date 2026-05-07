"""Backward-compatible import surface test module."""

# Import future modules
from __future__ import annotations

# Import built-in modules
import importlib.util
from pathlib import Path

_IMPORTS_PATH = Path(__file__).with_name("test_imports.py")
_SPEC = importlib.util.spec_from_file_location("_dcc_mcp_core_test_imports", _IMPORTS_PATH)
assert _SPEC is not None and _SPEC.loader is not None
_IMPORTS = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_IMPORTS)


def test_dcc_mcp_core_modules_are_importable() -> None:
    """Every importable package module should load without side effects."""
    _IMPORTS.assert_import_smoke()
