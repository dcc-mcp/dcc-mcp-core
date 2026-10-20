"""Opt-in real Core async MCP + managed FFmpeg acceptance, never mock success.

Generated PCM is deterministic TEST INPUT, not replacement showcase production.
The hosted qualification step must verify binaries against pinned archives first.
"""

from __future__ import annotations

from array import array
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import threading
import time
import urllib.request
import wave

import pytest

pytestmark = pytest.mark.skipif(
    os.environ.get("DCC_MCP_AUDIO_NATIVE") != "1", reason="Explicit hosted native lane only"
)

RATE = 48000


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fixture_wav(path, frames):
    """Periodic stereo, nonzero endpoints to expose dropped tail/transient samples."""
    with wave.open(str(path), "wb") as writer:
        writer.setparams((2, 2, RATE, frames, "NONE", "not compressed"))
        for start in range(0, frames, 4096):
            block = bytearray()
            for i in range(start, min(start + 4096, frames)):
                left = round(15000 * math.cos(2 * math.pi * 440 * i / RATE))
                right = round(13000 * math.cos(2 * math.pi * 997 * i / RATE))
                block.extend(struct.pack("<hh", left, right))
            writer.writeframesraw(block)


@pytest.fixture(scope="module")
def lane():
    from conftest import McpClient
    import dcc_mcp_core as core
    from dcc_mcp_core._server.inprocess_executor import run_skill_script

    root = Path(os.environ["MEDIA_AUDIO_EVIDENCE"]) / "native"
    root.mkdir(parents=True, exist_ok=True)
    qualification = json.loads((root.parent / "qualification.json").read_text())
    for name in ("ffmpeg", "ffprobe"):
        assert sha(Path(qualification["executables"][name]["path"])) == qualification["executables"][name]["sha256"]
    package = Path(core.__file__).resolve().parent
    # Candidate wheel, not a source-tree overlay or released wheel substitution.
    assert "site-packages" in str(package), package
    skill = package / "skills" / "media"
    assert "execution: async" in (skill / "tools.yaml").read_text()
    finished = {}
    raw_results = {}
    submitted = set()
    bridge = handle = None
    previous_cwd = Path.cwd()

    def observed_runner(script, params):
        key = params.get("output_path", "unknown")
        try:
            result = run_skill_script(script, params)
            raw_results[key] = result
            return result
        finally:
            finished.setdefault(key, threading.Event()).set()

    try:
        bridge = core.HostExecutionBridge(runner=observed_runner)
        server = core.create_skill_server("python", core.McpHttpConfig(port=0))
        server.set_in_process_executor(bridge.as_inprocess_executor())
        server.discover(extra_paths=[str(skill.parent)])
        assert "media__transcode_audio" in server.load_skill("media")
        handle = server.start()
        client = McpClient(handle.mcp_url())
        records = []
        os.chdir(root)  # Avoid repository vx.toml/provider overrides during qualification.

        def call(name, arguments):
            request = {
                "jsonrpc": "2.0",
                "id": len(records) + 1,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments},
            }
            _, response = client.post(request)
            result = response.get("result", {})
            structured = result.get("structuredContent")
            if structured is None and result.get("content"):
                structured = json.loads(result["content"][0]["text"])
            if isinstance(structured, dict) and structured.get("job_id"):
                submitted.add(structured["job_id"])
            records.append({"request": request, "response": response})
            (root / "mcp.json").write_text(json.dumps(records, indent=2))
            assert "error" not in response and not response["result"].get("isError"), response
            result = response["result"]
            return result.get("structuredContent") or json.loads(result["content"][0]["text"])

        def submit(source, output, container):
            key = str(output)
            finished[key] = threading.Event()
            response = call(
                "media__transcode_audio",
                {
                    "input_path": str(source),
                    "output_path": key,
                    "container": container,
                    "audio_codec": "libvorbis" if container == "ogg" else "pcm_s16le",
                    "timeout_secs": 90,
                },
            )
            assert response.get("job_id") and response["status"] in {"pending", "running"}, response
            return response["job_id"]

        def wait(job, seconds=100):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                result = call("jobs_get_status", {"job_id": job, "include_result": True})
                if result["status"] in {"completed", "failed", "cancelled", "interrupted"}:
                    return result
                time.sleep(0.1)
            raise AssertionError(f"Job {job} did not reach terminal state")

        _, descriptor = client.post({"jsonrpc": "2.0", "id": "list", "method": "tools/list"})
        (root / "descriptor.json").write_text(json.dumps(descriptor, indent=2))
        yield root, submit, wait, finished, raw_results, handle.mcp_url()
    finally:
        cleanup_errors = []
        if handle is not None:
            for job in submitted:
                try:
                    request = urllib.request.Request(
                        f"{handle.mcp_url().rsplit('/mcp', 1)[0]}/v1/jobs/{job}", method="DELETE"
                    )
                    with urllib.request.urlopen(request, timeout=5):
                        pass
                except Exception as exc:
                    # Completed jobs may reject cancellation; handler events are decisive.
                    cleanup_errors.append("cancel: " + repr(exc))
        deadline = time.monotonic() + 15
        drained = all(event.wait(max(0, deadline - time.monotonic())) for event in finished.values())
        shutdown_errors = []
        for owner, method in ((bridge, "shutdown_script_execution"), (handle, "shutdown")):
            if owner is not None:
                try:
                    getattr(owner, method)()
                except Exception as exc:
                    shutdown_errors.append(method + ": " + repr(exc))
        os.chdir(previous_cwd)
        try:
            (root / "handler-results.json").write_text(json.dumps(raw_results, indent=2, default=str))
            (root / "cleanup.json").write_text(
                json.dumps(
                    {
                        "handlers_drained": drained,
                        "submitted_jobs": sorted(submitted),
                        "cancel_errors": cleanup_errors,
                        "shutdown_errors": shutdown_errors,
                    },
                    indent=2,
                )
            )
        finally:
            assert drained and not shutdown_errors, "Native fixture cleanup is uncertain"


def decode(path, frames, root, label=""):
    from scripts.ci.qualify_media_audio import run

    out = path.with_suffix(path.suffix + label + ".f32")
    expected = frames * 2 * 4
    command = [
        os.environ["MEDIA_AUDIO_FFMPEG"],
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-n",
        "-xerror",
        "-i",
        str(path),
        "-map",
        "0:a:0",
        "-vn",
        "-c:a",
        "pcm_f32le",
        "-f",
        "f32le",
        "-fs",
        str(expected + 8),
        str(out),
    ]
    run(command, root, dict(os.environ), timeout=30)
    assert out.stat().st_size == expected, (path, out.stat().st_size, expected)
    pcm = array("f")
    with out.open("rb") as stream:
        pcm.fromfile(stream, frames * 2)
    return pcm


@pytest.mark.parametrize("frames", [6000, 1536000], ids=["short-cue", "32-second-loop"])
@pytest.mark.parametrize("container", ["wav", "ogg"])
def test_real_async_audio(lane, frames, container):
    root, submit, wait, finished, results, _ = lane
    source = root / f"test-input-{frames}.wav"
    if not source.exists():
        fixture_wav(source, frames)
    before = sha(source)
    output = root / f"output-{frames}.{container}"
    job = submit(source, output, container)
    terminal = wait(job)
    assert terminal["status"] == "completed", terminal
    assert finished[str(output)].wait(10), "Terminal status preceded unconfirmed handler cleanup"
    receipt = results[str(output)]
    assert receipt["success"], receipt
    context = receipt["context"]
    assert context["accepted"] and context["core_job_id"] == job
    assert context["cleanup_status"] == "reaped"
    assert context["input_decode"]["sample_frames"] == context["output_decode"]["sample_frames"] == frames
    assert context["output_media"]["sample_rate"] == RATE and context["output_media"]["channels"] == 2
    assert sha(source) == before == context["input_sha256"] == context["input_sha256_after"]
    assert sha(output) == context["output_sha256"]
    source_pcm = decode(source, frames, root, label="-" + container)
    actual = decode(output, frames, root)
    if container == "wav":
        assert source_pcm == actual, "PCM transcode changed samples"
    # Predetermined lossy tolerances, never used to waive frame-count failure.
    # Inspect first/last 256 frames separately, including final granule samples.
    metrics = {}
    for name, begin, end in (("all", 0, len(actual)), ("head", 0, 512), ("tail", len(actual) - 512, len(actual))):
        rmse = math.sqrt(sum((actual[i] - source_pcm[i]) ** 2 for i in range(begin, end)) / (end - begin))
        metrics[name + "_rmse"] = rmse
        assert rmse < (0.08 if name == "all" else 0.15), metrics
    if frames == 1536000:
        metrics["loop_boundary_error"] = max(
            abs((actual[c] - actual[-2 + c]) - (source_pcm[c] - source_pcm[-2 + c])) for c in range(2)
        )
        assert metrics["loop_boundary_error"] < 0.15, metrics
    (root / f"metrics-{frames}-{container}.json").write_text(json.dumps(metrics, indent=2))


def process_identity(pid):
    """Creation identity independent of current parentage; zombies are not reaped."""
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().split(") ", 1)[1].split()
        return {"pid": pid, "starttime": fields[19], "state": fields[0]}
    except (FileNotFoundError, ProcessLookupError):
        return None


def owned_encoding_children(source, output):
    """Match this job's real FFmpeg encoder, never vx/probe/decode preflight."""
    pids = set()
    for entry in Path("/proc/self/task").glob("*/children"):
        try:
            pids.update(int(pid) for pid in entry.read_text().split())
        except FileNotFoundError:
            continue
    result = []
    encoder = Path(os.environ["MEDIA_AUDIO_FFMPEG"]).resolve()
    for pid in pids:
        try:
            first = process_identity(pid)
            if not first or first["state"] in {"Z", "X"}:
                continue
            if Path(f"/proc/{pid}/exe").resolve() != encoder:
                continue
            argv = [v.decode() for v in Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0") if v]

            def option(flag, argv=argv):
                return argv[argv.index(flag) + 1] if flag in argv else None

            if (
                option("-i") != str(source)
                or option("-c:a") != "libvorbis"
                or option("-f") != "ogg"
                or option("-map") != "0:a:0"
                or "-n" not in argv
            ):
                continue
            staging = Path(argv[-1]).resolve()
            if (
                staging.name != "artifact.ogg"
                or staging.parent.parent != output.parent.resolve()
                or not staging.parent.name.startswith(".dcc-audio-")
            ):
                continue
            second = process_identity(pid)
            if second and second["starttime"] == first["starttime"] and second["state"] not in {"Z", "X"}:
                result.append({**second, "argv": argv, "staging_path": str(staging)})
        except (FileNotFoundError, ProcessLookupError, IndexError):
            continue
    return result


def test_real_async_cancellation_reaps_owned_child(lane):
    root, submit, wait, finished, results, url = lane
    source = root / "cancellation-test-input.wav"
    fixture_wav(source, RATE * 120)  # 23 MiB; test-only input, under production's cap.
    before = sha(source)
    output = root / "cancelled.ogg"
    job = submit(source, output, "ogg")
    deadline = time.monotonic() + 20
    observed = {}
    while time.monotonic() < deadline and not finished[str(output)].is_set():
        observed = owned_encoding_children(source, output)
        if observed:
            break
        time.sleep(0.002)
    assert len(observed) == 1, "No unique active encoder observed: cancellation proof inconclusive, not passed"
    active = observed[0]
    current = process_identity(active["pid"])
    assert current and current["starttime"] == active["starttime"] and current["state"] not in {"Z", "X"}, (
        "Encoder exited before cancellation: inconclusive"
    )
    request = urllib.request.Request(f"{url.rsplit('/mcp', 1)[0]}/v1/jobs/{job}", method="DELETE")
    with urllib.request.urlopen(request, timeout=5) as response:
        assert response.status in {200, 202, 204}
    terminal = wait(job, 15)
    assert terminal["status"] == "cancelled", terminal
    assert finished[str(output)].wait(10), "Cancellation did not finish handler cleanup"
    receipt = results[str(output)]
    assert receipt["success"] is False and receipt["error"] == "cancelled", receipt
    assert receipt["context"]["cleanup_status"] == "terminated_and_reaped", receipt
    assert receipt["context"]["command"] == active["argv"], (
        "Cancellation did not terminate the observed encoding command"
    )
    assert not output.exists() and sha(source) == before
    remaining = process_identity(active["pid"])
    assert remaining is None or remaining["starttime"] != active["starttime"], (
        "Same encoder identity remains, even if reparented or zombie"
    )
    assert not owned_encoding_children(source, output), "Owned encoding child remains after cancellation"
    (root / "cancellation.json").write_text(
        json.dumps(
            {"observed_encoding_identity": active, "job_id": job, "terminal": terminal, "receipt": receipt}, indent=2
        )
    )
