"""OpenSCAD and Inkscape integration tests for dcc-mcp-core.

Tests the dcc-mcp-core skill/action pipeline against OpenSCAD (CLI renderer)
and Inkscape (headless --actions mode).
All tests are conditionally skipped if the respective binary is not installed.

Run:  pytest -m dcc -k "OpenSCAD or Inkscape"
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
INKSCAPE_BIN = shutil.which("inkscape")

openscad_available = pytest.mark.skipif(OPENSCAD_BIN is None, reason="OpenSCAD not found in PATH")
inkscape_available = pytest.mark.skipif(INKSCAPE_BIN is None, reason="Inkscape not found in PATH")


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
        reg = dcc_mcp_core.ActionRegistry()
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


# ═══════════════════════════════════════════════════════════════════
# INKSCAPE  (headless --actions)
# ═══════════════════════════════════════════════════════════════════


@inkscape_available
class TestInkscapeIntegration:
    """Tests using Inkscape's headless --actions mode.

    Inkscape can run without a GUI via --actions on SVG files.
    We generate SVGs with Python and verify Inkscape transformations.
    """

    _SIMPLE_SVG = textwrap.dedent("""\
        <?xml version="1.0" encoding="UTF-8"?>
        <svg xmlns="http://www.w3.org/2000/svg"
             width="200" height="200" viewBox="0 0 200 200">
          <rect id="rect1" x="10" y="10" width="100" height="80"
                fill="blue" stroke="black" stroke-width="2"/>
          <circle id="circle1" cx="100" cy="100" r="40" fill="red"/>
          <text id="text1" x="50" y="170" font-size="14">DCC MCP</text>
        </svg>
    """)

    def _write_svg(self, path: Path) -> None:
        path.write_text(self._SIMPLE_SVG, encoding="utf-8")

    def test_inkscape_version(self) -> None:
        assert INKSCAPE_BIN is not None
        r = _run_subprocess([INKSCAPE_BIN, "--version"])
        assert r.returncode == 0
        assert "Inkscape" in r.stdout

    def test_inkscape_export_png(self, tmp_path: Path) -> None:
        """Export SVG to PNG via Inkscape headless."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        png_file = tmp_path / "test.png"
        self._write_svg(svg_file)
        r = _run_subprocess([INKSCAPE_BIN, "--export-type=png", f"--export-filename={png_file}", str(svg_file)])
        assert r.returncode == 0
        assert png_file.is_file()
        assert png_file.stat().st_size > 0

    def test_inkscape_export_pdf(self, tmp_path: Path) -> None:
        """Export SVG to PDF via Inkscape headless."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        pdf_file = tmp_path / "test.pdf"
        self._write_svg(svg_file)
        r = _run_subprocess([INKSCAPE_BIN, "--export-type=pdf", f"--export-filename={pdf_file}", str(svg_file)])
        assert r.returncode == 0
        assert pdf_file.is_file()
        assert pdf_file.stat().st_size > 0

    def test_inkscape_export_specific_area(self, tmp_path: Path) -> None:
        """Export a specific area of the SVG."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        png_file = tmp_path / "area.png"
        self._write_svg(svg_file)
        r = _run_subprocess(
            [
                INKSCAPE_BIN,
                "--export-type=png",
                "--export-area=0:0:100:100",
                f"--export-filename={png_file}",
                str(svg_file),
            ]
        )
        assert r.returncode == 0
        assert png_file.is_file()

    def test_inkscape_query_dimensions(self, tmp_path: Path) -> None:
        """Query SVG document dimensions."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        self._write_svg(svg_file)
        r = _run_subprocess(
            [
                INKSCAPE_BIN,
                "--query-all",
                str(svg_file),
            ]
        )
        assert r.returncode == 0
        assert "rect1" in r.stdout
        assert "circle1" in r.stdout

    def test_inkscape_svg_optimized_output(self, tmp_path: Path) -> None:
        """Use Inkscape to produce a plain SVG (optimized)."""
        assert INKSCAPE_BIN is not None
        svg_in = tmp_path / "test.svg"
        svg_out = tmp_path / "optimized.svg"
        self._write_svg(svg_in)
        r = _run_subprocess(
            [
                INKSCAPE_BIN,
                "--export-type=svg",
                f"--export-filename={svg_out}",
                "--export-plain-svg",
                str(svg_in),
            ]
        )
        assert r.returncode == 0
        assert svg_out.is_file()
        content = svg_out.read_text(encoding="utf-8")
        assert "<svg" in content


@inkscape_available
class TestInkscapeSkillPipeline:
    """dcc-mcp-core skill pipeline with Inkscape as target DCC."""

    def test_register_inkscape_actions(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="export_png", description="Export SVG to PNG", dcc="inkscape")
        reg.register(name="export_pdf", description="Export SVG to PDF", dcc="inkscape")
        reg.register(name="optimize_svg", description="Produce plain SVG", dcc="inkscape")
        reg.register(name="query_bounds", description="Query element bounding boxes", dcc="inkscape")
        inkscape_actions = reg.list_actions(dcc_name="inkscape")
        assert len(inkscape_actions) == 4

    def test_inkscape_skill_metadata(self, tmp_path: Path) -> None:
        from conftest import create_skill_dir

        create_skill_dir(
            str(tmp_path),
            "inkscape-vector-tools",
            frontmatter=(
                "name: inkscape-vector-tools\ndcc: inkscape\ntags:\n  - svg\n  - vector\n  - 2d\nversion: 1.0.0"
            ),
        )
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path / "inkscape-vector-tools"))
        assert meta is not None
        assert meta.dcc == "inkscape"
        assert "svg" in meta.tags
        assert "2d" in meta.tags

    def test_inkscape_action_result(self, tmp_path: Path) -> None:
        png_path = tmp_path / "output.png"
        result = dcc_mcp_core.success_result(
            "Exported SVG to PNG",
            source_svg="test.svg",
            output_png=str(png_path),
            width=200,
            height=200,
            dcc="inkscape",
        )
        assert result.success is True
        assert result.context["width"] == 200
        assert result.context["dcc"] == "inkscape"

    def test_inkscape_generate_svg_pipeline(self, tmp_path: Path) -> None:
        """Generate SVG with Python, export with Inkscape, verify file."""
        assert INKSCAPE_BIN is not None
        svg_content = textwrap.dedent("""\
            <?xml version="1.0" encoding="UTF-8"?>
            <svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
              <rect x="10" y="10" width="80" height="80" fill="green"/>
            </svg>
        """)
        svg_file = tmp_path / "generated.svg"
        svg_file.write_text(svg_content, encoding="utf-8")
        png_file = tmp_path / "generated.png"
        r = _run_subprocess([INKSCAPE_BIN, "--export-type=png", f"--export-filename={png_file}", str(svg_file)])
        result = dcc_mcp_core.validate_action_result(
            {"success": r.returncode == 0, "message": "Inkscape export", "context": {"file": str(png_file)}}
        )
        assert result.success is True
