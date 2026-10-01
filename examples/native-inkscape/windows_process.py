"""Read-only Windows process diagnostics for native extension provenance."""


def query_process(pid, retained_handle=None):
    """Read a retained process object's basic identity, times, and image name."""
    import ctypes
    from ctypes import wintypes

    class BasicInformation(ctypes.Structure):
        _fields_ = [
            ("exit_status", wintypes.LONG),
            ("peb", ctypes.c_void_p),
            ("affinity", ctypes.c_size_t),
            ("priority", wintypes.LONG),
            ("pid", ctypes.c_size_t),
            ("parent_pid", ctypes.c_size_t),
        ]

    class UnicodeString(ctypes.Structure):
        _fields_ = [("length", wintypes.USHORT), ("maximum_length", wintypes.USHORT), ("buffer", ctypes.c_void_p)]

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    kernel.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.LPWSTR,
        ctypes.POINTER(wintypes.DWORD),
    ]
    native = ctypes.WinDLL("ntdll")
    native.NtQueryInformationProcess.argtypes = [
        wintypes.HANDLE,
        wintypes.ULONG,
        ctypes.c_void_p,
        wintypes.ULONG,
        ctypes.POINTER(wintypes.ULONG),
    ]
    native.NtQueryInformationProcess.restype = wintypes.LONG
    handle = retained_handle or kernel.OpenProcess(0x1000, False, pid)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    report = {"requested_pid": pid}
    try:
        basic = BasicInformation()
        length = wintypes.ULONG()
        status = native.NtQueryInformationProcess(
            handle, 0, ctypes.byref(basic), ctypes.sizeof(basic), ctypes.byref(length)
        )
        if status < 0:
            raise OSError("ProcessBasicInformation failed: " + hex(status & 0xFFFFFFFF))
        report.update(pid=basic.pid, parent_pid=basic.parent_pid, exit_status=basic.exit_status)
        times = [wintypes.FILETIME() for _ in range(4)]
        if not kernel.GetProcessTimes(handle, *[ctypes.byref(item) for item in times]):
            raise ctypes.WinError(ctypes.get_last_error())
        report["creation_time"] = (times[0].dwHighDateTime << 32) | times[0].dwLowDateTime
        report["exit_time"] = (times[1].dwHighDateTime << 32) | times[1].dwLowDateTime
        image = ctypes.create_unicode_buffer(32768)
        size = wintypes.DWORD(len(image))
        if kernel.QueryFullProcessImageNameW(handle, 0, image, ctypes.byref(size)):
            report["image_win32"] = image.value
        else:
            report["image_win32_error"] = ctypes.get_last_error()
        storage = ctypes.create_string_buffer(65536)
        status = native.NtQueryInformationProcess(handle, 27, storage, len(storage), ctypes.byref(length))
        if status >= 0:
            name = UnicodeString.from_buffer(storage)
            report["image_native"] = ctypes.wstring_at(name.buffer, name.length // 2)
        else:
            report["image_native_error"] = hex(status & 0xFFFFFFFF)
        return report
    finally:
        if retained_handle is None:
            kernel.CloseHandle(handle)


class WindowsHelperObserver:
    """Retain read-only handles to the owned host's short-lived spawn helpers."""

    def __init__(self, host_pid):
        import ctypes
        from ctypes import wintypes
        import threading

        class ProcessEntry(ctypes.Structure):
            _fields_ = [
                ("size", wintypes.DWORD),
                ("usage", wintypes.DWORD),
                ("pid", wintypes.DWORD),
                ("heap", ctypes.c_size_t),
                ("module", wintypes.DWORD),
                ("threads", wintypes.DWORD),
                ("parent_pid", wintypes.DWORD),
                ("priority", wintypes.LONG),
                ("flags", wintypes.DWORD),
                ("filename", wintypes.WCHAR * 260),
            ]

        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
        self.kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        for function in (self.kernel.Process32FirstW, self.kernel.Process32NextW):
            function.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.process_entry = ProcessEntry
        self.host_pid = host_pid
        self.handles = {}
        self.errors = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._observe, daemon=True)
        self.thread.start()

    def _observe(self):
        import ctypes

        names = {"gspawn-win64-helper.exe", "gspawn-win64-helper-console.exe"}
        try:
            while not self.stop.is_set():
                snapshot = self.kernel.CreateToolhelp32Snapshot(2, 0)
                if snapshot == ctypes.c_void_p(-1).value:
                    raise ctypes.WinError(ctypes.get_last_error())
                try:
                    entry = self.process_entry()
                    entry.size = ctypes.sizeof(entry)
                    present = self.kernel.Process32FirstW(snapshot, ctypes.byref(entry))
                    while present:
                        if (
                            entry.parent_pid == self.host_pid
                            and entry.filename.lower() in names
                            and entry.pid not in self.handles
                        ):
                            handle = self.kernel.OpenProcess(0x1000, False, entry.pid)
                            if handle:
                                self.handles[entry.pid] = handle
                        present = self.kernel.Process32NextW(snapshot, ctypes.byref(entry))
                finally:
                    self.kernel.CloseHandle(snapshot)
                self.stop.wait(0.025)
        except OSError as exc:
            self.errors.append(str(exc))

    def finish(self):
        """Query retained helper objects, then release every owned read handle."""
        self.stop.set()
        self.thread.join()
        reports = []
        for pid, handle in self.handles.items():
            try:
                reports.append(query_process(pid, retained_handle=handle))
            except OSError as exc:
                reports.append({"requested_pid": pid, "error": str(exc)})
            finally:
                self.kernel.CloseHandle(handle)
        return {"helpers": reports, "errors": self.errors}
