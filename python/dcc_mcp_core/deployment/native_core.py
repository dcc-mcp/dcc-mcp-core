"""Embedded deployment preflight and actionable native loader failures."""

from __future__ import annotations

import importlib
import sys
from types import ModuleType
from typing import Any

from dcc_mcp_core._windows_dll_search import embedded_python_dll_directories
from dcc_mcp_core._windows_dll_search import prepare_embedded_python_dll_search

_MODULE = "dcc_mcp_core._core"


def _failure_report(exc: ImportError | OSError) -> dict[str, Any]:
    missing = isinstance(exc, ModuleNotFoundError) and exc.name == _MODULE
    report: dict[str, Any] = {
        "success": False,
        "reason": "native_extension_missing" if missing else "native_load_failed",
        "module": _MODULE,
        "platform": sys.platform,
        "python_version": sys.version.split()[0],
        "executable": sys.executable,
        "error_type": type(exc).__name__,
        "error": str(exc),
        "dll_search_directories": list(embedded_python_dll_directories()) if sys.platform == "win32" else [],
        "next_steps": ["Install the native dcc-mcp-core wheel matching this host interpreter and architecture."],
    }
    if sys.platform == "win32" and not missing:
        report["next_steps"].append(
            "Inspect _core.pyd dependencies: an ABI3 wheel requires python3.dll; "
            "other runtime DLLs may also be missing. "
            "If the host omits python3.dll, set DCC_MCP_PYTHON3_DLL_DIR to an absolute directory containing a trusted "
            "DLL compatible with this host Python version and architecture, then retry verify_native_core(). "
            "PATH alone does not configure Python 3.8+ extension dependency loading."
        )
    return report


def _load_native_core() -> ModuleType:
    """Load native APIs, preserving lite absence and the original failure cause."""
    if sys.modules.get(_MODULE) is None:
        prepare_embedded_python_dll_search()
    try:
        return importlib.import_module(_MODULE)
    except (ImportError, OSError) as exc:
        if isinstance(exc, ModuleNotFoundError) and exc.name == _MODULE:
            raise
        raise ImportError(_error_message(_failure_report(exc))) from exc


def _error_message(report: dict[str, Any]) -> str:
    """Format a stable loader error from a shared failure report."""
    directories = ", ".join(report["dll_search_directories"]) or "(none)"
    message = (
        f"Cannot load {_MODULE} in {sys.executable} (Python {report['python_version']}, {sys.platform}): "
        f"{report['error']}. " + " ".join(report["next_steps"])
    )
    if sys.platform == "win32":
        message += f" DLL directory candidates: {directories}."
    return message


def verify_native_core() -> dict[str, Any]:
    """Attempt the native import without starting a server or importing a DCC API.

    Returns:
        A JSON-safe report with ``success`` and a stable ``reason``. A failure
        includes the original loader error, interpreter and recovery guidance.
        Local paths are diagnostic data; redact them before public reporting.
        Success proves extension loading only, not DCC or dispatcher readiness.

    """
    prepare_embedded_python_dll_search()
    try:
        core = importlib.import_module(_MODULE)
    except (ImportError, OSError) as exc:
        return _failure_report(exc)
    return {
        "success": True,
        "reason": "native_core_available",
        "module": _MODULE,
        "native_version": str(getattr(core, "__version__", "unknown")),
        "python_version": sys.version.split()[0],
        "executable": sys.executable,
        "platform": sys.platform,
    }
