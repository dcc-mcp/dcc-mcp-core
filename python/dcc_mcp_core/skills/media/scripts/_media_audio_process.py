"""Bounded, cooperatively cancellable subprocess execution for audio receipts."""

from __future__ import annotations

from contextlib import suppress
import hashlib
import os
import signal
import subprocess
import threading
import time

from _media_common import MediaToolError

from dcc_mcp_core.cancellation import DccMcpCancelledError
from dcc_mcp_core.cancellation import check_dcc_cancelled

CAPTURE_LIMIT = 1024 * 1024
PCM_LIMIT = 256 * 1024 * 1024
STDERR_LIMIT = 4 * 1024 * 1024
OUTPUT_LIMIT = 64 * 1024 * 1024


def check_audio_cancelled():
    """Preserve the Core cancellation reason rather than a generic error."""
    try:
        check_dcc_cancelled()
    except DccMcpCancelledError as exc:
        raise MediaToolError("Audio operation cancelled.", "cancelled", context={"cancel_reason": str(exc)}) from exc


def _stop(process):
    """Stop the owned process tree, then reap the launcher; report uncertainty."""
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


def run_audio_command(command, deadline, *, pcm=False, output_path=None):
    """Run fixed argv with cancellation, byte budgets and one shared deadline.

    Output-path monitoring is a fail-closed acceptance limit, not an OS disk
    quota. The encoding argv also supplies FFmpeg's own bounded `-fs` limit.
    """
    try:
        check_audio_cancelled()
    except MediaToolError as exc:
        exc.context["cleanup_status"] = "not_started"
        raise
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise MediaToolError(
            "Audio operation timed out.", "timeout", context={"command": command, "cleanup_status": "not_started"}
        )
    options = {"start_new_session": True} if os.name != "nt" else {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
    try:
        process = subprocess.Popen(
            command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **options
        )
    except OSError as exc:
        raise MediaToolError(
            "Cannot launch the resolved audio command.",
            "command_unavailable",
            context={"command": command, "detail": str(exc), "cleanup_status": "not_started"},
        ) from exc
    state = {"stdout": bytearray(), "stderr": bytearray(), "bytes": 0, "stderr_bytes": 0, "error": None}
    failed = threading.Event()
    digest = hashlib.sha256()

    def read_stream(stream, name):
        try:
            while True:
                chunk = stream.read(65536)
                if not chunk:
                    break
                if name == "stdout":
                    state["bytes"] += len(chunk)
                    if state["bytes"] > (PCM_LIMIT if pcm else CAPTURE_LIMIT):
                        state["error"] = "decoded_pcm_limit" if pcm else "text_capture_limit"
                        failed.set()
                        break
                    if pcm:
                        digest.update(chunk)
                    else:
                        state[name].extend(chunk)
                else:
                    state["stderr_bytes"] += len(chunk)
                    state[name].extend(chunk)
                    del state[name][:-4096]
                    if state["stderr_bytes"] > STDERR_LIMIT:
                        state["error"] = "stderr_limit"
                        failed.set()
                        break
        except OSError as exc:
            state["error"] = str(exc)
            failed.set()
        finally:
            stream.close()

    threads = [
        threading.Thread(target=read_stream, args=(stream, name), daemon=True)
        for stream, name in ((process.stdout, "stdout"), (process.stderr, "stderr"))
    ]
    try:
        for thread in threads:
            thread.start()
        while True:
            check_audio_cancelled()
            if failed.is_set():
                raise MediaToolError(
                    "Audio stream exceeded a byte budget or could not be read.",
                    "resource_limit",
                    context={"reason": state["error"]},
                )
            if output_path is not None and output_path.exists() and output_path.stat().st_size > OUTPUT_LIMIT:
                raise MediaToolError(
                    "Audio output exceeded the acceptance size limit.",
                    "resource_limit",
                    context={"reason": "output_size_limit"},
                )
            if time.monotonic() >= deadline:
                raise MediaToolError("Audio operation timed out.", "timeout")
            if process.poll() is not None and not any(thread.is_alive() for thread in threads):
                break
            failed.wait(timeout=min(0.05, max(0, deadline - time.monotonic())))
        # A reader can fail after the loop's first failed check but before its
        # final is_alive check. Join the terminal readers, then recheck every
        # acceptance condition before considering returncode or decoded bytes.
        for thread in threads:
            thread.join(timeout=max(0, deadline - time.monotonic()))
        check_audio_cancelled()
        if failed.is_set():
            raise MediaToolError(
                "Audio stream exceeded a byte budget or could not be read.",
                "resource_limit",
                context={"reason": state["error"]},
            )
        if any(thread.is_alive() for thread in threads) or time.monotonic() >= deadline:
            raise MediaToolError("Audio operation timed out.", "timeout")
        if output_path is not None and output_path.exists() and output_path.stat().st_size > OUTPUT_LIMIT:
            raise MediaToolError(
                "Audio output exceeded the acceptance size limit.",
                "resource_limit",
                context={"reason": "output_size_limit"},
            )
    except BaseException as original:
        cleanup_error = None
        try:
            _stop(process)
        except Exception as exc:
            cleanup_error = str(exc)
        for thread in threads:
            if thread.ident is not None:
                thread.join(timeout=1)
        cleanup_status = "unknown" if cleanup_error or any(t.is_alive() for t in threads) else "terminated_and_reaped"
        if isinstance(original, MediaToolError):
            original.context.update(command=command, cleanup_status=cleanup_status, cleanup_error=cleanup_error)
            raise
        raise MediaToolError(
            "Audio command interrupted.",
            "interrupted",
            context={
                "command": command,
                "cleanup_status": cleanup_status,
                "cleanup_error": cleanup_error,
                "original_error": type(original).__name__,
            },
        ) from original
    stderr = bytes(state["stderr"]).decode("utf-8", "replace")
    if process.returncode != 0:
        code = "encoder_unavailable" if "Unknown encoder" in stderr else "command_failed"
        raise MediaToolError(
            "Audio command failed.",
            code,
            context={
                "command": command,
                "returncode": process.returncode,
                "stderr": stderr,
                "cleanup_status": "reaped",
            },
        )
    return {
        "stdout": bytes(state["stdout"]).decode("utf-8", "replace"),
        "decoded_bytes": state["bytes"],
        "decoded_sha256": digest.hexdigest() if pcm else None,
        "cleanup_status": "reaped",
    }
