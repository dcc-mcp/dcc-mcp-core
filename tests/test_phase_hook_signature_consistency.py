"""Verify phase hook method signatures match between server_base.py and _registration.py.

This prevents silent TypeError at registration time (PIP-2479, PIP-2468).
``run_registration_phases`` catches phase exceptions as non-fatal, so a
signature mismatch between a ``DccServerBase`` phase hook and the caller in
``_registration.py`` does NOT fail CI — it silently skips registration for
those tools.

These tests use AST parsing and never import ``DccServerBase``, so they work
without a built ``dcc_mcp_core._core`` binary.
"""

from __future__ import annotations

import ast
from pathlib import Path

_PROJECT_ROOT = Path(__file__).resolve().parent.parent
_SERVER_BASE = _PROJECT_ROOT / "python" / "dcc_mcp_core" / "server_base.py"
_REGISTRATION = _PROJECT_ROOT / "python" / "dcc_mcp_core" / "_registration.py"


def _get_non_self_param_count(file: Path, class_name: str, method_name: str) -> int:
    """Return the number of positional params excluding ``self``."""
    tree = ast.parse(file.read_bytes())

    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef) and node.name == class_name:
            for item in node.body:
                if isinstance(item, ast.FunctionDef) and item.name == method_name:
                    arg_names = [a.arg for a in item.args.args if a.arg != "self"]
                    return len(arg_names)
            break
    msg = f"Method {class_name}.{method_name} not found in {file}"
    raise ValueError(msg)


# -- phase hooks called WITH a RegistrationContext argument -------------------

CONTEXT_HOOKS: tuple[str, ...] = (
    "_register_core_builtin_actions",
    "_run_strict_skill_scan_phase",
    "_register_metadata_driven_tools",
    "_register_introspect_tools",
    "_register_feedback_tool",
    "_register_qt_ui_inspector",
    "_register_capability_manifest_tool",
    "_attach_project_tools",
    "_attach_resources",
    "_mark_skill_catalog_ready",
)

# -- phase hooks called WITH NO arguments (only self) ------------------------

NO_ARG_HOOKS: tuple[str, ...] = ()


class TestPhaseHookSignatureConsistency:
    """Every ``DccServerBase`` phase hook must match its caller in ``_registration.py``.

    ``_registration.py`` dispatch:
      - ``CoreBuiltinActionsPhase``:     ``server._register_core_builtin_actions(context)``
      - ``StrictSkillScanPhase``:        ``server._run_strict_skill_scan_phase(context)``
      - ``MetadataDrivenToolsPhase``:    ``server._register_metadata_driven_tools(context)``
      - ``IntrospectToolsPhase``:        ``server._register_introspect_tools(context)``
      - ``FeedbackToolPhase``:           ``server._register_feedback_tool(context)``
      - ``QtUiInspectorPhase``:          ``server._register_qt_ui_inspector(context)``
      - ``CapabilityManifestPhase``:     ``server._register_capability_manifest_tool(context)``
      - ``ProjectToolsPhase``:           ``server._attach_project_tools(context)``
      - ``ResourcesPhase``:              ``server._attach_resources(context)``
      - ``SkillCatalogReadyPhase``:      ``server._mark_skill_catalog_ready(context)``
    """

    def test_context_hooks_accept_one_arg(self) -> None:
        """Hooks dispatched with a ``RegistrationContext`` must accept exactly one required arg."""
        for hook_name in CONTEXT_HOOKS:
            count = _get_non_self_param_count(_SERVER_BASE, "DccServerBase", hook_name)
            assert count == 1, (
                f"{hook_name} is called with (context) in _registration.py "
                f"but its definition in DccServerBase expects {count} "
                f"positional args (expected 1)"
            )

    def test_no_arg_hooks_accept_zero_args(self) -> None:
        """Hooks dispatched with no arguments must not require any positional args beyond self."""
        for hook_name in NO_ARG_HOOKS:
            count = _get_non_self_param_count(_SERVER_BASE, "DccServerBase", hook_name)
            assert count == 0, (
                f"{hook_name} is called with no arguments in _registration.py "
                f"but its definition in DccServerBase expects {count} "
                f"positional args (expected 0)"
            )

    def test_all_standard_phase_hooks_are_covered(self) -> None:
        """Every phase-like method on DccServerBase is listed in one of the two groups."""
        tree = ast.parse(_SERVER_BASE.read_bytes())

        dcc_base_methods: set[str] = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.ClassDef) and node.name == "DccServerBase":
                for item in node.body:
                    if isinstance(item, ast.FunctionDef):
                        nm = item.name
                        if (
                            nm.startswith("_register_")
                            or nm.startswith("_run_")
                            or nm.startswith("_attach_")
                            or nm.startswith("_mark_")
                        ):
                            dcc_base_methods.add(nm)
                break

        # Explicitly exclude helpers that are not phase hooks
        non_phase: set[str] = {"_register_builtin_skills"}
        phase_methods = dcc_base_methods - non_phase
        covered = set(CONTEXT_HOOKS) | set(NO_ARG_HOOKS)
        missing = sorted(phase_methods - covered)
        assert not missing, (
            f"Phase hook methods {missing} are not covered by the "
            f"signature consistency check. Either add them to the "
            f"appropriate group in this test, or exclude them explicitly."
        )

    def test_all_registration_calls_match_dcc_server_base(self) -> None:
        """Verify each phase in _registration.py calls with the arg count that DccServerBase defines."""
        # Map: phase hook name → expected non-self positional parameters from _registration.py
        registration_calls: dict[str, int] = {}
        tree = ast.parse(_REGISTRATION.read_bytes())

        for node in ast.walk(tree):
            if not isinstance(node, ast.ClassDef):
                continue
            # Look for RegistrationPhase subclasses with a run() method that makes server calls
            for item in node.body:
                if not (isinstance(item, ast.FunctionDef) and item.name == "run"):
                    continue
                for child in ast.walk(item):
                    if (
                        isinstance(child, ast.Call)
                        and isinstance(child.func, ast.Attribute)
                        and isinstance(child.func.value, ast.Attribute)
                        and child.func.value.attr == "server"
                    ):
                        method_name = child.func.attr
                        n_args = len(child.args)
                        registration_calls.setdefault(method_name, n_args)
                        # Verify consistency: all calls to the same method should
                        # pass the same number of arguments
                        assert registration_calls[method_name] == n_args, (
                            f"Inconsistent calls to {method_name}: "
                            f"expected {registration_calls[method_name]} args, "
                            f"got {n_args}"
                        )

        covered = set(CONTEXT_HOOKS) | set(NO_ARG_HOOKS)
        for method_name, call_args in registration_calls.items():
            if method_name not in covered:
                continue
            # Skip methods not defined on DccServerBase (e.g. _readiness.mark_skill_catalog_ready)
            try:
                defined_args = _get_non_self_param_count(
                    _SERVER_BASE, "DccServerBase", method_name
                )
            except ValueError:
                continue  # method is not on DccServerBase, skip
            assert defined_args == call_args, (
                f"_registration.py calls {method_name} with {call_args} arg(s) "
                f"but DccServerBase defines it with {defined_args} non-self "
                f"param(s). This will raise TypeError at runtime."
            )
