"""Connect two nodes in a MetaSound graph.

Uses a structured connection descriptor:
  {"from_node": "...", "from_pin": "...", "to_node": "...", "to_pin": "..."}

Validates that both nodes exist in the graph before making the connection.
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
def connect_metasound_nodes(
    asset_path: str,
    from_node: str,
    from_pin: str,
    to_node: str,
    to_pin: str,
    **kwargs,
) -> dict:
    """Connect two nodes in a MetaSound graph.

    Args:
        asset_path: Path to the MetaSound Source asset under /Game/.
        from_node: Name of the source node.
        from_pin: Name of the output pin on the source node.
        to_node: Name of the target node.
        to_pin: Name of the input pin on the target node.

    Returns:
        Success/error dict with connection descriptor.
    """
    path_error = _validate_asset_path(asset_path)
    if path_error:
        return skill_error("Invalid asset path", path_error)

    if not from_node or not from_pin:
        return skill_error("Invalid source", "from_node and from_pin must be non-empty")
    if not to_node or not to_pin:
        return skill_error("Invalid target", "to_node and to_pin must be non-empty")

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

        # Connect the nodes
        graph.connect_nodes(
            asset,
            from_node,
            from_pin,
            to_node,
            to_pin,
        )

        connection = {
            "from_node": from_node,
            "from_pin": from_pin,
            "to_node": to_node,
            "to_pin": to_pin,
        }

        return skill_success(
            f"Connected {from_node}.{from_pin} -> {to_node}.{to_pin} in {asset_path}",
            prompt="Connection made. Build the graph with build_metasound "
                   "or validate with validate_metasound_graph.",
            asset_path=asset_path,
            connection=connection,
        )

    except Exception as exc:
        return skill_error(
            f"Failed to connect {from_node}.{from_pin} -> {to_node}.{to_pin}",
            str(exc),
            possible_solutions=[
                "Verify both node names exist in the graph",
                "Check that the pin names are correct for each node type",
                "Ensure the output pin type is compatible with the input pin type",
            ],
        )
