"""Registration phases own their standard behavior without base-class twins."""

from __future__ import annotations

import ast
from pathlib import Path

from dcc_mcp_core._registration import CoreBuiltinActionsPhase
from dcc_mcp_core._registration import RegistrationContext

_PROJECT_ROOT = Path(__file__).resolve().parent.parent
_SERVER_BASE = _PROJECT_ROOT / "python" / "dcc_mcp_core" / "server_base.py"

_REMOVED_PHASE_HOOKS = {
    "_attach_project_tools",
    "_attach_resources",
    "_mark_skill_catalog_ready",
    "_register_capability_manifest_tool",
    "_register_core_builtin_actions",
    "_register_feedback_tool",
    "_register_introspect_tools",
    "_register_metadata_driven_tools",
    "_register_qt_ui_inspector",
    "_run_strict_skill_scan_phase",
}


def test_dcc_server_base_does_not_duplicate_standard_phase_implementations() -> None:
    tree = ast.parse(_SERVER_BASE.read_bytes())
    server = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "DccServerBase")
    methods = {node.name for node in server.body if isinstance(node, ast.FunctionDef)}
    assert methods.isdisjoint(_REMOVED_PHASE_HOOKS)


def test_standard_phase_runs_without_a_base_class_hook() -> None:
    calls = []

    class Server:
        def register_builtin_actions(self, **kwargs):
            calls.append(kwargs)

    context = RegistrationContext(
        server=Server(),
        extra_skill_paths=["skills"],
        include_bundled=False,
        minimal_mode="minimal",
    )
    CoreBuiltinActionsPhase().run(context)
    assert calls == [
        {
            "extra_skill_paths": ["skills"],
            "include_bundled": False,
            "minimal_mode": "minimal",
        }
    ]


def test_legacy_adapter_phase_extension_remains_compatible() -> None:
    calls = []

    class AdapterServer:
        def _register_core_builtin_actions(self, context):
            calls.append(context)

        def register_builtin_actions(self, **kwargs):
            raise AssertionError("legacy extension should take precedence")

    context = RegistrationContext(server=AdapterServer())
    CoreBuiltinActionsPhase().run(context)
    assert calls == [context]
