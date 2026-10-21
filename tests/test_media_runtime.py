"""Deterministic local runtime selection; no native executable or network is run."""

from __future__ import annotations

import importlib
import os
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).parents[1] / "python/dcc_mcp_core/skills/media/scripts"


@pytest.fixture
def runtime(monkeypatch, tmp_path):
    monkeypatch.syspath_prepend(str(SCRIPTS))
    module = importlib.import_module("_media_runtime")
    monkeypatch.setenv("VX_HOME", str(tmp_path / "vx"))
    monkeypatch.setenv("PATH", str(tmp_path / "path"))
    for name in (module.FFMPEG_ENV, module.FFPROBE_ENV):
        monkeypatch.delenv(name, raising=False)
    return module


@pytest.fixture
def error(runtime):
    from _media_common import MediaToolError

    return MediaToolError


def pair(directory, header=b"\x7fELF"):
    directory.mkdir(parents=True, exist_ok=True)
    result = []
    for tool in ("ffmpeg", "ffprobe"):
        path = directory / (tool + (".exe" if os.name == "nt" else ""))
        path.write_bytes(header + b" synthetic fixture, never executed")
        path.chmod(0o700)
        result.append(path)
    return result


def test_explicit_beats_vx_and_path(runtime, error, tmp_path, monkeypatch):
    explicit, probe = pair(tmp_path / "with spaces ; no shell")
    pair(tmp_path / "vx/store/ffmpeg/8.1/bin")
    pair(tmp_path / "path")
    monkeypatch.setenv(runtime.FFMPEG_ENV, str(explicit))
    actual = runtime.resolve_runtime(error)
    assert actual["ffmpeg"]["path"] == str(explicit)
    assert actual["ffprobe"]["path"] == str(probe)
    assert actual["ffmpeg"]["source"] == "environment"


def test_vx_store_beats_path_without_vx_launcher(runtime, error, tmp_path):
    encoder, _ = pair(tmp_path / "vx/store/ffmpeg/8.1/bin")
    pair(tmp_path / "path")
    actual = runtime.resolve_runtime(error)
    assert actual["ffmpeg"]["path"] == str(encoder)
    assert actual["ffmpeg"]["source"] == "vx_store"


def test_numeric_version_order_and_project_pin_limitation(runtime, error, tmp_path):
    pair(tmp_path / "vx/store/ffmpeg/9.1/bin")
    encoder, _ = pair(tmp_path / "vx/store/ffmpeg/10.1/bin")
    pair(tmp_path / "vx/store/ffmpeg/99.1-preview/bin")
    assert runtime.resolve_runtime(error)["ffmpeg"]["path"] == str(encoder)


def test_platform_directory(runtime, error, tmp_path, monkeypatch):
    monkeypatch.setattr(runtime.platform, "system", lambda: "Linux")
    monkeypatch.setattr(runtime.platform, "machine", lambda: "x86_64")
    encoder, _ = pair(tmp_path / "vx/store/ffmpeg/8.1/linux-x64/bin")
    assert runtime.resolve_runtime(error)["ffmpeg"]["path"] == str(encoder)


def test_path_fallback_when_vx_has_no_complete_pair(runtime, error, tmp_path):
    broken = tmp_path / "vx/store/ffmpeg/8.1/bin"
    _, probe = pair(broken)
    probe.unlink()
    expected, _ = pair(tmp_path / "path")
    assert runtime.resolve_runtime(error)["ffmpeg"]["path"] == str(expected)


def test_missing_all_sources_is_actionable(runtime, error):
    with pytest.raises(error) as caught:
        runtime.resolve_runtime(error)
    assert caught.value.code == "command_unavailable"
    assert "No installation was attempted" in caught.value.prompt


@pytest.mark.parametrize("value", ["", "relative", "/nonexistent/ffmpeg"])
def test_invalid_override_never_falls_back(runtime, error, tmp_path, monkeypatch, value):
    pair(tmp_path / "path")
    monkeypatch.setenv(runtime.FFMPEG_ENV, value)
    with pytest.raises(error) as caught:
        runtime.resolve_runtime(error)
    assert caught.value.code == "invalid_runtime_override"


def test_invalid_probe_override_never_falls_back(runtime, error, tmp_path, monkeypatch):
    pair(tmp_path / "path")
    monkeypatch.setenv(runtime.FFPROBE_ENV, "")
    with pytest.raises(error, match="FFPROBE"):
        runtime.resolve_runtime(error)


def test_missing_sibling_does_not_mix_path_installations(runtime, error, tmp_path, monkeypatch):
    encoder, probe = pair(tmp_path / "explicit")
    probe.unlink()
    pair(tmp_path / "path")
    monkeypatch.setenv(runtime.FFMPEG_ENV, str(encoder))
    with pytest.raises(error, match="pair"):
        runtime.resolve_runtime(error)


def test_explicit_probe_can_be_in_separate_directory(runtime, error, tmp_path, monkeypatch):
    encoder, _ = pair(tmp_path / "encoder")
    _, probe = pair(tmp_path / "probe")
    monkeypatch.setenv(runtime.FFMPEG_ENV, str(encoder))
    monkeypatch.setenv(runtime.FFPROBE_ENV, str(probe))
    assert runtime.resolve_runtime(error)["ffprobe"]["path"] == str(probe)


@pytest.mark.parametrize("header", [b"\x7fELF", b"MZ\x90\0", b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe"])
def test_supported_native_headers(runtime, error, tmp_path, header):
    pair(tmp_path / "path", header)
    assert runtime.resolve_runtime(error)["ffmpeg"]["source"] == "PATH"


def test_script_wrapper_rejected_without_execution(runtime, error, tmp_path):
    pair(tmp_path / "path", b"#!/bin/sh\nvx ffmpeg")
    with pytest.raises(error):
        runtime.resolve_runtime(error)


def test_version_build_and_library_pairing(runtime, error, tmp_path):
    pair(tmp_path / "path")
    selected = runtime.resolve_runtime(error)

    def execute(argv):
        return Path(argv[0]).stem + " version 8.1 build\nconfiguration: --enable-gpl\nlibavcodec 62.0 / 62.0\n"

    runtime.inspect_versions(selected, execute, error)
    assert selected["ffmpeg"]["build_configuration"] == "configuration: --enable-gpl"


@pytest.mark.parametrize("different", ["version", "configuration", "library"])
def test_mismatched_pair_fails(runtime, error, tmp_path, different):
    pair(tmp_path / "path")
    selected = runtime.resolve_runtime(error)

    def execute(argv):
        tool = Path(argv[0]).stem
        version, config, library = "8.1", "--enable-gpl", "62"
        if tool == "ffprobe":
            if different == "version":
                version = "7.1"
            elif different == "configuration":
                config = "--disable-gpl"
            else:
                library = "61"
        return f"{tool} version {version}\nconfiguration: {config}\nlibavcodec {library}\n"

    with pytest.raises(error) as caught:
        runtime.inspect_versions(selected, execute, error)
    assert caught.value.code == "runtime_mismatch"


def test_preflight_checks_exact_encoder_names_without_external_process(runtime, error, tmp_path, monkeypatch):
    import check_runtime

    pair(tmp_path / "path")
    calls = []

    def execute(argv, deadline):
        calls.append(argv)
        if "-version" in argv:
            return {"stdout": Path(argv[0]).stem + " version 8.1\n"}
        return {"stdout": " A..... pcm_f32le PCM\n A..... libvorbis_fake Not Vorbis\n"}

    monkeypatch.setattr(check_runtime, "run_audio_command", execute)
    result = check_runtime.main(required_encoders=["libvorbis"])
    assert result["error"] == "encoder_unavailable"
    assert len(calls) == 3 and all("vx" not in Path(call[0]).name for call in calls)
    assert result["context"]["executables"]["ffmpeg"]["sha256"]
    assert check_runtime.main(required_encoders=["pcm_f32le"])["success"]


@pytest.mark.parametrize("required", ["libvorbis", ["; shell"], [1], ["a"] * 33])
def test_preflight_rejects_bad_encoder_requests(runtime, required):
    import check_runtime

    assert check_runtime.main(required_encoders=required)["error"] == "invalid_input"


def test_known_native_vx_shim_is_not_executed(runtime, error, tmp_path, monkeypatch):
    encoder, _ = pair(tmp_path / "vx/bin", b"MZ\x90\0")
    monkeypatch.setenv(runtime.FFMPEG_ENV, str(encoder))
    with pytest.raises(error) as caught:
        runtime.resolve_runtime(error)
    assert caught.value.code == "invalid_runtime_override"


def test_store_symlink_cannot_mislabel_an_external_binary(runtime, error, tmp_path):
    encoder, _ = pair(tmp_path / "path")
    directory = tmp_path / "vx/store/ffmpeg/8.1/bin"
    fake, _ = pair(directory)
    fake.unlink()
    try:
        fake.symlink_to(encoder)
    except OSError:
        pytest.skip("Symlink creation unavailable")
    assert runtime.resolve_runtime(error)["ffmpeg"]["source"] == "PATH"


def test_preflight_rejects_boolean_timeout(runtime):
    import check_runtime

    assert check_runtime.main(timeout_secs=True)["error"] == "invalid_input"


def test_probe_override_completes_vx_pair_before_path_fallback(runtime, error, tmp_path, monkeypatch):
    encoder, missing_probe = pair(tmp_path / "vx/store/ffmpeg/8.1/bin")
    missing_probe.unlink()
    _, probe = pair(tmp_path / "separate probe")
    pair(tmp_path / "path")
    monkeypatch.setenv(runtime.FFPROBE_ENV, str(probe))
    selected = runtime.resolve_runtime(error)
    assert selected["ffmpeg"]["path"] == str(encoder)
    assert selected["ffmpeg"]["selection_policy"] == "latest_numeric_installed_no_project_pins"
    assert selected["ffprobe"]["source"] == "environment"
    assert selected["ffprobe"]["store_version"] is None


def test_unreadable_optional_vx_store_falls_back(runtime, error, tmp_path, monkeypatch):
    pair(tmp_path / "vx/store/ffmpeg/8.1/bin")
    encoder, _ = pair(tmp_path / "path")

    def denied(path):
        raise PermissionError("synthetic unreadable store")

    monkeypatch.setattr(runtime.os, "scandir", denied)
    assert runtime.resolve_runtime(error)["ffmpeg"]["path"] == str(encoder)


def test_equal_numeric_versions_have_stable_tiebreak(runtime, error, tmp_path):
    pair(tmp_path / "vx/store/ffmpeg/08.1/bin")
    encoder, _ = pair(tmp_path / "vx/store/ffmpeg/8.1/bin")
    assert runtime.resolve_runtime(error)["ffmpeg"]["path"] == str(encoder)


def test_oversized_optional_store_falls_back_without_partial_choice(runtime, error, tmp_path, monkeypatch):
    from contextlib import contextmanager
    from types import SimpleNamespace

    pair(tmp_path / "vx/store/ffmpeg/8.1/bin")
    encoder, _ = pair(tmp_path / "path")

    @contextmanager
    def oversized(path):
        yield iter([SimpleNamespace(name="8.1", is_dir=lambda: True)] * 129)

    monkeypatch.setattr(runtime.os, "scandir", oversized)
    assert runtime.resolve_runtime(error)["ffmpeg"]["path"] == str(encoder)


def test_preflight_detects_changed_runtime(runtime, tmp_path, monkeypatch):
    import check_runtime

    encoder, _ = pair(tmp_path / "path")

    def execute(argv, deadline):
        if "-version" in argv:
            return {"stdout": Path(argv[0]).stem + " version 8.1\n"}
        encoder.write_bytes(b"\x7fELF changed")
        return {"stdout": " A..... pcm_f32le PCM\n"}

    monkeypatch.setattr(check_runtime, "run_audio_command", execute)
    assert check_runtime.main(required_encoders=["pcm_f32le"])["error"] == "executable_changed"


def test_encoder_parser_rejects_legend_and_keeps_hyphenated_names(runtime):
    assert runtime.encoder_names(" A..... = Audio\n V..... libvpx-vp9 VP9\n V..... libaom-av1 AV1\n") == {
        "libvpx-vp9",
        "libaom-av1",
    }


def test_preflight_accepts_legitimate_hyphenated_encoder(runtime, tmp_path, monkeypatch):
    import check_runtime

    pair(tmp_path / "path")

    def execute(argv, deadline):
        if "-version" in argv:
            return {"stdout": Path(argv[0]).stem + " version 8.1\n"}
        return {"stdout": " V..... libvpx-vp9 VP9\n"}

    monkeypatch.setattr(check_runtime, "run_audio_command", execute)
    assert check_runtime.main(required_encoders=["libvpx-vp9"])["success"]
