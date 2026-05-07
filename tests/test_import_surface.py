"""Import-surface tests for embedded Python compatibility."""

# Import future modules
from __future__ import annotations

# Import built-in modules
import importlib
import pkgutil

# Import local modules
import dcc_mcp_core


def test_dcc_mcp_core_modules_are_importable() -> None:
    """Every importable package module should load without side effects."""
    failures: list[tuple[str, str]] = []
    for module_info in pkgutil.walk_packages(dcc_mcp_core.__path__, prefix=f"{dcc_mcp_core.__name__}."):
        try:
            importlib.import_module(module_info.name)
        except Exception as exc:  # pragma: no cover - assertion reports details
            failures.append((module_info.name, f"{type(exc).__name__}: {exc}"))

    assert failures == []
