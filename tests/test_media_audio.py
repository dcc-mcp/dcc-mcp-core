"""Host-free tests for typed audio transcoding and bounded process ownership."""

from __future__ import annotations

import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

import pytest

SCRIPTS = Path(__file__).parents[1] / "python/dcc_mcp_core/skills/media/scripts"


@pytest.fixture
def audio(monkeypatch):
    """Load the standalone skill helpers without a DCC or native encoder."""
    monkeypatch.syspath_prepend(str(SCRIPTS))
    return importlib.import_module("_media_audio")


@pytest.fixture
def files(tmp_path):
    """Prepare tiny local inputs, not real media assets."""
    source = tmp_path / "source.wav"
    source.write_bytes(b"source-master")
    return source, tmp_path / "output.ogg"


class FakeRuntime:
    """Deterministic finite command responses, with no native media process."""

    def __init__(self, tmp_path, source, output):
        self.source, self.output = source, output
        self.paths = {}
        for tool in ("ffmpeg", "ffprobe"):
            path = tmp_path / tool
            path.write_bytes(tool.encode())
            self.paths[tool] = path
        self.calls = []
        self.encoder_available = True
        self.resolution_source = "vx"
        self.input_frames = self.output_frames = 1536000
        self.output_rate = 48000
        self.output_channels = 2
        self.output_codec = "vorbis"
        self.output_streams = 1
        self.failure = None
        self.mutate_source = False
        self.mutate_executable = False
        self.invalid_decode = False

    def __call__(self, command, deadline, *, pcm=False):
        assert len(self.calls) < 16  # A runaway loop cannot grow an unbounded mock log.
        self.calls.append(command)
        result = {"stdout": "", "decoded_bytes": 0, "decoded_sha256": "a" * 64}
        if "where" in command:
            request = command[-1]
            tool = "ffprobe" if request == "ffmpeg::ffprobe" else "ffmpeg"
            result["stdout"] = json.dumps(
                {"path": str(self.paths[tool]), "tool": request, "source": self.resolution_source}
            )
        elif "-version" in command:
            result["stdout"] = Path(command[0]).name + " version test-build\n"
        elif "-encoders" in command:
            result["stdout"] = " A..... pcm_f32le PCM\n A..... pcm_s16le PCM\n"
            if self.encoder_available:
                result["stdout"] += " A..... libvorbis Vorbis\n"
        elif "-show_streams" in command:
            is_output = command[-1] == str(self.output)
            stream = {
                "codec_type": "audio",
                "codec_name": self.output_codec if is_output else "pcm_s16le",
                "sample_rate": str(self.output_rate if is_output else 48000),
                "channels": self.output_channels if is_output else 2,
                "channel_layout": "stereo",
            }
            result["stdout"] = json.dumps(
                {
                    "streams": [stream] * (self.output_streams if is_output else 1),
                    "format": {"format_name": "ogg" if is_output else "wav"},
                }
            )
        elif pcm:
            is_output = command[command.index("-i") + 1] == str(self.output)
            result["decoded_bytes"] = (self.output_frames if is_output else self.input_frames) * 8
            if self.invalid_decode and is_output:
                result["decoded_bytes"] += 1
        else:
            assert "-c:a" in command and command[-1] == str(self.output)
            self.output.write_bytes(b"encoded")
            if self.mutate_source:
                self.source.write_bytes(b"changed")
            if self.mutate_executable:
                self.paths["ffmpeg"].write_bytes(b"changed encoder")
            if self.failure:
                raise self.failure
        return result


@pytest.fixture
def runtime(audio, files, tmp_path, monkeypatch):
    fake = FakeRuntime(tmp_path, *files)
    monkeypatch.setattr(audio, "run_audio_command", fake)
    return fake


def invoke(audio, files, **kwargs):
    """Use the same envelope path as the script entry point."""
    from _media_common import run_tool

    return run_tool(
        audio.transcode_audio,
        dict(input_path=str(files[0]), output_path=str(files[1]), container="ogg", audio_codec="libvorbis", **kwargs),
    )


def test_audio_builder_is_explicit_and_preserves_stream_dimensions(audio, files):
    command, source, output = audio.build_audio_command(str(files[0]), str(files[1]), "ogg", "libvorbis")
    assert command[:2] == ["vx", "ffmpeg"]
    assert command[command.index("-map") + 1] == "0:a:0"
    assert command[command.index("-q:a") + 1] == "5"
    assert all(flag in command for flag in ("-vn", "-sn", "-dn", "-n", "-nostdin"))
    assert not any(flag in command for flag in ("-ar", "-ac", "-af", "-c:v", "-y", "-t"))
    assert source == files[0] and output == files[1]


@pytest.mark.parametrize(
    "container,codec,suffix,quality",
    [
        ("ogg", "pcm_s16le", ".ogg", None),
        ("wav", "libvorbis", ".wav", None),
        ("mp3", "libmp3lame", ".mp3", None),
        ("ogg", "libvorbis", ".wav", None),
        ("wav", "pcm_s16le", ".wav", 5),
        ("ogg", "libvorbis", ".ogg", float("nan")),
        ("ogg", "libvorbis", ".ogg", float("inf")),
        ("ogg", "libvorbis", ".ogg", 11),
        ("ogg", "libvorbis", ".ogg", -2),
        ("ogg", "libvorbis", ".ogg", True),
        (None, "pcm_s16le", ".wav", None),
        ("wav", None, ".wav", None),
    ],
)
def test_invalid_contract_rejected_without_process(audio, files, runtime, container, codec, suffix, quality):
    with pytest.raises(audio.MediaToolError):
        audio.build_audio_command(str(files[0]), str(files[1].with_suffix(suffix)), container, codec, quality)
    assert runtime.calls == []


def test_wav_builder_has_no_quality_or_video_options(audio, files):
    command, _, _ = audio.build_audio_command(str(files[0]), str(files[1].with_suffix(".wav")), "wav", "pcm_s16le")
    assert "-q:a" not in command and "-c:v" not in command


@pytest.mark.parametrize("timeout", [0, 3601, True, float("inf"), float("nan"), 1.5])
def test_invalid_timeout_rejected_without_process(audio, files, runtime, timeout):
    assert not invoke(audio, files, timeout_secs=timeout)["success"]
    assert runtime.calls == []


def test_existing_output_and_source_alias_are_never_overwritten(audio, files, runtime):
    files[1].write_bytes(b"existing")
    assert invoke(audio, files)["error"] == "output_exists"
    with pytest.raises(audio.MediaToolError):
        audio.build_audio_command(str(files[0]), str(files[0]), "wav", "pcm_s16le")
    assert files[1].read_bytes() == b"existing" and files[0].read_bytes() == b"source-master"
    assert not runtime.calls


def test_success_receipt_binds_hashes_metadata_decode_counts_and_exact_argv(audio, files, runtime):
    result = invoke(audio, files)
    assert result["success"], result
    receipt = result["context"]
    assert receipt["accepted"] is True and receipt["output_status"] == "verified"
    assert receipt["input_sha256"] == receipt["input_sha256_after"] == hashlib.sha256(b"source-master").hexdigest()
    assert receipt["output_sha256"] == hashlib.sha256(b"encoded").hexdigest()
    assert receipt["input_decode"]["sample_frames"] == receipt["output_decode"]["sample_frames"] == 1536000
    assert receipt["output_decode"]["duration_seconds"] == 32
    assert receipt["commands"] == runtime.calls
    for tool, path in runtime.paths.items():
        assert receipt["executables"][tool]["sha256"] == hashlib.sha256(path.read_bytes()).hexdigest()
    encoding = next(command for command in runtime.calls if "-n" in command)
    assert encoding[0] == str(runtime.paths["ffmpeg"])
    assert files[0].read_bytes() == b"source-master"


@pytest.mark.parametrize("source", ["system", "global_package", "detected", "not_found"])
def test_unmanaged_resolution_fails_closed_without_encoding(audio, files, runtime, source):
    runtime.resolution_source = source
    assert invoke(audio, files)["error"] == "untrusted_executable"
    assert not files[1].exists() and len(runtime.calls) == 1


def test_unavailable_encoder_fails_before_source_decode_or_encoding(audio, files, runtime):
    runtime.encoder_available = False
    result = invoke(audio, files)
    assert result["error"] == "encoder_unavailable"
    assert not files[1].exists()
    assert all("-i" not in call for call in runtime.calls)
    assert result["context"]["output_status"] == "not_created"


@pytest.mark.parametrize("failure", ["timeout", "command_failed"])
def test_partial_failure_retains_receipt_without_retry_or_acceptance(audio, files, runtime, failure):
    runtime.failure = (
        subprocess.TimeoutExpired(["fake"], 1)
        if failure == "timeout"
        else audio.MediaToolError("Encoder failed", failure)
    )
    result = invoke(audio, files)
    assert result["error"] == failure
    assert result["context"]["output_status"] == "unverified_partial"
    assert not result["context"]["accepted"]
    assert sum("-n" in call for call in runtime.calls) == 1
    assert files[0].read_bytes() == b"source-master" and files[1].read_bytes() == b"encoded"


def test_missing_128_samples_are_rejected_without_padding(audio, files, runtime):
    runtime.output_frames -= 128
    result = invoke(audio, files)
    assert result["error"] == "sample_count_mismatch"
    assert result["context"]["output_decode"]["sample_frames"] == 1535872
    assert result["context"]["output_status"] == "unverified_partial"
    assert sum("-n" in call for call in runtime.calls) == 1


@pytest.mark.parametrize(
    "field,value,error",
    [
        ("output_rate", 44100, "audio_contract_mismatch"),
        ("output_channels", 1, "audio_contract_mismatch"),
        ("output_codec", "aac", "audio_contract_mismatch"),
        ("output_streams", 2, "audio_contract_mismatch"),
        ("mutate_source", True, "source_changed"),
        ("mutate_executable", True, "executable_changed"),
        ("invalid_decode", True, "invalid_audio_decode"),
    ],
)
def test_verification_failures_are_not_accepted(audio, files, runtime, field, value, error):
    setattr(runtime, field, value)
    result = invoke(audio, files)
    assert result["error"] == error, result
    assert not result["context"]["accepted"]


def test_pcm_stream_is_counted_without_buffering(audio):
    from _media_audio_process import run_audio_command

    command = [sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'x' * 2097152)"]
    result = run_audio_command(command, time.monotonic() + 10, pcm=True)
    assert result["stdout"] == "" and result["decoded_bytes"] == 2097152
    assert result["decoded_sha256"] == hashlib.sha256(b"x" * 2097152).hexdigest()


def test_text_capture_is_bounded(audio):
    from _media_audio_process import run_audio_command

    command = [sys.executable, "-c", "import sys; sys.stdout.buffer.write(b'x' * 2097152)"]
    with pytest.raises(audio.MediaToolError, match="capture was incomplete"):
        run_audio_command(command, time.monotonic() + 10)


def test_stderr_tail_is_bounded(audio):
    from _media_audio_process import run_audio_command

    command = [sys.executable, "-c", "import sys; sys.stderr.write('x' * 2097152); sys.exit(1)"]
    with pytest.raises(audio.MediaToolError) as caught:
        run_audio_command(command, time.monotonic() + 10)
    assert len(caught.value.context["stderr"]) == 4096


@pytest.mark.skipif(os.name == "nt", reason="POSIX owned process-group qualification")
def test_timeout_stops_owned_child_and_grandchild(audio, tmp_path):
    from _media_audio_process import run_audio_command

    marker = tmp_path / "orphan-ran"
    child = f"import time,pathlib; time.sleep(1.0); pathlib.Path({str(marker)!r}).write_text('orphan')"
    parent = f"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{child!r}]); time.sleep(20)"
    started = time.monotonic()
    with pytest.raises(subprocess.TimeoutExpired):
        run_audio_command([sys.executable, "-c", parent], started + 0.3)
    assert time.monotonic() - started < 3
    time.sleep(1.1)
    assert not marker.exists()


@pytest.mark.parametrize(
    "payload", ["bad JSON", "{}", "null", '{"path": null}', '{"path": "relative", "tool": "ffmpeg", "source": "vx"}']
)
def test_malformed_resolution_fails_without_encoder_launch(audio, files, monkeypatch, payload):
    calls = []

    def malformed(command, deadline, **kwargs):
        calls.append(command)
        return {"stdout": payload}

    monkeypatch.setattr(audio, "run_audio_command", malformed)
    result = invoke(audio, files)
    assert result["error"] == "untrusted_executable"
    assert len(calls) == 1 and not files[1].exists()


def test_probe_symlink_outside_managed_bundle_is_rejected(audio, files, runtime, tmp_path):
    outside = tmp_path / "elsewhere"
    outside.mkdir()
    probe = outside / "ffprobe"
    probe.write_bytes(b"unmanaged")
    runtime.paths["ffprobe"].unlink()
    try:
        runtime.paths["ffprobe"].symlink_to(probe)
    except OSError:
        pytest.skip("Symlink creation unavailable")
    result = invoke(audio, files)
    assert result["error"] == "untrusted_executable"
    assert len(runtime.calls) == 1


def test_probe_must_identify_itself_as_ffprobe(audio, files, runtime, monkeypatch):
    def wrong_version(command, deadline, **kwargs):
        result = runtime(command, deadline, **kwargs)
        if command == [str(runtime.paths["ffprobe"]), "-version"]:
            result["stdout"] = "ffmpeg version wrong-binary\n"
        return result

    monkeypatch.setattr(audio, "run_audio_command", wrong_version)
    assert invoke(audio, files)["error"] == "invalid_encoder_version"
    assert not files[1].exists()


def test_windows_cleanup_failure_is_not_reported_as_success(audio, monkeypatch):
    from types import SimpleNamespace

    import _media_audio_process as process_module

    class Process:
        pid = 12345
        killed = False
        reaped = False

        def poll(self):
            return None

        def kill(self):
            self.killed = True

        def wait(self, timeout):
            self.reaped = True

    process = Process()
    monkeypatch.setattr(process_module, "os", SimpleNamespace(name="nt"))
    monkeypatch.setattr(process_module.subprocess, "run", lambda *a, **kw: SimpleNamespace(returncode=1))
    with pytest.raises(audio.MediaToolError) as caught:
        process_module._stop(process)
    assert caught.value.code == "cleanup_uncertain"
    assert process.killed and process.reaped


def test_audio_schema_declares_core_async_and_no_conversion_options(audio):
    from dcc_mcp_core import parse_skill_md

    skill = parse_skill_md(str(SCRIPTS.parent))
    tool = next(tool for tool in skill.tools if tool.name == "transcode_audio")
    # The real Rust-backed parser must accept the new schema and script entry.
    assert tool.source_file == "scripts/transcode_audio.py"
    text = (SCRIPTS.parent / "tools.yaml").read_text()
    block = text.split("  - name: transcode_audio\n", 1)[1].split("  - name:", 1)[0]
    assert "execution: async" in block and "additionalProperties: false" in block
    assert "sample_rate:" not in block and "channels:" not in block and "overwrite:" not in block
