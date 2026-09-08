"""One-shot CI-only diagnostic for Core #2442, never a product repair.

Run one original normal-exit call, one live-descendant control, and one
separate handle-retirement control. There are no repeated attempts. Evidence
is persisted before a failing original assertion determines the exit status.
"""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import uuid

HERE = Path(__file__).resolve().parent
HELPER_SHA256 = "c042d426c6917d9ce4dbf066f9f4ea6a6dd136fa115dbc94ee3fb5111fd0e699"
MODES = ("original_normal_exit", "live_descendant", "separate_handle_retirement")
TIMEOUT_SECONDS = 5


def load_file(name: str, path: Path):
    """Load an explicitly selected sibling, independent of site/PYTHONPATH."""
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_helper():
    """Reject source drift before starting any child process."""
    path = HERE / "generated_lock_sync.py"
    digest = hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()
    if digest != HELPER_SHA256:
        raise RuntimeError("Pinned helper source changed; diagnostic refused")
    return load_file("core2442_original_helper", path)


def persist(path: Path, value: dict) -> None:
    """Write a new durable evidence artifact, refusing an overwrite."""
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2)
        stream.flush()
        os.fsync(stream.fileno())


def metadata() -> dict:
    """Read only explicitly allowed runner provenance, never the environment."""
    return {
        "python": sys.version,
        "python_bits": ctypes.sizeof(ctypes.c_void_p) * 8,
        "platform": platform.platform(),
        "windows_build": sys.getwindowsversion().build,
        "runner": {
            key: os.environ.get(key)
            for key in ("ImageOS", "ImageVersion", "RUNNER_OS", "GITHUB_SHA", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT")
        },
        "source_sha256_lf": HELPER_SHA256,
        "subject_timeout_seconds": TIMEOUT_SECONDS,
        "maximum_subjects": len(MODES),
        "modes": MODES,
        "run_retries": 0,
        "python_source": "actions/setup-python@v6, official actions/python-versions 3.14.7 x64",
        "started_unix": time.time(),
    }


def require_authorized_runner() -> None:
    """Do not accidentally repeat the completed local experiment or a CI run."""
    required = {"GITHUB_ACTIONS": "true", "GITHUB_RUN_ATTEMPT": "1", "RUNNER_OS": "Windows", "ImageOS": "win22"}
    if any(os.environ.get(key) != value for key, value in required.items()):
        raise RuntimeError("Requires the first authorized Windows 2022 GitHub Actions run")
    if sys.version_info[:3] != (3, 14, 7) or ctypes.sizeof(ctypes.c_void_p) != 8:
        raise RuntimeError("Requires official Python 3.14.7 x64")
    if sys.getwindowsversion().build != 20348:
        raise RuntimeError("Requires the Windows Server 2022 kernel build")


def live_control(role: str, event_name: str | None) -> None:
    """Use an owned named event to prove the real child is ready to outlive its leader."""
    api = ctypes.WinDLL("kernel32", use_last_error=True)
    for name, args, result in (
        ("CreateEventW", (ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_wchar_p), ctypes.c_void_p),
        ("OpenEventW", (ctypes.c_ulong, ctypes.c_int, ctypes.c_wchar_p), ctypes.c_void_p),
        ("SetEvent", (ctypes.c_void_p,), ctypes.c_int),
        ("WaitForSingleObject", (ctypes.c_void_p, ctypes.c_ulong), ctypes.c_ulong),
        ("CloseHandle", (ctypes.c_void_p,), ctypes.c_int),
    ):
        function = getattr(api, name)
        function.argtypes, function.restype = args, result
    if role == "child":
        ready = api.OpenEventW(2, False, event_name)
        blocker = api.CreateEventW(None, True, False, None)
        if not ready or not blocker or not api.SetEvent(ready):
            raise ctypes.WinError(ctypes.get_last_error())
        api.CloseHandle(ready)
        api.WaitForSingleObject(blocker, 60000)
        api.CloseHandle(blocker)
        return
    name = "Local\\core2442-diagnostic-" + uuid.uuid4().hex
    ready = api.CreateEventW(None, True, False, name)
    if not ready:
        raise ctypes.WinError(ctypes.get_last_error())
    # The leader is already in the helper's owned Job; child membership is
    # inherited before any child instruction runs. No breakaway flag is used.
    subprocess.Popen([sys.executable, "-I", "-B", str(Path(__file__).resolve()), "--control", "child", "--event", name])
    if api.WaitForSingleObject(ready, 2000) != 0:
        raise RuntimeError("Live-descendant readiness control failed")
    api.CloseHandle(ready)
    os._exit(0)


def retirement_control(module, observer, record: dict) -> None:
    """Observe handle-close timing in a distinct control, not a changed run_bounded."""
    job = module.WindowsJob()
    process = None
    try:
        process = module._create_suspended_windows_process(
            [sys.executable, "-c", "raise SystemExit(0)"], cwd=None, env=None
        )
        try:
            job.assign(process)
        except Exception:
            # A failed assignment has no Job containment yet; terminate only
            # the physical process created here, never an unrelated PID.
            process.terminate()
            process.wait(TIMEOUT_SECONDS)
            raise
        job._diagnostic_leader = process
        process.resume_initial_thread()
        observer.snapshot(job, "control_before_leader_wait")
        before = time.perf_counter_ns()
        process.wait(TIMEOUT_SECONDS)
        record["events"].append(
            {
                "kind": "control_wait_signaled",
                "before_ns": before,
                "after_ns": time.perf_counter_ns(),
                "exit_code": process.returncode,
            }
        )
        signaled = observer.snapshot(job, "control_after_signal_before_handle_close")
        evidence = signaled["classification"]
        if not evidence["conclusive_membership"] or evidence["live_pids"]:
            raise RuntimeError("Retirement control cannot prove every listed member exited")
        if not signaled["accounting"]["bool"] or signaled["accounting"]["values"]["TotalProcesses"] != 1:
            raise RuntimeError("Retirement control contains unexpected or unknown membership")
        if signaled["leader"]["wait_result"] != 0 or not signaled["leader"]["exit_ok"] or process.returncode != 0:
            raise RuntimeError("Retirement control leader did not exit successfully")
        record["events"].append({"kind": "control_process_close_before", "time_ns": time.perf_counter_ns()})
        process.close()
        record["events"].append({"kind": "control_process_close_after", "time_ns": time.perf_counter_ns()})
        observer.snapshot(job, "control_after_handle_close")
        # Existing bounded accounting retirement wait, only in this independent
        # control. It does not replace the original fail-closed decision.
        job.wait_empty(module.WINDOWS_JOB_WAIT_SECONDS)
        observer.snapshot(job, "control_after_accounting_retirement")
        record["outcome"] = "retirement_observed"
    finally:
        try:
            job.close()
        finally:
            if process is not None:
                process.close()


def validate_record(record: dict) -> list[str]:
    """Keep a natural failure, failed control, or uncertain observation red."""
    failures = []
    events = record.get("events", [])
    snapshots = [event for event in events if event.get("kind") == "snapshot"]
    if not snapshots:
        failures.append("no_snapshots")
    if any(event.get("kind") == "observer_error" for event in events):
        failures.append("observer_error")
    for snapshot in snapshots:
        if not snapshot["accounting"]["bool"] or not snapshot["classification"]["conclusive_membership"]:
            failures.append("inconclusive_snapshot:" + snapshot["phase"])
        if any(member.get("close_ok") is False for member in snapshot["members"]):
            failures.append("observer_handle_close_failed")
        leader = snapshot.get("leader", {})
        if not leader.get("handle_closed") and not (
            leader.get("identity_ok") and leader.get("exit_ok") and leader.get("wait_result") in (0, 258)
        ):
            failures.append("leader_identity_or_liveness_unproven:" + snapshot["phase"])
    mode = record["mode"]
    phases = {snapshot["phase"] for snapshot in snapshots}
    if mode != "separate_handle_retirement":
        required = {"before_leader_resume", "original_failure_seam", "before_original_job_close"}
        if not required.issubset(phases):
            failures.append("missing_required_original_phases")
        counts = [event for event in events if event.get("kind") == "original_returned_count"]
        seams = [snapshot for snapshot in snapshots if snapshot["phase"] == "original_failure_seam"]
        if not counts or not seams or counts[0]["count"] != seams[0]["accounting"]["values"]["ActiveProcesses"]:
            failures.append("original_count_evidence_missing_or_inconsistent")
        if not seams or seams[0]["leader"].get("wait_result") != 0 or seams[0]["leader"].get("exit_code") != 0:
            failures.append("successful_signaled_leader_unproven")
    if mode == "original_normal_exit":
        if record["outcome"] != "returned":
            failures.append("original_normal_exit_raised")
    elif mode == "live_descendant":
        seams = [snapshot for snapshot in snapshots if snapshot["phase"] == "original_failure_seam"]
        if not seams or not seams[0]["classification"]["live_descendant_pids"]:
            failures.append("live_descendant_not_proven")
        if record["outcome"] != "raised" or record.get("exception_type") != "RuntimeError":
            failures.append("live_descendant_not_rejected")
        if "descendants survived completion" not in record.get("exception_message", ""):
            failures.append("live_descendant_wrong_failure")
        cleanup = [snapshot for snapshot in snapshots if snapshot["phase"] == "before_original_job_close"]
        if not cleanup or cleanup[-1]["accounting"]["values"]["ActiveProcesses"] != 0:
            failures.append("live_descendant_cleanup_unproven")
    else:
        required = {
            "control_before_leader_wait",
            "control_after_signal_before_handle_close",
            "control_after_handle_close",
            "control_after_accounting_retirement",
        }
        if not required.issubset(phases) or record["outcome"] != "retirement_observed":
            failures.append("retirement_control_failed")
        retired = [snapshot for snapshot in snapshots if snapshot["phase"] == "control_after_accounting_retirement"]
        if not retired or retired[-1]["accounting"]["values"]["ActiveProcesses"] != 0:
            failures.append("retirement_not_proven")
    return failures


def run(output: Path) -> int:
    """Execute the fixed three-case delivery once and persist all outcomes."""
    require_authorized_runner()
    load_helper()
    output.mkdir(exist_ok=True)
    persist(output / "metadata.json", metadata())
    observer_module = load_file("core2442_observer", HERE / "core2442_windows_observer.py")
    started = time.monotonic()
    summary = {"cases": [], "attempts": 0, "maximum_cases": 3}
    for mode in MODES:
        # This is a fixed list of distinct controls, not a retry loop.
        if time.monotonic() - started > 45:
            summary["stopped"] = "diagnostic deadline reached; remaining cases not started"
            break
        module = load_helper()
        record = {
            "mode": mode,
            "events": [],
            "subject_timeout_seconds": TIMEOUT_SECONDS,
            "command_label": "python -c raise-SystemExit-0" if mode != "live_descendant" else "owned-event-child",
        }
        observer = observer_module.Observer(module, record["events"])
        summary["attempts"] += 1
        before = time.monotonic()
        try:
            if mode == "separate_handle_retirement":
                retirement_control(module, observer, record)
            else:
                observer.install()
                command = [sys.executable, "-c", "raise SystemExit(0)"]
                if mode == "live_descendant":
                    command = [sys.executable, "-I", "-B", str(Path(__file__).resolve()), "--control", "leader"]
                module.run_bounded(command, timeout_seconds=TIMEOUT_SECONDS)
                record["outcome"] = "returned"
        except Exception as exc:
            record.update(outcome="raised", exception_type=type(exc).__name__)
            # No traceback, environment, or arbitrary command arguments are
            # published. Helper errors are fixed strings except known counts.
            record["exception_message"] = str(exc) if isinstance(exc, RuntimeError) else type(exc).__name__
        record["elapsed_seconds"] = time.monotonic() - before
        record["validation_failures"] = validate_record(record)
        persist(output / (mode + ".json"), record)
        summary["cases"].append(
            {
                "mode": mode,
                "outcome": record["outcome"],
                "elapsed_seconds": record["elapsed_seconds"],
                "validation_failures": record["validation_failures"],
            }
        )
    summary["elapsed_seconds"] = time.monotonic() - started
    summary["status"] = (
        "failed"
        if summary.get("stopped") or any(case["validation_failures"] for case in summary["cases"])
        else "no_natural_failure_observed"
    )
    persist(output / "summary.json", summary)
    print(json.dumps(summary), flush=True)
    return 1 if summary["status"] == "failed" else 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--control", choices=("leader", "child"))
    parser.add_argument("--event")
    arguments = parser.parse_args()
    if arguments.control:
        live_control(arguments.control, arguments.event)
    elif arguments.output:
        raise SystemExit(run(arguments.output))
    else:
        parser.error("--output is required for the authorized CI diagnostic")
