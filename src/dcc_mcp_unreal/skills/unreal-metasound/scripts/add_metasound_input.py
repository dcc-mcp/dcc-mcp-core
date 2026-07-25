"""Add an input parameter to a MetaSound graph.

Supported input types: Float, Bool, Int, String, WaveTable, Object.
Validates the value_type against the allowed set and ensures the
MetaSound Source asset exists and is editable.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_success, skill_error

_ALLOWED_INPUT_TYPES = frozenset({
    "Float", "Bool", "Int", "String", "WaveTable", "Object",
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
def add_metasound_input(
    asset_path: str,
    input_name: str,
    value_type: str,
    default_value=None,
    **kwargs,
) -> dict:
    """Add an input parameter to a MetaSound graph.

    Args:
        asset_path: Path to the MetaSound Source asset under /Game/.
        input_name: Name for the input parameter.
        value_type: Parameter type (Float, Bool, Int, String, WaveTable, Object).
        default_value: Optional default value matching value_type.

    Returns:
        Success/error dict with input_name, value_type, and default_value.
    """
    path_error = _validate_asset_path(asset_path)
    if path_error:
        return skill_error("Invalid asset path", path_error)

    if not input_name or not input_name.strip():
        return skill_error("Invalid input name", "input_name must be a non-empty string")

    if value_type not in _ALLOWED_INPUT_TYPES:
        return skill_error(
            f"Unsupported input type: {value_type!r}",
            f"Allowed types: {', '.join(sorted(_ALLOWED_INPUT_TYPES))}",
        )

    # Lazy import: requires Unreal's embedded Python.
    import unreal  # noqa: F811

    try:
        # Load the MetaSound Source asset
        asset = unreal.EditorAssetLibrary.load_asset(asset_path)
        if asset is None:
            return skill_error(
                f"MetaSound Source not found at {asset_path}",
                "Asset could not be loaded — check the path and that it was created",
            )

        # Access the graph
        graph = asset.get_editor_subsystem(unreal.MetaSoundEditorSubsystem)
        if graph is None:
            return skill_error(
                "Cannot access MetaSound editor subsystem",
                "Ensure the MetaSound plugin is enabled",
            )

        # Map type string to Unreal enum
        type_map = {
            "Float": unreal.MetaSoundParameterType.Float,
            "Bool": unreal.MetaSoundParameterType.Boolean,
            "Int": unreal.MetaSoundParameterType.Int32,
            "String": unreal.MetaSoundParameterType.String,
            "WaveTable": unreal.MetaSoundParameterType.WaveTable,
            "Object": unreal.MetaSoundParameterType.Object,
        }
        unreal_type = type_map[value_type]

        # Add the input
        graph.add_input(
            asset,
            input_name,
            unreal_type,
            default_value if default_value is not None else unreal_type.default_value(),
        )

        return skill_success(
            f"Added input '{input_name}' ({value_type}) to {asset_path}",
            prompt=f"Input '{input_name}' added. Set its default with "
                   "set_metasound_parameter_default if needed.",
            asset_path=asset_path,
            input_name=input_name,
            value_type=value_type,
            default_value=default_value,
        )

    except Exception as exc:
        return skill_error(
            f"Failed to add input '{input_name}' to {asset_path}",
            str(exc),
            possible_solutions=[
                "Verify the asset exists and is a MetaSound Source",
                "Check that the input name is unique in this graph",
                "Ensure the value_type is one of: Float, Bool, Int, String, WaveTable, Object",
            ],
        )
