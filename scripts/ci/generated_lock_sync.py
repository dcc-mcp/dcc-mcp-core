#!/usr/bin/env python3
"""Fail-closed generated-lock synchronization helpers.

Generation is intentionally executed with a scrubbed environment.  The
workflow performs the read-only identity/diff checks before creating a local
commit and uses a force-with-lease push for the final, narrowly scoped write.
"""

from __future__ import annotations

import argparse
from contextlib import suppress
import ctypes
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
from threading import Event
from threading import Thread
import time
from typing import Iterable
from typing import Mapping
from typing import NamedTuple
from typing import NoReturn
from typing import Sequence
from urllib.parse import urlparse

try:
    import _winapi
except ImportError:  # pragma: no cover - Windows-only stdlib module
    _winapi = None


LOCK_OUTPUTS = frozenset(("Cargo.lock", "uv.lock", "crates/workspace-hack/Cargo.toml"))
BRANCH_PREFIXES = ("release-please--branches--main", "renovate/")
GIT_COMMAND_TIMEOUT_SECONDS = 30
WINDOWS_JOB_WAIT_SECONDS = 5.0
WINDOWS_CREATE_SUSPENDED = 0x00000004
# POSIX systems without a child subreaper (notably macOS) can reparent an
# escaped descendant while the leader is exiting.  Keep a bounded second
# convergence window long enough for that reparent/fork transition to become
# observable before any caller can proceed to credential-bearing work.
POSIX_CONVERGENCE_PASSES = 50
POSIX_CONVERGENCE_INTERVAL_SECONDS = 0.05
CREDENTIAL_ENV_KEYS = frozenset(
    (
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "PERSONAL_ACCESS_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "GIT_ASKPASS",
        "SSH_AUTH_SOCK",
    )
)


class _JobBasicLimitInformation(ctypes.Structure):
    """Windows JOBOBJECT_BASIC_LIMIT_INFORMATION layout."""

    _fields_ = [
        ("PerProcessUserTimeLimit", ctypes.c_longlong),
        ("PerJobUserTimeLimit", ctypes.c_longlong),
        ("LimitFlags", ctypes.c_ulong),
        ("MinimumWorkingSetSize", ctypes.c_size_t),
        ("MaximumWorkingSetSize", ctypes.c_size_t),
        ("ActiveProcessLimit", ctypes.c_ulong),
        ("Affinity", ctypes.c_size_t),
        ("PriorityClass", ctypes.c_ulong),
        ("SchedulingClass", ctypes.c_ulong),
    ]


class _IoCounters(ctypes.Structure):
    """Windows IO_COUNTERS layout."""

    _fields_ = [
        ("ReadOperationCount", ctypes.c_ulonglong),
        ("WriteOperationCount", ctypes.c_ulonglong),
        ("OtherOperationCount", ctypes.c_ulonglong),
        ("ReadTransferCount", ctypes.c_ulonglong),
        ("WriteTransferCount", ctypes.c_ulonglong),
        ("OtherTransferCount", ctypes.c_ulonglong),
    ]


class _JobExtendedLimitInformation(ctypes.Structure):
    """Windows JOBOBJECT_EXTENDED_LIMIT_INFORMATION layout."""

    _fields_ = [
        ("BasicLimitInformation", _JobBasicLimitInformation),
        ("IoInfo", _IoCounters),
        ("ProcessMemoryLimit", ctypes.c_size_t),
        ("JobMemoryLimit", ctypes.c_size_t),
        ("PeakProcessMemoryUsed", ctypes.c_size_t),
        ("PeakJobMemoryUsed", ctypes.c_size_t),
    ]


class _JobBasicAccountingInformation(ctypes.Structure):
    """Windows JOBOBJECT_BASIC_ACCOUNTING_INFORMATION layout."""

    _fields_ = [
        ("TotalUserTime", ctypes.c_longlong),
        ("TotalKernelTime", ctypes.c_longlong),
        ("ThisPeriodTotalUserTime", ctypes.c_longlong),
        ("ThisPeriodTotalKernelTime", ctypes.c_longlong),
        ("TotalPageFaultCount", ctypes.c_ulong),
        ("TotalProcesses", ctypes.c_ulong),
        ("ActiveProcesses", ctypes.c_ulong),
        ("TotalTerminatedProcesses", ctypes.c_ulong),
    ]


def _configure_process_api(kernel32) -> None:
    """Declare the Win32 calls owned by a suspended process handle pair."""
    kernel32.ResumeThread.argtypes = (ctypes.c_void_p,)
    kernel32.ResumeThread.restype = ctypes.c_ulong
    kernel32.WaitForSingleObject.argtypes = (ctypes.c_void_p, ctypes.c_ulong)
    kernel32.WaitForSingleObject.restype = ctypes.c_ulong
    kernel32.GetExitCodeProcess.argtypes = (ctypes.c_void_p, ctypes.c_void_p)
    kernel32.GetExitCodeProcess.restype = ctypes.c_int
    kernel32.TerminateProcess.argtypes = (ctypes.c_void_p, ctypes.c_uint)
    kernel32.TerminateProcess.restype = ctypes.c_int
    kernel32.CloseHandle.argtypes = (ctypes.c_void_p,)
    kernel32.CloseHandle.restype = ctypes.c_int


class _SuspendedWindowsProcess:
    """Own the process and primary-thread handles returned by CreateProcess."""

    _WAIT_OBJECT_0 = 0
    _WAIT_TIMEOUT = 0x00000102
    _RESUME_FAILED = 0xFFFFFFFF

    def __init__(
        self,
        *,
        process_handle: int,
        thread_handle: int,
        pid: int,
        command: Sequence[str],
        kernel32,
    ) -> None:
        self._process_handle = process_handle
        self._thread_handle = thread_handle
        self.pid = pid
        self._command = tuple(command)
        self._kernel32 = kernel32
        self.returncode: int | None = None

    @property
    def process_handle(self) -> int:
        """Return the physical process handle used for Job assignment."""
        return self._process_handle

    def _close_owned_handle(self, attribute: str, description: str) -> None:
        handle = getattr(self, attribute, None)
        if handle is None:
            return
        if not self._kernel32.CloseHandle(ctypes.c_void_p(handle)):
            raise RuntimeError(f"could not close Windows {description} handle: error {ctypes.get_last_error()}")
        setattr(self, attribute, None)

    def resume_initial_thread(self) -> int:
        """Release exactly the suspension introduced by CREATE_SUSPENDED."""
        handle = self._thread_handle
        if handle is None:
            raise RuntimeError("Windows process primary-thread handle is unavailable")
        error: RuntimeError | None = None
        previous_count: int | None = None
        result = self._kernel32.ResumeThread(ctypes.c_void_p(handle))
        if result == self._RESUME_FAILED:
            error = RuntimeError(f"could not resume suspended process primary thread: error {ctypes.get_last_error()}")
        elif result < 1:
            error = RuntimeError("could not resume suspended process primary thread: it was not suspended")
        else:
            previous_count = int(result)
        try:
            self._close_owned_handle("_thread_handle", "primary-thread")
        except RuntimeError as close_error:
            error = error or close_error
        if error is not None:
            raise error
        assert previous_count is not None
        return previous_count

    def wait(self, timeout_seconds: float) -> int:
        """Wait a bounded interval for this physical process object."""
        if self.returncode is not None:
            return self.returncode
        wait_ms = max(1, min(0xFFFFFFFE, int(timeout_seconds * 1000)))
        result = self._kernel32.WaitForSingleObject(ctypes.c_void_p(self._process_handle), wait_ms)
        if result == self._WAIT_TIMEOUT:
            raise subprocess.TimeoutExpired(self._command, timeout_seconds)
        if result != self._WAIT_OBJECT_0:
            raise RuntimeError(f"Windows process wait failed: result {result}")
        exit_code = ctypes.c_ulong()
        if not self._kernel32.GetExitCodeProcess(ctypes.c_void_p(self._process_handle), ctypes.byref(exit_code)):
            raise RuntimeError(f"could not read Windows process exit code: error {ctypes.get_last_error()}")
        self.returncode = int(exit_code.value)
        return self.returncode

    def terminate(self) -> None:
        """Terminate this exact process object without consulting its PID."""
        if not self._kernel32.TerminateProcess(ctypes.c_void_p(self._process_handle), 1):
            raise RuntimeError(f"could not terminate Windows process: error {ctypes.get_last_error()}")

    def close(self) -> None:
        """Close every remaining owned process-creation handle."""
        first_error: RuntimeError | None = None
        for attribute, description in (
            ("_thread_handle", "primary-thread"),
            ("_process_handle", "process"),
        ):
            try:
                self._close_owned_handle(attribute, description)
            except RuntimeError as exc:
                first_error = first_error or exc
        if first_error is not None:
            raise first_error


def _create_suspended_windows_process(
    command: Sequence[str],
    *,
    cwd: Path | None,
    env: Mapping[str, str] | None,
) -> _SuspendedWindowsProcess:
    """Create a suspended process while retaining both returned handles."""
    if os.name != "nt" or _winapi is None:
        raise RuntimeError("Windows process creation is unavailable on this platform")
    arguments = tuple(os.fsdecode(os.fspath(value)) for value in command)
    if not arguments:
        raise ValueError("Windows process command must not be empty")
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _configure_process_api(kernel32)
    startup_info = subprocess.STARTUPINFO()
    creation_flags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0) | WINDOWS_CREATE_SUSPENDED
    process_handle, thread_handle, pid, _thread_id = _winapi.CreateProcess(
        None,
        subprocess.list2cmdline(arguments),
        None,
        None,
        0,
        creation_flags,
        dict(env) if env is not None else None,
        str(cwd) if cwd is not None else None,
        startup_info,
    )
    return _SuspendedWindowsProcess(
        process_handle=int(process_handle),
        thread_handle=int(thread_handle),
        pid=int(pid),
        command=arguments,
        kernel32=kernel32,
    )


class WindowsJob:
    """Own a kill-on-close Windows Job Object containment identity."""

    _JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9
    _JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION = 1
    _JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
    _WAIT_OBJECT_0 = 0
    _WAIT_TIMEOUT = 0x00000102

    def __init__(self) -> None:
        if os.name != "nt":
            raise RuntimeError("Windows Job Objects are unavailable on this platform")
        self._kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self._configure_api()
        self._handle = self._kernel32.CreateJobObjectW(None, None)
        if not self._handle:
            raise RuntimeError(f"could not create Windows Job Object: error {ctypes.get_last_error()}")
        info = _JobExtendedLimitInformation()
        info.BasicLimitInformation.LimitFlags = self._JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not self._kernel32.SetInformationJobObject(
            self._handle,
            self._JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            ctypes.byref(info),
            ctypes.sizeof(info),
        ):
            error = ctypes.get_last_error()
            self.close()
            raise RuntimeError(f"could not configure Windows Job Object: error {error}")

    def _configure_api(self) -> None:
        self._kernel32.CreateJobObjectW.argtypes = (ctypes.c_void_p, ctypes.c_wchar_p)
        self._kernel32.CreateJobObjectW.restype = ctypes.c_void_p
        self._kernel32.SetInformationJobObject.argtypes = (
            ctypes.c_void_p,
            ctypes.c_int,
            ctypes.c_void_p,
            ctypes.c_ulong,
        )
        self._kernel32.SetInformationJobObject.restype = ctypes.c_int
        self._kernel32.AssignProcessToJobObject.argtypes = (ctypes.c_void_p, ctypes.c_void_p)
        self._kernel32.AssignProcessToJobObject.restype = ctypes.c_int
        self._kernel32.QueryInformationJobObject.argtypes = (
            ctypes.c_void_p,
            ctypes.c_int,
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_void_p,
        )
        self._kernel32.QueryInformationJobObject.restype = ctypes.c_int
        self._kernel32.TerminateJobObject.argtypes = (ctypes.c_void_p, ctypes.c_uint)
        self._kernel32.TerminateJobObject.restype = ctypes.c_int
        self._kernel32.WaitForSingleObject.argtypes = (ctypes.c_void_p, ctypes.c_ulong)
        self._kernel32.WaitForSingleObject.restype = ctypes.c_ulong
        self._kernel32.CloseHandle.argtypes = (ctypes.c_void_p,)
        self._kernel32.CloseHandle.restype = ctypes.c_int

    def assign(self, process: _SuspendedWindowsProcess) -> None:
        """Assign a suspended process by its owned process handle."""
        if not self._kernel32.AssignProcessToJobObject(
            self._handle,
            ctypes.c_void_p(process.process_handle),
        ):
            raise RuntimeError(f"could not assign process to Windows Job Object: error {ctypes.get_last_error()}")

    def active_processes(self) -> int:
        """Return the count of live processes physically owned by the job."""
        accounting = _JobBasicAccountingInformation()
        if not self._kernel32.QueryInformationJobObject(
            self._handle,
            self._JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION,
            ctypes.byref(accounting),
            ctypes.sizeof(accounting),
            None,
        ):
            raise RuntimeError(f"could not query Windows Job Object: error {ctypes.get_last_error()}")
        return int(accounting.ActiveProcesses)

    def terminate(self) -> None:
        """Terminate every process owned by the job."""
        if not self._kernel32.TerminateJobObject(self._handle, 1):
            raise RuntimeError(f"could not terminate Windows Job Object: error {ctypes.get_last_error()}")

    def wait_empty(self, timeout_seconds: float) -> None:
        """Wait a bounded interval for the job to report zero active processes."""
        deadline = time.monotonic() + timeout_seconds
        while True:
            active = self.active_processes()
            if not active:
                return
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise RuntimeError(f"Windows Job Object cleanup timed out with {active} active process(es)")
            wait_ms = max(1, min(50, int(remaining * 1000)))
            result = self._kernel32.WaitForSingleObject(self._handle, wait_ms)
            if result not in (self._WAIT_OBJECT_0, self._WAIT_TIMEOUT):
                raise RuntimeError(f"Windows Job Object wait failed: result {result}")

    def close(self) -> None:
        """Close the Job handle; kill-on-close is the last-resort fence."""
        handle = getattr(self, "_handle", None)
        if handle:
            self._handle = None
            if not self._kernel32.CloseHandle(handle):
                raise RuntimeError(f"could not close Windows Job Object: error {ctypes.get_last_error()}")


class PullRequestIdentity(NamedTuple):
    """Immutable identity tuple for the eligible pull request head."""

    repository: str
    number: int
    head_repository: str
    head_branch: str
    head_sha: str
    title: str


def sanitized_environment(source: Mapping[str, str], *, isolated_home: Path) -> dict[str, str]:
    """Return an environment safe for project-controlled build backends."""
    env = {key: value for key, value in source.items() if key not in CREDENTIAL_ENV_KEYS}
    # Do not allow a repository checkout to re-enable a credential-bearing
    # global/system Git config or prompt for credentials.
    env.pop("GIT_CONFIG_GLOBAL", None)
    env.pop("GIT_CONFIG_SYSTEM", None)
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["GIT_CONFIG_GLOBAL"] = os.devnull
    env["GIT_TERMINAL_PROMPT"] = "0"
    # Keep user-level Git/Cargo/pip/uv/cloud stores outside the runner user's
    # home for the entire generation subprocess lifetime.
    root = str(isolated_home)
    env["HOME"] = root
    env["USERPROFILE"] = root
    env["XDG_CONFIG_HOME"] = str(isolated_home / "config")
    env["XDG_DATA_HOME"] = str(isolated_home / "data")
    env["XDG_CACHE_HOME"] = str(isolated_home / "cache")
    env["CARGO_HOME"] = str(isolated_home / "cargo")
    env["PIP_CONFIG_FILE"] = str(isolated_home / "pip.conf")
    env["UV_CONFIG_FILE"] = str(isolated_home / "uv.toml")
    return env


def validate_identity(expected: PullRequestIdentity, observed: PullRequestIdentity) -> list[str]:
    """Compare every PR identity field and reject forks/unsupported branches."""
    errors: list[str] = []
    if observed.head_repository != observed.repository:
        errors.append("forked pull requests are not eligible for generated-lock writes")
    if observed.repository != expected.repository:
        errors.append("repository identity drift")
    if observed.number != expected.number:
        errors.append("pull request number drift")
    if observed.head_repository != expected.head_repository:
        errors.append("head repository identity drift")
    if observed.head_branch != expected.head_branch:
        errors.append("head branch identity drift")
    if observed.head_sha != expected.head_sha:
        errors.append("head SHA identity drift")
    if observed.title != expected.title:
        errors.append("pull request title identity drift")
    if not observed.head_branch.startswith(BRANCH_PREFIXES):
        errors.append("head branch is outside the approved release/automation branch class")
    if observed.head_branch.startswith("release-please--branches--main") and not observed.title.startswith(
        "chore(main): release "
    ):
        errors.append("release-please branch has an unexpected title")
    return errors


def validate_changed_files(paths: Iterable[str]) -> list[str]:
    """Return a stable error when generation touched anything beyond lock outputs."""
    unexpected = sorted(set(paths) - LOCK_OUTPUTS)
    if unexpected:
        return [f"unexpected generated-lock diff paths: {', '.join(unexpected)}"]
    return []


def run_generation(root: Path, *, timeout_seconds: int = 900) -> None:
    """Run lock generators with bounded timeouts and a scrubbed environment."""
    with tempfile.TemporaryDirectory(prefix="dcc-lock-sync-") as isolated_home:
        env = sanitized_environment(os.environ, isolated_home=Path(isolated_home))
        commands = (("cargo", "update", "-w"), ("cargo", "hakari", "generate"), ("vx", "uv", "lock"))
        for command in commands:
            run_bounded(command, cwd=root, env=env, timeout_seconds=timeout_seconds)


def process_exists(pid: int) -> bool:
    """Return whether a process ID is still live."""
    if os.name == "nt":
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.argtypes = (ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong)
        kernel32.OpenProcess.restype = ctypes.c_void_p
        kernel32.WaitForSingleObject.argtypes = (ctypes.c_void_p, ctypes.c_ulong)
        kernel32.WaitForSingleObject.restype = ctypes.c_ulong
        kernel32.CloseHandle.argtypes = (ctypes.c_void_p,)
        kernel32.CloseHandle.restype = ctypes.c_int
        handle = kernel32.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
        if not handle:
            error = ctypes.get_last_error()
            if error == 87:  # ERROR_INVALID_PARAMETER: no such process
                return False
            if error == 5:  # ERROR_ACCESS_DENIED still proves that the PID is live
                return True
            raise RuntimeError(f"could not open Windows process handle for PID {pid}: error {error}")
        try:
            result = kernel32.WaitForSingleObject(handle, 0)
            if result == 0:
                return False
            if result == 0x00000102:
                return True
            raise RuntimeError(f"Windows process wait failed for PID {pid}: result {result}")
        finally:
            if not kernel32.CloseHandle(handle):
                raise RuntimeError(
                    f"could not close Windows process handle for PID {pid}: error {ctypes.get_last_error()}"
                )
    try:
        if sys.platform.startswith("linux"):
            stat = Path(f"/proc/{pid}/stat")
            if not stat.exists():
                return False
            if stat.read_text(encoding="utf-8").split()[2] == "Z":
                return False
        os.kill(pid, 0)
    except (OSError, ProcessLookupError):
        return False
    return True


def _terminate_windows_job(job: WindowsJob, process: _SuspendedWindowsProcess) -> None:
    """Terminate the owned job and confirm every member plus the leader exited."""
    job.terminate()
    job.wait_empty(WINDOWS_JOB_WAIT_SECONDS)
    try:
        process.wait(WINDOWS_JOB_WAIT_SECONDS)
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError("Windows process handle did not signal after Job Object cleanup") from exc


def run_bounded_windows(
    command: Sequence[str],
    *,
    cwd: Path | None,
    env: Mapping[str, str] | None,
    timeout_seconds: float,
) -> None:
    """Run a command in a Job Object established before its first instruction."""
    job = WindowsJob()
    process: _SuspendedWindowsProcess | None = None
    assigned = False
    try:
        process = _create_suspended_windows_process(command, cwd=cwd, env=env)
        try:
            job.assign(process)
            assigned = True
            process.resume_initial_thread()
        except RuntimeError as exc:
            try:
                if assigned:
                    _terminate_windows_job(job, process)
                else:
                    process.terminate()
                    process.wait(WINDOWS_JOB_WAIT_SECONDS)
            except RuntimeError as cleanup_error:
                raise RuntimeError("could not establish Windows process containment; cleanup failed") from cleanup_error
            raise RuntimeError("could not establish Windows process containment") from exc
        try:
            process.wait(timeout_seconds)
        except subprocess.TimeoutExpired:
            _terminate_windows_job(job, process)
            raise
        active = job.active_processes()
        if active:
            _terminate_windows_job(job, process)
            raise RuntimeError(
                f"process containment failed; descendants survived completion: {active} Windows Job Object process(es)"
            )
        job.wait_empty(0)
        if process.returncode:
            raise subprocess.CalledProcessError(process.returncode, command)
    finally:
        # Close the Job first so KILL_ON_JOB_CLOSE remains the final process
        # fence even if an earlier setup/query path failed unexpectedly.
        try:
            job.close()
        finally:
            if process is not None:
                process.close()


def run_bounded(
    command: Sequence[str],
    *,
    cwd: Path | None = None,
    env: Mapping[str, str] | None = None,
    timeout_seconds: float,
) -> None:
    """Run a command in a process group/tree and kill descendants on timeout."""
    if os.name not in ("nt", "posix"):
        raise RuntimeError("process containment is unavailable on this platform")
    if os.name == "nt":
        run_bounded_windows(
            command,
            cwd=cwd,
            env=env,
            timeout_seconds=timeout_seconds,
        )
        return
    # macOS has no child-subreaper or cgroup equivalent.  A detached setsid
    # descendant can be reparented to launchd after its intermediate leader
    # exits, making zero-residual containment unprovable.  Refuse to start
    # generation there rather than risking credentials after an escape.
    if sys.platform == "darwin":
        raise RuntimeError("process containment is unavailable on macOS")
    _enable_child_subreaper()
    baseline = set(_descendant_pids(os.getpid()))
    process = subprocess.Popen(
        command,
        cwd=str(cwd) if cwd is not None else None,
        env=dict(env) if env is not None else None,
        start_new_session=True,
    )
    observed: set[int] = set()
    observer_errors: list[RuntimeError] = []
    stop_observer = Event()

    def collect_descendants() -> None:
        for root in (process.pid, *tuple(observed)):
            observed.update(_descendant_pids(root))
        observed.update(set(_descendant_pids(os.getpid())) - baseline - {process.pid})

    def observe_descendants() -> None:
        while not stop_observer.is_set():
            try:
                collect_descendants()
            except RuntimeError as exc:
                observer_errors.append(exc)
                stop_observer.set()
                return
            stop_observer.wait(0.02)

    observer = Thread(target=observe_descendants, daemon=True)
    observer.start()
    try:
        process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        # Capture the complete tree before terminating the leader.  A POSIX
        # child can call setsid() and leave the leader's process group; taking
        # this snapshot lets the fail-closed cleanup still target that PID.
        stop_observer.set()
        observer.join()
        descendants = set(observed)
        cleanup_error: RuntimeError | None = None
        try:
            collect_descendants()
        except RuntimeError as exc:
            cleanup_error = exc
        descendants.update(observed)
        with suppress(ProcessLookupError):
            os.killpg(process.pid, signal.SIGKILL)
        for _ in range(POSIX_CONVERGENCE_PASSES):
            try:
                # Once the leader exits, escaped descendants are reparented
                # to this process (the Linux subreaper). Converge over both
                # roots so an escaped child cannot be hidden by reparenting.
                current = set(_descendant_pids(process.pid))
                current.update(set(_descendant_pids(os.getpid())) - baseline - {process.pid})
            except RuntimeError as exc:
                cleanup_error = cleanup_error or exc
                current = set()
            descendants.update(current)
            for pid in descendants | current:
                with suppress(ProcessLookupError):
                    os.kill(pid, signal.SIGKILL)
            if not current:
                break
            time.sleep(POSIX_CONVERGENCE_INTERVAL_SECONDS)
        try:
            process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            raise RuntimeError("process containment failed; command pipes did not close after cleanup") from None
        remaining = [pid for pid in descendants if process_exists(pid)]
        if remaining:
            raise RuntimeError(f"process containment failed; descendants survived timeout: {remaining}") from None
        if cleanup_error is not None:
            raise RuntimeError("process containment enumeration failed during timeout cleanup") from cleanup_error
        raise
    finally:
        stop_observer.set()
        observer.join()
        # Stop observation before the final bounded convergence pass so a
        # last fork racing with process exit cannot be hidden by a stale
        # observer snapshot.
        for _ in range(POSIX_CONVERGENCE_PASSES):
            try:
                collect_descendants()
            except RuntimeError as exc:
                observer_errors.append(exc)
                break
            time.sleep(POSIX_CONVERGENCE_INTERVAL_SECONDS)
    collect_descendants()
    if observer_errors:
        raise RuntimeError("process observer failed; containment is not provable") from observer_errors[0]
    escaped = [pid for pid in observed if process_exists(pid)]
    if escaped:
        for pid in escaped:
            with suppress(ProcessLookupError):
                os.kill(pid, signal.SIGKILL)
        # SIGKILL delivery is asynchronous; converge with bounded probes so
        # callers never proceed while an escaped daemon still has a chance to
        # observe subsequent credentials.
        remaining = set(escaped)
        for _ in range(POSIX_CONVERGENCE_PASSES):
            remaining = {pid for pid in remaining if process_exists(pid)}
            if not remaining:
                break
            for pid in remaining:
                with suppress(ProcessLookupError):
                    os.kill(pid, signal.SIGKILL)
            time.sleep(POSIX_CONVERGENCE_INTERVAL_SECONDS)
        if remaining:
            raise RuntimeError(f"process containment failed; descendants survived completion: {sorted(remaining)}")
        raise RuntimeError(f"process containment failed; descendants survived completion: {escaped}")
    if process.returncode:
        raise subprocess.CalledProcessError(process.returncode, command)


def _enable_child_subreaper() -> None:
    """Arrange for escaped POSIX descendants to be reparented to this process."""
    if os.name != "posix" or not sys.platform.startswith("linux"):
        return
    try:
        libc = ctypes.CDLL(None)
        libc.prctl(36, 1, 0, 0, 0)  # PR_SET_CHILD_SUBREAPER
    except (AttributeError, OSError):
        return


def _descendant_pids(root_pid: int) -> list[int]:
    """Return the currently observable descendant process IDs."""
    children: dict[int, list[int]] = {}
    try:
        result = subprocess.run(("ps", "-eo", "pid=,ppid="), check=False, stdout=subprocess.PIPE, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise RuntimeError("process enumeration timed out or failed") from exc
    if result.returncode != 0:
        raise RuntimeError(f"process enumeration failed with exit {result.returncode}")
    entries = []
    for line in result.stdout.splitlines():
        fields = line.split()
        if len(fields) == 2:
            try:
                entries.append((int(fields[0]), int(fields[1])))
            except ValueError as exc:
                raise RuntimeError("process enumeration returned malformed output") from exc
    for pid, parent in entries:
        children.setdefault(parent, []).append(pid)
    pending = list(children.get(root_pid, []))
    descendants: list[int] = []
    while pending:
        pid = pending.pop()
        descendants.append(pid)
        pending.extend(children.get(pid, []))
    return descendants


def _remote_repository(url: str) -> str | None:
    """Parse a GitHub remote URL into owner/repository, rejecting ambiguity."""
    value = url.strip()
    if value.startswith("git@"):
        prefix, separator, path = value.partition(":")
        if separator and prefix == "git@github.com":
            return path[:-4].strip("/") if path.endswith(".git") else path.strip("/")
        return None
    parsed = urlparse(value)
    if parsed.scheme not in ("https", "ssh") or parsed.hostname != "github.com" or parsed.port is not None:
        return None
    if parsed.username not in (None, "git") or parsed.password is not None:
        return None
    path = parsed.path
    return (path[:-4] if path.endswith(".git") else path).strip("/")


def validate_remote_url(url: str, expected_repository: str) -> list[str]:
    """Ensure a remote URL targets github.com and the expected owner/repo."""
    repository = _remote_repository(url)
    if repository != expected_repository:
        return [f"remote URL is not bound to expected repository {expected_repository!r}"]
    return []


def validate_remote_urls(urls: Iterable[str], expected_repository: str, kind: str) -> list[str]:
    """Require one and only one origin URL for each fetch/push direction."""
    entries = [url.strip() for url in urls if url.strip()]
    if len(entries) != 1:
        return [f"origin {kind} URL must have exactly one configured entry"]
    return validate_remote_url(entries[0], expected_repository)


def validate_remote(root: Path, expected_repository: str) -> None:
    """Validate both fetch and push URLs before any generated-lock mutation."""
    for kind, args in (
        ("fetch", ("get-url", "--all", "origin")),
        ("push", ("get-url", "--push", "--all", "origin")),
    ):
        result = subprocess.run(("git", "remote", *args), cwd=str(root), check=False, stdout=subprocess.PIPE, text=True)
        if result.returncode != 0:
            _fail(f"could not read origin {kind} URL")
        errors = validate_remote_urls(result.stdout.splitlines(), expected_repository, kind)
        if errors:
            _fail(f"origin {kind} URL rejected: {errors[0]}")
    config = subprocess.run(
        ("git", "config", "--local", "--name-only", "--get-regexp", ".*"),
        cwd=str(root),
        check=False,
        capture_output=True,
        text=True,
        timeout=GIT_COMMAND_TIMEOUT_SECONDS,
    )
    if config.returncode not in (0, 1):
        _fail("could not inspect local Git configuration")
    proxy_keys = [
        key.strip()
        for key in config.stdout.splitlines()
        if key.strip().lower().endswith(".proxy") or key.strip().lower() == "core.gitproxy"
    ]
    if proxy_keys:
        _fail(f"local Git proxy configuration is not allowed: {proxy_keys[0]}")


def validate_force_with_lease(expected_sha: str, observed_sha: str) -> list[str]:
    """Return an error when the remote head changed since preflight."""
    if expected_sha != observed_sha:
        return ["stale head: remote branch advanced since preflight"]
    return []


def _fail(message: str) -> NoReturn:
    print(f"::error::{message}", file=sys.stderr)
    raise SystemExit(1)


def _identity_from_env() -> PullRequestIdentity:
    values = {
        "repository": os.environ.get("GITHUB_REPOSITORY", ""),
        "head_repository": os.environ.get("PR_HEAD_REPOSITORY", ""),
        "head_branch": os.environ.get("PR_HEAD_REF", ""),
        "head_sha": os.environ.get("PR_HEAD_SHA", ""),
        "title": os.environ.get("PR_TITLE", ""),
    }
    try:
        number = int(os.environ.get("PR_NUMBER", "0"))
    except ValueError:
        number = 0
    if not values["repository"] or not values["head_repository"] or not values["head_branch"] or not values["head_sha"]:
        _fail("all pull request identity fields are required")
    return PullRequestIdentity(number=number, **values)


def _git(root: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ("git", *args),
            cwd=str(root),
            check=True,
            stdout=subprocess.PIPE,
            text=True,
            timeout=GIT_COMMAND_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError("git command timed out") from exc
    return result.stdout.strip()


def _remote_preflight(expected: PullRequestIdentity) -> None:
    """Re-capture the remote PR identity using a read-only token."""
    if os.environ.get("DCC_LOCK_SYNC_SKIP_REMOTE") == "1":
        return
    result = subprocess.run(
        ("gh", "api", f"repos/{expected.repository}/pulls/{expected.number}"),
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=30,
    )
    if result.returncode != 0:
        _fail(f"could not recapture pull request identity: {result.stdout.strip()}")
    payload = json.loads(result.stdout)
    if payload.get("state") != "open" or payload.get("base", {}).get("ref") != "main":
        _fail("pull request is no longer an open PR targeting main")
    if payload.get("base", {}).get("repo", {}).get("full_name") != expected.repository:
        _fail("pull request base repository identity drift")
    remote = PullRequestIdentity(
        repository=expected.repository,
        number=expected.number,
        head_repository=str(payload.get("head", {}).get("repo", {}).get("full_name", "")),
        head_branch=str(payload.get("head", {}).get("ref", "")),
        head_sha=str(payload.get("head", {}).get("sha", "")),
        title=str(payload.get("title", "")),
    )
    errors = validate_identity(expected, remote)
    if errors:
        _fail("; ".join(errors))


def preflight(root: Path) -> None:
    """Revalidate the exact local head and remote PR head."""
    expected = _identity_from_env()
    observed = PullRequestIdentity(
        repository=expected.repository,
        number=expected.number,
        head_repository=expected.head_repository,
        # The checkout is intentionally detached at the immutable PR SHA;
        # branch identity comes from the event and remote re-capture below.
        head_branch=expected.head_branch,
        head_sha=_git(root, "rev-parse", "HEAD"),
        title=expected.title,
    )
    errors = validate_identity(expected, observed)
    if errors:
        _fail("; ".join(errors))
    _remote_preflight(expected)


def verify_diff(root: Path) -> None:
    """Reject tracked or untracked changes outside the lock output allowlist."""
    paths = _git(root, "diff", "--name-only").splitlines()
    for status in _git(root, "status", "--porcelain=v1", "--untracked-files=all").splitlines():
        if not status:
            continue
        path = status[3:]
        if " -> " in path:
            paths.extend(path.split(" -> "))
        else:
            paths.append(path)
    errors = validate_changed_files(paths)
    if errors:
        _fail(errors[0])


def verify_commit(root: Path, expected_parent: str) -> None:
    """Ensure the commit has exactly the immutable PR head as its parent."""
    parents = _git(root, "rev-list", "--parents", "-n", "1", "HEAD").split()
    if len(parents) != 2 or parents[1] != expected_parent:
        _fail("generated commit parent is not the immutable original PR head")
    paths = _git(root, "diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD").splitlines()
    errors = validate_changed_files(paths)
    if errors:
        _fail(errors[0])


def main() -> None:
    """Dispatch the requested generated-lock contract operation."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        choices=("generate", "preflight", "remote-preflight", "validate-remote", "verify-diff", "verify-commit"),
    )
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--expected-parent", default=os.environ.get("PR_HEAD_SHA", ""))
    args = parser.parse_args()
    if args.command == "generate":
        run_generation(args.root)
    elif args.command == "preflight":
        preflight(args.root)
    elif args.command == "remote-preflight":
        _remote_preflight(_identity_from_env())
    elif args.command == "validate-remote":
        repository = os.environ.get("GITHUB_REPOSITORY", "")
        if not repository:
            _fail("GITHUB_REPOSITORY is required")
        validate_remote(args.root, repository)
    elif args.command == "verify-commit":
        if not args.expected_parent:
            _fail("expected immutable PR head is required")
        verify_commit(args.root, args.expected_parent)
    else:
        verify_diff(args.root)


if __name__ == "__main__":
    main()
