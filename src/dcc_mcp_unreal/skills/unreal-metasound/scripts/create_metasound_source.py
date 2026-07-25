"""Create a new MetaSound Source asset under /Game/.

Uses unreal.MetaSound API to create a MetaSound Source asset factory,
configure it with the given authoring class, and register it in the
Content Browser at the specified /Game/ path.

Asset path is validated before creation: rejects paths outside /Game/
and rejects non-relative absolute filesystem paths.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_success, skill_error

# Allowed asset types for MetaSound Source creation
_ALLOWED_AUTHORING_CLASSES = frozenset({
    "MetasoundSource",
})

# Minimum Unreal Engine version for MetaSound stable API
_MIN_UE_VERSION = (5, 4)


def _validate_asset_path(asset_path: str) -> str | None:
    """Validate that asset_path is under /Game/ and not an absolute path.

    Returns an error message string on failure, or None on success.
    """
    if not asset_path:
        return "asset_path must not be empty"

    # Reject absolute filesystem paths
    if asset_path.startswith("/") and not asset_path.startswith("/Game/"):
        return f"asset_path must start with '/Game/', got: {asset_path!r}"

    # Reject Windows-style absolute paths
    if ":" in asset_path or asset_path.startswith("\\"):
        return f"asset_path looks like a filesystem path, not a /Game/ asset path: {asset_path!r}"

    # Reject paths with .. traversal
    if ".." in asset_path:
        return f"asset_path must not contain '..', got: {asset_path!r}"

    return None


def _check_ue_version() -> dict | None:
    """Return compatible=False result if UE version is below 5.4, else None."""
    # Lazy import: requires Unreal's embedded Python.
    import unreal  # noqa: F811

    try:
        major = unreal.SystemLibrary.get_engine_version().split(".")[0]
        minor = unreal.SystemLibrary.get_engine_version().split(".")[1]
        engine_version = (int(major), int(minor))
    except Exception:
        # Fallback: try the newer API
        try:
            ver_str = str(unreal.SystemLibrary.get_engine_version())
            parts = ver_str.split(".")
            engine_version = (int(parts[0]), int(parts[1]) if len(parts) > 1 else 0)
        except Exception:
            return None  # can't determine — proceed optimistically

    if engine_version < _MIN_UE_VERSION:
        return {
            "success": False,
            "message": f"Unreal Engine {engine_version[0]}.{engine_version[1]} "
                       f"is below minimum {_MIN_UE_VERSION[0]}.{_MIN_UE_VERSION[1]} for MetaSound",
            "error": "Unreal Engine version too low",
            "context": {
                "ue_version": f"{engine_version[0]}.{engine_version[1]}",
                "min_required": f"{_MIN_UE_VERSION[0]}.{_MIN_UE_VERSION[1]}",
                "compatible": False,
            },
        }

    return None


@skill_entry
def create_metasound_source(
    asset_path: str,
    authoring_class: str = "MetasoundSource",
    **kwargs,
) -> dict:
    """Create a MetaSound Source asset at the given /Game/ path.

    Args:
        asset_path: Asset path under /Game/, e.g. '/Game/Audio/MySound'.
        authoring_class: MetaSound authoring class (default: MetasoundSource).

    Returns:
        Success/error dict with asset_path, authoring_class, and graph handle.
    """
    # Validate asset path
    path_error = _validate_asset_path(asset_path)
    if path_error:
        return skill_error("Invalid asset path", path_error)

    # Validate authoring class
    if authoring_class not in _ALLOWED_AUTHORING_CLASSES:
        return skill_error(
            f"Unsupported authoring class: {authoring_class!r}",
            f"Allowed values: {', '.join(sorted(_ALLOWED_AUTHORING_CLASSES))}",
        )

    # Lazy import: requires Unreal's embedded Python.
    import unreal  # noqa: F811

    # Check UE version compatibility
    version_check = _check_ue_version()
    if version_check is not None:
        return version_check

    try:
        # Create the asset factory
        asset_tools = unreal.AssetToolsHelpers.get_asset_tools()
        factory = unreal.MetaSoundSourceFactoryNew()

        # Create the asset
        asset = asset_tools.create_asset(
            asset_name=asset_path.rsplit("/", 1)[-1],
            package_path=asset_path.rsplit("/", 1)[0] if "/" in asset_path else "/Game",
            asset_class=unreal.MetaSoundSource,
            factory=factory,
        )

        if asset is None:
            return skill_error(
                f"Failed to create MetaSound Source at {asset_path}",
                "Asset creation returned None — asset may already exist or path is invalid",
            )

        ue_version = unreal.SystemLibrary.get_engine_version()

        return skill_success(
            f"Created MetaSound Source at {asset_path}",
            prompt="MetaSound Source created. Add inputs with add_metasound_input "
                  "or nodes with add_metasound_node.",
            asset_path=asset_path,
            authoring_class=authoring_class,
            meta={
                "ue_version": ue_version,
            },
        )

    except Exception as exc:
        return skill_error(
            f"Failed to create MetaSound Source at {asset_path}",
            str(exc),
            possible_solutions=[
                "Ensure the path is under /Game/ and does not already exist",
                "Check that the MetaSound plugin is enabled in your project",
                "Verify Unreal Engine version is 5.4 or later",
            ],
        )
