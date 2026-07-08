"""Cross-DCC asset import contracts.

This module defines the data contracts that an *asset import* skill is
expected to consume and return.  The core library ships only the shape;
the actual DCC-specific import logic (opening the file in Blender, Maya,
Unreal, Houdini, ...) lives in the downstream DCC-specific repositories.

Why it lives here
-----------------

Keeping :class:`AssetDescriptor` and :class:`ImportToSceneRequest` in
``dcc-mcp-core`` gives every downstream adapter a single, versioned
contract to target.  Contract tests in this repo (see
``tests/test_asset_import_contract.py``) freeze the shape; downstream CI
pipelines can then assert their importer accepts and returns the same
fields so round-trip scenarios (file → DCC → scene) stay portable.

Intentional minimalism
----------------------

Python-only v1 with frozen dataclasses (same pattern as :class:`SceneStats`).
No Rust crate — migrate to ``dcc-mcp-asset-import`` crate later if needed.
``BoundingBox`` stays cm/Y-up to match ``_core.BoundingBox`` native units;
adapters convert host-native → cm.

Paths-first: ``local_path`` required, ``file_ref`` optional; no artefact URI
infrastructure in v1.

Example:
-------
::

    from dcc_mcp_core import AssetDescriptor, AssetFormat, AssetFileVariant

    desc = AssetDescriptor(
        asset_id="arch/city-bank/desk",
        variants=[
            AssetFileVariant(local_path="/data/desk.fbx", format=AssetFormat.FBX),
        ],
        up_axis=AxisHint.Y,
        unit_hint=UnitHint.CENTIMETER,
    )
    desc.validate()

"""

from __future__ import annotations

# Import built-in modules
from dataclasses import dataclass
from dataclasses import field
from typing import Any
from typing import Dict
from typing import List
from typing import Mapping
from typing import Optional

__all__ = [
    "AssetAttribution",
    "AssetDescriptor",
    "AssetFileVariant",
    "AssetFormat",
    "AssetImportValidationError",
    "AxisHint",
    "ImportToSceneRequest",
    "ImportToSceneResult",
    "ImportWarning",
    "ImportWarningCode",
    "MaterialMode",
    "PlacementHint",
    "UnitHint",
]


# ---------------------------------------------------------------------------
# Enums / stable string constants
# ---------------------------------------------------------------------------


class AssetFormat:
    """Stable asset format strings.

    These values match the common interchange format identifiers used across
    DCC tools and file-browser filters.
    """

    OBJ = "obj"
    FBX = "fbx"
    GLTF = "gltf"
    GLB = "glb"
    USD = "usd"
    USDZ = "usdz"
    ABC = "abc"
    BLEND = "blend"
    UNKNOWN = "unknown"


class AxisHint:
    """Up-axis hint for the host scene.

    Adapters use this to rotate imported geometry so the asset lands in the
    correct orientation relative to the host's world-up convention.
    """

    Y = "y"
    Z = "z"


class UnitHint:
    """Unit hint for the asset's native scale.

    Adapters use this to scale imported geometry so it matches the host
    scene's working units.  ``meters_per_unit`` on :class:`AssetDescriptor`
    carries the numeric conversion factor.
    """

    METER = "meter"
    CENTIMETER = "centimeter"
    INCH = "inch"
    UNITLESS = "unitless"


class MaterialMode:
    """Material import strategy hint.

    Controls how the importer should handle materials found in the source file.
    """

    AS_AUTHORED = "as_authored"
    DEFAULT_GRAY = "default_gray"
    SKIP = "skip"


class ImportWarningCode:
    """Stable import warning code strings.

    These codes let downstream consumers match warning semantics without
    parsing human-readable messages.
    """

    MISSING_TEXTURES = "missing_textures"
    MISSING_PLUGIN = "missing_plugin"
    SCALE_CLAMPED = "scale_clamped"
    ROTATION_CLAMPED = "rotation_clamped"
    NON_UNIFORM_SCALE_BAKED = "non_uniform_scale_baked"
    DEGENERATE_GEOMETRY_SKIPPED = "degenerate_geometry_skipped"
    HIERARCHY_FLATTENED = "hierarchy_flattened"
    NAMES_COLLIDED = "names_collided"
    MATERIAL_FALLBACK = "material_fallback"
    ANIMATION_STRIPPED = "animation_stripped"
    UNSUPPORTED_FEATURE = "unsupported_feature"
    UNKNOWN = "unknown"


# ---------------------------------------------------------------------------
# Custom exception
# ---------------------------------------------------------------------------


class AssetImportValidationError(ValueError):
    """Raised by :meth:`AssetDescriptor.validate` on hard validation failures.

    Inherits from :class:`ValueError` so callers that already catch ``ValueError``
    for bad input don't need to change their exception handling.
    """


# ---------------------------------------------------------------------------
# Data contracts (frozen dataclasses, same pattern as verifier.py)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ImportWarning:
    """Non-fatal warning encountered during asset import.

    Parameters
    ----------
    code:
        Stable warning code from :class:`ImportWarningCode`.
    message:
        Human-readable description of the warning.
    detail:
        Optional machine-readable detail (e.g. a list of missing texture paths).

    """

    code: str
    message: str
    detail: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this warning to a plain dict."""
        result: Dict[str, Any] = {
            "code": self.code,
            "message": self.message,
        }
        if self.detail is not None:
            result["detail"] = self.detail
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> ImportWarning:
        """Reconstruct an :class:`ImportWarning` from its dict form."""
        return cls(
            code=str(data["code"]),
            message=str(data["message"]),
            detail=data.get("detail"),
        )


@dataclass(frozen=True)
class AssetAttribution:
    """Attribution metadata for an imported asset.

    Parameters
    ----------
    source_url:
        URL where the asset was obtained. Required — must be non-empty
        and must pass :meth:`AssetDescriptor.validate`.
    license_spdx:
        SPDX license identifier (e.g. ``"CC-BY-4.0"``). Optional unless
        ``license_text`` is also empty.
    license_text:
        Full license text. Optional unless ``license_spdx`` is also empty.
    author:
        Original author or creator name.
    title:
        Human-readable title of the asset.
    attribution_text:
        Pre-formatted attribution string for display.

    """

    source_url: str
    license_spdx: Optional[str] = None
    license_text: Optional[str] = None
    author: Optional[str] = None
    title: Optional[str] = None
    attribution_text: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this attribution to a plain dict."""
        result: Dict[str, Any] = {"source_url": self.source_url}
        if self.license_spdx is not None:
            result["license_spdx"] = self.license_spdx
        if self.license_text is not None:
            result["license_text"] = self.license_text
        if self.author is not None:
            result["author"] = self.author
        if self.title is not None:
            result["title"] = self.title
        if self.attribution_text is not None:
            result["attribution_text"] = self.attribution_text
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> AssetAttribution:
        """Reconstruct an :class:`AssetAttribution` from its dict form."""
        return cls(
            source_url=str(data["source_url"]),
            license_spdx=data.get("license_spdx"),
            license_text=data.get("license_text"),
            author=data.get("author"),
            title=data.get("title"),
            attribution_text=data.get("attribution_text"),
        )


@dataclass(frozen=True)
class AssetFileVariant:
    """A single file variant of an asset.

    Parameters
    ----------
    local_path:
        Absolute or relative path to the asset file on disk. Required.
    format:
        File format hint from :class:`AssetFormat`.
    preferred:
        Whether this variant is the preferred one for import. Defaults to False.
    mime:
        MIME type of the file (e.g. ``"model/fbx"``).
    file_ref:
        Optional structured file reference (URI-based, from ``_core.FileRef``).
        Not required in v1; ``local_path`` is the primary identifier.

    """

    local_path: str
    format: str = AssetFormat.UNKNOWN
    preferred: bool = False
    mime: Optional[str] = None
    file_ref: Optional[Dict[str, Any]] = None

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this variant to a plain dict."""
        result: Dict[str, Any] = {
            "local_path": self.local_path,
            "format": self.format,
        }
        if self.preferred:
            result["preferred"] = self.preferred
        if self.mime is not None:
            result["mime"] = self.mime
        if self.file_ref is not None:
            result["file_ref"] = dict(self.file_ref)
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> AssetFileVariant:
        """Reconstruct an :class:`AssetFileVariant` from its dict form."""
        return cls(
            local_path=str(data["local_path"]),
            format=str(data.get("format", AssetFormat.UNKNOWN)),
            preferred=bool(data.get("preferred", False)),
            mime=data.get("mime"),
            file_ref=dict(data["file_ref"]) if data.get("file_ref") is not None else None,
        )


@dataclass(frozen=True)
class PlacementHint:
    """Optional placement hint for an imported asset.

    All vectors are plain float lists to avoid binding to :class:`ObjectTransform`
    from ``_core``.  Adapters convert these hints into host-native transforms.

    Parameters
    ----------
    translate:
        Translation offset [x, y, z] in host units.
    rotate:
        Rotation [rx, ry, rz] in degrees (Euler).
    scale:
        Scale factors [sx, sy, sz].
    parent_name:
        Name of the parent object to attach to in the host scene.

    """

    translate: Optional[List[float]] = None
    rotate: Optional[List[float]] = None
    scale: Optional[List[float]] = None
    parent_name: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this placement hint to a plain dict."""
        result: Dict[str, Any] = {}
        if self.translate is not None:
            result["translate"] = list(self.translate)
        if self.rotate is not None:
            result["rotate"] = list(self.rotate)
        if self.scale is not None:
            result["scale"] = list(self.scale)
        if self.parent_name is not None:
            result["parent_name"] = self.parent_name
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> PlacementHint:
        """Reconstruct a :class:`PlacementHint` from its dict form."""
        translate = data.get("translate")
        rotate = data.get("rotate")
        scale = data.get("scale")
        return cls(
            translate=list(translate) if translate is not None else None,
            rotate=list(rotate) if rotate is not None else None,
            scale=list(scale) if scale is not None else None,
            parent_name=data.get("parent_name"),
        )


@dataclass(frozen=True)
class AssetDescriptor:
    """Complete asset import descriptor.

    This is the primary contract object.  Adapter importers consume this
    descriptor to locate files, apply unit/axis conversions, and carry
    attribution through to the host scene.

    Parameters
    ----------
    asset_id:
        Stable identifier for the asset (e.g. ``"arch/city-bank/desk"``).
    variants:
        One or more file variants. At least one variant with ``local_path``
        must be present.
    attribution:
        Optional attribution metadata.
    preview:
        Optional path to a preview image/thumbnail.
    unit_hint:
        Hint about the asset's native unit system. Defaults to ``unitless``.
    meters_per_unit:
        Conversion factor from asset units to meters. When ``unit_hint`` is
        ``centimeter`` this would be ``0.01``. Defaults to ``1.0``.
    up_axis:
        Up-axis of the source asset. Defaults to ``"y"``.
    scale_hint:
        Suggested scale factor for import (multiplier).
    source_bbox:
        Bounding box from the source tool (cm, Y-up), as a dict matching
        ``_core.BoundingBox`` shape.
    tags:
        Free-form tags for categorisation / search.
    extra:
        Free-form mapping for DCC-specific enrichments.

    """

    asset_id: str
    variants: List[AssetFileVariant] = field(default_factory=list)
    attribution: Optional[AssetAttribution] = None
    preview: Optional[str] = None
    unit_hint: str = UnitHint.UNITLESS
    meters_per_unit: float = 1.0
    up_axis: str = AxisHint.Y
    scale_hint: Optional[float] = None
    source_bbox: Optional[Dict[str, Any]] = None
    tags: List[str] = field(default_factory=list)
    extra: Dict[str, Any] = field(default_factory=dict)

    def validate(self) -> None:
        """Validate this descriptor against the contract invariants.

        Raises
        ------
        AssetImportValidationError
            If any hard invariant is violated.

        Checks:
        - ``variants`` must not be empty.
        - Every variant must have a non-empty ``local_path`` (no ``file_ref``-only
          variants in v1).
        - If ``attribution`` is set, ``source_url`` must be non-empty and at least
          one of ``license_spdx`` / ``license_text`` must be non-empty.

        """
        if not self.variants:
            raise AssetImportValidationError("AssetDescriptor.variants must not be empty")

        for i, variant in enumerate(self.variants):
            if not variant.local_path:
                raise AssetImportValidationError(
                    f"AssetDescriptor.variants[{i}].local_path must not be empty "
                    f"(file_ref-only variants are not supported in v1)"
                )

        if self.attribution is not None:
            if not self.attribution.source_url:
                raise AssetImportValidationError("AssetDescriptor.attribution.source_url must not be empty")
            if not self.attribution.license_spdx and not self.attribution.license_text:
                raise AssetImportValidationError(
                    "AssetDescriptor.attribution must have at least one of license_spdx or license_text"
                )

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this descriptor to a plain dict."""
        result: Dict[str, Any] = {
            "asset_id": self.asset_id,
            "variants": [v.to_dict() for v in self.variants],
            "unit_hint": self.unit_hint,
            "meters_per_unit": self.meters_per_unit,
            "up_axis": self.up_axis,
            "tags": list(self.tags),
            "extra": dict(self.extra),
        }
        if self.attribution is not None:
            result["attribution"] = self.attribution.to_dict()
        if self.preview is not None:
            result["preview"] = self.preview
        if self.scale_hint is not None:
            result["scale_hint"] = self.scale_hint
        if self.source_bbox is not None:
            result["source_bbox"] = dict(self.source_bbox)
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> AssetDescriptor:
        """Reconstruct an :class:`AssetDescriptor` from its dict form.

        Raises
        ------
        KeyError
            If ``asset_id`` is missing.

        """
        variants_raw = data.get("variants", [])
        variants = [AssetFileVariant.from_dict(v) for v in variants_raw]

        attribution_raw = data.get("attribution")
        attribution = AssetAttribution.from_dict(attribution_raw) if attribution_raw is not None else None

        extra_value = data.get("extra", {})
        if not isinstance(extra_value, Mapping):
            raise TypeError(f"AssetDescriptor.extra must be a mapping, got {type(extra_value).__name__}")

        return cls(
            asset_id=str(data["asset_id"]),
            variants=variants,
            attribution=attribution,
            preview=data.get("preview"),
            unit_hint=str(data.get("unit_hint", UnitHint.UNITLESS)),
            meters_per_unit=float(data.get("meters_per_unit", 1.0)),
            up_axis=str(data.get("up_axis", AxisHint.Y)),
            scale_hint=float(data["scale_hint"]) if data.get("scale_hint") is not None else None,
            source_bbox=dict(data["source_bbox"]) if data.get("source_bbox") is not None else None,
            tags=[str(t) for t in data.get("tags", [])],
            extra=dict(extra_value),
        )


@dataclass(frozen=True)
class ImportToSceneRequest:
    """Request to import an asset into a host scene.

    Parameters
    ----------
    descriptor:
        The asset descriptor describing what to import.
    material_mode:
        How to handle materials during import.
    placement:
        Optional placement hint for where to put the asset.
    target_collection:
        Optional name of the collection/layer to place the asset into.
    skip_existing:
        If True, skip the import when the asset_id is already present in
        the scene. Defaults to False.
    extra:
        Free-form mapping for adapter-specific options.

    """

    descriptor: AssetDescriptor
    material_mode: str = MaterialMode.AS_AUTHORED
    placement: Optional[PlacementHint] = None
    target_collection: Optional[str] = None
    skip_existing: bool = False
    extra: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this request to a plain dict."""
        result: Dict[str, Any] = {
            "descriptor": self.descriptor.to_dict(),
            "material_mode": self.material_mode,
            "skip_existing": self.skip_existing,
            "extra": dict(self.extra),
        }
        if self.placement is not None:
            result["placement"] = self.placement.to_dict()
        if self.target_collection is not None:
            result["target_collection"] = self.target_collection
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> ImportToSceneRequest:
        """Reconstruct an :class:`ImportToSceneRequest` from its dict form."""
        placement_raw = data.get("placement")
        placement = PlacementHint.from_dict(placement_raw) if placement_raw is not None else None

        extra_value = data.get("extra", {})
        if not isinstance(extra_value, Mapping):
            raise TypeError(f"ImportToSceneRequest.extra must be a mapping, got {type(extra_value).__name__}")

        return cls(
            descriptor=AssetDescriptor.from_dict(data["descriptor"]),
            material_mode=str(data.get("material_mode", MaterialMode.AS_AUTHORED)),
            placement=placement,
            target_collection=data.get("target_collection"),
            skip_existing=bool(data.get("skip_existing", False)),
            extra=dict(extra_value),
        )


@dataclass(frozen=True)
class ImportToSceneResult:
    """Result of importing an asset into a host scene.

    Parameters
    ----------
    success:
        Whether the import completed successfully.
    imported_nodes:
        List of node names/paths that were created or updated in the scene.
    warnings:
        Non-fatal warnings encountered during import.
    error_message:
        Error description if ``success`` is False.
    extra:
        Free-form mapping for DCC-specific enrichments.

    """

    success: bool
    imported_nodes: List[str] = field(default_factory=list)
    warnings: List[ImportWarning] = field(default_factory=list)
    error_message: Optional[str] = None
    extra: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this result to a plain dict."""
        result: Dict[str, Any] = {
            "success": self.success,
            "imported_nodes": list(self.imported_nodes),
            "warnings": [w.to_dict() for w in self.warnings],
            "extra": dict(self.extra),
        }
        if self.error_message is not None:
            result["error_message"] = self.error_message
        return result

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> ImportToSceneResult:
        """Reconstruct an :class:`ImportToSceneResult` from its dict form."""
        warnings_raw = data.get("warnings", [])
        warnings = [ImportWarning.from_dict(w) for w in warnings_raw]

        extra_value = data.get("extra", {})
        if not isinstance(extra_value, Mapping):
            raise TypeError(f"ImportToSceneResult.extra must be a mapping, got {type(extra_value).__name__}")

        return cls(
            success=bool(data["success"]),
            imported_nodes=[str(n) for n in data.get("imported_nodes", [])],
            warnings=warnings,
            error_message=data.get("error_message"),
            extra=dict(extra_value),
        )
