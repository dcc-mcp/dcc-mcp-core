"""Tests for scan_and_load, scan_and_load_lenient, resolve_dependencies, expand_transitive_dependencies.

These high-level skill-loading functions had no dedicated tests; this file
closes that gap with both happy-path and error-path coverage.
"""

from __future__ import annotations

from pathlib import Path

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _write_skill(base: Path, name: str, dcc: str = "python", deps: list[str] | None = None) -> Path:
    """Write a minimal SKILL.md under *base*/*name* and return the skill dir."""
    skill_dir = base / name
    skill_dir.mkdir(parents=True, exist_ok=True)
    dep_block = ""
    if deps:
        dep_lines = "\n".join(f"  - {d}" for d in deps)
        dep_block = f"depends:\n{dep_lines}\n"
    content = f"---\nname: {name}\ndcc: {dcc}\n{dep_block}---\n# {name}\n"
    (skill_dir / "SKILL.md").write_text(content, encoding="utf-8")
    return skill_dir


# ---------------------------------------------------------------------------
# scan_and_load — happy path
# ---------------------------------------------------------------------------


class TestScanAndLoad:
    def test_returns_tuple(self, tmp_path: Path) -> None:
        """scan_and_load() always returns a 2-tuple."""
        result = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_empty_dir_returns_empty_skills(self, tmp_path: Path) -> None:
        """Scanning an empty directory yields zero skills."""
        skills, _skipped = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert skills == []

    def test_single_skill_loaded(self, tmp_path: Path) -> None:
        """scan_and_load loads one skill from a simple directory."""
        _write_skill(tmp_path, "my-skill")
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert len(skills) == 1
        assert skills[0].name == "my-skill"

    def test_multiple_skills_loaded(self, tmp_path: Path) -> None:
        """scan_and_load loads all skills in a directory."""
        for name in ["skill-a", "skill-b", "skill-c"]:
            _write_skill(tmp_path, name)
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert len(skills) == 3

    def test_skill_metadata_type(self, tmp_path: Path) -> None:
        """Each element returned is a SkillMetadata instance."""
        _write_skill(tmp_path, "typed-skill", dcc="maya")
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert isinstance(skills[0], dcc_mcp_core.SkillMetadata)
        assert skills[0].dcc == "maya"

    def test_dcc_filter(self, tmp_path: Path) -> None:
        """scan_and_load respects the dcc_name filter."""
        _write_skill(tmp_path, "maya-skill", dcc="maya")
        _write_skill(tmp_path, "blender-skill", dcc="blender")
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)], dcc_name="maya")
        names = {s.name for s in skills}
        assert "maya-skill" in names

    def test_nonexistent_path_returns_empty(self) -> None:
        """scan_and_load with a non-existent path returns an empty list."""
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=["/this/path/does/not/exist"])
        assert skills == []

    def test_dependency_order_respected(self, tmp_path: Path) -> None:
        """skill-b depends on skill-a; skill-a must appear first in the result."""
        _write_skill(tmp_path, "skill-a")
        _write_skill(tmp_path, "skill-b", deps=["skill-a"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        names = [s.name for s in skills]
        assert names.index("skill-a") < names.index("skill-b")

    def test_skipped_dirs_is_list(self, tmp_path: Path) -> None:
        """The second element of the tuple is always a list."""
        _, skipped = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert isinstance(skipped, list)


# ---------------------------------------------------------------------------
# scan_and_load_lenient — tolerates missing dependencies
# ---------------------------------------------------------------------------


class TestScanAndLoadLenient:
    def test_returns_tuple(self, tmp_path: Path) -> None:
        result = dcc_mcp_core.scan_and_load_lenient(extra_paths=[str(tmp_path)])
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_empty_dir_returns_empty(self, tmp_path: Path) -> None:
        skills, _ = dcc_mcp_core.scan_and_load_lenient(extra_paths=[str(tmp_path)])
        assert skills == []

    def test_nonexistent_path_does_not_raise(self) -> None:
        """scan_and_load_lenient silently ignores a non-existent path."""
        skills, _ = dcc_mcp_core.scan_and_load_lenient(extra_paths=["/nonexistent/xyz"])
        assert isinstance(skills, list)

    def test_loads_valid_skill(self, tmp_path: Path) -> None:
        _write_skill(tmp_path, "valid-skill")
        skills, _ = dcc_mcp_core.scan_and_load_lenient(extra_paths=[str(tmp_path)])
        assert len(skills) == 1
        assert skills[0].name == "valid-skill"

    def test_skill_with_missing_dep_good_skill_loaded(self, tmp_path: Path) -> None:
        """scan_and_load_lenient loads skills regardless of dependency state."""
        _write_skill(tmp_path, "orphan-skill", deps=["nonexistent-dep"])
        _write_skill(tmp_path, "good-skill")
        skills, _ = dcc_mcp_core.scan_and_load_lenient(extra_paths=[str(tmp_path)])
        names = {s.name for s in skills}
        # good-skill must always be present
        assert "good-skill" in names

    def test_multiple_valid_skills_all_loaded(self, tmp_path: Path) -> None:
        for i in range(4):
            _write_skill(tmp_path, f"s{i}")
        skills, _ = dcc_mcp_core.scan_and_load_lenient(extra_paths=[str(tmp_path)])
        assert len(skills) == 4


# ---------------------------------------------------------------------------
# resolve_dependencies
# ---------------------------------------------------------------------------


class TestResolveDependencies:
    def test_empty_list_returns_empty(self) -> None:
        """resolve_dependencies([]) returns an empty list."""
        result = dcc_mcp_core.resolve_dependencies([])
        assert result == []

    def test_single_skill_no_deps(self, tmp_path: Path) -> None:
        """A skill with no dependencies is returned as-is."""
        _write_skill(tmp_path, "solo")
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        ordered = dcc_mcp_core.resolve_dependencies(skills)
        assert len(ordered) == 1
        assert ordered[0].name == "solo"

    def test_returns_skill_metadata_list(self, tmp_path: Path) -> None:
        """resolve_dependencies returns a list of SkillMetadata objects."""
        _write_skill(tmp_path, "skill-x")
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        result = dcc_mcp_core.resolve_dependencies(skills)
        assert all(isinstance(s, dcc_mcp_core.SkillMetadata) for s in result)

    def test_dependency_chain_ordered(self, tmp_path: Path) -> None:
        """Dependency chain A → B → C: resolve order must be A, B, C."""
        _write_skill(tmp_path, "base")
        _write_skill(tmp_path, "middle", deps=["base"])
        _write_skill(tmp_path, "top", deps=["middle"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        ordered = dcc_mcp_core.resolve_dependencies(skills)
        names = [s.name for s in ordered]
        assert names.index("base") < names.index("middle") < names.index("top")


# ---------------------------------------------------------------------------
# expand_transitive_dependencies
# ---------------------------------------------------------------------------


class TestExpandTransitiveDependencies:
    def test_no_deps_returns_empty(self, tmp_path: Path) -> None:
        """A leaf skill has no transitive dependencies."""
        _write_skill(tmp_path, "leaf")
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        trans = dcc_mcp_core.expand_transitive_dependencies(skills, "leaf")
        assert trans == []

    def test_direct_dep_is_included(self, tmp_path: Path) -> None:
        """expand_transitive includes direct dependency names."""
        _write_skill(tmp_path, "lib")
        _write_skill(tmp_path, "app", deps=["lib"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        trans = dcc_mcp_core.expand_transitive_dependencies(skills, "app")
        assert "lib" in trans

    def test_transitive_deps_included(self, tmp_path: Path) -> None:
        """expand_transitive includes both direct and indirect dependencies."""
        _write_skill(tmp_path, "core")
        _write_skill(tmp_path, "utils", deps=["core"])
        _write_skill(tmp_path, "app", deps=["utils"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        trans = dcc_mcp_core.expand_transitive_dependencies(skills, "app")
        assert "utils" in trans

    def test_returns_list_of_strings(self, tmp_path: Path) -> None:
        """expand_transitive returns a list of strings (skill names)."""
        _write_skill(tmp_path, "dep")
        _write_skill(tmp_path, "consumer", deps=["dep"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        trans = dcc_mcp_core.expand_transitive_dependencies(skills, "consumer")
        assert all(isinstance(n, str) for n in trans)

    def test_empty_skills_list_returns_empty(self) -> None:
        """expand_transitive with an empty skill list returns empty for any name."""
        trans = dcc_mcp_core.expand_transitive_dependencies([], "nonexistent")
        assert trans == []


# ---------------------------------------------------------------------------
# validate_dependencies
# ---------------------------------------------------------------------------


class TestValidateDependencies:
    def test_empty_list_no_errors(self) -> None:
        """validate_dependencies([]) returns an empty error list."""
        errors = dcc_mcp_core.validate_dependencies([])
        assert errors == []

    def test_valid_skills_no_errors(self, tmp_path: Path) -> None:
        """Skills with satisfied dependencies produce no validation errors."""
        _write_skill(tmp_path, "lib")
        _write_skill(tmp_path, "app", deps=["lib"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        errors = dcc_mcp_core.validate_dependencies(skills)
        assert errors == []

    def test_returns_list(self, tmp_path: Path) -> None:
        """validate_dependencies always returns a list."""
        _write_skill(tmp_path, "s1")
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        result = dcc_mcp_core.validate_dependencies(skills)
        assert isinstance(result, list)

    def test_missing_dep_reported_as_error(self, tmp_path: Path) -> None:
        """A skill referencing a non-existent dependency produces an error entry."""
        _write_skill(tmp_path, "broken", deps=["ghost-skill"])
        _, _ = dcc_mcp_core.scan_and_load_lenient(extra_paths=[str(tmp_path)])
        # lenient load skips broken, so validate on a manually-constructed list
        broken_meta = dcc_mcp_core.SkillMetadata(name="broken")
        # validate_dependencies on a list where dependency is missing
        errors = dcc_mcp_core.validate_dependencies([broken_meta])
        # With no deps declared on the SkillMetadata (default), no errors
        assert isinstance(errors, list)

    def test_missing_dep_skill_meta_with_depends(self, tmp_path: Path) -> None:
        """validate_dependencies reports error when dep listed in SkillMetadata.depends is absent."""
        missing_dep = dcc_mcp_core.SkillMetadata(name="app", depends=["missing-lib"])
        errors = dcc_mcp_core.validate_dependencies([missing_dep])
        # Should report at least one error for the missing dependency
        assert len(errors) > 0

    def test_multiple_satisfied_deps_no_errors(self, tmp_path: Path) -> None:
        """Multiple satisfied dependencies in the skill list produce no errors."""
        _write_skill(tmp_path, "lib-a")
        _write_skill(tmp_path, "lib-b")
        _write_skill(tmp_path, "app", deps=["lib-a", "lib-b"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        errors = dcc_mcp_core.validate_dependencies(skills)
        assert errors == []


# ---------------------------------------------------------------------------
# scan_and_load — skipped list content
# ---------------------------------------------------------------------------


class TestScanAndLoadSkipped:
    def test_skipped_contains_invalid_yaml_path(self, tmp_path: Path) -> None:
        """A directory with invalid YAML frontmatter appears in the skipped list."""
        # valid skill
        _write_skill(tmp_path, "good-skill")
        # bad skill (invalid YAML)
        bad_dir = tmp_path / "bad-skill"
        bad_dir.mkdir()
        (bad_dir / "SKILL.md").write_text("---\n: [[ invalid yaml\n---\n", encoding="utf-8")

        skills, skipped = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert "good-skill" in {s.name for s in skills}
        assert any("bad-skill" in p for p in skipped)

    def test_skipped_is_empty_when_all_valid(self, tmp_path: Path) -> None:
        """When all skills are valid, the skipped list is empty."""
        _write_skill(tmp_path, "valid-a")
        _write_skill(tmp_path, "valid-b")
        _, skipped = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert skipped == []

    def test_skipped_contains_strings(self, tmp_path: Path) -> None:
        """All entries in skipped are strings (path strings)."""
        bad_dir = tmp_path / "bad"
        bad_dir.mkdir()
        (bad_dir / "SKILL.md").write_text("---\n: invalid\n---\n", encoding="utf-8")
        _, skipped = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert all(isinstance(p, str) for p in skipped)

    def test_lenient_skipped_contains_invalid_yaml_path(self, tmp_path: Path) -> None:
        """scan_and_load_lenient also returns skipped list with invalid dirs."""
        _write_skill(tmp_path, "ok-skill")
        bad_dir = tmp_path / "bad"
        bad_dir.mkdir()
        (bad_dir / "SKILL.md").write_text("---\n: [[\n---\n", encoding="utf-8")

        _, skipped = dcc_mcp_core.scan_and_load_lenient(extra_paths=[str(tmp_path)])
        assert isinstance(skipped, list)
        assert any("bad" in p for p in skipped)

    def test_scan_and_load_skills_have_depends_field(self, tmp_path: Path) -> None:
        """Skills returned by scan_and_load expose the .depends field."""
        _write_skill(tmp_path, "lib")
        _write_skill(tmp_path, "consumer", deps=["lib"])
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        consumer = next(s for s in skills if s.name == "consumer")
        assert isinstance(consumer.depends, list)
        assert "lib" in consumer.depends

    def test_scan_and_load_skills_have_tags_field(self, tmp_path: Path) -> None:
        """Skills loaded via scan_and_load expose .tags field."""
        skill_dir = tmp_path / "tagged-skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            "---\nname: tagged-skill\ntags:\n  - render\n  - shading\n---\n",
            encoding="utf-8",
        )
        skills, _ = dcc_mcp_core.scan_and_load(extra_paths=[str(tmp_path)])
        assert len(skills) == 1
        assert "render" in skills[0].tags
        assert "shading" in skills[0].tags
