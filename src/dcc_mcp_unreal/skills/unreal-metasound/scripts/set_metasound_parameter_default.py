"""Set the default value of a MetaSound input parameter.

Validates that the input exists and the provided value is compatible
with the parameter's declared type.
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
def set_metasound_parameter_default(
    asset_path: str,
    input_name: str,
    value,
    **kwargs,
) -> dict:
    """Set the default value of a MetaSound input parameter.

    Args:
        asset_path: Path to the MetaSound Source asset under /Game/.
        input_name: Name of the input parameter to update.
        value: New default value (type must match parameter's declared type).

    Returns:
        Success/error dict with input_name and new value.
    """
    path_error = _validate_asset_path(asset_path)
    if path_error:
        return skill_error("Invalid asset path", path_error)

    if not input_name or not input_name.strip():
        return skill_error("Invalid input name", "input_name must be non-empty")

    if value is None:
        return skill_error(
            "Invalid default value",
            "value must not be None — provide a valid default for the parameter type",
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

        # Set the default value on the input
        graph.set_input_default(asset, input_name, value)

        return skill_success(
            f"Set default value for '{input_name}' in {asset_path}",
            prompt=f"Default for '{input_name}' updated. Build the graph "
                   "with build_metasound to apply.",
            asset_path=asset_path,
            input_name=input_name,
            value=value,
        )

    except Exception as exc:
        return skill_error(
            f"Failed to set default for '{input_name}' in {asset_path}",
            str(exc),
            possible_solutions=[
                "Verify the input name exists in this MetaSound graph",
                "Check that the value type matches the parameter's declared type",
                "For WaveTable/Object types, ensure the referenced asset exists",
            ],
        )
