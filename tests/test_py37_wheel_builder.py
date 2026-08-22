"""Metadata projection tests for the extensionless Python 3.7 wheel."""

from __future__ import annotations

from scripts.build_py37_pure_wheel import _read_console_scripts
from scripts.build_py37_pure_wheel import _read_runtime_requirements


def test_lite_wheel_inherits_project_runtime_dependencies() -> None:
    requirements = _read_runtime_requirements()
    assert "dcc-mcp-server>=0.18.17,<1.0.0" in requirements
    assert "typing-extensions==4.7.1; python_version<'3.8'" in requirements


def test_lite_wheel_inherits_project_console_scripts() -> None:
    assert _read_console_scripts() == {
        "dcc-mcp-install-lifecycle": "dcc_mcp_core.install_lifecycle_cli:main",
        "dcc-mcp-ui-control-server": "dcc_mcp_core.ui_control_server:cli",
    }
