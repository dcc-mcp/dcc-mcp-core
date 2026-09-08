"""Test-only Win32 observations for the one-shot Core #2442 diagnostic.

No production import uses this module. Native query results and original
cleanup ordering are preserved. Only IDs listed by the owned Job are opened.
"""

from __future__ import annotations

import ctypes
import time


def tick() -> int:
    """Return a monotonic timestamp without wall-clock conversion."""
    return time.perf_counter_ns()


def classification(snapshot: dict) -> dict:
    """Classify captured evidence without interpreting unknowns as dead."""
    members = snapshot.get("members", [])
    complete = snapshot.get("process_list", {}).get("complete", False)
    listed_pids = snapshot.get("process_list", {}).get("pids", [])
    member_pids = [member.get("pid") for member in members]
    known = (
        complete
        and len(member_pids) == len(listed_pids) == len(set(listed_pids))
        and set(member_pids) == set(listed_pids)
        and all(
            member.get("identity_ok") and member.get("exit_ok") and member.get("wait_result") in (0, 258)
            for member in members
        )
    )
    leader_pid = snapshot.get("leader", {}).get("pid")
    live = [member["pid"] for member in members if member.get("identity_ok") and member.get("wait_result") == 258]
    return {
        "conclusive_membership": known,
        "live_pids": live,
        "live_descendant_pids": [pid for pid in live if pid != leader_pid],
    }


class Observer:
    """Observe an owned Job without replacing any product decision."""

    def __init__(self, module, events: list[dict]) -> None:
        self.module = module
        self.events = events
        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        for name, args, result in (
            ("OpenProcess", (ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong), ctypes.c_void_p),
            ("GetProcessId", (ctypes.c_void_p,), ctypes.c_ulong),
            ("GetProcessTimes", (ctypes.c_void_p,) * 5, ctypes.c_int),
            ("IsProcessInJob", (ctypes.c_void_p,) * 3, ctypes.c_int),
            ("GetExitCodeProcess", (ctypes.c_void_p,) * 2, ctypes.c_int),
            ("WaitForSingleObject", (ctypes.c_void_p, ctypes.c_ulong), ctypes.c_ulong),
            ("CloseHandle", (ctypes.c_void_p,), ctypes.c_int),
        ):
            function = getattr(self.api, name)
            function.argtypes, function.restype = args, result

    def identity(self, handle: int, job_handle: int, expected_pid: int) -> dict:
        """Read physical identity and liveness while a process handle is held."""
        creation, exit_time, kernel_time, user_time = [ctypes.c_ulonglong() for _ in range(4)]
        times_ok = self.api.GetProcessTimes(
            handle, ctypes.byref(creation), ctypes.byref(exit_time), ctypes.byref(kernel_time), ctypes.byref(user_time)
        )
        times_error = ctypes.get_last_error()
        belongs = ctypes.c_int()
        membership_ok = self.api.IsProcessInJob(handle, job_handle, ctypes.byref(belongs))
        membership_error = ctypes.get_last_error()
        wait_before = tick()
        wait = self.api.WaitForSingleObject(handle, 0)
        wait_error, wait_after = ctypes.get_last_error(), tick()
        exit_code = ctypes.c_ulong()
        exit_ok = self.api.GetExitCodeProcess(handle, ctypes.byref(exit_code))
        exit_error = ctypes.get_last_error()
        pid = int(self.api.GetProcessId(handle))
        return {
            "handle": int(handle),
            "expected_pid": expected_pid,
            "pid": pid,
            "creation_filetime": creation.value,
            "exit_filetime": exit_time.value,
            "times_ok": bool(times_ok),
            "times_error": times_error,
            "membership_ok": bool(membership_ok),
            "membership_error": membership_error,
            "in_job": bool(belongs.value),
            "wait_before_ns": wait_before,
            "wait_after_ns": wait_after,
            "wait_result": int(wait),
            "wait_error": wait_error,
            "exit_ok": bool(exit_ok),
            "exit_error": exit_error,
            "exit_code": exit_code.value,
            "identity_ok": bool(
                times_ok and creation.value and membership_ok and belongs.value and pid == expected_pid
            ),
        }

    def accounting(self, job, label: str) -> dict:
        """Query current accounting on the still-owned Job handle."""
        info = self.module._JobBasicAccountingInformation()
        before = tick()
        native = getattr(job._kernel32, "native", job._kernel32)
        ok = native.QueryInformationJobObject(job._handle, 1, ctypes.byref(info), ctypes.sizeof(info), None)
        error, after = ctypes.get_last_error(), tick()
        return {
            "phase": label,
            "before_ns": before,
            "after_ns": after,
            "bool": bool(ok),
            "last_error": error,
            "job_handle": int(job._handle),
            "size": ctypes.sizeof(info),
            "values": {name: int(getattr(info, name)) for name, _ in info._fields_},
        }

    def snapshot(self, job, label: str, *, original_accounting: dict | None = None) -> dict:
        """Read all listed members; a bounded buffer overflow is inconclusive."""

        class ProcessList(ctypes.Structure):
            _fields_ = [("assigned", ctypes.c_ulong), ("listed", ctypes.c_ulong), ("pids", ctypes.c_size_t * 256)]

        process = job._diagnostic_leader
        result = {
            "kind": "snapshot",
            "phase": label,
            "time_ns": tick(),
            "accounting": original_accounting or self.accounting(job, label),
            "leader_cached_returncode": process.returncode,
        }
        if process.process_handle is not None:
            result["leader"] = self.identity(process.process_handle, job._handle, process.pid)
        else:
            result["leader"] = {"pid": process.pid, "handle_closed": True}
        listed = ProcessList()
        native = getattr(job._kernel32, "native", job._kernel32)
        before = tick()
        ok = native.QueryInformationJobObject(job._handle, 3, ctypes.byref(listed), ctypes.sizeof(listed), None)
        error, after = ctypes.get_last_error(), tick()
        result["process_list"] = {
            "before_ns": before,
            "after_ns": after,
            "bool": bool(ok),
            "last_error": error,
            "assigned": listed.assigned,
            "listed": listed.listed,
            "capacity": 256,
            "complete": bool(ok and listed.assigned == listed.listed <= 256),
            "pids": [int(pid) for pid in listed.pids[: min(listed.listed, 256)]],
        }
        result["members"] = []
        # A failed query may leave unusable data; never open IDs from it.
        for pid in result["process_list"]["pids"] if ok else []:
            if pid == process.pid and process.process_handle is not None:
                result["members"].append({"borrowed_leader_handle": True, **result["leader"]})
                continue
            before = tick()
            handle = self.api.OpenProcess(0x100000 | 0x1000, False, pid)
            error, after = ctypes.get_last_error(), tick()
            member = {
                "expected_pid": pid,
                "open_before_ns": before,
                "open_after_ns": after,
                "open_ok": bool(handle),
                "open_error": error,
                "identity_ok": False,
            }
            if handle:
                try:
                    member.update(self.identity(handle, job._handle, pid))
                finally:
                    member["close_before_ns"] = tick()
                    member["close_ok"] = bool(self.api.CloseHandle(handle))
                    member["close_error"], member["close_after_ns"] = ctypes.get_last_error(), tick()
            result["members"].append(member)
        result["classification"] = classification(result)
        self.events.append(result)
        return result

    def passive_snapshot(self, job, label: str, **kwargs) -> None:
        """Do not let an observer error change the product cleanup path."""
        try:
            self.snapshot(job, label, **kwargs)
        except Exception as exc:
            self.events.append(
                {"kind": "observer_error", "phase": label, "type": type(exc).__name__, "time_ns": tick()}
            )

    def install(self) -> None:
        """Wrap native boundaries; every original decision and close still runs."""
        observer = self
        original_job = self.module.WindowsJob
        original_wait = self.module._SuspendedWindowsProcess.wait
        original_close = self.module._SuspendedWindowsProcess.close

        class CaptureApi:
            def __init__(self, native):
                self.native = native
                self.latest = None

            def __getattr__(self, name):
                return getattr(self.native, name)

            def QueryInformationJobObject(self, *args):
                before = tick()
                ok = self.native.QueryInformationJobObject(*args)
                error, after = ctypes.get_last_error(), tick()
                if args[1] == 1:
                    info = ctypes.cast(args[2], ctypes.POINTER(observer.module._JobBasicAccountingInformation)).contents
                    self.latest = {
                        "phase": "original_query",
                        "before_ns": before,
                        "after_ns": after,
                        "bool": bool(ok),
                        "last_error": error,
                        "job_handle": int(args[0]),
                        "size": args[3],
                        "values": {name: int(getattr(info, name)) for name, _ in info._fields_},
                    }
                    observer.events.append({"kind": "original_accounting", **self.latest})
                ctypes.set_last_error(error)
                return ok

        class ObservedJob(original_job):
            def __init__(self):
                super().__init__()
                self._kernel32 = CaptureApi(self._kernel32)
                self._diagnostic_first_count = True
                self._diagnostic_terminated = False

            def assign(self, process):
                result = super().assign(process)
                self._diagnostic_leader = process
                process._diagnostic_job = self
                # Capture before resume, while the original helper still owns
                # the suspension. Do not spend the exit-race window enumerating.
                observer.passive_snapshot(self, "before_leader_resume")
                return result

            def active_processes(self):
                count = super().active_processes()
                if self._diagnostic_first_count:
                    self._diagnostic_first_count = False
                    phase = "cleanup_first_count" if self._diagnostic_terminated else "original_failure_seam"
                    observer.passive_snapshot(self, phase, original_accounting=self._kernel32.latest)
                    observer.events.append({"kind": "original_returned_count", "phase": phase, "count": count})
                return count

            def terminate(self):
                self._diagnostic_terminated = True
                observer.events.append({"kind": "terminate_before", "time_ns": tick()})
                result = super().terminate()
                observer.events.append({"kind": "terminate_after", "time_ns": tick()})
                return result

            def close(self):
                if getattr(self, "_handle", None) and hasattr(self, "_diagnostic_leader"):
                    observer.passive_snapshot(self, "before_original_job_close")
                observer.events.append({"kind": "job_close_before", "time_ns": tick()})
                try:
                    return super().close()
                finally:
                    observer.events.append({"kind": "job_close_after", "time_ns": tick()})

        def wait(process, timeout_seconds):
            before = tick()
            try:
                return original_wait(process, timeout_seconds)
            finally:
                # No additional native query or disk I/O between this signal
                # timestamp and the original first accounting query.
                observer.events.append(
                    {
                        "kind": "leader_wait_returned",
                        "before_ns": before,
                        "after_ns": tick(),
                        "timeout_seconds": timeout_seconds,
                        "returncode": process.returncode,
                    }
                )

        def close(process):
            observer.events.append({"kind": "process_close_before", "time_ns": tick(), "pid": process.pid})
            try:
                return original_close(process)
            finally:
                observer.events.append(
                    {
                        "kind": "process_close_after",
                        "time_ns": tick(),
                        "pid": process.pid,
                        "accounting": None,
                        "reason": "original_job_already_closed_no_duplicate_held",
                    }
                )

        self.module.WindowsJob = ObservedJob
        self.module._SuspendedWindowsProcess.wait = wait
        self.module._SuspendedWindowsProcess.close = close
