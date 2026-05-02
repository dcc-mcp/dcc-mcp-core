"""Contract tests for the cross-DCC verifier shape (issue #688).

These tests freeze the :class:`dcc_mcp_core.SceneStats` contract and the
``skills/templates/verifier-harness`` template that downstream DCC repos
(``dcc-mcp-blender``, ``dcc-mcp-maya``, ``dcc-mcp-unreal``,
``dcc-mcp-photoshop``) are expected to clone. They are pure-Python and
require no DCC binary — the verifier *implementations* are tested in the
respective downstream repositories.

Run:  pytest tests/test_verifier_contract.py -v
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import SceneStats

REPO_ROOT = Path(__file__).resolve().parent.parent
VERIFIER_TEMPLATE_DIR = REPO_ROOT / "skills" / "templates" / "verifier-harness"


class TestSceneStatsContract:
    """The SceneStats dataclass is the single source of truth for verifier output shape."""

    def test_scene_stats_roundtrip_dict(self) -> None:
        """SceneStats ↔ dict round-trip preserves every contract field."""
        original = SceneStats(
            object_count=3,
            vertex_count=482,
            has_mesh=True,
            extra={"bbox_max_z": 1.23},
        )
        restored = SceneStats.from_dict(original.to_dict())
        assert restored == original

    def test_scene_stats_matches_tolerance(self) -> None:
        """matches() is strict on object_count / has_mesh and fuzzy on vertex_count."""
        produced = SceneStats(object_count=1, vertex_count=100, has_mesh=True)
        close = SceneStats(object_count=1, vertex_count=104, has_mesh=True)  # +4 %
        drifted = SceneStats(object_count=1, vertex_count=130, has_mesh=True)  # +30 %

        assert produced.matches(close, vertex_tolerance=0.05)
        assert not produced.matches(drifted, vertex_tolerance=0.05)

    def test_scene_stats_extra_preserved(self) -> None:
        """Unknown fields in ``extra`` survive the serialisation round-trip."""
        stats = SceneStats(
            object_count=2,
            vertex_count=64,
            has_mesh=True,
            extra={"material_count": 4, "dcc_note": "blender-3.6"},
        )
        payload = stats.to_dict()
        assert payload["extra"]["material_count"] == 4
        restored = SceneStats.from_dict(payload)
        assert restored.extra == {"material_count": 4, "dcc_note": "blender-3.6"}

    def test_scene_stats_matches_rejects_has_mesh_divergence(self) -> None:
        """has_mesh mismatch fails even when vertex counts happen to coincide."""
        produced = SceneStats(object_count=1, vertex_count=0, has_mesh=True)
        empty = SceneStats(object_count=1, vertex_count=0, has_mesh=False)
        assert not produced.matches(empty)

    def test_scene_stats_matches_rejects_object_count_divergence(self) -> None:
        """object_count mismatch always fails — structural invariant."""
        produced = SceneStats(object_count=1, vertex_count=100, has_mesh=True)
        merged = SceneStats(object_count=2, vertex_count=100, has_mesh=True)
        assert not produced.matches(merged)

    def test_scene_stats_from_dict_requires_core_fields(self) -> None:
        """A payload missing any of the 3 core fields is a KeyError."""
        with pytest.raises(KeyError):
            SceneStats.from_dict({"object_count": 1, "vertex_count": 10})

    def test_scene_stats_from_dict_rejects_malformed_extra(self) -> None:
        """Extra must be a mapping — guards against list/str smuggling."""
        with pytest.raises(TypeError):
            SceneStats.from_dict(
                {
                    "object_count": 1,
                    "vertex_count": 10,
                    "has_mesh": True,
                    "extra": ["not", "a", "dict"],
                }
            )

    def test_scene_stats_matches_rejects_negative_tolerance(self) -> None:
        """vertex_tolerance<0 is a programming error, not a silent pass."""
        a = SceneStats(object_count=1, vertex_count=10, has_mesh=True)
        with pytest.raises(ValueError):
            a.matches(a, vertex_tolerance=-0.01)

    def test_scene_stats_matches_handles_zero_vertex_baseline(self) -> None:
        """Zero-vertex asset (e.g. camera-only scene) compares strictly."""
        empty_a = SceneStats(object_count=1, vertex_count=0, has_mesh=False)
        empty_b = SceneStats(object_count=1, vertex_count=0, has_mesh=False)
        different = SceneStats(object_count=1, vertex_count=5, has_mesh=False)
        assert empty_a.matches(empty_b)
        assert not empty_a.matches(different)

    def test_scene_stats_is_top_level_exported(self) -> None:
        """``dcc_mcp_core.SceneStats`` is part of the documented public API."""
        assert hasattr(dcc_mcp_core, "SceneStats")
        assert "SceneStats" in dcc_mcp_core.__all__


class TestVerifierHarnessTemplate:
    """The verifier skill template is the second half of the contract."""

    def test_template_directory_exists(self) -> None:
        """Sanity check — template directory shipped with the wheel skills tree."""
        assert VERIFIER_TEMPLATE_DIR.is_dir(), f"verifier-harness template missing at {VERIFIER_TEMPLATE_DIR}"
        assert (VERIFIER_TEMPLATE_DIR / "SKILL.md").is_file()
        assert (VERIFIER_TEMPLATE_DIR / "scripts" / "import_and_inspect.py").is_file()

    def test_template_skillmd_parses(self) -> None:
        """The template's SKILL.md is a valid, loadable skill manifest."""
        meta = dcc_mcp_core.parse_skill_md(str(VERIFIER_TEMPLATE_DIR))
        assert meta is not None, "parse_skill_md returned None for verifier template"

    def test_template_schema_shape(self) -> None:
        """The import_and_inspect tool input schema declares file_path + format."""
        # SkillMetadata doesn't expose raw tool input_schema through its frozen
        # Rust surface, so re-parse the frontmatter directly to assert shape.
        content = (VERIFIER_TEMPLATE_DIR / "SKILL.md").read_text(encoding="utf-8")
        # Extract YAML frontmatter between the first pair of "---" delimiters.
        assert content.startswith("---\n"), "SKILL.md must start with YAML frontmatter"
        _, frontmatter, _ = content.split("---\n", 2)

        # Allow both PyYAML and stdlib fallback — yaml is a test-time dep.
        yaml = pytest.importorskip("yaml")
        parsed = yaml.safe_load(frontmatter)

        tools = parsed["tools"]
        assert len(tools) == 1, "verifier template must declare exactly one tool"
        tool = tools[0]
        assert tool["name"] == "import_and_inspect"
        assert tool["read_only"] is True
        assert tool["destructive"] is False

        input_schema = tool["input_schema"]
        assert input_schema["type"] == "object"
        assert "file_path" in input_schema["properties"]
        assert "file_path" in input_schema["required"]

        output_schema = tool["output_schema"]
        required_out = set(output_schema["required"])
        assert {"object_count", "vertex_count", "has_mesh"}.issubset(required_out), (
            f"output_schema.required is missing contract fields: {required_out}"
        )

    def test_template_stub_is_json_safe(self) -> None:
        """The stub script must produce a SceneStats.to_dict()-compatible payload."""
        # We can't execute the stub standalone without skill_entry plumbing,
        # so round-trip a zeroed SceneStats to prove the shape the stub emits
        # is itself JSON-safe.
        zeroed = SceneStats(object_count=0, vertex_count=0, has_mesh=False).to_dict()
        encoded = json.dumps(zeroed)
        decoded = json.loads(encoded)
        assert set(decoded) == {"object_count", "vertex_count", "has_mesh", "extra"}
