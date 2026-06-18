"""Contract tests for the cross-DCC asset import shape (PIP-1825).

These tests freeze the :class:`dcc_mcp_core.AssetDescriptor` contract that
downstream DCC repos (``dcc-mcp-blender``, ``dcc-mcp-maya``,
``dcc-mcp-unreal``, ``dcc-mcp-houdini``) build their asset import skills
against.  They are pure-Python and require no DCC binary — the import
*implementations* are tested in the respective downstream repositories.

Run:  pytest tests/test_asset_import_contract.py -v
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
import pathlib

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core.asset_import import AssetAttribution
from dcc_mcp_core.asset_import import AssetDescriptor
from dcc_mcp_core.asset_import import AssetFileVariant
from dcc_mcp_core.asset_import import AssetFormat
from dcc_mcp_core.asset_import import AssetImportValidationError
from dcc_mcp_core.asset_import import AxisHint
from dcc_mcp_core.asset_import import ImportToSceneRequest
from dcc_mcp_core.asset_import import ImportToSceneResult
from dcc_mcp_core.asset_import import ImportWarning
from dcc_mcp_core.asset_import import ImportWarningCode
from dcc_mcp_core.asset_import import MaterialMode
from dcc_mcp_core.asset_import import PlacementHint
from dcc_mcp_core.asset_import import UnitHint

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

FIXTURE_DIR = pathlib.Path(__file__).parent / "fixtures" / "asset_import"


def _load_fixture(name: str) -> dict:
    """Load a JSON fixture from tests/fixtures/asset_import/."""
    path = FIXTURE_DIR / name
    with path.open(encoding="utf-8") as fh:
        return json.load(fh)


# ---------------------------------------------------------------------------
# AssetFormat / AxisHint / UnitHint / MaterialMode / ImportWarningCode
# ---------------------------------------------------------------------------


class TestEnums:
    """String-constant enums are stable and importable."""

    def test_asset_format_values(self) -> None:
        assert AssetFormat.OBJ == "obj"
        assert AssetFormat.FBX == "fbx"
        assert AssetFormat.GLTF == "gltf"
        assert AssetFormat.GLB == "glb"
        assert AssetFormat.USD == "usd"
        assert AssetFormat.USDZ == "usdz"
        assert AssetFormat.ABC == "abc"
        assert AssetFormat.BLEND == "blend"
        assert AssetFormat.UNKNOWN == "unknown"

    def test_axis_hint_values(self) -> None:
        assert AxisHint.Y == "y"
        assert AxisHint.Z == "z"

    def test_unit_hint_values(self) -> None:
        assert UnitHint.METER == "meter"
        assert UnitHint.CENTIMETER == "centimeter"
        assert UnitHint.INCH == "inch"
        assert UnitHint.UNITLESS == "unitless"

    def test_material_mode_values(self) -> None:
        assert MaterialMode.AS_AUTHORED == "as_authored"
        assert MaterialMode.DEFAULT_GRAY == "default_gray"
        assert MaterialMode.SKIP == "skip"

    def test_import_warning_code_values(self) -> None:
        assert ImportWarningCode.MISSING_TEXTURES == "missing_textures"
        assert ImportWarningCode.MISSING_PLUGIN == "missing_plugin"
        assert ImportWarningCode.SCALE_CLAMPED == "scale_clamped"
        assert ImportWarningCode.MATERIAL_FALLBACK == "material_fallback"
        assert ImportWarningCode.UNKNOWN == "unknown"


# ---------------------------------------------------------------------------
# ImportWarning
# ---------------------------------------------------------------------------


class TestImportWarning:
    """ImportWarning round-trip and validation."""

    def test_roundtrip_dict(self) -> None:
        original = ImportWarning(
            code=ImportWarningCode.MISSING_TEXTURES,
            message="3 textures not found",
            detail="['brick.png', 'wood.png', 'metal.png']",
        )
        restored = ImportWarning.from_dict(original.to_dict())
        assert restored == original
        assert restored.code == ImportWarningCode.MISSING_TEXTURES

    def test_roundtrip_no_detail(self) -> None:
        original = ImportWarning(
            code=ImportWarningCode.MATERIAL_FALLBACK,
            message="Material 'glass' fell back to default",
        )
        restored = ImportWarning.from_dict(original.to_dict())
        assert restored == original
        assert restored.detail is None

    def test_json_safe(self) -> None:
        warning = ImportWarning(
            code=ImportWarningCode.MISSING_PLUGIN,
            message="Plugin not found: alembic",
        )
        payload = warning.to_dict()
        decoded = json.loads(json.dumps(payload))
        assert decoded["code"] == "missing_plugin"
        assert "detail" not in decoded


# ---------------------------------------------------------------------------
# AssetAttribution
# ---------------------------------------------------------------------------


class TestAssetAttribution:
    """AssetAttribution round-trip and validation."""

    def test_roundtrip_full(self) -> None:
        original = AssetAttribution(
            source_url="https://example.com/asset",
            license_spdx="CC-BY-4.0",
            license_text="Creative Commons Attribution 4.0",
            author="Example Author",
            title="Example Asset",
            attribution_text="Asset by Example Author, CC-BY-4.0",
        )
        restored = AssetAttribution.from_dict(original.to_dict())
        assert restored == original

    def test_roundtrip_minimal(self) -> None:
        original = AssetAttribution(source_url="https://example.com/asset")
        restored = AssetAttribution.from_dict(original.to_dict())
        assert restored == original
        assert restored.license_spdx is None
        assert restored.author is None

    def test_json_safe(self) -> None:
        attr = AssetAttribution(
            source_url="https://example.com/asset",
            license_spdx="MIT",
        )
        payload = attr.to_dict()
        decoded = json.loads(json.dumps(payload))
        assert decoded["source_url"] == "https://example.com/asset"
        assert decoded["license_spdx"] == "MIT"
        assert "license_text" not in decoded


# ---------------------------------------------------------------------------
# AssetFileVariant
# ---------------------------------------------------------------------------


class TestAssetFileVariant:
    """AssetFileVariant round-trip and validation."""

    def test_roundtrip_full(self) -> None:
        original = AssetFileVariant(
            local_path="/data/desk.fbx",
            format=AssetFormat.FBX,
            preferred=True,
            mime="model/fbx",
            file_ref={"uri": "file:///data/desk.fbx"},
        )
        restored = AssetFileVariant.from_dict(original.to_dict())
        assert restored == original
        assert restored.preferred is True

    def test_roundtrip_minimal(self) -> None:
        original = AssetFileVariant(local_path="/data/desk.obj")
        restored = AssetFileVariant.from_dict(original.to_dict())
        assert restored == original
        assert restored.format == AssetFormat.UNKNOWN
        assert restored.preferred is False

    def test_json_safe(self) -> None:
        variant = AssetFileVariant(
            local_path="/data/robot.glb",
            format=AssetFormat.GLB,
            preferred=True,
            mime="model/gltf-binary",
        )
        payload = variant.to_dict()
        decoded = json.loads(json.dumps(payload))
        assert decoded["local_path"] == "/data/robot.glb"
        assert decoded["format"] == "glb"
        assert decoded["preferred"] is True


# ---------------------------------------------------------------------------
# PlacementHint
# ---------------------------------------------------------------------------


class TestPlacementHint:
    """PlacementHint round-trip."""

    def test_roundtrip_full(self) -> None:
        original = PlacementHint(
            translate=[0.0, 0.0, 100.0],
            rotate=[0.0, 45.0, 0.0],
            scale=[1.0, 1.0, 1.0],
            parent_name="root",
        )
        restored = PlacementHint.from_dict(original.to_dict())
        assert restored == original

    def test_roundtrip_empty(self) -> None:
        original = PlacementHint()
        restored = PlacementHint.from_dict(original.to_dict())
        assert restored == original
        assert restored.translate is None

    def test_json_safe(self) -> None:
        hint = PlacementHint(translate=[1.0, 2.0, 3.0])
        payload = hint.to_dict()
        decoded = json.loads(json.dumps(payload))
        assert decoded["translate"] == [1.0, 2.0, 3.0]
        assert "rotate" not in decoded


# ---------------------------------------------------------------------------
# AssetDescriptor
# ---------------------------------------------------------------------------


class TestAssetDescriptor:
    """AssetDescriptor is the primary contract object."""

    def test_roundtrip_dict(self) -> None:
        original = AssetDescriptor(
            asset_id="props/table",
            variants=[
                AssetFileVariant(
                    local_path="/data/table.fbx",
                    format=AssetFormat.FBX,
                    preferred=True,
                ),
            ],
            attribution=AssetAttribution(
                source_url="https://example.com/table",
                license_spdx="CC-BY-4.0",
            ),
            unit_hint=UnitHint.CENTIMETER,
            meters_per_unit=0.01,
            up_axis=AxisHint.Y,
            tags=["furniture"],
            extra={"notes": "test"},
        )
        restored = AssetDescriptor.from_dict(original.to_dict())
        assert restored == original

    def test_roundtrip_minimal(self) -> None:
        original = AssetDescriptor(
            asset_id="props/cube",
            variants=[AssetFileVariant(local_path="/data/cube.obj")],
        )
        restored = AssetDescriptor.from_dict(original.to_dict())
        assert restored == original

    def test_stub_payload_is_json_safe(self) -> None:
        desc = AssetDescriptor(
            asset_id="test/stub",
            variants=[AssetFileVariant(local_path="/data/stub.obj")],
        )
        payload = desc.to_dict()
        decoded = json.loads(json.dumps(payload))
        assert decoded["asset_id"] == "test/stub"
        assert len(decoded["variants"]) == 1
        assert decoded["unit_hint"] == "unitless"
        assert decoded["meters_per_unit"] == 1.0
        assert decoded["up_axis"] == "y"

    def test_is_top_level_exported(self) -> None:
        """All asset import types are part of the documented public API."""
        for name in [
            "AssetDescriptor",
            "AssetFileVariant",
            "AssetFormat",
            "AssetAttribution",
            "ImportWarning",
            "ImportWarningCode",
            "PlacementHint",
            "MaterialMode",
            "UnitHint",
            "AxisHint",
            "ImportToSceneRequest",
            "ImportToSceneResult",
            "AssetImportValidationError",
        ]:
            assert hasattr(dcc_mcp_core, name), f"Missing top-level export: {name}"
            assert name in dcc_mcp_core.__all__, f"Missing from __all__: {name}"


# ---------------------------------------------------------------------------
# AssetDescriptor.validate()
# ---------------------------------------------------------------------------


class TestAssetDescriptorValidation:
    """Hard validation invariants from the acceptance criteria."""

    def test_rejects_empty_variants(self) -> None:
        desc = AssetDescriptor(asset_id="test/empty", variants=[])
        with pytest.raises(AssetImportValidationError, match="variants must not be empty"):
            desc.validate()

    def test_rejects_missing_local_path(self) -> None:
        desc = AssetDescriptor(
            asset_id="test/no-path",
            variants=[AssetFileVariant(local_path="")],
        )
        with pytest.raises(AssetImportValidationError, match="local_path must not be empty"):
            desc.validate()

    def test_rejects_empty_source_url(self) -> None:
        desc = AssetDescriptor(
            asset_id="test/empty-url",
            variants=[AssetFileVariant(local_path="/data/test.obj")],
            attribution=AssetAttribution(source_url=""),
        )
        with pytest.raises(AssetImportValidationError, match="source_url must not be empty"):
            desc.validate()

    def test_rejects_both_license_fields_empty(self) -> None:
        desc = AssetDescriptor(
            asset_id="test/no-license",
            variants=[AssetFileVariant(local_path="/data/test.obj")],
            attribution=AssetAttribution(source_url="https://example.com/test"),
        )
        with pytest.raises(AssetImportValidationError, match="must have at least one of"):
            desc.validate()

    def test_accepts_valid_descriptor(self) -> None:
        desc = AssetDescriptor(
            asset_id="test/valid",
            variants=[AssetFileVariant(local_path="/data/test.obj")],
        )
        desc.validate()  # should not raise

    def test_accepts_valid_attribution(self) -> None:
        desc = AssetDescriptor(
            asset_id="test/valid-attrib",
            variants=[AssetFileVariant(local_path="/data/test.obj")],
            attribution=AssetAttribution(
                source_url="https://example.com/test",
                license_spdx="MIT",
            ),
        )
        desc.validate()  # should not raise

    def test_accepts_attribution_with_license_text_only(self) -> None:
        desc = AssetDescriptor(
            asset_id="test/license-text",
            variants=[AssetFileVariant(local_path="/data/test.obj")],
            attribution=AssetAttribution(
                source_url="https://example.com/test",
                license_text="MIT License text here",
            ),
        )
        desc.validate()  # should not raise


# ---------------------------------------------------------------------------
# ImportToSceneRequest / ImportToSceneResult
# ---------------------------------------------------------------------------


class TestImportToSceneRequest:
    """ImportToSceneRequest round-trip."""

    def test_roundtrip_full(self) -> None:
        original = ImportToSceneRequest(
            descriptor=AssetDescriptor(
                asset_id="test/desk",
                variants=[AssetFileVariant(local_path="/data/desk.fbx")],
            ),
            material_mode=MaterialMode.AS_AUTHORED,
            placement=PlacementHint(translate=[0.0, 0.0, 100.0]),
            target_collection="Furniture",
            skip_existing=True,
            extra={"layer": "01"},
        )
        restored = ImportToSceneRequest.from_dict(original.to_dict())
        assert restored == original
        assert restored.skip_existing is True

    def test_roundtrip_minimal(self) -> None:
        original = ImportToSceneRequest(
            descriptor=AssetDescriptor(
                asset_id="test/minimal",
                variants=[AssetFileVariant(local_path="/data/minimal.obj")],
            ),
        )
        restored = ImportToSceneRequest.from_dict(original.to_dict())
        assert restored == original

    def test_json_safe(self) -> None:
        req = ImportToSceneRequest(
            descriptor=AssetDescriptor(
                asset_id="test/json-safe",
                variants=[AssetFileVariant(local_path="/data/test.obj")],
            ),
            material_mode=MaterialMode.SKIP,
        )
        payload = req.to_dict()
        decoded = json.loads(json.dumps(payload))
        assert decoded["material_mode"] == "skip"
        assert "placement" not in decoded


class TestImportToSceneResult:
    """ImportToSceneResult round-trip."""

    def test_roundtrip_success(self) -> None:
        original = ImportToSceneResult(
            success=True,
            imported_nodes=["desk_mesh", "desk_chair"],
            warnings=[
                ImportWarning(
                    code=ImportWarningCode.MISSING_TEXTURES,
                    message="1 texture not found",
                    detail="['wood.png']",
                ),
            ],
            extra={"elapsed_ms": 1234},
        )
        restored = ImportToSceneResult.from_dict(original.to_dict())
        assert restored == original
        assert len(restored.imported_nodes) == 2
        assert len(restored.warnings) == 1

    def test_roundtrip_failure(self) -> None:
        original = ImportToSceneResult(
            success=False,
            error_message="File not found: /data/missing.fbx",
        )
        restored = ImportToSceneResult.from_dict(original.to_dict())
        assert restored == original
        assert restored.success is False

    def test_json_safe(self) -> None:
        result = ImportToSceneResult(
            success=True,
            imported_nodes=["node1"],
            warnings=[],
        )
        payload = result.to_dict()
        decoded = json.loads(json.dumps(payload))
        assert decoded["success"] is True
        assert "error_message" not in decoded


# ---------------------------------------------------------------------------
# Fixture-based tests (acceptance criteria 4)
# ---------------------------------------------------------------------------


class TestDescriptorFixtures:
    """All format fixtures round-trip through AssetDescriptor."""

    def test_fixture_obj(self) -> None:
        data = _load_fixture("descriptor_obj.json")
        desc = AssetDescriptor.from_dict(data)
        assert desc.asset_id == "props/table-round"
        assert desc.variants[0].format == AssetFormat.OBJ
        assert desc.unit_hint == UnitHint.CENTIMETER
        assert desc.up_axis == AxisHint.Y
        desc.validate()
        # Round-trip
        restored = AssetDescriptor.from_dict(desc.to_dict())
        assert restored == desc

    def test_fixture_fbx(self) -> None:
        data = _load_fixture("descriptor_fbx.json")
        desc = AssetDescriptor.from_dict(data)
        assert desc.asset_id == "arch/city-bank/desk"
        assert desc.variants[0].format == AssetFormat.FBX
        assert desc.source_bbox is not None
        assert desc.source_bbox["min"] == [-60.0, 0.0, -40.0]
        assert desc.scale_hint == 1.0
        desc.validate()
        restored = AssetDescriptor.from_dict(desc.to_dict())
        assert restored == desc

    def test_fixture_gltf(self) -> None:
        data = _load_fixture("descriptor_gltf.json")
        desc = AssetDescriptor.from_dict(data)
        assert desc.asset_id == "characters/robot-v2"
        assert len(desc.variants) == 2
        assert desc.variants[0].format == AssetFormat.GLTF
        assert desc.variants[1].format == AssetFormat.GLB
        assert desc.preview == "/data/assets/chars/robot_thumb.png"
        assert desc.unit_hint == UnitHint.METER
        desc.validate()
        restored = AssetDescriptor.from_dict(desc.to_dict())
        assert restored == desc

    def test_fixture_usd(self) -> None:
        data = _load_fixture("descriptor_usd.json")
        desc = AssetDescriptor.from_dict(data)
        assert desc.asset_id == "sets/sci-fi-corridor"
        assert desc.variants[0].format == AssetFormat.USD
        assert desc.variants[1].format == AssetFormat.USDZ
        assert desc.up_axis == AxisHint.Z
        assert desc.scale_hint == 0.01
        desc.validate()
        restored = AssetDescriptor.from_dict(desc.to_dict())
        assert restored == desc


# ---------------------------------------------------------------------------
# Backward compatibility
# ---------------------------------------------------------------------------


class TestBackwardCompatibility:
    """No changes to SceneStats semantics."""

    def test_scene_stats_still_works(self) -> None:
        from dcc_mcp_core import SceneStats

        stats = SceneStats(object_count=1, vertex_count=100, has_mesh=True)
        payload = stats.to_dict()
        restored = SceneStats.from_dict(payload)
        assert restored == stats

    def test_scene_stats_still_exported(self) -> None:
        assert hasattr(dcc_mcp_core, "SceneStats")
        assert "SceneStats" in dcc_mcp_core.__all__
