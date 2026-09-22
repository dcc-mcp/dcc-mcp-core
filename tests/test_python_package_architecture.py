"""Executable ownership and import-direction rules for the Python package."""

from __future__ import annotations

import ast
from pathlib import Path

from dcc_mcp_core._exports import _EXPERIMENTAL_LAZY
from dcc_mcp_core._exports import _STABLE_LAZY

_PACKAGE = Path(__file__).resolve().parent.parent / "python" / "dcc_mcp_core"

# Ownership pins for lazy exports that a silent re-source could move.
#
# ``test_lazy_export_map_has_no_duplicate_keys`` only catches repeated keys. A
# key stays unique when its *value* is re-pointed at another module, and that
# mutation is invisible to the AST guard, to ``ruff`` F601 and to the runtime:
# ``DEFAULT_PROMOTION_THRESHOLD`` is defined as ``3`` by both
# ``escape_hatch_policy`` and ``skill_promotion``, so re-sourcing it changes no
# observable behaviour at all.
#
# A pin is only worth maintaining when the name is reachable from more than one
# module: if a name lives in exactly one module, re-sourcing it raises
# ``AttributeError`` on first access and cannot slip through. Every entry below
# is therefore a symbol that another top-level namespace also exposes, which
# keeps the table far smaller than a full snapshot of ``_ALL_LAZY`` while making
# every silent re-source fail here instead.
_LAZY_EXPORT_OWNERSHIP: dict[str, str] = {
    # Defined independently by ``escape_hatch_policy`` and ``skill_promotion``
    # (both ``3``); the escape-hatch policy owns the public name.
    "DEFAULT_PROMOTION_THRESHOLD": "dcc_mcp_core.escape_hatch_policy",
    # Also defined by ``constants``.
    "ENV_EXCLUDE_STUBS_FROM_TOOLS_LIST": "dcc_mcp_core.server",
    "ENV_LOG_DIR": "dcc_mcp_core._core",
    # ``script_execution`` re-exports these from ``runtime``; re-sourcing them
    # would move the public contract onto the internal namespaces.
    "SceneDigestError": "dcc_mcp_core.script_execution",
    "SceneDigestExecution": "dcc_mcp_core.script_execution",
    "SceneDigestExecutionError": "dcc_mcp_core.script_execution",
    "SceneDigestSnapshot": "dcc_mcp_core.script_execution",
    "ScriptExecutionContext": "dcc_mcp_core.script_execution",
    "StateDigestProvider": "dcc_mcp_core.script_execution",
    "capture_state_digest": "dcc_mcp_core.script_execution",
    "execute_with_state_digest": "dcc_mcp_core.script_execution",
    "get_default_script_execution_context": "dcc_mcp_core.script_execution",
    "register_state_digest_provider": "dcc_mcp_core.script_execution",
    "reset_default_script_execution_context_for_tests": "dcc_mcp_core.script_execution",
    "update_script_namespace": "dcc_mcp_core.script_execution",
    # ``deployment`` owns these; ``install_lifecycle`` is the compatibility shim
    # that still re-exports them.
    "inspect_install_root": "dcc_mcp_core.deployment",
    "plan_runtime_updates": "dcc_mcp_core.deployment",
    "resolve_deployment_layout": "dcc_mcp_core.deployment",
    "safe_remove_tree": "dcc_mcp_core.deployment",
    "safe_replace_tree": "dcc_mcp_core.deployment",
    "stop_runtime_entries": "dcc_mcp_core.deployment",
}

# Compatibility modules may be removed, but new capabilities must choose an
# ownership-oriented subpackage instead of growing the root again.
_LEGACY_TOP_LEVEL_MODULES = {
    "__init__",
    "_exports",
    "_install_lifecycle_process",
    "_install_lifecycle_readiness",
    "_install_lifecycle_runtime",
    "_install_lifecycle_sidecar",
    "_json_codec",
    "_lazy",
    "_lifecycle_events",
    "_lite_fallback",
    "_path_util",
    "_py37_fallback",
    "_registration",
    "_tool_registration",
    "_typing",
    "_version_util",
    "_windows_dll_search",
    "adapter_context",
    "adapter_contracts",
    "admin_sqlite_lane",
    "admin_tools",
    "agent_memory",
    "asset_import",
    "asset_sync",
    "auth",
    "batch",
    "bridge",
    "cancellation",
    "capabilities",
    "capability_graph",
    "checkpoint",
    "chunked_runner",
    "constants",
    "cua_cli",
    "daemon_launch",
    "dcc_api_executor",
    "dcc_server",
    "docs_resources",
    "elicitation",
    "env",
    "errors",
    "escape_hatch_policy",
    "factory",
    "feedback",
    "gateway_election",
    "guardrails",
    "host_errors",
    "hotreload",
    "install_lifecycle",
    "install_lifecycle_cli",
    "introspect",
    "lifecycle_hooks",
    "loaded_state_store",
    "metadata_registration",
    "observability_query",
    "plugin_manifest",
    "project",
    "qt_dispatcher",
    "readiness",
    "recipes",
    "result_envelope",
    "rich_content",
    "schema",
    "script_execution",
    "script_materialization",
    "script_materialization_tools",
    "semantic_skill_index",
    "server_base",
    "sidecar",
    "skill",
    # Advisory promotion proposals for repeated escape-hatch scripts. Kept at
    # the root next to ``observability_query`` because it is the payload type
    # for that query's response, not an independently owned capability.
    "skill_promotion",
    "skill_reference_docs",
    "skills_helper",
    "spatial",
    "ui_control_server",
    "usd_resources",
    "vector_embedder",
    "vector_skill_index",
    "verifier",
    "workflow_yaml",
}


def test_new_python_capabilities_do_not_grow_the_flat_namespace() -> None:
    current = {path.stem for path in _PACKAGE.glob("*.py")}
    assert current <= _LEGACY_TOP_LEVEL_MODULES


def _read_all_lazy_map() -> dict[str, str]:
    """Parse ``_ALL_LAZY`` straight from the source tree.

    Reading the AST rather than importing ``dcc_mcp_core._exports`` keeps these
    guards honest when the installed wheel is older than the checkout, and keeps
    them runnable on a pure-Python checkout with no built extension module.
    """
    tree = ast.parse((_PACKAGE / "_exports.py").read_text(encoding="utf-8"))
    all_lazy = next(
        node.value
        for node in tree.body
        if isinstance(node, (ast.Assign, ast.AnnAssign))
        and getattr(node.targets[0] if isinstance(node, ast.Assign) else node.target, "id", None) == "_ALL_LAZY"
        and isinstance(node.value, ast.Dict)
    )
    return {ast.literal_eval(key): ast.literal_eval(value) for key, value in zip(all_lazy.keys, all_lazy.values)}


def test_lazy_export_map_has_no_duplicate_keys() -> None:
    """Guard the facade map against silently shadowed symbols.

    ``_ALL_LAZY`` is the single source of truth for ``from dcc_mcp_core import X``.
    A repeated key is accepted by Python (last write wins) and by every runtime
    test, so only an explicit check catches it before it reaches ``main``.
    """
    tree = ast.parse((_PACKAGE / "_exports.py").read_text(encoding="utf-8"))
    all_lazy = next(
        (
            node.value
            for node in tree.body
            if isinstance(node, (ast.Assign, ast.AnnAssign))
            and getattr(node.targets[0] if isinstance(node, ast.Assign) else node.target, "id", None) == "_ALL_LAZY"
            and isinstance(node.value, ast.Dict)
        ),
        None,
    )
    # Default keeps a future refactor of ``_ALL_LAZY`` (e.g. built by a helper
    # instead of a module-level dict literal) reporting as a clean assertion
    # failure rather than a bare ``StopIteration``.
    if all_lazy is None:
        raise AssertionError("no module-level _ALL_LAZY dict literal found in _exports.py")

    seen: dict[str, int] = {}
    duplicates: dict[str, list[int]] = {}
    for key in all_lazy.keys:
        try:
            name = ast.literal_eval(key)
        except (ValueError, TypeError):
            continue
        if not isinstance(name, str):
            continue
        line = getattr(key, "lineno", 0)
        if name in seen:
            duplicates.setdefault(name, [seen[name]]).append(line)
        else:
            seen[name] = line

    assert duplicates == {}, f"duplicate lazy export keys (first wins is NOT the behaviour): {duplicates}"


def test_ambiguous_lazy_exports_keep_their_owning_module() -> None:
    """Guard the facade map against silently re-sourced symbols.

    Unlike a duplicate key, pointing an existing key at a different module keeps
    the map valid and keeps every runtime test green whenever the two modules
    happen to agree on the value. ``_LAZY_EXPORT_OWNERSHIP`` pins the owning
    module for exactly those symbols that another namespace also exposes, so the
    next re-source has to be a deliberate edit to this table.
    """
    all_lazy = _read_all_lazy_map()
    mismatched = {
        name: {"expected": expected, "actual": all_lazy.get(name)}
        for name, expected in _LAZY_EXPORT_OWNERSHIP.items()
        if all_lazy.get(name) != expected
    }
    assert mismatched == {}, f"lazy exports re-sourced away from their owning module: {mismatched}"


def test_stable_exports_never_source_private_python_packages() -> None:
    private_targets = {
        name: module
        for name, module in _STABLE_LAZY.items()
        if module.startswith("dcc_mcp_core._") and module != "dcc_mcp_core._core"
    }
    assert private_targets == {}


def test_internal_helpers_are_experimental_not_stable() -> None:
    assert {"lazy_dir", "resolve_lazy_symbol"} <= _EXPERIMENTAL_LAZY.keys()
    assert _STABLE_LAZY.keys().isdisjoint(_EXPERIMENTAL_LAZY)


def test_ownership_oriented_namespaces_exist() -> None:
    for namespace in ("deployment", "experimental", "host", "runtime", "server", "skill_index", "skills", "wire"):
        assert (_PACKAGE / namespace / "__init__.py").is_file()


def test_public_namespaces_preserve_compatibility_identity() -> None:
    import dcc_mcp_core
    from dcc_mcp_core import deployment
    from dcc_mcp_core import install_lifecycle
    from dcc_mcp_core import qt_dispatcher
    from dcc_mcp_core import server
    from dcc_mcp_core.host import qt_dispatcher as host_qt_dispatcher

    assert dcc_mcp_core.DccServerOptions is server.DccServerOptions
    assert deployment.resolve_deployment_layout is install_lifecycle.resolve_deployment_layout
    assert host_qt_dispatcher.start_qt_server is qt_dispatcher.start_qt_server


def test_experimental_symbols_keep_root_compatibility_without_star_export() -> None:
    import dcc_mcp_core
    from dcc_mcp_core import experimental

    assert dcc_mcp_core.resolve_lazy_symbol is experimental.resolve_lazy_symbol
    assert "resolve_lazy_symbol" not in dcc_mcp_core.__all__


def test_dcc_server_base_exposes_owned_components() -> None:
    from dcc_mcp_core.server_base import DccServerBase

    instance = DccServerBase.__new__(DccServerBase)
    assert instance.skill_discovery is instance.skill_discovery
    assert instance.execution is instance.execution
    assert instance.lifecycle is instance.lifecycle
    assert instance.observability is instance.observability
