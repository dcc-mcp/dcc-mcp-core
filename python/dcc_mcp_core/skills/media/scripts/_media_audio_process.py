"""Bounded, owned subprocess execution for audio verification.

PCM is counted and hashed while streaming, never retained in memory or on disk.
Other stdout is capped; stderr retains only a diagnostic tail.
"""

from __future__ import annotations

from contextlib import suppress
import hashlib
import os
import signal
import subprocess
import threading
import time

from _media_common import MediaToolError

_CAPTURE_LIMIT = 1024 * 1024


def _stop(process):
    """Stop the owned process tree, then reap the launcher."""
    tree_error = None
    try:
        if os.name == "nt":
            if process.poll() is not None:
                tree_error = "Launcher exited before Windows tree cleanup could be confirmed."
            else:
                result = subprocess.run(
                    ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    timeout=10,
                    check=False,
                )
                if result.returncode != 0:
                    tree_error = "Windows taskkill did not confirm tree cleanup."
        else:
            with suppress(ProcessLookupError):
                os.killpg(process.pid, signal.SIGKILL)
    except (OSError, subprocess.SubprocessError) as exc:
        tree_error = str(exc)
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=10)
    if tree_error:
        raise MediaToolError(
            "Audio process-tree cleanup is uncertain.", "cleanup_uncertain", context={"detail": tree_error}
        )


def run_audio_command(command, deadline, *, pcm=False):
    """Run a fixed argv with bounded capture and a shared monotonic deadline."""
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise MediaToolError("Audio operation timed out.", "timeout", context={"command": command})
    options = {"start_new_session": True} if os.name != "nt" else {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    try:
        process = subprocess.Popen(
            command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **options
        )
    except OSError as exc:
        raise MediaToolError(
            "Cannot launch vx-managed audio command; install vx explicitly if missing.",
            "command_unavailable",
            context={"command": command, "detail": str(exc)},
        ) from exc
    state = {"stdout": bytearray(), "stderr": bytearray(), "bytes": 0, "overflow": False, "errors": []}
    digest = hashlib.sha256()

    def read_stream(stream, name):
        try:
            while True:
                chunk = stream.read(65536)
                if not chunk:
                    break
                if name == "stdout":
                    state["bytes"] += len(chunk)
                    if pcm:
                        digest.update(chunk)
                    elif len(state[name]) + len(chunk) <= _CAPTURE_LIMIT:
                        state[name].extend(chunk)
                    else:
                        state["overflow"] = True
                else:
                    state[name].extend(chunk)
                    del state[name][:-4096]
        except OSError as exc:
            state["errors"].append(str(exc))
        finally:
            stream.close()

    threads = [
        threading.Thread(target=read_stream, args=(stream, name), daemon=True)
        for stream, name in ((process.stdout, "stdout"), (process.stderr, "stderr"))
    ]
    for thread in threads:
        thread.start()
    try:
        process.wait(timeout=max(0.001, deadline - time.monotonic()))
        for thread in threads:
            thread.join(timeout=max(0.001, deadline - time.monotonic()))
        if any(thread.is_alive() for thread in threads):
            raise subprocess.TimeoutExpired(command, remaining)
    except BaseException as original:
        cleanup_error = None
        try:
            _stop(process)
        except Exception as exc:
            cleanup_error = str(exc)
        for thread in threads:
            thread.join(timeout=1)
        if cleanup_error or any(thread.is_alive() for thread in threads):
            raise MediaToolError(
                "Audio command interrupted; process cleanup could not be confirmed.",
                "timeout" if isinstance(original, subprocess.TimeoutExpired) else "interrupted",
                context={
                    "command": command,
                    "cleanup_status": "unknown",
                    "cleanup_error": cleanup_error,
                    "original_error": type(original).__name__,
                },
            ) from original
        raise
    stderr = bytes(state["stderr"]).decode("utf-8", "replace")
    if process.returncode != 0:
        code = "encoder_unavailable" if "Unknown encoder" in stderr else "command_failed"
        raise MediaToolError(
            "Audio command failed.",
            code,
            context={"command": command, "returncode": process.returncode, "stderr": stderr},
        )
    if state["overflow"] or state["errors"]:
        raise MediaToolError(
            "Audio command capture was incomplete.",
            "capture_failed",
            context={"command": command, "errors": state["errors"]},
        )
    return {
        "stdout": bytes(state["stdout"]).decode("utf-8", "replace"),
        "decoded_bytes": state["bytes"],
        "decoded_sha256": digest.hexdigest() if pcm else None,
    }
