"""Python 3.7 compatibility guard for the bridge capacity module (issue #2821)."""

from __future__ import annotations

import ast
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[1] / "python" / "dcc_mcp_core" / "wire" / "capacity.py"


def test_capacity_module_parses_as_python37():
    source = SOURCE.read_text(encoding="utf-8")

    tree = ast.parse(source, filename=str(SOURCE), feature_version=(3, 7))

    assert isinstance(tree, ast.Module)
