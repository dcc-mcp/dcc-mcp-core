"""Cross-DCC pipeline integration tests for dcc-mcp-core.

Tests that verify the skill system works consistently across all supported DCCs.
These tests run without any external DCC binary — they only exercise
dcc-mcp-core's Python/Rust bindings with DCC-themed data.

Run:  pytest tests/test_integration_dcc_cross.py -v
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from pathlib import Path

# Import local modules
import dcc_mcp_core

# All supported DCCs for cross-DCC pipeline tests (module-level constant to avoid RUF012)
_ALL_DCC_LIST = ["blender", "freecad", "godot", "openscad", "maya", "houdini"]


class TestCrossDCCPipeline:
    """Tests that verify the skill system works consistently across all DCCs.

    These tests run without any external DCC binary — they only exercise
    dcc-mcp-core's Python/Rust bindings with DCC-themed data.
    """

    def test_multi_dcc_action_registry(self) -> None:
        """Register actions for multiple DCCs and verify isolation."""
        reg = dcc_mcp_core.ToolRegistry()
        for dcc in _ALL_DCC_LIST:
            for action in ["create", "export", "validate"]:
                reg.register(name=f"{dcc}_{action}", dcc=dcc, description=f"{dcc}: {action}")
        assert len(reg) == len(_ALL_DCC_LIST) * 3
        for dcc in _ALL_DCC_LIST:
            actions = reg.list_actions(dcc_name=dcc)
            assert len(actions) == 3
            names = {a["name"] for a in actions}
            assert f"{dcc}_create" in names
            assert f"{dcc}_export" in names

    def test_multi_dcc_skill_scanning(self, tmp_path: Path) -> None:
        """Create skill directories for each DCC and verify scanning."""
        from conftest import create_skill_dir

        for dcc in _ALL_DCC_LIST:
            create_skill_dir(str(tmp_path), f"{dcc}-tools", dcc=dcc)
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[str(tmp_path)])
        names = {Path(d).name for d in dirs}
        for dcc in _ALL_DCC_LIST:
            assert f"{dcc}-tools" in names

    def test_multi_dcc_event_routing(self) -> None:
        """Verify EventBus correctly routes events per DCC."""
        bus = dcc_mcp_core.EventBus()
        log: dict[str, list] = {dcc: [] for dcc in _ALL_DCC_LIST}
        for dcc in _ALL_DCC_LIST:
            bus.subscribe(f"{dcc}.action.completed", lambda d=dcc, **kw: log[d].append(kw))
        for dcc in _ALL_DCC_LIST:
            bus.publish(f"{dcc}.action.completed", action="export", dcc=dcc)
        for dcc in _ALL_DCC_LIST:
            assert len(log[dcc]) == 1
            assert log[dcc][0]["dcc"] == dcc

    def test_multi_dcc_result_models(self) -> None:
        """Generate success/error results for each DCC and verify type consistency."""
        for dcc in _ALL_DCC_LIST:
            success = dcc_mcp_core.success_result(f"Action completed for {dcc}", dcc=dcc)
            error = dcc_mcp_core.error_result(
                f"Action failed for {dcc}",
                "Timeout",
                dcc=dcc,
                possible_solutions=["Retry", "Check DCC connection"],
            )
            assert success.success is True
            assert success.context["dcc"] == dcc
            assert error.success is False
            assert len(error.context["possible_solutions"]) == 2

    def test_dcc_specific_tool_definitions(self) -> None:
        """Create ToolDefinition objects for each DCC and verify serialization."""
        for dcc in _ALL_DCC_LIST:
            td = dcc_mcp_core.ToolDefinition(
                name=f"{dcc}_create_object",
                description=f"Create a 3D object in {dcc}",
                input_schema=json.dumps(
                    {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "dcc": {"type": "string", "const": dcc},
                        },
                        "required": ["name"],
                    }
                ),
            )
            assert td.name == f"{dcc}_create_object"
            assert dcc in td.description

    def test_skill_version_consistency(self, tmp_path: Path) -> None:
        """Verify version fields are consistently populated across DCC skills."""
        from conftest import create_skill_dir

        for i, dcc in enumerate(_ALL_DCC_LIST):
            version = f"1.{i}.0"
            create_skill_dir(
                str(tmp_path),
                f"{dcc}-skill-v{i}",
                frontmatter=f"name: {dcc}-skill\ndcc: {dcc}\nversion: {version}",
            )
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[str(tmp_path)])
        for skill_dir in dirs:
            meta = dcc_mcp_core.parse_skill_md(skill_dir)
            assert meta is not None
            assert meta.version != "", f"Missing version for {meta.name}"
            assert meta.dcc in _ALL_DCC_LIST, f"Unexpected DCC: {meta.dcc}"

    def test_cross_dcc_scene_stats_manifest_contract(self) -> None:
        """Producer/verifier SceneStats round-trip through success_result context.

        Demonstrates the pattern downstream CI uses when one DCC exports an
        asset and another DCC imports it back: the verifier payload is a
        ``SceneStats.to_dict()`` blob nested in a ``success_result`` context,
        and the core ``SceneStats.matches()`` helper adjudicates round-trip
        fidelity without any DCC binary being involved.
        """
        produced = dcc_mcp_core.SceneStats(
            object_count=1,
            vertex_count=482,
            has_mesh=True,
            extra={"producer_dcc": "blender"},
        )

        # Simulate the verifier's final wrap: ToolResult context nests the
        # stats dict alongside a human-readable status message.
        verifier_result = dcc_mcp_core.success_result(
            "Imported and inspected asset",
            verifier_dcc="godot",
            stats=produced.to_dict(),
        )
        assert verifier_result.success is True
        nested = verifier_result.context["stats"]
        observed = dcc_mcp_core.SceneStats.from_dict(nested)

        # Observed stats survive the producer → JSON → verifier round-trip.
        assert observed == produced
        assert produced.matches(observed, vertex_tolerance=0.05)
        assert observed.extra == {"producer_dcc": "blender"}
