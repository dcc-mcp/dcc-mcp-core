"""Shared fixtures for the ``dcc-mcp-cli`` wrapper package tests.

The wrapper is a separate distribution under ``pkg/``, so it is neither
importable nor on ``sys.path`` by default. This conftest makes both the
package sources and the repository's release scripts importable, whether the
suite is started from the repository root or from inside ``pkg/``.
"""

from __future__ import annotations

from pathlib import Path
import sys

PACKAGE_ROOT = Path(__file__).resolve().parent.parent
PYTHON_ROOT = PACKAGE_ROOT / "python"
REPO_ROOT = PACKAGE_ROOT.parent.parent

for _path in (str(PYTHON_ROOT), str(REPO_ROOT)):
    if _path not in sys.path:
        sys.path.insert(0, _path)
