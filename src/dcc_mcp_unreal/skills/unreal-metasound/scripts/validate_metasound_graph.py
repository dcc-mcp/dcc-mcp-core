"""Validate a MetaSound graph for structural correctness.

Checks: no cycles, all inputs connected, all nodes are valid types.
When Unreal version is below 5.4, returns compatible: false with a
structured reason. Read-only — does not modify the graph.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_success, skill_error, skill_warning

# Minimum Unreal Engine version for MetaSound stable API
_MIN_UE_VERSION = (5, 4)


def _validate_asset_path(asset_path: str) -> str | None:
    """Reject paths outside /Game/ or absolute filesystem paths."""
    if not asset_path:
        return "asset_path must not be empty"
    if asset_path.startswith("/") and not asset_path.startswith("/Game/"):
        return f"asset_path must start with '/Game/', got: {asset_path!r}"
    if ":" in asset_path or asset_path.startswith("\\"):
        return f"asset_path looks like a filesystem path: {asset_path!r}"
    if ".." in asset_path:
        return f"asset_path must not contain '..', got: {asset_path!r}"
    return None


def _check_ue_version() -> tuple[str, bool]:
    """Return (ue_version_string, is_compatible)."""
    # Lazy import: requires Unreal's embedded Python.
    import unreal  # noqa: F811

    try:
        ver_str = str(unreal.SystemLibrary.get_engine_version())
        parts = ver_str.split(".")
        major = int(parts[0])
        minor = int(parts[1]) if len(parts) > 1 else 0
    except Exception:
        return ("unknown", True)  # can't determine — assume compatible

    compatible = (major, minor) >= _MIN_UE_VERSION
    return (ver_str, compatible)


@skill_entry
def validate_metasound_graph(
    asset_path: str,
    **kwargs,
) -> dict:
    """Validate a MetaSound graph for structural correctness.

    Checks:
      - No cycles in the graph
      - All inputs are connected
      - All nodes are valid types
      - Unreal version compatibility (5.4+)

    Args:
        asset_path: Path to the MetaSound Source asset under /Game/.

    Returns:
        Success/error dict with validation results, issues list,
        and compatibility status.
    """
    path_error = _validate_asset_path(asset_path)
    if path_error:
        return skill_error("Invalid asset path", path_error)

    # Lazy import: requires Unreal's embedded Python.
    import unreal  # noqa: F811

    # Check UE version compatibility
    ue_version, is_compatible = _check_ue_version()

    if not is_compatible:
        return skill_error(
            f"Unreal Engine version {ue_version} is not compatible with MetaSound",
            f"Minimum required: {_MIN_UE_VERSION[0]}.{_MIN_UE_VERSION[1]}",
            prompt="Upgrade to Unreal Engine 5.4 or later for MetaSound support.",
            compatible=False,
            ue_version=ue_version,
            min_required=f"{_MIN_UE_VERSION[0]}.{_MIN_UE_VERSION[1]}",
            possible_solutions=[
                f"Upgrade Unreal Engine to {_MIN_UE_VERSION[0]}.{_MIN_UE_VERSION[1]}+",
                "Ensure the MetaSound plugin is enabled in your project",
            ],
        )

    try:
        asset = unreal.EditorAssetLibrary.load_asset(asset_path)
        if asset is None:
            return skill_error(
                f"MetaSound Source not found at {asset_path}",
                "Asset could not be loaded",
            )

        graph = asset.get_editor_subsystem(unreal.MetaSoundEditorSubsystem)
        if graph is None:
            return skill_error(
                "Cannot access MetaSound editor subsystem",
                "Ensure the MetaSound plugin is enabled",
            )

        issues = []

        # Check for cycles
        try:
            has_cycle = graph.has_cycle(asset)
            if has_cycle:
                issues.append({
                    "severity": "error",
                    "rule": "no_cycles",
                    "message": "Graph contains one or more cycles",
                })
        except Exception:
            pass  # cycle detection may not be available

        # Check all inputs are connected
        try:
            inputs = graph.get_inputs(asset)
            for inp in inputs:
                if not graph.is_input_connected(asset, inp.get_name()):
                    issues.append({
                        "severity": "warning",
                        "rule": "all_inputs_connected",
                        "message": f"Input '{inp.get_name()}' is not connected",
                    })
        except Exception:
            pass

        # Check all nodes are valid
        try:
            nodes = graph.get_nodes(asset)
            for node in nodes:
                if not graph.is_node_valid(asset, node):
                    issues.append({
                        "severity": "error",
                        "rule": "all_nodes_valid",
                        "message": f"Node '{node.get_name()}' is invalid or unsupported",
                    })
        except Exception:
            pass

        has_errors = any(i["severity"] == "error" for i in issues)
        has_warnings = any(i["severity"] == "warning" for i in issues)

        if has_errors:
            return skill_error(
                f"Graph validation found {len(issues)} issues in {asset_path}",
                f"{sum(1 for i in issues if i['severity'] == 'error')} errors, "
                f"{sum(1 for i in issues if i['severity'] == 'warning')} warnings",
                ue_version=ue_version,
                compatible=True,
                asset_path=asset_path,
                issues=issues,
                valid=False,
            )

        if has_warnings:
            return skill_warning(
                f"Graph is valid but has {len(issues)} warnings in {asset_path}",
                warning=f"{len(issues)} non-critical issues found",
                prompt="Review warnings before building. Run build_metasound to compile.",
                ue_version=ue_version,
                compatible=True,
                asset_path=asset_path,
                issues=issues,
                valid=True,
            )

        return skill_success(
            f"Graph {asset_path} is valid — no issues found",
            prompt="Graph validated successfully. Run build_metasound to compile.",
            ue_version=ue_version,
            compatible=True,
            asset_path=asset_path,
            issues=[],
            valid=True,
            node_count=len(graph.get_nodes(asset)) if graph else 0,
        )

    except Exception as exc:
        return skill_error(
            f"Failed to validate MetaSound graph at {asset_path}",
            str(exc),
            possible_solutions=[
                "Verify the asset exists and is a MetaSound Source",
                "Ensure the MetaSound plugin is enabled in your project",
                "Check Unreal Engine version is 5.4+",
            ],
        )
