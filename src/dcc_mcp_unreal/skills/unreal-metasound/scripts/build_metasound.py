"""Compile / build a MetaSound asset.

Triggers the MetaSound graph compilation and returns build status
along with any errors or warnings. The asset must be built before
it can be used at runtime.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_success, skill_error


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


@skill_entry
def build_metasound(
    asset_path: str,
    **kwargs,
) -> dict:
    """Compile / build a MetaSound asset.

    Args:
        asset_path: Path to the MetaSound Source asset under /Game/.

    Returns:
        Success/error dict with build status, errors, and warnings.
    """
    path_error = _validate_asset_path(asset_path)
    if path_error:
        return skill_error("Invalid asset path", path_error)

    # Lazy import: requires Unreal's embedded Python.
    import unreal  # noqa: F811

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

        # Build / compile the graph
        build_result = graph.build(asset)

        # Save the asset after successful build
        unreal.EditorAssetLibrary.save_asset(asset_path)

        return skill_success(
            f"Built MetaSound Source {asset_path} successfully",
            prompt="MetaSound built and saved. Validate with "
                   "validate_metasound_graph to check for issues.",
            asset_path=asset_path,
            build_status="success",
            errors=getattr(build_result, "errors", []),
            warnings=getattr(build_result, "warnings", []),
        )

    except Exception as exc:
        return skill_error(
            f"Failed to build MetaSound Source at {asset_path}",
            str(exc),
            possible_solutions=[
                "Check the graph for unconnected required inputs",
                "Run validate_metasound_graph to detect structural issues",
                "Verify all node types and connections are valid",
            ],
        )
