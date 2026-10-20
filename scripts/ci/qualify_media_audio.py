"""Hosted-only media runtime qualification. Never invoked by ordinary unit tests.

Known vx 0.9.37 update-check HTTP/cache and metrics writes are permitted in the
isolated HOME, not described as offline. No shell installer or system fallback.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tarfile
import time
import traceback
import urllib.request

VX_URL = "https://github.com/vx-org/vx/releases/download/v0.9.37/vx-0.9.37-x86_64-unknown-linux-gnu.tar.gz"
VX_SIZE = 11284396
VX_SHA = "18b751f7735502723cff7f9a78dd90bcf40572448435a25dcafa1c6a44f8c7f6"
FF_URL = "https://github.com/vx-org/mirrors/releases/download/ffmpeg-8.1/ffmpeg-8.1-linux64-lgpl.tar.xz"
FF_SIZE = 127584512
FF_SHA = "dd03558b41409d111784bfe5da9bce4ddf8dae92871ede3b34acb3ad0f0889b0"
FLOOR = 2 * 1024**3


def digest(path):
    """Stream a file hash without loading the complete archive."""
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda stream=stream: stream.read(1024**2), b""):
            h.update(chunk)
    return h.hexdigest()


def download(url, target, size, sha):
    """Download one pinned public artifact with a checked byte/time budget."""
    deadline = time.monotonic() + 180
    count = 0
    h = hashlib.sha256()
    with urllib.request.urlopen(url, timeout=20) as response, target.open("xb") as out:
        assert response.geturl().startswith("https://")
        while True:
            chunk = response.read(1024**2)
            if not chunk:
                break
            count += len(chunk)
            if count > size or time.monotonic() >= deadline:
                raise RuntimeError("Download exceeded pinned byte/time bound")
            if shutil.disk_usage(target.parent).free < FLOOR:
                raise RuntimeError("Hosted disk reserve exhausted")
            out.write(chunk)
            h.update(chunk)
    assert count == size and h.hexdigest() == sha, (url, count, h.hexdigest())


def _bounded_text(path, maximum):
    with path.open("rb") as stream:
        data = stream.read(maximum + 1)
    if len(data) > maximum:
        raise RuntimeError("Closed command log exceeded read budget")
    return data.decode("utf-8", "replace")


def _stderr_tail(path, maximum=4000):
    with path.open("rb") as stream:
        stream.seek(0, os.SEEK_END)
        stream.seek(max(0, stream.tell() - maximum))
        return stream.read(maximum).decode("utf-8", "replace")


def run(argv, root, env, timeout=30):
    """Bound logs before reads, stop the owned group, and preserve uncertainty."""
    limit = 4 * 1024**2
    name = f"command-{len(list(root.glob('command-*.json'))):03d}"
    out, err = root / (name + ".out"), root / (name + ".err")
    record = {"argv": argv, "timeout_seconds": timeout, "cleanup_status": "not_started"}
    p = None
    original = None
    cleanup_errors = []
    try:
        with out.open("xb") as stdout, err.open("xb") as stderr:
            p = subprocess.Popen(
                argv, cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True
            )
            record["cleanup_status"] = "running"
            deadline = time.monotonic() + timeout
            while p.poll() is None:
                if (
                    time.monotonic() > deadline
                    or out.stat().st_size + err.stat().st_size > limit
                    or shutil.disk_usage(root).free < FLOOR
                ):
                    raise RuntimeError("Command exceeded time/log/disk bound")
                time.sleep(0.05)
            record["returncode"] = p.returncode
    except BaseException as exc:
        original = exc
        record["original_error"] = repr(exc)
    finally:
        if p is not None:
            # Stop descendants even after launcher exit. Never read open logs.
            try:
                os.killpg(p.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            except OSError as exc:
                cleanup_errors.append(repr(exc))
            try:
                p.wait(timeout=10)
                record["launcher_reaped"] = True
            except (OSError, subprocess.SubprocessError) as exc:
                cleanup_errors.append(repr(exc))
            record["cleanup_status"] = "unknown" if cleanup_errors else "terminated_and_reaped"
        if cleanup_errors:
            record["cleanup_errors"] = cleanup_errors
        # Child may have exceeded the cap immediately before exiting. Check the
        # final sizes after cleanup, including on unsuccessful return codes.
        sizes = {path.name: path.stat().st_size for path in (out, err) if path.exists()}
        record["closed_log_sizes"] = sizes
        record["log_limit_exceeded"] = sum(sizes.values()) > limit
        (root / (name + ".json")).write_text(json.dumps(record, indent=2))
    if cleanup_errors:
        raise RuntimeError("Command cleanup is uncertain; see command receipt") from original
    if record["log_limit_exceeded"]:
        raise RuntimeError("Closed command logs exceeded 4 MiB budget") from original
    if original is not None:
        raise original
    if p.returncode != 0:
        raise RuntimeError((argv, p.returncode, _stderr_tail(err)))
    return _bounded_text(out, limit)


def archive_hashes(archive):
    """Read verified archive members without extracting arbitrary paths."""
    result = {}
    total = 0
    with tarfile.open(archive, "r|xz") as tar:
        for member in tar:
            total += member.size
            if total > 1024**3:
                raise RuntimeError("Archive expansion exceeds 1 GiB budget")
            if member.name in {
                "ffmpeg-n8.1-latest-linux64-lgpl-8.1/bin/ffmpeg",
                "ffmpeg-n8.1-latest-linux64-lgpl-8.1/bin/ffprobe",
            }:
                assert member.isfile() and member.size <= 512 * 1024**2
                h = hashlib.sha256()
                stream = tar.extractfile(member)
                assert stream is not None
                for chunk in iter(lambda stream=stream: stream.read(1024**2), b""):
                    h.update(chunk)
                result[Path(member.name).name] = {"sha256": h.hexdigest(), "size": member.size}
    assert set(result) == {"ffmpeg", "ffprobe"}, result
    return result, total


def child_environment(home, bin_dir):
    """Construct a closed child environment without inheriting runner variables."""
    return {
        "HOME": str(home),
        "VX_HOME": str(home / ".vx"),
        "XDG_CACHE_HOME": str(home / ".cache"),
        "TMPDIR": str(home / "tmp"),
        "PATH": str(bin_dir),
        "LANG": "C.UTF-8",
        "DCC_MCP_MEDIA_AUTO_INSTALL_VX": "0",
    }


def native_environment(root, qualification):
    """Construct the entire native test process environment, including isolation."""
    vx = Path(qualification["vx"]["path"])
    env = child_environment(root / "home", vx.parent)
    env.update(
        {
            "DCC_MCP_MEDIA_VX_BIN": str(vx),
            "MEDIA_AUDIO_EVIDENCE": str(root),
            "MEDIA_AUDIO_FFMPEG": qualification["executables"]["ffmpeg"]["path"],
            "MEDIA_AUDIO_FFPROBE": qualification["executables"]["ffprobe"]["path"],
            "DCC_MCP_AUDIO_NATIVE": "1",
            "DCC_MCP_GATEWAY_PORT": "0",
            "DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS": "1",
            "DCC_MCP_REGISTRY_DIR": str(root / "registry"),
            "DCC_MCP_CHECKPOINT_DIR": str(root / "checkpoints"),
        }
    )
    return env


def native_tests():
    """Replace the launcher with pytest; never restore the runner environment."""
    assert os.environ.get("GITHUB_ACTIONS") == "true", "Hosted CI only"
    root = Path(os.environ["MEDIA_AUDIO_EVIDENCE"]).resolve()
    qualification = json.loads((root / "qualification.json").read_text())
    env = native_environment(root, qualification)
    for key in ("TMPDIR", "DCC_MCP_REGISTRY_DIR", "DCC_MCP_CHECKPOINT_DIR"):
        Path(env[key]).mkdir(parents=True, exist_ok=True)
    executable = str(Path(sys.executable).resolve())
    os.execve(
        executable,
        [
            executable,
            "-m",
            "pytest",
            "tests/test_media_audio_hosted.py",
            "-v",
            "--tb=short",
            "--junitxml=" + str(root / "junit.xml"),
        ],
        env,
    )


def main():
    """Qualify official hosted media tools before exporting test configuration."""
    assert os.environ.get("GITHUB_ACTIONS") == "true", "Hosted CI only"
    root = Path(os.environ["MEDIA_AUDIO_EVIDENCE"]).resolve()
    root.mkdir(parents=True, exist_ok=True)
    assert shutil.disk_usage(root).free >= 5 * 1024**3, "Need 5 GiB free before qualification"
    home, bin_dir = root / "home", root / "bin"
    home.mkdir(exist_ok=True)
    bin_dir.mkdir(exist_ok=True)
    env = child_environment(home, bin_dir)
    (home / "tmp").mkdir(exist_ok=True)
    vx_archive, ff_archive = root / "vx.tar.gz", root / "ffmpeg.tar.xz"
    download(VX_URL, vx_archive, VX_SIZE, VX_SHA)
    download(FF_URL, ff_archive, FF_SIZE, FF_SHA)
    expected, expansion = archive_hashes(ff_archive)
    with tarfile.open(vx_archive, "r:gz") as tar:
        matches = [m for m in tar.getmembers() if Path(m.name).name == "vx" and m.isfile()]
        assert len(matches) == 1 and matches[0].size <= 128 * 1024**2
        stream = tar.extractfile(matches[0])
        assert stream is not None
        vx = bin_dir / "vx"
        with vx.open("xb") as output:
            shutil.copyfileobj(stream, output, 1024**2)
        vx.chmod(0o700)
    env["DCC_MCP_MEDIA_VX_BIN"] = str(vx)
    # Official provider installation, not fabricated store paths or cache seeding.
    # A second official FFmpeg fetch is expected; vx's own network total is not
    # hard byte-limited by this script. Its wall time/log/disk growth are bounded.
    vx_version = run([str(vx), "--version"], root, env).strip()
    assert vx_version == "vx 0.9.37", vx_version
    run([str(vx), "install", "ffmpeg@8.1"], root, env, timeout=240)
    resolution = json.loads(run([str(vx), "--output-format", "json", "where", "ffmpeg@8.1"], root, env))
    assert resolution["source"] == "vx"
    ffmpeg = Path(resolution["path"]).resolve()
    assert ffmpeg.is_relative_to(home / ".vx"), resolution
    probe = ffmpeg.with_name("ffprobe").resolve()
    assert probe.parent == ffmpeg.parent
    binaries = {}
    for name, path in (("ffmpeg", ffmpeg), ("ffprobe", probe)):
        assert path.is_file() and path.stat().st_size == expected[name]["size"]
        actual = digest(path)
        assert actual == expected[name]["sha256"], (name, actual, expected[name])
        binaries[name] = {
            "path": str(path),
            "sha256": actual,
            "version": run([str(path), "-version"], root, env).splitlines()[0],
        }
    encoders = run([str(ffmpeg), "-hide_banner", "-encoders"], root, env)
    available = {p[1] for line in encoders.splitlines() for p in [line.split()] if len(p) >= 2 and p[0].startswith("A")}
    assert {"libvorbis", "pcm_s16le", "pcm_f32le"} <= available
    receipt = {
        "vx": {"path": str(vx), "sha256": digest(vx), "archive_sha256": VX_SHA, "version": vx_version},
        "ffmpeg_archive_sha256": FF_SHA,
        "archive_unpacked_bytes": expansion,
        "verified_reference_members": expected,
        "executables": binaries,
        "known_effects": "vx update HTTP/cache and metrics writes allowed in isolated HOME",
        "expected_tool_archive_transfer_bytes": VX_SIZE + 2 * FF_SIZE,
        "network_budget_note": "384 MiB planning allowance, not enforced for vx internal traffic/retries",
        "resolution": resolution,
    }
    (root / "qualification.json").write_text(json.dumps(receipt, indent=2))
    with Path(os.environ["GITHUB_ENV"]).open("a") as output:
        for key in ("HOME", "VX_HOME", "XDG_CACHE_HOME", "DCC_MCP_MEDIA_VX_BIN", "DCC_MCP_MEDIA_AUTO_INSTALL_VX"):
            output.write(f"{key}={env[key]}\n")
        output.write(f"MEDIA_AUDIO_FFMPEG={ffmpeg}\nMEDIA_AUDIO_FFPROBE={probe}\n")


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["--native-tests"]:
            native_tests()
        else:
            assert not sys.argv[1:], "Unknown qualifier arguments"
            main()
    except BaseException:
        root = Path(os.environ["MEDIA_AUDIO_EVIDENCE"])
        root.mkdir(parents=True, exist_ok=True)
        (root / "qualification-failure.json").write_text(json.dumps({"traceback": traceback.format_exc()}))
        raise
