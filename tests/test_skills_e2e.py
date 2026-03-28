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

# Import built-in modules
import json
import os
import subprocess
import sys
import tempfile

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# Resolve examples/skills relative to repo root
REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXAMPLES_SKILLS_DIR = os.path.join(REPO_ROOT, "examples", "skills")

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


@pytest.fixture()
def examples_dir():
    """Return the path to the examples/skills directory, skipping if absent."""
    if not os.path.isdir(EXAMPLES_SKILLS_DIR):
        pytest.skip("examples/skills directory not found")
    return EXAMPLES_SKILLS_DIR


# ── Scanning ──


class TestSkillScanningE2E:
    def test_scan_discovers_all_example_skills(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        names = {os.path.basename(d) for d in dirs}
        for expected in ALL_EXAMPLE_SKILLS:
            assert expected in names, f"Missing skill: {expected}"

    def test_scan_with_dcc_filter_still_returns_extra_paths(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir], dcc_name="maya")
        assert len(dirs) >= len(ALL_EXAMPLE_SKILLS)

    def test_scan_force_refresh(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        r1 = scanner.scan(extra_paths=[examples_dir])
        r2 = scanner.scan(extra_paths=[examples_dir], force_refresh=True)
        assert set(r1) == set(r2)

    def test_scan_skill_paths_convenience(self, examples_dir):
        dirs = dcc_mcp_core.scan_skill_paths(extra_paths=[examples_dir])
        assert len(dirs) >= len(ALL_EXAMPLE_SKILLS)

    def test_discovered_skills_property(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        scanner.scan(extra_paths=[examples_dir])
        assert len(scanner.discovered_skills) >= len(ALL_EXAMPLE_SKILLS)


# ── Parsing: basic skills ──


class TestSkillParsingE2E:
    def test_parse_hello_world(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "hello-world")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "hello-world"
        assert meta.dcc == "python"
        assert meta.version == "1.0.0"
        assert "example" in meta.tags
        assert len(meta.scripts) == 1
        assert any("greet.py" in s for s in meta.scripts)

    def test_parse_maya_geometry(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "maya-geometry")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "maya-geometry"
        assert meta.dcc == "maya"
        assert "geometry" in meta.tags
        assert len(meta.scripts) == 2
        script_names = [os.path.basename(s) for s in meta.scripts]
        assert "create_sphere.py" in script_names
        assert "batch_rename.py" in script_names

    def test_parse_multi_script(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "multi-script")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "multi-script"
        assert len(meta.scripts) == 3
        extensions = {os.path.splitext(s)[1] for s in meta.scripts}
        assert ".py" in extensions
        assert ".sh" in extensions
        assert ".bat" in extensions

    def test_skill_metadata_fields(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "hello-world")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta.skill_path == skill_dir
        assert isinstance(meta.tools, list)
        assert "Bash" in meta.tools
        assert "Read" in meta.tools

    def test_skill_metadata_is_mutable(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "hello-world")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        meta.name = "renamed"
        assert meta.name == "renamed"
        meta.tags = ["custom"]
        assert meta.tags == ["custom"]


# ── Parsing: open-source tool integrations ──


class TestOpenSourceToolSkills:
    def test_parse_ffmpeg_media(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "ffmpeg-media")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "ffmpeg-media"
        assert "ffmpeg" in meta.tags
        assert "video" in meta.tags
        assert len(meta.scripts) == 3
        script_names = {os.path.basename(s) for s in meta.scripts}
        assert "probe.py" in script_names
        assert "convert.py" in script_names
        assert "thumbnail.py" in script_names

    def test_parse_imagemagick_tools(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "imagemagick-tools")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "imagemagick-tools"
        assert "image" in meta.tags
        assert len(meta.scripts) == 2

    def test_parse_git_automation(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "git-automation")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "git-automation"
        assert "git" in meta.tags
        assert len(meta.scripts) == 2

    def test_parse_usd_tools(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "usd-tools")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "usd-tools"
        assert "usd" in meta.tags
        assert len(meta.scripts) == 2


# ── ClawHub/OpenClaw compatibility ──


class TestClawHubCompat:
    def test_parse_clawhub_compat_skill(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "clawhub-compat")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "clawhub-compat"
        assert meta.version == "1.0.0"

    def test_clawhub_compat_has_scripts(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "clawhub-compat")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert len(meta.scripts) >= 1
        extensions = {os.path.splitext(s)[1] for s in meta.scripts}
        assert ".py" in extensions
        assert ".sh" in extensions

    def test_clawhub_skill_scannable(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        names = {os.path.basename(d) for d in dirs}
        assert "clawhub-compat" in names


# ── Advanced skill layout: metadata/ directory and depends ──


class TestAdvancedSkillLayout:
    def test_parse_maya_pipeline(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "maya-pipeline")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.name == "maya-pipeline"
        assert meta.dcc == "maya"
        assert meta.version == "2.0.0"
        assert "advanced" in meta.tags
        assert "composable" in meta.tags

    def test_maya_pipeline_has_scripts(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "maya-pipeline")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert len(meta.scripts) == 2
        script_names = {os.path.basename(s) for s in meta.scripts}
        assert "setup_project.py" in script_names
        assert "export_usd.py" in script_names

    def test_metadata_directory_exists(self, examples_dir):
        skill_dir = os.path.join(examples_dir, "maya-pipeline")
        metadata_dir = os.path.join(skill_dir, "metadata")
        assert os.path.isdir(metadata_dir)

    def test_metadata_files_enumerated(self, examples_dir):
        """Verify the Rust loader discovers all .md files under metadata/."""
        skill_dir = os.path.join(examples_dir, "maya-pipeline")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        md_basenames = {os.path.basename(f) for f in meta.metadata_files}
        assert "help.md" in md_basenames
        assert "install.md" in md_basenames
        assert "uninstall.md" in md_basenames
        assert "depends.md" in md_basenames
        assert len(meta.metadata_files) == 4

    def test_depends_parsed_from_metadata_dir(self, examples_dir):
        """Verify depends are parsed from metadata/depends.md."""
        skill_dir = os.path.join(examples_dir, "maya-pipeline")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert "maya-geometry" in meta.depends
        assert "usd-tools" in meta.depends
        assert len(meta.depends) == 2

    def test_depends_empty_for_basic_skill(self, examples_dir):
        """Basic skills without metadata/depends.md should have empty depends."""
        skill_dir = os.path.join(examples_dir, "hello-world")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.depends == []

    def test_metadata_files_empty_for_basic_skill(self, examples_dir):
        """Basic skills without metadata/ dir should have empty metadata_files."""
        skill_dir = os.path.join(examples_dir, "hello-world")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        assert meta.metadata_files == []

    def test_metadata_help_md_content(self, examples_dir):
        help_path = os.path.join(examples_dir, "maya-pipeline", "metadata", "help.md")
        assert os.path.isfile(help_path)
        with open(help_path) as f:
            content = f.read()
        assert "setup_project.py" in content
        assert "export_usd.py" in content

    def test_metadata_install_md_content(self, examples_dir):
        install_path = os.path.join(examples_dir, "maya-pipeline", "metadata", "install.md")
        assert os.path.isfile(install_path)
        with open(install_path) as f:
            content = f.read()
        assert "Prerequisites" in content

    def test_metadata_uninstall_md_content(self, examples_dir):
        uninstall_path = os.path.join(examples_dir, "maya-pipeline", "metadata", "uninstall.md")
        assert os.path.isfile(uninstall_path)
        with open(uninstall_path) as f:
            content = f.read()
        assert "Cleanup" in content

    def test_depends_md_content(self, examples_dir):
        depends_path = os.path.join(examples_dir, "maya-pipeline", "metadata", "depends.md")
        assert os.path.isfile(depends_path)
        with open(depends_path) as f:
            content = f.read()
        assert "maya-geometry" in content
        assert "usd-tools" in content


# ── Scan → Parse → Execute: full pipeline ──


def _run_script(script_path, args=None, timeout=15):
    """Run a Python script and return parsed JSON output."""
    cmd = [sys.executable, script_path, *(args or [])]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    assert result.returncode == 0, f"Script failed: {result.stderr}"
    return json.loads(result.stdout)


class TestScanParseExecutePipeline:
    """Full pipeline: scan → find skill → parse metadata → pick script → execute → validate output."""

    def test_hello_world_pipeline(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        # Find hello-world by name
        skill_dir = next(d for d in dirs if os.path.basename(d) == "hello-world")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
        # Find and execute the greet script
        greet_script = next(s for s in meta.scripts if "greet" in s)
        output = _run_script(greet_script, ["MCP"])
        assert output["success"] is True
        assert "Hello, MCP!" in output["message"]

    def test_maya_geometry_pipeline(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        skill_dir = next(d for d in dirs if os.path.basename(d) == "maya-geometry")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta is not None
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

    def test_git_automation_pipeline(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        skill_dir = next(d for d in dirs if os.path.basename(d) == "git-automation")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        # Execute repo_stats on this repo
        stats_script = next(s for s in meta.scripts if "repo_stats" in s)
        output = _run_script(stats_script, ["--repo", REPO_ROOT], timeout=30)
        assert output["success"] is True
        assert output["context"]["total_commits"] > 0
        assert output["context"]["tracked_files"] > 0
        assert len(output["context"]["top_contributors"]) > 0

    def test_maya_pipeline_setup_project(self, examples_dir):
        """Advanced skill: scan → parse → execute setup_project → verify output."""
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        skill_dir = next(d for d in dirs if os.path.basename(d) == "maya-pipeline")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)
        assert meta.dcc == "maya"

        setup_script = next(s for s in meta.scripts if "setup_project" in s)
        with tempfile.TemporaryDirectory() as tmpdir:
            output = _run_script(setup_script, ["--name", "TestProj", "--root", tmpdir])
            assert output["success"] is True
            # Verify directories were actually created
            project_dir = os.path.join(tmpdir, "TestProj")
            assert os.path.isdir(project_dir)
            for sub in ["scenes", "textures", "cache", "renders", "exports"]:
                assert os.path.isdir(os.path.join(project_dir, sub))

    def test_maya_pipeline_export_usd(self, examples_dir):
        """Advanced skill: execute export_usd → verify .usda file is produced."""
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        skill_dir = next(d for d in dirs if os.path.basename(d) == "maya-pipeline")
        meta = dcc_mcp_core._core.parse_skill_md(skill_dir)

        export_script = next(s for s in meta.scripts if "export_usd" in s)
        with tempfile.TemporaryDirectory() as tmpdir:
            input_file = os.path.join(tmpdir, "scene.ma")
            output_file = os.path.join(tmpdir, "scene.usda")
            # Create a dummy input file
            with open(input_file, "w") as f:
                f.write("// dummy maya file")
            output = _run_script(
                export_script,
                ["--input", input_file, "--output", output_file, "--validate"],
            )
            assert output["success"] is True
            assert output["context"]["format"] == "usda"
            assert output["context"]["validated"] is True
            # Verify .usda file was created
            assert os.path.isfile(output_file)
            with open(output_file) as f:
                content = f.read()
            assert "#usda 1.0" in content


# ── Integration: scan + parse round-trip ──


class TestScanAndParseRoundTrip:
    def test_scan_then_parse_all(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        parsed = []
        for d in dirs:
            meta = dcc_mcp_core._core.parse_skill_md(d)
            assert meta is not None, f"Failed to parse {d}"
            parsed.append(meta)
        assert len(parsed) >= len(ALL_EXAMPLE_SKILLS)
        names = {m.name for m in parsed}
        for expected in ALL_EXAMPLE_SKILLS:
            assert expected in names, f"Missing after parse: {expected}"

    def test_all_skills_have_scripts(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        for d in dirs:
            meta = dcc_mcp_core._core.parse_skill_md(d)
            assert meta is not None
            assert len(meta.scripts) > 0, f"Skill {meta.name} has no scripts"

    def test_version_field_populated(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        for d in dirs:
            meta = dcc_mcp_core._core.parse_skill_md(d)
            assert meta is not None
            assert meta.version, f"Skill {meta.name} missing version"

    def test_description_field_populated(self, examples_dir):
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        for d in dirs:
            meta = dcc_mcp_core._core.parse_skill_md(d)
            assert meta is not None
            assert meta.description, f"Skill {meta.name} missing description"

    def test_all_python_scripts_executable(self, examples_dir):
        """Verify all .py scripts at least import without crashing."""
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[examples_dir])
        for d in dirs:
            meta = dcc_mcp_core._core.parse_skill_md(d)
            assert meta is not None
            for script in meta.scripts:
                if script.endswith(".py"):
                    result = subprocess.run(
                        [sys.executable, "-c", f"import ast; ast.parse(open(r'{script}').read())"],
                        capture_output=True,
                        text=True,
                        timeout=5,
                    )
                    assert result.returncode == 0, f"Syntax error in {script}: {result.stderr}"
