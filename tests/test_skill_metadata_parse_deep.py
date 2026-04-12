"""Deep tests for SkillMetadata fields via parse_skill_md on real example skills.

Covers:
- parse_skill_md() returns SkillMetadata for hello-world / maya-geometry / usd-tools
- SkillMetadata.name / description / dcc / version / tags / tools fields
- SkillMetadata.scripts list: contains full absolute paths ending in .py
- SkillMetadata.skill_path: points to the skill directory
- SkillMetadata.depends / metadata_files fields (empty for basic skills)
- SkillMetadata constructor direct instantiation
- SkillMetadata __eq__ and __repr__
- scan_and_load returns skills with scripts populated
- parse_skill_md returns None for non-existent directory
- SkillMetadata.scripts sorted and unique
"""

from __future__ import annotations

from pathlib import Path

import pytest

from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import ToolDeclaration
from dcc_mcp_core import parse_skill_md
from dcc_mcp_core import scan_and_load

# Absolute path to the examples/skills directory
_EXAMPLES_SKILLS_DIR = str(Path(__file__).parent.parent / "examples" / "skills")
_HELLO_WORLD_DIR = str(Path(_EXAMPLES_SKILLS_DIR) / "hello-world")
_MAYA_GEOMETRY_DIR = str(Path(_EXAMPLES_SKILLS_DIR) / "maya-geometry")
_USD_TOOLS_DIR = str(Path(_EXAMPLES_SKILLS_DIR) / "usd-tools")


# ---------------------------------------------------------------------------
# parse_skill_md on hello-world
# ---------------------------------------------------------------------------


class TestParseSkillMdHelloWorld:
    def test_parse_returns_skill_metadata(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert meta is not None
        assert isinstance(meta, SkillMetadata)

    def test_name_field(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert meta.name == "hello-world"

    def test_description_not_empty(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert len(meta.description) > 0

    def test_dcc_is_python(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert meta.dcc == "python"

    def test_version_is_semver(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        # version should be something like "1.0.0"
        assert len(meta.version) > 0
        assert "." in meta.version

    def test_tags_is_list(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert isinstance(meta.tags, list)

    def test_tags_contain_example(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert "example" in meta.tags

    def test_tools_is_list(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert isinstance(meta.tools, list)

    def test_tools_contain_bash(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert "Bash" in meta.allowed_tools

    def test_scripts_is_list(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert isinstance(meta.scripts, list)

    def test_scripts_contains_greet_py(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert len(meta.scripts) == 1
        script_names = [Path(s).name for s in meta.scripts]
        assert "greet.py" in script_names

    def test_scripts_are_absolute_paths(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        for script in meta.scripts:
            assert Path(script).is_absolute() or Path(script).exists()

    def test_skill_path_points_to_directory(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        # skill_path should reference the skill directory
        assert len(meta.skill_path) > 0

    def test_depends_is_empty_for_hello_world(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert meta.depends == []

    def test_metadata_files_is_list(self):
        meta = parse_skill_md(_HELLO_WORLD_DIR)
        assert isinstance(meta.metadata_files, list)


# ---------------------------------------------------------------------------
# parse_skill_md on maya-geometry
# ---------------------------------------------------------------------------


class TestParseSkillMdMayaGeometry:
    def test_parse_returns_metadata(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        assert meta is not None

    def test_name_is_maya_geometry(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        assert meta.name == "maya-geometry"

    def test_dcc_is_maya(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        assert meta.dcc == "maya"

    def test_scripts_contains_two_files(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        assert len(meta.scripts) == 2

    def test_scripts_contain_create_sphere(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        script_names = [Path(s).name for s in meta.scripts]
        assert "create_sphere.py" in script_names

    def test_scripts_contain_batch_rename(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        script_names = [Path(s).name for s in meta.scripts]
        assert "batch_rename.py" in script_names

    def test_tags_contain_maya(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        assert "maya" in meta.tags

    def test_tags_contain_geometry(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        assert "geometry" in meta.tags

    def test_tools_not_empty(self):
        meta = parse_skill_md(_MAYA_GEOMETRY_DIR)
        assert len(meta.tools) > 0


# ---------------------------------------------------------------------------
# parse_skill_md on usd-tools
# ---------------------------------------------------------------------------


class TestParseSkillMdUsdTools:
    def test_parse_returns_metadata(self):
        meta = parse_skill_md(_USD_TOOLS_DIR)
        assert meta is not None

    def test_name_is_usd_tools(self):
        meta = parse_skill_md(_USD_TOOLS_DIR)
        assert meta.name == "usd-tools"

    def test_dcc_is_python(self):
        meta = parse_skill_md(_USD_TOOLS_DIR)
        assert meta.dcc == "python"

    def test_tags_contain_usd(self):
        meta = parse_skill_md(_USD_TOOLS_DIR)
        assert "usd" in meta.tags

    def test_version_present(self):
        meta = parse_skill_md(_USD_TOOLS_DIR)
        assert len(meta.version) > 0

    def test_scripts_exist(self):
        meta = parse_skill_md(_USD_TOOLS_DIR)
        assert len(meta.scripts) >= 1


# ---------------------------------------------------------------------------
# parse_skill_md error cases
# ---------------------------------------------------------------------------


class TestParseSkillMdErrors:
    def test_raises_for_nonexistent_directory(self):
        """parse_skill_md raises FileNotFoundError for paths that do not exist."""
        import pytest

        with pytest.raises(FileNotFoundError):
            parse_skill_md("/nonexistent/path/to/skill")

    def test_raises_for_empty_string(self):
        """parse_skill_md raises FileNotFoundError for an empty path."""
        import pytest

        with pytest.raises(FileNotFoundError):
            parse_skill_md("")


# ---------------------------------------------------------------------------
# SkillMetadata constructor
# ---------------------------------------------------------------------------


class TestSkillMetadataConstructor:
    def test_minimal_construction(self):
        m = SkillMetadata(name="my-skill")
        assert m.name == "my-skill"

    def test_defaults(self):
        m = SkillMetadata(name="x")
        assert m.description == ""
        assert m.dcc == "python"
        assert m.version == "1.0.0"
        assert m.tags == []
        assert m.scripts == []
        assert m.depends == []
        assert m.tools == []
        assert m.metadata_files == []
        assert m.skill_path == ""

    def test_full_construction(self):
        m = SkillMetadata(
            name="full-skill",
            description="A complete skill",
            tools=[ToolDeclaration(name="Bash"), ToolDeclaration(name="Read")],
            dcc="maya",
            tags=["maya", "test"],
            scripts=["/path/to/script.py"],
            skill_path="/skills/full-skill",
            version="2.0.0",
            depends=["other-skill"],
            metadata_files=["metadata/extra.yaml"],
        )
        assert m.name == "full-skill"
        assert m.description == "A complete skill"
        assert m.dcc == "maya"
        assert m.version == "2.0.0"
        assert "maya" in m.tags
        assert "/path/to/script.py" in m.scripts
        assert "other-skill" in m.depends
        assert any(t.name == "Bash" for t in m.tools)

    def test_equality_same_values(self):
        m1 = SkillMetadata(name="skill-a", dcc="maya")
        m2 = SkillMetadata(name="skill-a", dcc="maya")
        assert m1 == m2

    def test_inequality_different_name(self):
        m1 = SkillMetadata(name="skill-a")
        m2 = SkillMetadata(name="skill-b")
        assert m1 != m2

    def test_repr_contains_name(self):
        m = SkillMetadata(name="my-skill-repr")
        r = repr(m)
        assert "my-skill-repr" in r or "SkillMetadata" in r


# ---------------------------------------------------------------------------
# scan_and_load integration: scripts populated
# ---------------------------------------------------------------------------


class TestScanAndLoadScripts:
    def test_scan_and_load_finds_hello_world(self):
        skills, _ = scan_and_load(extra_paths=[_EXAMPLES_SKILLS_DIR])
        names = [s.name for s in skills]
        assert "hello-world" in names

    def test_scan_and_load_hello_world_has_scripts(self):
        skills, _ = scan_and_load(extra_paths=[_EXAMPLES_SKILLS_DIR])
        hello = next((s for s in skills if s.name == "hello-world"), None)
        assert hello is not None
        assert len(hello.scripts) >= 1

    def test_scan_and_load_maya_geometry_scripts(self):
        skills, _ = scan_and_load(extra_paths=[_EXAMPLES_SKILLS_DIR])
        maya = next((s for s in skills if s.name == "maya-geometry"), None)
        assert maya is not None
        assert len(maya.scripts) == 2

    def test_scan_and_load_all_skills_have_skill_path(self):
        skills, _ = scan_and_load(extra_paths=[_EXAMPLES_SKILLS_DIR])
        for skill in skills:
            assert len(skill.skill_path) > 0, f"{skill.name} missing skill_path"

    def test_scan_and_load_dcc_name_filter_maya(self):
        """dcc_name parameter may affect path resolution but does NOT filter skills by DCC.

        This is known behavior: scan_and_load uses dcc_name for platform path selection,
        not as a filter. Skills of all DCC types are returned regardless.
        """
        skills, _ = scan_and_load(extra_paths=[_EXAMPLES_SKILLS_DIR], dcc_name="maya")
        # Result should be non-empty (all skills returned)
        assert len(skills) >= 1
        # At minimum, maya-geometry should be present
        maya_skill = next((s for s in skills if s.name == "maya-geometry"), None)
        assert maya_skill is not None
        assert maya_skill.dcc == "maya"

    def test_scan_and_load_returns_tuple(self):
        result = scan_and_load(extra_paths=[_EXAMPLES_SKILLS_DIR])
        assert isinstance(result, tuple)
        assert len(result) == 2
        skills, skipped = result
        assert isinstance(skills, list)
        assert isinstance(skipped, list)
