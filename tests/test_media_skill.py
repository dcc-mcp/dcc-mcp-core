"""Tests for the bundled vx-backed media skill."""

from __future__ import annotations

from contextlib import contextmanager
from contextlib import suppress
import functools
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys

import pytest

_SKILL_DIR = Path(__file__).parent.parent / "python" / "dcc_mcp_core" / "skills" / "media"
_COMMON = _SKILL_DIR / "scripts" / "_media_common.py"
_STATS = _SKILL_DIR / "scripts" / "_media_image_stats.py"
_SEQUENCE_SCRIPT = _SKILL_DIR / "scripts" / "sequence_to_mp4.py"

# `vx` resolves the ffmpeg build on first use, so finding the launcher on PATH
# does not mean `vx ffmpeg` can run: a CDN or network failure only surfaces
# when a smoke test actually invokes it. Probe once per process and cache the
# answer, so an unavailable ffmpeg skips the smoke test instead of failing the
# lane on infrastructure.
_VX_FFMPEG_PROBE_TIMEOUT_SECS = 90


@functools.lru_cache(maxsize=None)
def _vx_ffmpeg_available():
    """Whether ``vx ffmpeg`` can be provisioned and executed on this host."""
    vx = shutil.which("vx")
    if vx is None:
        return False
    try:
        probe = subprocess.run(
            [vx, "ffmpeg", "-version"],
            capture_output=True,
            timeout=_VX_FFMPEG_PROBE_TIMEOUT_SECS,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return probe.returncode == 0


@contextmanager
def _skill_script_import_context(script_path: Path):
    script_dir = str(script_path.resolve().parent)
    owns_path = script_dir not in sys.path
    if owns_path:
        sys.path.insert(0, script_dir)
    try:
        yield
    finally:
        if owns_path and script_dir in sys.path:
            sys.path.remove(script_dir)


def _load_script_module(module_name: str, script_path: Path):
    spec = importlib.util.spec_from_file_location(module_name, script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    with _skill_script_import_context(script_path):
        spec.loader.exec_module(module)
    return module


@pytest.fixture()
def media_common():
    return _load_script_module("_media_common_under_test", _COMMON)


@pytest.fixture()
def media_stats():
    return _load_script_module("_media_image_stats_under_test", _STATS)


def _write_stub_file(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"stub")


def _write_ppm(path: Path, color, size: int = 16) -> None:
    r, g, b = color
    row = " ".join(f"{channel}" for _ in range(size) for channel in (r, g, b))
    path.write_text(
        f"P3\n{size} {size}\n255\n" + "\n".join(row for _ in range(size)) + "\n",
        encoding="ascii",
    )


def test_media_skill_parseable_and_declares_expected_tools():
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    assert meta.name == "media"
    assert meta.dcc == "python"
    assert {tool.name for tool in meta.tools} == {
        "probe",
        "image_stats",
        "sequence_to_mp4",
        "transcode",
        "extract_frames",
        "thumbnail",
    }


def test_media_skill_discoverable_from_source_skill_path():
    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry

    registry = ToolRegistry()
    catalog = SkillCatalog(registry)
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])

    names = [skill.name for skill in catalog.list_skills()]
    assert "media" in names

    results = catalog.search_skills(query="convert image sequence to mp4", limit=10)
    assert any(result.name == "media" and result.tool_count == 6 for result in results)


def test_media_tool_registers_prefixed_actions_after_load():
    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry

    registry = ToolRegistry()
    catalog = SkillCatalog(registry)
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])
    catalog.load_skill("media")

    action_names = {action["name"] for action in registry.list_actions()}
    assert "media__sequence_to_mp4" in action_names
    assert "media__probe" in action_names


def test_media_read_only_metadata_is_limited_to_probe_and_image_stats():
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    read_only_tools = {tool.name for tool in meta.tools if tool.read_only}

    assert read_only_tools == {"probe", "image_stats"}
    probe_tool = next(tool for tool in meta.tools if tool.name == "probe")
    assert probe_tool.destructive is False
    assert probe_tool.idempotent is True
    image_stats_tool = next(tool for tool in meta.tools if tool.name == "image_stats")
    assert image_stats_tool.destructive is False
    assert image_stats_tool.idempotent is True


def test_sequence_command_uses_vx_ffmpeg_without_shell(media_common, tmp_path):
    frame = tmp_path / "frames" / "frame_0001.png"
    _write_stub_file(frame)
    output = tmp_path / "review.mp4"

    command, resolved_output, source = media_common.build_sequence_to_mp4_command(
        input_dir=str(frame.parent),
        frame_glob="frame_*.png",
        framerate=24,
        output_path=str(output),
        overwrite=False,
    )

    assert command[:2] == ["vx", "ffmpeg"]
    assert "-pattern_type" in command
    assert "glob" in command
    assert "-framerate" in command
    assert "-q:v" in command
    assert "-crf" not in command
    assert source.endswith("frame_*.png")
    assert resolved_output == output
    assert all(isinstance(part, str) for part in command)


def test_sequence_command_uses_crf_for_x264(media_common, tmp_path):
    frame = tmp_path / "frame_0001.png"
    _write_stub_file(frame)

    command, _, _ = media_common.build_sequence_to_mp4_command(
        input_pattern=str(tmp_path / "frame_%04d.png"),
        output_path=str(tmp_path / "out.mp4"),
        codec="libx264",
        quality=23,
        overwrite=False,
    )

    assert "-crf" in command
    assert command[command.index("-crf") + 1] == "23"
    assert "-q:v" not in command


def test_probe_transcode_and_thumbnail_commands_use_fixed_vx_tools(media_common, tmp_path):
    input_file = tmp_path / "clip.mp4"
    _write_stub_file(input_file)

    probe_command = media_common.build_probe_command(str(input_file))
    transcode_command, transcode_output = media_common.build_transcode_command(
        input_path=str(input_file),
        output_path=str(tmp_path / "review.mp4"),
        overwrite=False,
    )
    thumbnail_command, thumbnail_output = media_common.build_thumbnail_command(
        input_path=str(input_file),
        output_path=str(tmp_path / "thumb.png"),
        width=320,
    )

    assert probe_command[:2] == ["vx", "ffprobe"]
    assert transcode_command[:2] == ["vx", "ffmpeg"]
    assert thumbnail_command[:2] == ["vx", "ffmpeg"]
    assert "-c:v" in transcode_command
    assert "mpeg4" in transcode_command
    assert "-q:v" in transcode_command
    assert "-crf" not in transcode_command
    assert "scale=320:-1" in thumbnail_command
    assert transcode_output == tmp_path / "review.mp4"
    assert thumbnail_output == tmp_path / "thumb.png"


def test_sequence_command_rejects_unlisted_codec(media_common, tmp_path):
    frame = tmp_path / "frame_0001.png"
    _write_stub_file(frame)

    with pytest.raises(media_common.MediaToolError) as exc:
        media_common.build_sequence_to_mp4_command(
            input_pattern=str(tmp_path / "frame_%04d.png"),
            output_path=str(tmp_path / "out.mp4"),
            codec="; rm -rf .",
        )

    assert exc.value.code == "invalid_enum"


def test_output_parent_must_exist(media_common, tmp_path):
    frame = tmp_path / "frame_0001.png"
    _write_stub_file(frame)

    with pytest.raises(media_common.MediaToolError) as exc:
        media_common.build_sequence_to_mp4_command(
            input_pattern=str(tmp_path / "frame_%04d.png"),
            output_path=str(tmp_path / "missing" / "out.mp4"),
        )

    assert exc.value.code == "output_parent_missing"


def test_extract_frames_rejects_nested_output_pattern(media_common, tmp_path):
    movie = tmp_path / "in.mp4"
    _write_stub_file(movie)

    with pytest.raises(media_common.MediaToolError) as exc:
        media_common.build_extract_frames_command(
            input_path=str(movie),
            output_dir=str(tmp_path),
            frame_pattern="nested/frame_%04d.png",
        )

    assert exc.value.code == "invalid_path"


def test_extract_frames_rejects_existing_outputs_without_overwrite(media_common, tmp_path):
    movie = tmp_path / "in.mp4"
    existing_frame = tmp_path / "frame_0001.png"
    _write_stub_file(movie)
    _write_stub_file(existing_frame)

    with pytest.raises(media_common.MediaToolError) as exc:
        media_common.build_extract_frames_command(
            input_path=str(movie),
            output_dir=str(tmp_path),
            frame_pattern="frame_%04d.png",
            overwrite=False,
        )

    assert exc.value.code == "output_exists"
    assert exc.value.context["frame_count"] == 1


def test_run_command_reports_missing_vx(media_common):
    with pytest.raises(media_common.MediaToolError) as exc:
        media_common.run_command(["definitely_missing_vx_binary", "ffmpeg", "-version"], 1)

    assert exc.value.code == "vx_not_found"
    assert "possible_solutions" not in exc.value.context


def test_run_command_bootstraps_vx_when_default_vx_is_missing(media_common, tmp_path, monkeypatch):
    downloaded_vx = tmp_path / ("vx.exe" if sys.platform == "win32" else "vx")
    downloaded_vx.write_bytes(b"stub")
    calls = []

    class Completed:
        returncode = 0
        stdout = "ok"
        stderr = ""

    def fake_run(command, **kwargs):
        calls.append(list(command))
        if len(calls) == 1:
            raise FileNotFoundError("vx")
        return Completed()

    monkeypatch.delenv("DCC_MCP_MEDIA_VX_BIN", raising=False)
    monkeypatch.delenv("DCC_MCP_MEDIA_AUTO_INSTALL_VX", raising=False)
    monkeypatch.setattr(media_common, "_download_and_install_vx", lambda: str(downloaded_vx))
    monkeypatch.setattr(media_common.subprocess, "run", fake_run)

    assert media_common.run_command(["vx", "ffmpeg", "-version"], 5) == "ok"
    assert calls[0][0] == "vx"
    assert calls[1][0] == str(downloaded_vx)


def test_run_command_can_disable_vx_bootstrap(media_common, monkeypatch):
    def fake_run(command, **kwargs):
        raise FileNotFoundError("vx")

    monkeypatch.delenv("DCC_MCP_MEDIA_VX_BIN", raising=False)
    monkeypatch.setenv("DCC_MCP_MEDIA_AUTO_INSTALL_VX", "0")
    monkeypatch.setattr(media_common.subprocess, "run", fake_run)

    with pytest.raises(media_common.MediaToolError) as exc:
        media_common.run_command(["vx", "ffmpeg", "-version"], 5)

    assert exc.value.code == "vx_not_found"
    assert "automatic vx bootstrap is disabled" in exc.value.message


def test_probe_does_not_bootstrap_vx_when_marked_read_only(media_common, tmp_path, monkeypatch):
    input_file = tmp_path / "clip.mp4"
    _write_stub_file(input_file)

    def fake_run(command, **kwargs):
        raise FileNotFoundError("vx")

    def fail_bootstrap():
        pytest.fail("read-only probe must not bootstrap vx")

    monkeypatch.delenv("DCC_MCP_MEDIA_VX_BIN", raising=False)
    monkeypatch.delenv("DCC_MCP_MEDIA_AUTO_INSTALL_VX", raising=False)
    monkeypatch.setattr(media_common.subprocess, "run", fake_run)
    monkeypatch.setattr(media_common, "_download_and_install_vx", fail_bootstrap)

    with pytest.raises(media_common.MediaToolError) as exc:
        media_common.probe(str(input_file), timeout_secs=5)

    assert exc.value.code == "vx_not_found"
    assert exc.value.context["allow_auto_install"] is False


def test_vx_bootstrap_uses_official_install_scripts(media_common, monkeypatch):
    monkeypatch.setattr(media_common._vx_bootstrap.sys, "platform", "win32")
    windows_command = media_common._vx_bootstrap.installer_command(media_common.MediaToolError)
    assert windows_command[-1] == "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"

    monkeypatch.setattr(media_common._vx_bootstrap.sys, "platform", "linux")
    linux_command = media_common._vx_bootstrap.installer_command(media_common.MediaToolError)
    assert linux_command == [
        "bash",
        "-lc",
        "curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash",
    ]


def test_download_and_install_vx_runs_installer_and_returns_installed_path(media_common, tmp_path, monkeypatch):
    installed_vx = tmp_path / ("vx.exe" if sys.platform == "win32" else "vx")
    installer_calls = []
    find_calls = []

    class Completed:
        returncode = 0
        stdout = "installed"
        stderr = ""

    def fake_find_vx():
        find_calls.append(True)
        return str(installed_vx) if len(find_calls) > 1 else None

    def fake_run(command, **kwargs):
        installer_calls.append(list(command))
        return Completed()

    monkeypatch.setattr(media_common._vx_bootstrap, "find_vx", fake_find_vx)
    monkeypatch.setattr(media_common._vx_bootstrap, "installer_command", lambda error_cls: ["installer"])
    monkeypatch.setattr(media_common._vx_bootstrap.subprocess, "run", fake_run)

    assert media_common._download_and_install_vx() == str(installed_vx)
    assert installer_calls == [["installer"]]


def test_sequence_entrypoint_import_resolves_sibling_modules_through_runner_context():
    source = _SEQUENCE_SCRIPT.read_text(encoding="utf-8")
    assert "sys.path.insert" not in source
    assert "sys.path.append" not in source
    spec = importlib.util.spec_from_file_location("_sequence_entrypoint_under_test", _SEQUENCE_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    with _skill_script_import_context(_SEQUENCE_SCRIPT):
        spec.loader.exec_module(module)
    assert callable(module.main)


def test_sequence_entrypoint_accepts_stdin_json(media_common, tmp_path, monkeypatch):
    frame = tmp_path / "frame_0001.png"
    _write_stub_file(frame)
    output = tmp_path / "out.mp4"

    def fake_run(command, timeout_secs):
        output.write_bytes(b"not really a movie")
        return ""

    monkeypatch.setattr(media_common, "run_command", fake_run)

    result = media_common.run_tool(
        media_common.sequence_to_mp4,
        {
            "input_pattern": str(tmp_path / "frame_%04d.png"),
            "output_path": str(output),
            "overwrite": True,
        },
    )

    assert result["success"] is True
    assert result["context"]["command"][:2] == ["vx", "ffmpeg"]


@pytest.mark.skipif(
    not _vx_ffmpeg_available(),
    reason="vx cannot provision ffmpeg on this host",
)
def test_sequence_to_mp4_smoke_with_vx(tmp_path):
    _write_ppm(tmp_path / "frame_0001.ppm", (255, 0, 0))
    _write_ppm(tmp_path / "frame_0002.ppm", (0, 255, 0))
    output = tmp_path / "smoke.mp4"

    params = {
        "input_pattern": str(tmp_path / "frame_%04d.ppm"),
        "output_path": str(output),
        "framerate": 1,
        "overwrite": True,
        "timeout_secs": 180,
    }
    result = subprocess.run(
        [sys.executable, str(_SEQUENCE_SCRIPT)],
        input=json.dumps(params),
        capture_output=True,
        text=True,
        timeout=240,
    )
    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    assert payload["success"] is True, json.dumps(payload, indent=2, sort_keys=True)
    assert output.is_file()
    assert output.stat().st_size > 0


# --- image_stats (behavior verification contract, core#2269) -----------------


def test_compute_image_stats_black_frame_is_uniform(media_stats):
    stats = media_stats.compute_image_stats_from_gray(b"\x00" * 16, 4, 4)
    assert stats["mean_luma"] == 0.0
    assert stats["min_luma"] == 0.0
    assert stats["max_luma"] == 0.0
    assert stats["stddev_luma"] == 0.0
    assert stats["uniform"] is True
    assert stats["dominant_bin_fraction"] == 1.0


def test_compute_image_stats_white_frame_is_uniform(media_stats):
    stats = media_stats.compute_image_stats_from_gray(b"\xff" * 16, 4, 4)
    assert stats["mean_luma"] == 1.0
    assert stats["max_luma"] == 1.0
    assert stats["uniform"] is True


def test_compute_image_stats_gradient_is_not_uniform(media_stats):
    # Four pixels spanning the full 0..255 range.
    stats = media_stats.compute_image_stats_from_gray(bytes([0, 85, 170, 255]), 2, 2)
    assert stats["mean_luma"] == pytest.approx(0.5, abs=1e-6)
    assert stats["uniform"] is False
    assert sum(stats["histogram"]) == pytest.approx(1.0, abs=1e-6)
    assert stats["dominant_bin_fraction"] == pytest.approx(0.25, abs=1e-6)


def test_compute_image_stats_rejects_short_frame(media_stats):
    with pytest.raises(media_stats.MediaToolError) as exc:
        media_stats.compute_image_stats_from_gray(b"\x00", 4, 4)
    assert exc.value.code == "short_frame"


def test_image_stats_command_uses_vx_ffmpeg_rawvideo(media_stats, tmp_path):
    input_file = tmp_path / "frame.png"
    _write_stub_file(input_file)

    command, tmp_out, size = media_stats.build_image_stats_command(str(input_file), sample_size=32)
    try:
        assert command[:2] == ["vx", "ffmpeg"]
        assert "rawvideo" in command
        assert "gray" in command
        assert "scale=32:32:flags=area,format=gray" in command
        assert size == 32
        assert tmp_out.name.endswith(".gray")
    finally:
        with suppress(OSError):
            tmp_out.unlink()


def test_image_stats_command_overwrites_the_precreated_output(media_stats, tmp_path):
    """Regression: the output is reserved with mkstemp, so ffmpeg needs -y.

    Without ``-y`` ffmpeg sees an already-existing output path, asks for an
    interactive overwrite confirmation, reads EOF as "no" and exits non-zero -
    which made ``image_stats()`` fail for every valid input. The mocked
    end-to-end test below cannot catch this, so assert it on the argv.
    """
    input_file = tmp_path / "frame.png"
    _write_stub_file(input_file)

    command, tmp_out, _size = media_stats.build_image_stats_command(str(input_file), sample_size=16)
    try:
        assert "-y" in command, "the output path already exists; ffmpeg needs -y to overwrite it"
        # Global options must precede the input so ffmpeg applies them to the output.
        assert command.index("-y") < command.index("-i")
        assert command[-1] == str(tmp_out)
        assert tmp_out.is_file(), "the output path is created up front and must be overwritten"
    finally:
        with suppress(OSError):
            tmp_out.unlink()


def test_image_stats_command_runs_against_real_ffmpeg(media_stats, tmp_path):
    """End-to-end against a real ffmpeg binary when one is available.

    This is the only test that exercises the argv ``build_image_stats_command``
    actually produces: a mocked ``run_command`` writes the output itself and so
    would pass with or without ``-y``. Skipped (not failed) when ffmpeg is not
    installed, since vx installs it on demand at runtime.
    """
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg is None:
        pytest.skip("ffmpeg is not installed; vx installs it on demand at runtime")

    source = tmp_path / "frame.png"
    generated = subprocess.run(
        [
            ffmpeg,
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x64:rate=1:duration=1",
            "-frames:v",
            "1",
            str(source),
        ],
        capture_output=True,
    )
    if generated.returncode != 0 or not source.is_file():
        pytest.skip("the installed ffmpeg cannot synthesize a test frame")

    command, tmp_out, size = media_stats.build_image_stats_command(str(source), sample_size=16)
    try:
        # command is vx-managed (["vx", "ffmpeg", ...]); swap in the real binary.
        assert command[:2] == ["vx", "ffmpeg"]
        executed = subprocess.run([ffmpeg, *command[2:]], capture_output=True)
        assert executed.returncode == 0, executed.stderr.decode("utf-8", "replace")
        assert tmp_out.is_file()
        assert tmp_out.stat().st_size == size * size

        stats = media_stats.compute_image_stats_from_gray(tmp_out.read_bytes(), size, size)
        assert 0.0 <= stats["mean_luma"] <= 1.0
        assert stats["uniform"] is False
    finally:
        with suppress(OSError):
            tmp_out.unlink()


def test_image_stats_end_to_end_with_mocked_ffmpeg(media_stats, tmp_path, monkeypatch):
    input_file = tmp_path / "frame.png"
    _write_stub_file(input_file)
    sample = bytes([128]) * (16 * 16)

    def fake_run(command, timeout_secs, **kwargs):
        out_path = Path(command[-1])
        out_path.write_bytes(sample)
        return ""

    def fake_probe(path, timeout_secs=30):
        return {"context": {"media": {"video": {"width": 1920, "height": 1080}}}}

    monkeypatch.setattr(media_stats, "run_command", fake_run)
    monkeypatch.setattr(media_stats, "probe", fake_probe)

    result = media_stats.image_stats(str(input_file), sample_size=16)

    assert result["success"] is True
    assert result["context"]["width"] == 1920
    assert result["context"]["height"] == 1080
    assert result["context"]["sample_size"] == 16
    stats = result["context"]["stats"]
    assert stats["mean_luma"] == pytest.approx(128 / 255.0, abs=1e-6)
    assert stats["uniform"] is True
    assert abs(sum(stats["histogram"]) - 1.0) < 1e-6
