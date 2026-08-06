from __future__ import annotations

import importlib.util
import sys

from conftest import REPO_ROOT
import dcc_mcp_core


def _load_spatial_module():
    path = REPO_ROOT / "python" / "dcc_mcp_core" / "spatial.py"
    spec = importlib.util.spec_from_file_location("dcc_mcp_core_spatial_test", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_spatial_interchange_contract_and_skill(monkeypatch) -> None:
    spatial = _load_spatial_module()
    blender = {"right": "-X", "up": "+Z", "forward": "-Y", "meters_per_unit": 1.0}
    gltf = {"right": "-X", "up": "+Y", "forward": "+Z", "meters_per_unit": 1.0}
    plan = spatial.plan_spatial_conversion(blender, gltf, [1, 2, 3])
    assert plan["axis_matrix"] == [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]]
    assert plan["converted_sample_point"] == [1.0, 3.0, -2.0]
    assert plan["mesh_winding"] == "preserve"

    unity = {"right": "+X", "up": "+Y", "forward": "+Z", "meters_per_unit": 1.0}
    unreal = {"right": "+Y", "up": "+Z", "forward": "+X", "meters_per_unit": 0.01}
    plan = spatial.plan_spatial_conversion(unity, unreal, [1, 2, 3])
    assert plan["position_scale"] == 100.0
    assert plan["converted_sample_point"] == [300.0, 100.0, 200.0]

    mirrored = spatial.plan_spatial_conversion(
        {"right": "+X", "up": "+Y", "forward": "-Z", "meters_per_unit": 1.0},
        unity,
    )
    assert mirrored["orientation_reversing"] is True
    assert mirrored["mesh_winding"] == "reverse"

    try:
        spatial.SpatialConvention("+X", "-X", "+Z", 1.0)
    except ValueError as exc:
        assert "distinct axes" in str(exc)
    else:
        raise AssertionError("duplicate axes must be rejected")

    skill_dir = REPO_ROOT / "skills" / "spatial-interchange"
    assert dcc_mcp_core.validate_skill(str(skill_dir)).is_clean
    monkeypatch.setattr(dcc_mcp_core, "SpatialConvention", spatial.SpatialConvention, raising=False)
    monkeypatch.setattr(dcc_mcp_core, "plan_spatial_conversion", spatial.plan_spatial_conversion, raising=False)
    script = skill_dir / "scripts" / "plan_conversion.py"
    spec = importlib.util.spec_from_file_location("spatial_interchange_plan", script)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    result = module.main(source=unity, target=unreal, sample_point=[1, 2, 3])
    assert result["success"] is True
    assert result["context"]["converted_sample_point"] == [300.0, 100.0, 200.0]
