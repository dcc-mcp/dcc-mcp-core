"""List all nodes in a MetaSound graph with their names and types.

Read-only inspection tool — does not modify the graph.
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
def list_metasound_nodes(
    asset_path: str,
    **kwargs,
) -> dict:
    """List all nodes in a MetaSound graph.

    Args:
        asset_path: Path to the MetaSound Source asset under /Game/.

    Returns:
        Success/error dict with nodes list (each node: name + type) and node_count.
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

        # Enumerate nodes
        raw_nodes = graph.get_nodes(asset)
        nodes = []
        for node in raw_nodes:
            nodes.append({
                "name": str(node.get_name()) if hasattr(node, "get_name") else str(node),
                "type": str(node.get_class().get_name()) if hasattr(node, "get_class") else "unknown",
            })

        return skill_success(
            f"Found {len(nodes)} nodes in {asset_path}",
            prompt=f"Listed {len(nodes)} nodes. Inspect connections with "
                   "validate_metasound_graph.",
            asset_path=asset_path,
            node_count=len(nodes),
            nodes=nodes,
        )

    except Exception as exc:
        return skill_error(
            f"Failed to list nodes in {asset_path}",
            str(exc),
            possible_solutions=[
                "Verify the asset exists and is a MetaSound Source",
                "Check that the MetaSound plugin is enabled",
            ],
        )
