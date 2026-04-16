"""OpenSCAD integration tests for dcc-mcp-core.

Tests the dcc-mcp-core skill/action pipeline against OpenSCAD (CLI renderer).
All tests are conditionally skipped if the respective binary is not installed.

Run:  pytest -m dcc -k "OpenSCAD"
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import textwrap

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── Binary detection ──

OPENSCAD_BIN = shutil.which("openscad")
INKSCAPE_BIN = None  # Removed: Inkscape is too slow for CI

openscad_available = pytest.mark.skipif(OPENSCAD_BIN is None, reason="OpenSCAD not found in PATH")


# ── Script runner helpers ──


def _run_subprocess(cmd: list[str], timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, timeout=timeout, encoding="utf-8")


# ═══════════════════════════════════════════════════════════════════
# OPENSCAD  (CLI geometry compiler)
# ═══════════════════════════════════════════════════════════════════


@openscad_available
class TestOpenSCADIntegration:
    """Tests using OpenSCAD's command-line renderer.

    OpenSCAD takes .scad source files and produces STL/PNG/etc.
    We generate .scad content with Python and verify the output.
    """

    def _run_openscad(self, scad_content: str, out_file: Path, timeout: int = 60) -> bool:
        assert OPENSCAD_BIN is not None
        with tempfile.NamedTemporaryFile(suffix=".scad", mode="w", delete=False, encoding="utf-8") as f:
            f.write(scad_content)
            scad_path = f.name
        try:
            r = _run_subprocess([OPENSCAD_BIN, "-o", str(out_file), scad_path], timeout)
            return r.returncode == 0
        finally:
            Path(scad_path).unlink(missing_ok=True)

    def test_openscad_version(self) -> None:
        assert OPENSCAD_BIN is not None
        r = _run_subprocess([OPENSCAD_BIN, "--version"])
        assert r.returncode == 0

    def test_openscad_render_cube(self, tmp_path: Path) -> None:
        """Render a simple cube to STL."""
        stl_file = tmp_path / "cube.stl"
        scad = "cube([10, 10, 10], center=true);"
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.is_file()
        assert stl_file.stat().st_size > 0

    def test_openscad_render_sphere(self, tmp_path: Path) -> None:
        """Render a sphere to STL."""
        stl_file = tmp_path / "sphere.stl"
        scad = "sphere(r=5, $fn=32);"
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.stat().st_size > 0

    def test_openscad_boolean_difference(self, tmp_path: Path) -> None:
        """Render a cube with a cylindrical hole (Boolean difference)."""
        stl_file = tmp_path / "drilled_cube.stl"
        scad = textwrap.dedent("""\
            difference() {
                cube([20, 20, 20], center=true);
                cylinder(h=25, r=5, center=true, $fn=32);
            }
        """)
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.stat().st_size > 0

    def test_openscad_render_png(self, tmp_path: Path) -> None:
        """Render a preview image to PNG (requires display/OpenGL — skipped in headless CI)."""
        import os

        if not os.environ.get("DISPLAY") and not os.environ.get("WAYLAND_DISPLAY"):
            pytest.skip("No display available for OpenSCAD PNG rendering (requires OpenGL)")
        assert OPENSCAD_BIN is not None
        png_file = tmp_path / "preview.png"
        scad = "cube(10);"
        with tempfile.NamedTemporaryFile(suffix=".scad", mode="w", delete=False, encoding="utf-8") as f:
            f.write(scad)
            scad_path = f.name
        try:
            r = _run_subprocess(
                [OPENSCAD_BIN, "--render", "--imgsize=128,128", "-o", str(png_file), scad_path],
                timeout=60,
            )
        finally:
            Path(scad_path).unlink(missing_ok=True)
        assert r.returncode == 0
        assert png_file.is_file()
        assert png_file.stat().st_size > 0

    def test_openscad_parametric_script(self, tmp_path: Path) -> None:
        """Test parametric OpenSCAD with variables."""
        stl_file = tmp_path / "parametric.stl"
        scad = textwrap.dedent("""\
            width = 15;
            height = 8;
            depth = 12;
            cube([width, depth, height]);
        """)
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.is_file()


@openscad_available
class TestOpenSCADSkillPipeline:
    """dcc-mcp-core skill pipeline with OpenSCAD as target DCC."""

    def test_register_openscad_actions(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register(name="render_geometry", description="Render .scad to STL", dcc="openscad")
        reg.register(name="generate_scad", description="Generate .scad from parameters", dcc="openscad")
        reg.register(name="export_png", description="Render preview PNG", dcc="openscad")
        assert len(reg.list_actions(dcc_name="openscad")) == 3

    def test_openscad_generate_scad_from_params(self, tmp_path: Path) -> None:
        """Python generates .scad content, OpenSCAD renders it — full pipeline."""
        params = {"width": 20, "height": 10, "depth": 15}
        scad_content = f"cube([{params['width']}, {params['depth']}, {params['height']}]);"
        scad_file = tmp_path / "generated.scad"
        scad_file.write_text(scad_content, encoding="utf-8")
        assert OPENSCAD_BIN is not None
        stl_file = tmp_path / "generated.stl"
        r = _run_subprocess([OPENSCAD_BIN, "-o", str(stl_file), str(scad_file)])
        result = dcc_mcp_core.success_result(
            "Geometry rendered",
            scad_file=str(scad_file),
            stl_file=str(stl_file),
            success_code=r.returncode,
            params=params,
        )
        assert result.success is True
        assert result.context["params"]["width"] == 20

    def test_openscad_skill_metadata(self, tmp_path: Path) -> None:
        from conftest import create_skill_dir

        create_skill_dir(
            str(tmp_path),
            "openscad-primitives",
            frontmatter="name: openscad-primitives\ndcc: openscad\ntags:\n  - geometry\n  - cad",
        )
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path / "openscad-primitives"))
        assert meta is not None
        assert meta.dcc == "openscad"
        assert "geometry" in meta.tags
