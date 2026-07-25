"""verify_environment — Validate DCC environment compatibility.

Checks Python version, dcc-mcp-core version, and known compatibility issues.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from typing import Any


def verify_environment(
    instance_id: str | None = None,
    dcc_type: str | None = None,
    check_dependencies: bool = True,
    check_config: bool = True,
) -> dict[str, Any]:
    """Validate the DCC environment.

    Args:
        instance_id: Specific instance to check.
        dcc_type: DCC type filter.
        check_dependencies: Whether to validate dependencies.
        check_config: Whether to validate env config.

    Returns:
        Structured environment report.
    """
    issues: list[dict[str, str]] = []
    recommendations: list[str] = []

    # Python version
    py_version = f"{sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}"
    py_version_tuple = (sys.version_info.major, sys.version_info.minor)
    py37_ok = True

    if py_version_tuple < (3, 7):
        issues.append({
            "severity": "error",
            "category": "python_version",
            "message": f"Python {py_version} is below minimum 3.7.",
        })
        py37_ok = False
    if py_version_tuple == (3, 7):
        recommendations.append(
            "Python 3.7 detected. Verify dcc-mcp-core version is compatible "
            "(0.18.x for native py37 support, or 0.19.x+ with py37-lite fallback). "
            "See ADR 011 for the py37 compatibility contract."
        )

    # dcc-mcp-core version
    core_version = "unknown"
    try:
        import dcc_mcp_core
        core_version = dcc_mcp_core.__version__
    except ImportError:
        issues.append({
            "severity": "error",
            "category": "core_import",
            "message": "dcc_mcp_core is not importable.",
        })
    except Exception:
        issues.append({
            "severity": "warning",
            "category": "core_version",
            "message": "dcc_mcp_core imported but __version__ not available.",
        })

    # py37 compatibility check
    if py_version_tuple == (3, 7) and core_version != "unknown":
        try:
            core_major = int(core_version.split(".")[1]) if core_version.startswith("0.") else 0
            if core_major >= 19:
                issues.append({
                    "severity": "warning",
                    "category": "py37_compat",
                    "message": (
                        f"dcc-mcp-core {core_version} on Python 3.7 may enter py37-lite "
                        "fallback mode. Dispatch requires native wheel. "
                        "Consider pinning to 0.18.x for full dispatch support."
                    ),
                })
        except (ValueError, IndexError):
            pass

    # Config checks
    config_issues = []
    if check_config:
        # Check DCC_MCP_* env vars
        mcp_vars = {k: v for k, v in os.environ.items() if k.startswith("DCC_MCP_")}
        if "DCC_MCP_LOG_DIR" in mcp_vars:
            log_dir = mcp_vars["DCC_MCP_LOG_DIR"]
            if not os.path.isdir(log_dir):
                config_issues.append({
                    "severity": "warning",
                    "category": "config",
                    "message": f"DCC_MCP_LOG_DIR points to non-existent directory: {log_dir}",
                })
        if "DCC_MCP_BASE_URL" in mcp_vars:
            recommendations.append(
                f"DCC_MCP_BASE_URL is set to {mcp_vars['DCC_MCP_BASE_URL']}. "
                "Local profile ignores this; remove if using local-first workflow."
            )

    issues.extend(config_issues)

    compatible = not any(i["severity"] == "error" for i in issues)
    if not recommendations:
        recommendations.append("Environment looks compatible. No issues detected.")

    return {
        "success": True,
        "compatible": compatible,
        "python_version": py_version,
        "core_version": core_version,
        "issues": issues,
        "recommendations": recommendations,
    }
