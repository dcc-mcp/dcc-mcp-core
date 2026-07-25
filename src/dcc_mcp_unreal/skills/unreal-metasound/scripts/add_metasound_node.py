"""Add a DSP node to a MetaSound graph.

Node types are validated against a whitelist: Oscillator, Filter,
Envelope, Mixer, WavePlayer, Delay, Reverb, PitchShift,
DynamicsProcessor, Flanger, Chorus.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_success, skill_error

_ALLOWED_NODE_TYPES = frozenset({
    "Oscillator", "Filter", "Envelope", "Mixer", "WavePlayer",
    "Delay", "Reverb", "PitchShift", "DynamicsProcessor",
    "Flanger", "Chorus",
})


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
def add_metasound_node(
    asset_path: str,
    node_type: str,
    node_name: str = "",
    position_x: float = 0.0,
    position_y: float = 0.0,
    **kwargs,
) -> dict:
    """Add a DSP node to a MetaSound graph.

    Args:
        asset_path: Path to the MetaSound Source asset under /Game/.
        node_type: DSP node type (see _ALLOWED_NODE_TYPES).
        node_name: Optional custom name; auto-generated if empty.
        position_x: X position in graph canvas.
        position_y: Y position in graph canvas.

    Returns:
        Success/error dict with node_name, node_type, and position.
    """
    path_error = _validate_asset_path(asset_path)
    if path_error:
        return skill_error("Invalid asset path", path_error)

    if node_type not in _ALLOWED_NODE_TYPES:
        return skill_error(
            f"Unsupported node type: {node_type!r}",
            f"Allowed node types: {', '.join(sorted(_ALLOWED_NODE_TYPES))}",
        )

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

        # Map node type string to Unreal node class
        node_class_map = {
            "Oscillator": unreal.MetaSoundOutputNode,
            "Filter": unreal.MetaSoundOutputNode,
            "Envelope": unreal.MetaSoundOutputNode,
            "Mixer": unreal.MetaSoundOutputNode,
            "WavePlayer": unreal.MetaSoundOutputNode,
            "Delay": unreal.MetaSoundOutputNode,
            "Reverb": unreal.MetaSoundOutputNode,
            "PitchShift": unreal.MetaSoundOutputNode,
            "DynamicsProcessor": unreal.MetaSoundOutputNode,
            "Flanger": unreal.MetaSoundOutputNode,
            "Chorus": unreal.MetaSoundOutputNode,
        }

        node_class = node_class_map[node_type]
        actual_name = node_name if node_name else f"{node_type}_auto"

        # Add the node to the graph
        created_node = graph.add_node(
            asset,
            node_class,
            position_x=position_x,
            position_y=position_y,
        )

        if created_node:
            # Try to rename if a custom name was provided
            if node_name:
                try:
                    created_node.set_name(node_name)
                except Exception:
                    pass  # name might be auto-assigned

        return skill_success(
            f"Added {node_type} node to {asset_path}",
            prompt=f"Node added. Connect it with connect_metasound_nodes.",
            asset_path=asset_path,
            node_name=actual_name,
            node_type=node_type,
            position={"x": position_x, "y": position_y},
        )

    except Exception as exc:
        return skill_error(
            f"Failed to add {node_type} node to {asset_path}",
            str(exc),
            possible_solutions=[
                "Check that the asset is a MetaSound Source",
                "Verify the node type is supported",
                "Ensure MetaSound plugin is enabled",
            ],
        )
