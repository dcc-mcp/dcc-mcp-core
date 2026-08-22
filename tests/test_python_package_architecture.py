"""Executable ownership and import-direction rules for the Python package."""

from __future__ import annotations

from pathlib import Path

from dcc_mcp_core._exports import _EXPERIMENTAL_LAZY
from dcc_mcp_core._exports import _STABLE_LAZY

_PACKAGE = Path(__file__).resolve().parent.parent / "python" / "dcc_mcp_core"

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
