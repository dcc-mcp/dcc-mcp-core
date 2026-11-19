"""Python 3.7 compatibility guard for the bridge capacity module (issue #2821)."""

from __future__ import annotations

import ast
from pathlib import Path
import sys

PYTHON_ROOT = Path(__file__).resolve().parents[1] / "python"
SOURCES = (
    PYTHON_ROOT / "dcc_mcp_core" / "wire" / "capacity.py",
    PYTHON_ROOT / "dcc_mcp_core" / "bridge_capacity.py",
)


def test_capacity_modules_parse_as_python37():
    for source_path in SOURCES:
        source = source_path.read_text(encoding="utf-8")

        # Python 3.7 has no ast.parse(feature_version); compile with that interpreter.
        if sys.version_info[:2] == (3, 7):
            compile(source, str(source_path), "exec")
        else:
            tree = ast.parse(source, filename=str(source_path), feature_version=(3, 7))
            assert isinstance(tree, ast.Module)
