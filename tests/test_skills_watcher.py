"""Tests for SkillWatcher PyO3 bindings.

Covers:
- Construction with default and custom debounce values
- watch() / unwatch() behaviour
- skill_count() / skills() / watched_paths() accessors
- reload() manual trigger
- __repr__ string
- Error path: watch non-existent directory
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import contextlib
from pathlib import Path
import time

# Import third-party modules
import pytest

from conftest import create_skill_dir

# Import local modules
import dcc_mcp_core

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _write_skill(base: Path, name: str, dcc: str = "python") -> Path:
    """Write a minimal SKILL.md under *base*/*name* and return the skill dir."""
    skill_dir = base / name
    skill_dir.mkdir(parents=True, exist_ok=True)
    content = f"---\nname: {name}\ndcc: {dcc}\n---\n# {name}\n\nTest skill."
    (skill_dir / "SKILL.md").write_text(content, encoding="utf-8")
    return skill_dir


# ---------------------------------------------------------------------------
# Construction
# ---------------------------------------------------------------------------


class TestSkillWatcherNew:
    def test_default_debounce(self) -> None:
        """SkillWatcher() with default 300 ms debounce constructs without error."""
        w = dcc_mcp_core.SkillWatcher()
        assert w is not None

    def test_custom_debounce(self) -> None:
        """SkillWatcher(debounce_ms=100) accepts custom debounce."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=100)
        assert w is not None

    def test_zero_debounce(self) -> None:
        """SkillWatcher(debounce_ms=0) is valid (no debouncing)."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=0)
        assert w is not None

    def test_large_debounce(self) -> None:
        """SkillWatcher accepts very large debounce values."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=60_000)
        assert w is not None

    def test_initial_skill_count_is_zero(self) -> None:
        """Fresh watcher has zero skills before any watch() call."""
        w = dcc_mcp_core.SkillWatcher()
        assert w.skill_count() == 0

    def test_initial_watched_paths_empty(self) -> None:
        """Fresh watcher has an empty watched_paths list."""
        w = dcc_mcp_core.SkillWatcher()
        assert w.watched_paths() == []

    def test_initial_skills_empty(self) -> None:
        """skills() returns an empty list before any watch() call."""
        w = dcc_mcp_core.SkillWatcher()
        assert w.skills() == []


# ---------------------------------------------------------------------------
# watch()
# ---------------------------------------------------------------------------


class TestSkillWatcherWatch:
    def test_watch_valid_empty_dir(self, tmp_path: Path) -> None:
        """watch() on an empty directory succeeds and records the path."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert str(tmp_path) in w.watched_paths()

    def test_watch_invalid_dir_raises(self) -> None:
        """watch() on a non-existent path raises RuntimeError."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        with pytest.raises(RuntimeError):
            w.watch("/this/path/absolutely/does/not/exist/xyz_abc")

    def test_watch_loads_existing_skills_immediately(self, tmp_path: Path) -> None:
        """watch() performs an immediate reload — skills are visible right away."""
        _write_skill(tmp_path, "alpha")
        _write_skill(tmp_path, "beta")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))

        assert w.skill_count() == 2

    def test_watch_skill_names_correct(self, tmp_path: Path) -> None:
        """Loaded skills have the names declared in their SKILL.md."""
        _write_skill(tmp_path, "my-skill", dcc="maya")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))

        skills = w.skills()
        assert len(skills) == 1
        assert skills[0].name == "my-skill"
        assert skills[0].dcc == "maya"

    def test_watch_multiple_directories(self, tmp_path: Path) -> None:
        """watch() can be called multiple times to monitor separate directories."""
        dir_a = tmp_path / "dirA"
        dir_b = tmp_path / "dirB"
        dir_a.mkdir()
        dir_b.mkdir()

        _write_skill(dir_a, "skill-a")
        _write_skill(dir_b, "skill-b")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(dir_a))
        w.watch(str(dir_b))

        paths = w.watched_paths()
        assert str(dir_a) in paths
        assert str(dir_b) in paths
        assert w.skill_count() == 2

    def test_watch_empty_dir_zero_skills(self, tmp_path: Path) -> None:
        """watch() on an empty directory reports zero skills."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 0

    def test_watch_many_skills(self, tmp_path: Path) -> None:
        """Loading 10 skills at once works correctly."""
        for i in range(10):
            _write_skill(tmp_path, f"skill-{i:02d}")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))

        assert w.skill_count() == 10

    def test_watch_path_recorded_as_string(self, tmp_path: Path) -> None:
        """watched_paths() returns string representations of paths."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        paths = w.watched_paths()
        assert all(isinstance(p, str) for p in paths)

    def test_watch_idempotent_same_path(self, tmp_path: Path) -> None:
        """Watching the same path twice does not cause errors."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        # Second call on same dir may raise or succeed depending on OS — just
        # ensure no Python-level crash.
        with contextlib.suppress(RuntimeError):
            w.watch(str(tmp_path))


# ---------------------------------------------------------------------------
# unwatch()
# ---------------------------------------------------------------------------


class TestSkillWatcherUnwatch:
    def test_unwatch_known_path_returns_true(self, tmp_path: Path) -> None:
        """unwatch() returns True for a path that was being watched."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        result = w.unwatch(str(tmp_path))
        assert result is True

    def test_unwatch_removes_path_from_list(self, tmp_path: Path) -> None:
        """After unwatch(), the path no longer appears in watched_paths()."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert str(tmp_path) in w.watched_paths()
        w.unwatch(str(tmp_path))
        assert str(tmp_path) not in w.watched_paths()

    def test_unwatch_unknown_path_returns_false(self) -> None:
        """unwatch() returns False for a path that was never watched."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        result = w.unwatch("/never/watched/path")
        assert result is False

    def test_unwatch_clears_skills(self, tmp_path: Path) -> None:
        """After unwatch(), a manual reload finds zero skills for the removed path."""
        _write_skill(tmp_path, "skill-x")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 1

        w.unwatch(str(tmp_path))
        # Paths list is empty; reload clears skills
        assert w.skill_count() == 0

    def test_unwatch_one_of_two_directories(self, tmp_path: Path) -> None:
        """Unwatching one of two directories keeps the other intact."""
        dir_a = tmp_path / "dirA"
        dir_b = tmp_path / "dirB"
        dir_a.mkdir()
        dir_b.mkdir()
        _write_skill(dir_a, "skill-a")
        _write_skill(dir_b, "skill-b")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(dir_a))
        w.watch(str(dir_b))
        assert w.skill_count() == 2

        w.unwatch(str(dir_a))
        # dir_b is still watched; manual reload should keep skill-b
        assert str(dir_b) in w.watched_paths()
        assert str(dir_a) not in w.watched_paths()


# ---------------------------------------------------------------------------
# reload()
# ---------------------------------------------------------------------------


class TestSkillWatcherReload:
    def test_reload_picks_up_new_skill(self, tmp_path: Path) -> None:
        """Manual reload detects a skill added after watch()."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 0

        _write_skill(tmp_path, "late-addition")
        w.reload()
        assert w.skill_count() == 1

    def test_reload_reflects_removed_skill(self, tmp_path: Path) -> None:
        """Manual reload drops a skill whose directory was deleted."""
        _write_skill(tmp_path, "removable")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 1

        import shutil

        shutil.rmtree(str(tmp_path / "removable"))
        w.reload()
        assert w.skill_count() == 0

    def test_reload_without_watching_is_safe(self) -> None:
        """reload() on a watcher with no watched paths does not raise."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.reload()  # must not raise
        assert w.skill_count() == 0

    def test_reload_is_idempotent(self, tmp_path: Path) -> None:
        """Calling reload() multiple times on unchanged content is stable."""
        _write_skill(tmp_path, "stable-skill")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        for _ in range(5):
            w.reload()
        assert w.skill_count() == 1

    def test_reload_updates_existing_skill_metadata(self, tmp_path: Path) -> None:
        """Manual reload picks up changed SKILL.md content."""
        skill_dir = tmp_path / "mutable-skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text("---\nname: mutable-skill\ndcc: maya\n---\n", encoding="utf-8")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skills()[0].dcc == "maya"

        # Change the DCC field
        (skill_dir / "SKILL.md").write_text("---\nname: mutable-skill\ndcc: blender\n---\n", encoding="utf-8")
        w.reload()
        assert w.skills()[0].dcc == "blender"

    def test_reload_called_after_add_and_remove(self, tmp_path: Path) -> None:
        """reload() correctly handles a skill being added then removed."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 0

        _write_skill(tmp_path, "transient")
        w.reload()
        assert w.skill_count() == 1

        import shutil

        shutil.rmtree(str(tmp_path / "transient"))
        w.reload()
        assert w.skill_count() == 0


# ---------------------------------------------------------------------------
# skills() accessor
# ---------------------------------------------------------------------------


class TestSkillWatcherSkills:
    def test_skills_returns_list(self, tmp_path: Path) -> None:
        """skills() always returns a list (even when empty)."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        result = w.skills()
        assert isinstance(result, list)

    def test_skills_contains_skill_metadata(self, tmp_path: Path) -> None:
        """Each element in skills() is a SkillMetadata instance."""
        _write_skill(tmp_path, "meta-check", dcc="houdini")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        for item in w.skills():
            assert isinstance(item, dcc_mcp_core.SkillMetadata)

    def test_skills_snapshot_is_independent(self, tmp_path: Path) -> None:
        """Mutating the returned list does not change watcher state."""
        _write_skill(tmp_path, "snap-skill")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        snap = w.skills()
        snap.clear()  # mutate the snapshot
        # Original watcher state should be unchanged
        assert w.skill_count() == 1

    def test_skill_count_matches_skills_length(self, tmp_path: Path) -> None:
        """skill_count() equals len(skills())."""
        for i in range(4):
            _write_skill(tmp_path, f"sk-{i}")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == len(w.skills())

    def test_skills_have_correct_dcc(self, tmp_path: Path) -> None:
        """SkillMetadata objects carry the DCC field from SKILL.md."""
        _write_skill(tmp_path, "unreal-skill", dcc="unreal")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skills()[0].dcc == "unreal"

    def test_multiple_dccs(self, tmp_path: Path) -> None:
        """Skills with different DCC values are all loaded."""
        for name, dcc in [("m", "maya"), ("b", "blender"), ("h", "houdini")]:
            _write_skill(tmp_path, name, dcc=dcc)
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        dccs = {s.dcc for s in w.skills()}
        assert "maya" in dccs
        assert "blender" in dccs
        assert "houdini" in dccs


# ---------------------------------------------------------------------------
# __repr__
# ---------------------------------------------------------------------------


class TestSkillWatcherRepr:
    def test_repr_contains_skill_watcher(self) -> None:
        """repr() output includes 'SkillWatcher'."""
        w = dcc_mcp_core.SkillWatcher()
        assert "SkillWatcher" in repr(w)

    def test_repr_shows_zero_skills_initially(self) -> None:
        """repr() reflects skill_count before any watch()."""
        w = dcc_mcp_core.SkillWatcher()
        r = repr(w)
        assert "0" in r

    def test_repr_updates_after_watch(self, tmp_path: Path) -> None:
        """repr() reflects updated skill count after watch()."""
        _write_skill(tmp_path, "repr-skill")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        r = repr(w)
        assert "1" in r

    def test_repr_is_string(self) -> None:
        """repr() returns a str."""
        w = dcc_mcp_core.SkillWatcher()
        assert isinstance(repr(w), str)


# ---------------------------------------------------------------------------
# Integration: create skill_dir via conftest helper
# ---------------------------------------------------------------------------


class TestSkillWatcherWithConftestHelper:
    def test_create_skill_dir_and_watch(self, tmp_path: Path) -> None:
        """Skill created via conftest helper is detected by watcher."""
        create_skill_dir(str(tmp_path), "helper-skill", dcc="maya")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 1
        assert w.skills()[0].name == "helper-skill"

    def test_watch_then_scan_consistency(self, tmp_path: Path) -> None:
        """SkillWatcher and SkillScanner agree on the number of skills."""
        for i in range(3):
            create_skill_dir(str(tmp_path), f"cmp-{i}")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))

        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[str(tmp_path)])
        assert w.skill_count() == len(dirs)

    def test_skill_path_is_set(self, tmp_path: Path) -> None:
        """SkillMetadata.skill_path is non-empty after loading."""
        create_skill_dir(str(tmp_path), "path-skill")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        for skill in w.skills():
            assert skill.skill_path != ""


# ---------------------------------------------------------------------------
# Edge cases
# ---------------------------------------------------------------------------


class TestSkillWatcherEdgeCases:
    def test_skill_with_scripts(self, tmp_path: Path) -> None:
        """Skill with a scripts/ subdirectory is loaded and scripts list is populated."""
        skill_dir = tmp_path / "scripted-skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text("---\nname: scripted-skill\ndcc: maya\n---\n", encoding="utf-8")
        scripts_dir = skill_dir / "scripts"
        scripts_dir.mkdir()
        (scripts_dir / "run.py").write_text("print('hello')", encoding="utf-8")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 1
        assert len(w.skills()[0].scripts) >= 1

    def test_dir_without_skill_md_ignored(self, tmp_path: Path) -> None:
        """Subdirectory without SKILL.md is not loaded as a skill."""
        not_skill = tmp_path / "not-a-skill"
        not_skill.mkdir()
        (not_skill / "README.md").write_text("# Not a skill", encoding="utf-8")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 0

    def test_invalid_skill_md_skipped(self, tmp_path: Path) -> None:
        """SKILL.md with invalid YAML is silently skipped."""
        bad_skill = tmp_path / "bad-skill"
        bad_skill.mkdir()
        (bad_skill / "SKILL.md").write_text("---\n: invalid: yaml: [[\n---\n", encoding="utf-8")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 0

    def test_mix_valid_and_invalid_skills(self, tmp_path: Path) -> None:
        """Only valid skills are loaded; invalid ones are silently dropped."""
        _write_skill(tmp_path, "good-skill")
        bad = tmp_path / "bad-skill"
        bad.mkdir()
        (bad / "SKILL.md").write_text("---\n: bad [[\n---\n", encoding="utf-8")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skill_count() == 1
        assert w.skills()[0].name == "good-skill"

    def test_watched_paths_returns_list_of_strings(self, tmp_path: Path) -> None:
        """watched_paths() always returns List[str]."""
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        paths = w.watched_paths()
        assert isinstance(paths, list)
        assert all(isinstance(p, str) for p in paths)

    def test_skill_version_default(self, tmp_path: Path) -> None:
        """Skill without explicit version gets the default '1.0.0'."""
        _write_skill(tmp_path, "version-skill")
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skills()[0].version == "1.0.0"

    def test_skill_with_explicit_version(self, tmp_path: Path) -> None:
        """Skill with explicit version field in SKILL.md is parsed correctly."""
        skill_dir = tmp_path / "versioned"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            "---\nname: versioned\ndcc: python\nversion: 2.3.1\n---\n",
            encoding="utf-8",
        )
        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)
        w.watch(str(tmp_path))
        assert w.skills()[0].version == "2.3.1"

    def test_multiple_watches_and_unwatches(self, tmp_path: Path) -> None:
        """Repeated watch/unwatch cycles leave the watcher in a consistent state."""
        dir_a = tmp_path / "a"
        dir_b = tmp_path / "b"
        dir_a.mkdir()
        dir_b.mkdir()
        _write_skill(dir_a, "skill-a")
        _write_skill(dir_b, "skill-b")

        w = dcc_mcp_core.SkillWatcher(debounce_ms=50)

        w.watch(str(dir_a))
        assert w.skill_count() == 1

        w.watch(str(dir_b))
        assert w.skill_count() == 2

        w.unwatch(str(dir_a))
        assert w.skill_count() == 1
        assert str(dir_a) not in w.watched_paths()

        w.unwatch(str(dir_b))
        assert w.skill_count() == 0
        assert w.watched_paths() == []
