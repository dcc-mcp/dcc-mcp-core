"""Tests for the bundled vx-backed media skill."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys

import pytest

_SKILL_DIR = Path(__file__).parent.parent / "python" / "dcc_mcp_core" / "skills" / "media"
_COMMON = _SKILL_DIR / "scripts" / "_media_common.py"
_SEQUENCE_SCRIPT = _SKILL_DIR / "scripts" / "sequence_to_mp4.py"


@pytest.fixture()
def media_common():
    spec = importlib.util.spec_from_file_location("_media_common_under_test", _COMMON)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def _write_stub_file(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"stub")


def _write_ppm(path: Path, color) -> None:
    r, g, b = color
    path.write_text(
        f"P3\n2 2\n255\n{r} {g} {b}  {r} {g} {b}\n{r} {g} {b}  {r} {g} {b}\n",
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
    assert any(result.name == "media" and result.tool_count == 5 for result in results)


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


def test_media_read_only_metadata_is_limited_to_probe():
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    read_only_tools = {tool.name for tool in meta.tools if tool.read_only}

    assert read_only_tools == {"probe"}
    probe_tool = next(tool for tool in meta.tools if tool.name == "probe")
    assert probe_tool.destructive is False
    assert probe_tool.idempotent is True


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


def test_sequence_entrypoint_import_resolves_sibling_modules():
    spec = importlib.util.spec_from_file_location("_sequence_entrypoint_under_test", _SEQUENCE_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
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


@pytest.mark.skipif(shutil.which("vx") is None, reason="vx is not available")
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
    assert payload["success"] is True, payload
    assert output.is_file()
    assert output.stat().st_size > 0
