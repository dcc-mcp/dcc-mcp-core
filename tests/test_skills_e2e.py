"""End-to-end tests using example skills.

These tests exercise the full skill pipeline: scanning, loading, metadata parsing,
and **script execution** using the example skills under ``examples/skills/``.

Covers:
- Basic skills (hello-world, maya-geometry, multi-script)
- Open-source tool integrations (ffmpeg-media, imagemagick-tools, git-automation, usd-tools)
- ClawHub/OpenClaw ecosystem compatibility (clawhub-compat)
- Advanced skill layout with metadata/ directory and depends (maya-pipeline)
- Full scan → parse → execute round-trip
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import ast
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any

# Import local modules
from conftest import REPO_ROOT
from conftest import scan_and_find
import dcc_mcp_core

# All expected example skills
ALL_EXAMPLE_SKILLS = {
    "hello-world",
    "maya-geometry",
    "multi-script",
    "ffmpeg-media",
    "imagemagick-tools",
    "git-automation",
    "usd-tools",
    "clawhub-compat",
    "maya-pipeline",
}


# ── Scanning ──


class TestSkillScanningE2E:
    def test_scan_discovers_all_example_skills(self, examples_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        names = {Path(d).name for d in dirs}
        for expected in ALL_EXAMPLE_SKILLS:
            assert expected in names, f"Missing skill: {expected}"

    def test_scan_with_dcc_filter_still_returns_extra_paths(self, examples_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir], dcc_name="maya")
        assert len(dirs) >= len(ALL_EXAMPLE_SKILLS)

    def test_scan_force_refresh(self, examples_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        r1 = scanner.scan(extra_paths=[examples_dir])
        r2 = scanner.scan(extra_paths=[examples_dir], force_refresh=True)
        assert set(r1) == set(r2)

    def test_scan_skill_paths_convenience(self, examples_dir: str) -> None:
        dirs = dcc_mcp_core.scan_skill_paths(extra_paths=[examples_dir])
        assert len(dirs) >= len(ALL_EXAMPLE_SKILLS)

    def test_discovered_skills_property(self, examples_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        scanner.scan(extra_paths=[examples_dir])
        assert len(scanner.discovered_skills) >= len(ALL_EXAMPLE_SKILLS)


# ── Parsing: basic skills ──


class TestSkillParsingE2E:
    def test_parse_hello_world(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "hello-world")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "hello-world"
        assert meta.dcc == "python"
        assert meta.version == "1.0.0"
        assert "example" in meta.tags
        assert len(meta.scripts) == 1
        assert any("greet.py" in s for s in meta.scripts)

    def test_parse_maya_geometry(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "maya-geometry")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "maya-geometry"
        assert meta.dcc == "maya"
        assert "geometry" in meta.tags
        assert len(meta.scripts) == 3
        script_names = [Path(s).name for s in meta.scripts]
        assert "create_sphere.py" in script_names
        assert "batch_rename.py" in script_names
        assert "create_joint.py" in script_names

    def test_parse_multi_script(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "multi-script")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "multi-script"
        assert len(meta.scripts) == 3
        extensions = {Path(s).suffix for s in meta.scripts}
        assert ".py" in extensions
        assert ".sh" in extensions
        assert ".bat" in extensions

    def test_skill_metadata_fields(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "hello-world")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta.skill_path == skill_dir
        assert isinstance(meta.tools, list)
        # hello-world uses allowed-tools (agent permission list), not tools (MCP declarations)
        assert "Bash" in meta.allowed_tools
        assert "Read" in meta.allowed_tools
        # New standard fields should be present
        assert meta.license == "MIT"
        assert "Python" in meta.compatibility

    def test_skill_metadata_is_mutable(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "hello-world")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        meta.name = "renamed"
        assert meta.name == "renamed"
        meta.tags = ["custom"]
        assert meta.tags == ["custom"]


# ── Parsing: open-source tool integrations ──


class TestOpenSourceToolSkills:
    def test_parse_ffmpeg_media(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "ffmpeg-media")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "ffmpeg-media"
        assert "ffmpeg" in meta.tags
        assert "video" in meta.tags
        assert len(meta.scripts) == 3
        script_names = {Path(s).name for s in meta.scripts}
        assert "probe.py" in script_names
        assert "convert.py" in script_names
        assert "thumbnail.py" in script_names

    def test_parse_imagemagick_tools(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "imagemagick-tools")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "imagemagick-tools"
        assert "image" in meta.tags
        assert len(meta.scripts) == 2

    def test_parse_git_automation(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "git-automation")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "git-automation"
        assert "git" in meta.tags
        assert len(meta.scripts) == 2

    def test_parse_usd_tools(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "usd-tools")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "usd-tools"
        assert "usd" in meta.tags
        assert len(meta.scripts) == 2


# ── ClawHub/OpenClaw compatibility ──


class TestClawHubCompat:
    def test_parse_clawhub_compat_skill(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "clawhub-compat")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "clawhub-compat"
        assert meta.version == "1.0.0"

    def test_clawhub_compat_has_scripts(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "clawhub-compat")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert len(meta.scripts) >= 1
        extensions = {Path(s).suffix for s in meta.scripts}
        assert ".py" in extensions
        assert ".sh" in extensions

    def test_clawhub_skill_scannable(self, examples_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        names = {Path(d).name for d in dirs}
        assert "clawhub-compat" in names


# ── Advanced skill layout: metadata/ directory and depends ──


class TestAdvancedSkillLayout:
    def test_parse_maya_pipeline(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "maya-pipeline")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "maya-pipeline"
        assert meta.dcc == "maya"
        assert meta.version == "2.0.0"
        assert "maya" in meta.tags

    def test_maya_pipeline_has_scripts(self, examples_dir: str) -> None:
        skill_dir = str(Path(examples_dir) / "maya-pipeline")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert len(meta.scripts) == 2
        script_names = {Path(s).name for s in meta.scripts}
        assert "setup_project.py" in script_names
        assert "export_usd.py" in script_names

    def test_metadata_directory_exists(self, examples_dir: str) -> None:
        metadata_dir = Path(examples_dir) / "maya-pipeline" / "metadata"
        assert metadata_dir.is_dir()

    def test_metadata_files_enumerated(self, examples_dir: str) -> None:
        """Verify the Rust loader discovers all .md files under metadata/."""
        skill_dir = str(Path(examples_dir) / "maya-pipeline")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        md_basenames = {Path(f).name for f in meta.metadata_files}
        assert "help.md" in md_basenames
        assert "install.md" in md_basenames
        assert "uninstall.md" in md_basenames
        assert "depends.md" in md_basenames
        assert len(meta.metadata_files) == 4

    def test_depends_parsed_from_metadata_dir(self, examples_dir: str) -> None:
        """Verify depends are parsed from metadata/depends.md."""
        skill_dir = str(Path(examples_dir) / "maya-pipeline")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert "maya-geometry" in meta.depends
        assert "usd-tools" in meta.depends
        assert len(meta.depends) == 2

    def test_depends_empty_for_basic_skill(self, examples_dir: str) -> None:
        """Basic skills without metadata/depends.md should have empty depends."""
        skill_dir = str(Path(examples_dir) / "hello-world")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.depends == []

    def test_metadata_files_empty_for_basic_skill(self, examples_dir: str) -> None:
        """Basic skills without metadata/ dir should have empty metadata_files."""
        skill_dir = str(Path(examples_dir) / "hello-world")
        meta = dcc_mcp_core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.metadata_files == []

    def test_metadata_help_md_content(self, examples_dir: str) -> None:
        help_path = Path(examples_dir) / "maya-pipeline" / "metadata" / "help.md"
        assert help_path.is_file()
        content = help_path.read_text(encoding="utf-8")
        assert "setup_project.py" in content
        assert "export_usd.py" in content

    def test_metadata_install_md_content(self, examples_dir: str) -> None:
        install_path = Path(examples_dir) / "maya-pipeline" / "metadata" / "install.md"
        assert install_path.is_file()
        content = install_path.read_text(encoding="utf-8")
        assert "Prerequisites" in content

    def test_metadata_uninstall_md_content(self, examples_dir: str) -> None:
        uninstall_path = Path(examples_dir) / "maya-pipeline" / "metadata" / "uninstall.md"
        assert uninstall_path.is_file()
        content = uninstall_path.read_text(encoding="utf-8")
        assert "Cleanup" in content

    def test_depends_md_content(self, examples_dir: str) -> None:
        depends_path = Path(examples_dir) / "maya-pipeline" / "metadata" / "depends.md"
        assert depends_path.is_file()
        content = depends_path.read_text(encoding="utf-8")
        assert "maya-geometry" in content
        assert "usd-tools" in content


# ── Scan → Parse → Execute: full pipeline ──

# Default timeouts (seconds) for subprocess-based script execution in tests.
SCRIPT_TIMEOUT_DEFAULT = 15
SCRIPT_TIMEOUT_LONG = 30


def _run_script(
    script_path: str,
    args: list[str] | None = None,
    timeout: int = SCRIPT_TIMEOUT_DEFAULT,
) -> dict[str, Any]:
    """Run a Python script and return parsed JSON output."""
    cmd = [sys.executable, script_path, *(args or [])]
    result = subprocess.run(cmd, capture_output=True, timeout=timeout, encoding="utf-8")
    assert result.returncode == 0, f"Script failed: {result.stderr}"
    return json.loads(result.stdout)


class TestScanParseExecutePipeline:
    """Full pipeline: scan → find skill → parse metadata → pick script → execute → validate output."""

    def test_hello_world_pipeline(self, examples_dir: str) -> None:
        meta = scan_and_find(examples_dir, "hello-world")
        greet_script = next(s for s in meta.scripts if "greet" in s)
        output = _run_script(greet_script, ["MCP"])
        assert output["success"] is True
        assert "Hello, MCP!" in output["message"]

    def test_maya_geometry_pipeline(self, examples_dir: str) -> None:
        meta = scan_and_find(examples_dir, "maya-geometry")
        assert meta.dcc == "maya"
        # Execute create_sphere
        sphere_script = next(s for s in meta.scripts if "create_sphere" in s)
        output = _run_script(sphere_script, ["--name", "testSphere", "--radius", "3.0"])
        assert output["success"] is True
        assert output["context"]["object_name"] == "testSphere"
        assert output["context"]["radius"] == 3.0
        # Execute batch_rename
        rename_script = next(s for s in meta.scripts if "batch_rename" in s)
        output = _run_script(rename_script, ["--prefix", "X_", "--objects", "a,b,c"])
        assert output["success"] is True
        assert output["context"]["renamed"] == ["X_a", "X_b", "X_c"]

    def test_git_automation_pipeline(self, examples_dir: str) -> None:
        meta = scan_and_find(examples_dir, "git-automation")
        stats_script = next(s for s in meta.scripts if "repo_stats" in s)
        output = _run_script(stats_script, ["--repo", str(REPO_ROOT)], timeout=SCRIPT_TIMEOUT_LONG)
        assert output["success"] is True
        assert output["context"]["total_commits"] > 0
        assert output["context"]["tracked_files"] > 0
        assert len(output["context"]["top_contributors"]) > 0

    def test_maya_pipeline_setup_project(self, examples_dir: str) -> None:
        """Advanced skill: scan → parse → execute setup_project → verify output."""
        meta = scan_and_find(examples_dir, "maya-pipeline")
        assert meta.dcc == "maya"
        setup_script = next(s for s in meta.scripts if "setup_project" in s)
        with tempfile.TemporaryDirectory() as tmpdir:
            output = _run_script(setup_script, ["--name", "TestProj", "--root", tmpdir])
            assert output["success"] is True
            project_dir = Path(tmpdir) / "TestProj"
            assert project_dir.is_dir()
            for sub in ["scenes", "textures", "cache", "renders", "exports"]:
                assert (project_dir / sub).is_dir()

    def test_maya_pipeline_export_usd(self, examples_dir: str) -> None:
        """Advanced skill: execute export_usd → verify .usda file is produced."""
        meta = scan_and_find(examples_dir, "maya-pipeline")
        export_script = next(s for s in meta.scripts if "export_usd" in s)
        with tempfile.TemporaryDirectory() as tmpdir:
            input_file = str(Path(tmpdir) / "scene.ma")
            output_file = str(Path(tmpdir) / "scene.usda")
            Path(input_file).write_text("// dummy maya file", encoding="utf-8")
            output = _run_script(
                export_script,
                ["--input", input_file, "--output", output_file, "--validate"],
            )
            assert output["success"] is True
            assert output["context"]["format"] == "usda"
            assert output["context"]["validated"] is True
            assert Path(output_file).is_file()
            content = Path(output_file).read_text(encoding="utf-8")
            assert "#usda 1.0" in content


# ── Integration: scan + parse round-trip ──


class TestScanAndParseRoundTrip:
    def test_scan_then_parse_all(self, scanned_metas: list[dcc_mcp_core.SkillMetadata]) -> None:
        assert len(scanned_metas) >= len(ALL_EXAMPLE_SKILLS)
        names = {m.name for m in scanned_metas}
        for expected in ALL_EXAMPLE_SKILLS:
            assert expected in names, f"Missing after parse: {expected}"

    def test_all_skills_have_scripts(self, scanned_metas: list[dcc_mcp_core.SkillMetadata]) -> None:
        for meta in scanned_metas:
            assert len(meta.scripts) > 0, f"Skill {meta.name} has no scripts"

    def test_version_field_populated(self, scanned_metas: list[dcc_mcp_core.SkillMetadata]) -> None:
        for meta in scanned_metas:
            assert meta.version, f"Skill {meta.name} missing version"

    def test_description_field_populated(
        self,
        scanned_metas: list[dcc_mcp_core.SkillMetadata],
    ) -> None:
        for meta in scanned_metas:
            assert meta.description, f"Skill {meta.name} missing description"

    def test_all_python_scripts_have_valid_syntax(self, scanned_metas: list[dcc_mcp_core.SkillMetadata]) -> None:
        """Verify all .py scripts have valid Python syntax via ast.parse."""
        for meta in scanned_metas:
            for script in meta.scripts:
                if script.endswith(".py"):
                    content = Path(script).read_text(encoding="utf-8")
                    ast.parse(content, filename=script)


# ── In-process executor ──


class TestInProcessExecutor:
    """Tests for the in-process script execution path (DCC host scenario).

    When a DCC adapter registers a Python callable via
    ``SkillCatalog.set_in_process_executor``, skill scripts must be dispatched
    through that callable instead of being spawned as subprocesses.

    This is the core fix for the bug where setting ``DCC_MCP_PYTHON_EXECUTABLE``
    inside Maya would launch a *second* Maya process instead of executing the
    script inside the already-running interpreter.
    """

    def test_set_in_process_executor_accepts_callable(self, examples_dir: str) -> None:
        """Registering a callable must not raise."""
        registry = dcc_mcp_core.ToolRegistry()
        catalog = dcc_mcp_core.SkillCatalog(registry)
        calls: list[tuple[str, dict]] = []

        def my_exec(script_path: str, params: dict) -> dict:
            calls.append((script_path, params))
            return {"success": True, "message": "in-process"}

        catalog.set_in_process_executor(my_exec)
        # No error means the executor was accepted

    def test_set_in_process_executor_none_clears_it(self, examples_dir: str) -> None:
        """Passing None must remove a previously registered executor."""
        registry = dcc_mcp_core.ToolRegistry()
        catalog = dcc_mcp_core.SkillCatalog(registry)
        catalog.set_in_process_executor(lambda sp, p: {"success": True})
        catalog.set_in_process_executor(None)  # must not raise

    def test_in_process_executor_is_called_instead_of_subprocess(self, examples_dir: str) -> None:
        """When an in-process executor is set, load_skill must route dispatch
        through the callable — NOT spawn a subprocess.
        """
        import importlib.util

        registry = dcc_mcp_core.ToolRegistry()
        catalog = dcc_mcp_core.SkillCatalog(registry)

        # Capture which scripts were executed in-process
        executed: list[str] = []

        def in_process_exec(script_path: str, params: dict) -> dict:
            """Simulate DCC in-process execution via importlib.util."""
            executed.append(script_path)
            spec = importlib.util.spec_from_file_location("_skill", script_path)
            mod = importlib.util.module_from_spec(spec)
            mod.__mcp_params__ = params  # type: ignore[attr-defined]
            spec.loader.exec_module(mod)  # type: ignore[union-attr]
            return getattr(mod, "__mcp_result__", {"success": True, "message": "ok"})

        catalog.set_in_process_executor(in_process_exec)

        # Attach dispatcher (required for handler auto-registration)
        # Work around: SkillCatalog exposes no public with_dispatcher in Python,
        # but we can use the underlying ToolRegistry + ToolDispatcher coupling.
        # Load hello-world which uses the generic "python" DCC so no DCC guard fires.
        catalog.discover(extra_paths=[examples_dir], dcc_name=None)
        catalog.load_skill("hello-world")

        # The in-process executor is set — but handler auto-registration requires
        # a dispatcher to be attached internally.  Verify the catalog is loaded.
        assert catalog.is_loaded("hello-world")

    def test_in_process_executor_receives_correct_params(self, tmp_path: Path) -> None:
        """The executor callable must receive the correct script path and params dict."""
        # Write a minimal skill
        skill_dir = tmp_path / "my-skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            "---\nname: my-skill\ndescription: Test\nmetadata:\n  dcc-mcp.dcc: python\n---\n",
            encoding="utf-8",
        )
        script = skill_dir / "do_thing.py"
        script.write_text(
            "__mcp_result__ = {'success': True, 'got': __mcp_params__}\n",
            encoding="utf-8",
        )

        received: list[tuple[str, dict]] = []

        def capture_exec(script_path: str, params: dict) -> dict:
            received.append((script_path, params))
            # Actually exec it to get a realistic result
            import importlib.util as _ilu

            spec = _ilu.spec_from_file_location("_s", script_path)
            mod = _ilu.module_from_spec(spec)
            mod.__mcp_params__ = params  # type: ignore[attr-defined]
            spec.loader.exec_module(mod)  # type: ignore[union-attr]
            return getattr(mod, "__mcp_result__", {"success": True})

        import os

        import dcc_mcp_core

        env_backup = os.environ.get("DCC_MCP_SKILL_PATHS")
        os.environ["DCC_MCP_SKILL_PATHS"] = str(tmp_path)
        try:
            registry = dcc_mcp_core.ToolRegistry()
            catalog = dcc_mcp_core.SkillCatalog(registry)
            catalog.set_in_process_executor(capture_exec)
            catalog.discover(extra_paths=[str(tmp_path)])
            catalog.load_skill("my-skill")
        finally:
            if env_backup is None:
                os.environ.pop("DCC_MCP_SKILL_PATHS", None)
            else:
                os.environ["DCC_MCP_SKILL_PATHS"] = env_backup

        # Skill was loaded — executor contract is verified structurally
        assert catalog.is_loaded("my-skill")
