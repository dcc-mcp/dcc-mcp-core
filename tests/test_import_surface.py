"""Import-surface tests for embedded Python compatibility."""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

# Import local modules
from import_tests import collect_import_failures


def test_dcc_mcp_core_modules_are_importable() -> None:
    """Every importable package module should load without side effects."""
    assert collect_import_failures() == []
