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
        for tool in ("vx", "ffmpeg", "ffprobe"):
            path = tmp_path / tool
            path.write_bytes(tool.encode())
            path.chmod(0o700)
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

    def __call__(self, command, deadline, *, pcm=False, output_path=None):
        assert len(self.calls) < 20  # A runaway loop cannot grow an unbounded mock log.
        self.calls.append(command)
        result = {"stdout": "", "decoded_bytes": 0, "decoded_sha256": "a" * 64, "cleanup_status": "reaped"}
        if "where" in command:
            request = command[-1]
            tool = "ffprobe" if request == "ffmpeg::ffprobe" else "ffmpeg"
            result["stdout"] = json.dumps(
                {"path": str(self.paths[tool]), "tool": request, "source": self.resolution_source}
            )
        elif "--version" in command:
            result["stdout"] = "vx 0.9.37\n"
        elif "-version" in command:
            result["stdout"] = Path(command[0]).name + " version test-build\n"
        elif "-encoders" in command:
            result["stdout"] = " A..... pcm_f32le PCM\n A..... pcm_s16le PCM\n"
            if self.encoder_available:
                result["stdout"] += " A..... libvorbis Vorbis\n"
        elif "-show_streams" in command:
            is_output = command[-1] != str(self.source)
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
            is_output = command[command.index("-i") + 1] != str(self.source)
            result["decoded_bytes"] = (self.output_frames if is_output else self.input_frames) * 8
            if self.invalid_decode and is_output:
                result["decoded_bytes"] += 1
        else:
            assert "-c:a" in command and Path(command[-1]).name.startswith("artifact")
            assert not self.output.exists()
            Path(command[-1]).write_bytes(b"encoded")
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
    monkeypatch.setenv("DCC_MCP_MEDIA_VX_BIN", str(fake.paths["vx"]))
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
    assert receipt["accepted"] is True and receipt["output_status"] == "verified_published"
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
    assert not files[1].exists() and len(runtime.calls) == 2


def test_unavailable_encoder_fails_before_source_decode_or_encoding(audio, files, runtime):
    runtime.encoder_available = False
    result = invoke(audio, files)
    assert result["error"] == "encoder_unavailable"
    assert not files[1].exists()
    assert all("-i" not in call for call in runtime.calls)
    assert result["context"]["output_status"] == "not_published"


@pytest.mark.parametrize("failure", ["timeout", "command_failed"])
def test_partial_failure_retains_receipt_without_retry_or_acceptance(audio, files, runtime, failure):
    runtime.failure = audio.MediaToolError("Encoder failed", failure)
    result = invoke(audio, files)
    assert result["error"] == failure
    assert result["context"]["staging_status"] == "unverified_partial"
    assert not result["context"]["accepted"]
    assert sum("-n" in call for call in runtime.calls) == 1
    assert (
        files[0].read_bytes() == b"source-master" and Path(result["context"]["staging_path"]).read_bytes() == b"encoded"
    )
    assert not files[1].exists()


def test_missing_128_samples_are_rejected_without_padding(audio, files, runtime):
    runtime.output_frames -= 128
    result = invoke(audio, files)
    assert result["error"] == "sample_count_mismatch"
    assert result["context"]["output_decode"]["sample_frames"] == 1535872
    assert result["context"]["staging_status"] == "unverified_partial"
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
    with pytest.raises(audio.MediaToolError, match="exceeded a byte budget"):
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
    with pytest.raises(audio.MediaToolError, match="timed out"):
        run_audio_command([sys.executable, "-c", parent], started + 0.3)
    assert time.monotonic() - started < 3
    time.sleep(1.1)
    assert not marker.exists()


@pytest.mark.parametrize(
    "payload", ["bad JSON", "{}", "null", '{"path": null}', '{"path": "relative", "tool": "ffmpeg", "source": "vx"}']
)
def test_malformed_resolution_fails_without_encoder_launch(audio, files, runtime, monkeypatch, payload):
    calls = []

    def malformed(command, deadline, **kwargs):
        calls.append(command)
        return {"stdout": "vx 0.9.37\n" if "--version" in command else payload}

    monkeypatch.setattr(audio, "run_audio_command", malformed)
    result = invoke(audio, files)
    assert result["error"] == "untrusted_executable"
    assert len(calls) == 2 and not files[1].exists()


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
    assert len(runtime.calls) == 2


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


def test_unknown_error_preserves_stage_receipt_and_job_identity(audio, files, runtime):
    from dcc_mcp_core.cancellation import CancelToken
    from dcc_mcp_core.cancellation import reset_cancel_token
    from dcc_mcp_core.cancellation import set_cancel_token

    reset = set_cancel_token(CancelToken(job_id="owned-audio-test"))
    runtime.failure = RuntimeError("unexpected encoding fault")
    try:
        result = invoke(audio, files)
    finally:
        reset_cancel_token(reset)
    assert result["error"] == "unexpected_error"
    assert result["context"]["core_job_id"] == "owned-audio-test"
    assert result["context"]["job_id_owner"] == "core"
    assert result["context"]["commands"]
    assert Path(result["context"]["staging_path"]).read_bytes() == b"encoded"
    assert not files[1].exists()


def test_late_destination_race_does_not_replace_existing_file(audio, files, runtime, monkeypatch):
    original_link = audio.os.link

    def concurrent_publish(source, destination):
        files[1].write_bytes(b"other-owner")
        original_link(source, destination)

    monkeypatch.setattr(audio.os, "link", concurrent_publish)
    result = invoke(audio, files)
    assert result["error"] == "publish_failed"
    assert not result["context"]["accepted"]
    assert result["context"]["staging_status"] == "verified_unpublished"
    assert files[1].read_bytes() == b"other-owner"
    assert Path(result["context"]["staging_path"]).read_bytes() == b"encoded"


def test_cancel_during_encoding_keeps_job_and_never_publishes(audio, files, runtime, monkeypatch):
    from dcc_mcp_core.cancellation import CancelToken
    from dcc_mcp_core.cancellation import reset_cancel_token
    from dcc_mcp_core.cancellation import set_cancel_token

    token = CancelToken(job_id="cancelled-audio-test")
    reset = set_cancel_token(token)

    def cancel_encoding(command, deadline, **kwargs):
        result = runtime(command, deadline, **kwargs)
        if "-n" in command:
            token.cancel()
        return result

    monkeypatch.setattr(audio, "run_audio_command", cancel_encoding)
    try:
        result = invoke(audio, files)
    finally:
        reset_cancel_token(reset)
    assert result["error"] == "cancelled"
    assert result["context"]["core_job_id"] == "cancelled-audio-test"
    assert result["context"]["cancel_reason"]
    assert not files[1].exists()
    assert Path(result["context"]["staging_path"]).is_file()
    assert sum("-n" in call for call in runtime.calls) == 1


def test_input_limit_rejects_before_process_creation(audio, files, runtime, monkeypatch):
    monkeypatch.setattr(audio, "INPUT_LIMIT", 4)
    assert invoke(audio, files)["error"] == "resource_limit"
    assert runtime.calls == [] and not files[1].exists()


@pytest.mark.parametrize("field,value", [("output_rate", 1000000000), ("output_channels", 1000000000)])
def test_out_of_bounds_audio_metadata_is_rejected(audio, files, runtime, field, value):
    setattr(runtime, field, value)
    result = invoke(audio, files)
    assert result["error"] == "invalid_audio_metadata"
    assert not files[1].exists()


def test_pcm_limit_stops_writer_before_deadline(audio, monkeypatch):
    import _media_audio_process as process_module

    monkeypatch.setattr(process_module, "PCM_LIMIT", 1024)
    command = [
        sys.executable,
        "-c",
        "import sys,time; sys.stdout.buffer.write(b'x'*65536); sys.stdout.flush(); time.sleep(20)",
    ]
    start = time.monotonic()
    with pytest.raises(audio.MediaToolError) as caught:
        process_module.run_audio_command(command, start + 10, pcm=True)
    assert caught.value.code == "resource_limit"
    assert caught.value.context["reason"] == "decoded_pcm_limit"
    assert caught.value.context["cleanup_status"] == "terminated_and_reaped"
    assert time.monotonic() - start < 3


def test_output_acceptance_limit_stops_process(audio, tmp_path, monkeypatch):
    import _media_audio_process as process_module

    output = tmp_path / "oversized-stage"
    monkeypatch.setattr(process_module, "OUTPUT_LIMIT", 1024)
    command = [
        sys.executable,
        "-c",
        f"import pathlib,time; pathlib.Path({str(output)!r}).write_bytes(b'x'*2048); time.sleep(20)",
    ]
    start = time.monotonic()
    with pytest.raises(audio.MediaToolError) as caught:
        process_module.run_audio_command(command, start + 10, output_path=output)
    assert caught.value.code == "resource_limit"
    assert caught.value.context["reason"] == "output_size_limit"
    assert time.monotonic() - start < 3


@pytest.mark.skipif(os.name == "nt", reason="POSIX owned process-group cancellation")
def test_core_cancellation_stops_native_child_and_preserves_reason(audio, tmp_path):
    import threading

    from _media_audio_process import run_audio_command

    from dcc_mcp_core.cancellation import CancelToken
    from dcc_mcp_core.cancellation import reset_cancel_token
    from dcc_mcp_core.cancellation import set_cancel_token

    marker = tmp_path / "cancel-orphan"
    child = f"import time,pathlib; time.sleep(1); pathlib.Path({str(marker)!r}).write_text('orphan')"
    parent = f"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c',{child!r}]); time.sleep(20)"
    token = CancelToken(job_id="process-cancel-test")
    reset = set_cancel_token(token)
    timer = threading.Timer(0.2, token.cancel)
    timer.start()
    start = time.monotonic()
    try:
        with pytest.raises(audio.MediaToolError) as caught:
            run_audio_command([sys.executable, "-c", parent], start + 10)
    finally:
        timer.join()
        reset_cancel_token(reset)
    assert caught.value.code == "cancelled"
    assert caught.value.context["cancel_reason"]
    assert caught.value.context["cleanup_status"] == "terminated_and_reaped"
    assert time.monotonic() - start < 3
    time.sleep(1.1)
    assert not marker.exists()


def test_cleanup_uncertainty_is_preserved_in_the_final_receipt(audio, files, runtime):
    runtime.failure = audio.MediaToolError(
        "Cancelled with unknown cleanup", "cancelled", context={"cleanup_status": "unknown", "cancel_reason": "request"}
    )
    result = invoke(audio, files)
    assert result["error"] == "cancelled"
    assert result["context"]["cleanup_status"] == "unknown"
    assert result["context"]["cancel_reason"] == "request"
    assert not files[1].exists()


def test_cancel_immediately_before_publication_leaves_verified_stage(audio, files, runtime, monkeypatch):
    from dcc_mcp_core.cancellation import CancelToken
    from dcc_mcp_core.cancellation import reset_cancel_token
    from dcc_mcp_core.cancellation import set_cancel_token

    token = CancelToken(job_id="pre-publication-cancel")
    reset = set_cancel_token(token)
    original_hash = audio._hash_file
    probe_hashes = 0

    def cancel_after_final_hash(path, deadline, maximum=audio.INPUT_LIMIT):
        nonlocal probe_hashes
        result = original_hash(path, deadline, maximum)
        if path == runtime.paths["ffprobe"]:
            probe_hashes += 1
            if probe_hashes == 2:
                token.cancel()
        return result

    monkeypatch.setattr(audio, "_hash_file", cancel_after_final_hash)
    try:
        result = invoke(audio, files)
    finally:
        reset_cancel_token(reset)
    assert result["error"] == "cancelled"
    assert result["context"]["staging_status"] == "verified_unpublished"
    assert result["context"]["core_job_id"] == "pre-publication-cancel"
    assert not files[1].exists()


def test_already_cancelled_request_runs_no_process(audio, files, runtime):
    from dcc_mcp_core.cancellation import CancelToken
    from dcc_mcp_core.cancellation import reset_cancel_token
    from dcc_mcp_core.cancellation import set_cancel_token

    token = CancelToken(job_id="pre-cancel")
    token.cancel()
    reset = set_cancel_token(token)
    try:
        result = invoke(audio, files)
    finally:
        reset_cancel_token(reset)
    assert result["error"] == "cancelled"
    assert result["context"]["cleanup_status"] == "not_started"
    assert runtime.calls == []


def test_stderr_total_limit_stops_noisy_writer(audio, monkeypatch):
    import _media_audio_process as process_module

    monkeypatch.setattr(process_module, "STDERR_LIMIT", 1024)
    command = [sys.executable, "-c", "import sys,time; sys.stderr.write('x'*65536); sys.stderr.flush(); time.sleep(20)"]
    started = time.monotonic()
    with pytest.raises(audio.MediaToolError) as caught:
        process_module.run_audio_command(command, started + 10)
    assert caught.value.code == "resource_limit"
    assert caught.value.context["reason"] == "stderr_limit"
    assert time.monotonic() - started < 3


def test_oversized_staged_artifact_is_never_published(audio, files, runtime, monkeypatch):
    monkeypatch.setattr(audio, "OUTPUT_LIMIT", 4)
    result = invoke(audio, files)
    assert result["error"] == "resource_limit"
    assert not files[1].exists()
    assert Path(result["context"]["staging_path"]).read_bytes() == b"encoded"
    command = next(command for command in runtime.calls if "-n" in command)
    assert command[command.index("-fs") + 1] == "4"


def test_success_removes_only_owned_staging_and_records_launcher(audio, files, runtime):
    result = invoke(audio, files)
    assert result["success"]
    receipt = result["context"]
    assert files[1].read_bytes() == b"encoded"
    assert not Path(receipt["staging_path"]).exists()
    assert receipt["executables"]["vx"]["path"] == str(runtime.paths["vx"])
    assert receipt["executables"]["vx"]["source"] == "operator_configured"
    assert receipt["limits"]["decoded_pcm_bytes_per_stream"] == 256 * 1024 * 1024


def test_unknown_error_does_not_reuse_previous_commands_cleanup_status(audio, files, runtime):
    runtime.failure = RuntimeError("worker outcome uncertain")
    result = invoke(audio, files)
    assert result["error"] == "unexpected_error"
    assert result["context"]["cleanup_status"] == "unknown"
    assert not files[1].exists()


def test_terminal_reader_overflow_cannot_race_owner_success(audio, monkeypatch):
    """Schedule reader overflow strictly after the first owner failed check."""
    import io
    from types import SimpleNamespace

    import _media_audio_process as process_module

    schedule = []

    class DeferredReader:
        def __init__(self, target, args, daemon):
            self.target, self.args = target, args
            self.ident = None
            self.finished = False

        def start(self):
            self.ident = 1

        def is_alive(self):
            if not self.finished:
                assert schedule == ["owner-polled"], schedule
                self.target(*self.args)
                self.finished = True
            return False

        def join(self, timeout):
            assert self.finished

    def poll():
        schedule.append("owner-polled")
        return 0

    process = SimpleNamespace(
        stdout=io.BytesIO(b"x" * 2048), stderr=io.BytesIO(b""), pid=12345, returncode=0, poll=poll
    )
    stopped = []
    monkeypatch.setattr(process_module, "PCM_LIMIT", 1024)
    monkeypatch.setattr(process_module.subprocess, "Popen", lambda *args, **kwargs: process)
    monkeypatch.setattr(process_module.threading, "Thread", DeferredReader)
    monkeypatch.setattr(process_module, "_stop", lambda process: stopped.append(True))
    with pytest.raises(audio.MediaToolError) as caught:
        process_module.run_audio_command(["never-launched"], time.monotonic() + 10, pcm=True)
    assert caught.value.code == "resource_limit"
    assert caught.value.context["reason"] == "decoded_pcm_limit"
    assert caught.value.context["cleanup_status"] == "terminated_and_reaped"
    assert stopped == [True]
