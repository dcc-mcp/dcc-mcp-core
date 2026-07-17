//! Versioned protocol and process boundary for Windows HWND capture.
//!
//! `PrintWindow` is synchronous and has no deadline. Calling it in the DCC
//! process can therefore block that process forever when the target window is
//! hung. The production path runs the literal
//! `PrintWindow(PW_RENDERFULLCONTENT)` call in a short-lived helper process.
//! The parent owns the child, kills and waits for it at the configured
//! deadline, and receives top-down BGRA pixels through [`dcc_mcp_shm`].

use std::ffi::OsString;

/// File name used by Windows packages and local builds.
pub const HELPER_BINARY_NAME: &str = "dcc-mcp-capture-helper.exe";

/// Exact protocol version shared by the parent and helper binary.
pub const HELPER_PROTOCOL_VERSION: u32 = 1;

/// Hidden argv marker used when the standalone server hosts the same helper
/// protocol inside a separately spawned server process.
pub const EMBEDDED_HELPER_ARG: &str = "--dcc-mcp-capture-helper";

#[cfg(target_os = "windows")]
const RESPONSE_MAGIC: &[u8; 8] = b"DCCPWBG1";
#[cfg(target_os = "windows")]
const RESPONSE_HEADER_LEN: usize = 32;
#[cfg(target_os = "windows")]
const EXIT_USAGE: i32 = 64;
#[cfg(target_os = "windows")]
const EXIT_CAPTURE_FAILED: i32 = 70;
#[cfg(not(target_os = "windows"))]
const EXIT_UNSUPPORTED: i32 = 78;

/// Run the dedicated helper binary using `std::env::args_os()`.
///
/// This is intentionally a tiny, stable entry point so the helper executable
/// and its parent always compile against the same protocol implementation.
#[doc(hidden)]
pub fn run_dedicated_from_env() -> i32 {
    run_dedicated(std::env::args_os().skip(1))
}

/// Run the helper protocol when the current executable was started with the
/// hidden embedded-helper marker.
///
/// The standalone server calls this before logging, CLI parsing, or network
/// startup. It keeps the raw one-file server release safe even when its
/// optional companion helper has not been downloaded yet.
#[doc(hidden)]
pub fn run_embedded_if_requested() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(EMBEDDED_HELPER_ARG)) {
        return None;
    }
    Some(run_dedicated(args))
}

fn run_dedicated(args: impl IntoIterator<Item = OsString>) -> i32 {
    #[cfg(target_os = "windows")]
    {
        match windows_impl::run_helper(args) {
            Ok(()) => 0,
            Err(HelperRunError::Usage(message)) => {
                eprintln!("capture helper protocol error: {message}");
                EXIT_USAGE
            }
            Err(HelperRunError::Capture(message)) => {
                eprintln!("capture helper failed: {message}");
                EXIT_CAPTURE_FAILED
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = args;
        eprintln!("capture helper is only supported on Windows");
        EXIT_UNSUPPORTED
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
enum HelperRunError {
    Usage(String),
    Capture(String),
}

#[cfg(target_os = "windows")]
pub(crate) use windows_impl::{
    capture_same_thread_bgra, capture_via_helper, window_is_same_thread,
};

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::collections::HashSet;
    use std::ffi::{OsStr, OsString};
    use std::io::Read;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use dcc_mcp_shm::SharedBuffer;
    use windows::Win32::Foundation::{HMODULE, HWND};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetWindowDC, HBITMAP, HDC, HGDIOBJ,
        ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    use windows::core::PCWSTR;

    use super::{
        EMBEDDED_HELPER_ARG, HELPER_BINARY_NAME, HELPER_PROTOCOL_VERSION, HelperRunError,
        RESPONSE_HEADER_LEN, RESPONSE_MAGIC,
    };
    use crate::error::{CaptureError, CaptureResult};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(0x0000_0002);

    #[derive(Debug)]
    struct HelperRequest {
        hwnd: u64,
        width: i32,
        height: i32,
        shm_name: String,
        shm_id: String,
    }

    pub(crate) fn window_is_same_thread(hwnd: u64) -> bool {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        unsafe { GetWindowThreadProcessId(hwnd, None) == GetCurrentThreadId() }
    }

    /// Capture a same-thread window without sending any synchronous message.
    pub(crate) fn capture_same_thread_bgra(
        hwnd: u64,
        width: i32,
        height: i32,
    ) -> CaptureResult<Vec<u8>> {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        unsafe { capture_bgra(hwnd, width, height, CaptureMethod::BitBltOnly) }
    }

    /// Spawn the isolated helper and return its validated BGRA response.
    pub(crate) fn capture_via_helper(
        hwnd: u64,
        width: i32,
        height: i32,
        timeout_ms: u64,
    ) -> CaptureResult<Vec<u8>> {
        let started = Instant::now();
        let helper = discover_helper()?;
        let pixel_len = pixel_len(width, height)?;
        let response_len = RESPONSE_HEADER_LEN.checked_add(pixel_len).ok_or_else(|| {
            CaptureError::InvalidConfig("capture buffer size overflow".to_string())
        })?;
        let buffer = SharedBuffer::create(response_len)
            .map_err(|error| CaptureError::Platform(format!("capture shared memory: {error}")))?;

        let mut command = Command::new(&helper.executable);
        if helper.embedded {
            command.arg(EMBEDDED_HELPER_ARG);
        }
        command
            .arg("--protocol-version")
            .arg(HELPER_PROTOCOL_VERSION.to_string())
            .arg("--hwnd")
            .arg(hwnd.to_string())
            .arg("--width")
            .arg(width.to_string())
            .arg("--height")
            .arg(height.to_string())
            .arg("--shm-name")
            .arg(buffer.name())
            .arg("--shm-id")
            .arg(&buffer.id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);

        let child = command.spawn().map_err(|error| {
            CaptureError::Platform(format!(
                "failed to start capture helper {}: {error}",
                helper.executable.display()
            ))
        })?;
        let mut child = OwnedChild::new(child);
        let bounded_timeout_ms = timeout_ms.max(1).min(u32::MAX as u64);
        let deadline = started + Duration::from_millis(bounded_timeout_ms);

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stderr = child.read_stderr();
                    if !status.success() {
                        let detail = if stderr.trim().is_empty() {
                            format!("exit status {status}")
                        } else {
                            stderr.trim().to_string()
                        };
                        return Err(CaptureError::Platform(format!(
                            "capture helper rejected protocol v{HELPER_PROTOCOL_VERSION} or failed: {detail}"
                        )));
                    }
                    let response = buffer.read().map_err(|error| {
                        CaptureError::Platform(format!("capture shared-memory read: {error}"))
                    })?;
                    return decode_response(&response, width, height);
                }
                Ok(None) => {}
                Err(error) => {
                    child.terminate_and_wait();
                    return Err(CaptureError::Platform(format!(
                        "capture helper status check failed: {error}"
                    )));
                }
            }

            let now = Instant::now();
            if now >= deadline {
                child.terminate_and_wait();
                return Err(CaptureError::Timeout(timeout_ms));
            }
            thread::sleep((deadline - now).min(Duration::from_millis(1)));
        }
    }

    pub(super) fn run_helper(
        args: impl IntoIterator<Item = OsString>,
    ) -> Result<(), HelperRunError> {
        let request = parse_request(args)?;
        let hwnd = HWND(request.hwnd as *mut core::ffi::c_void);
        let pixels = unsafe {
            capture_bgra(
                hwnd,
                request.width,
                request.height,
                CaptureMethod::PrintWindowThenBitBlt,
            )
        }
        .map_err(|error| HelperRunError::Capture(error.to_string()))?;
        let response = encode_response(request.width, request.height, &pixels)
            .map_err(HelperRunError::Capture)?;
        let buffer = SharedBuffer::open(&request.shm_name, &request.shm_id)
            .map_err(|error| HelperRunError::Capture(format!("open shared memory: {error}")))?;
        if buffer.capacity() < response.len() {
            return Err(HelperRunError::Capture(format!(
                "shared memory capacity {} is smaller than response {}",
                buffer.capacity(),
                response.len()
            )));
        }
        buffer
            .write(&response)
            .map_err(|error| HelperRunError::Capture(format!("write shared memory: {error}")))?;
        Ok(())
    }

    fn parse_request(
        args: impl IntoIterator<Item = OsString>,
    ) -> Result<HelperRequest, HelperRunError> {
        let mut args = args.into_iter();
        let mut protocol = None;
        let mut hwnd = None;
        let mut width = None;
        let mut height = None;
        let mut shm_name = None;
        let mut shm_id = None;

        while let Some(flag) = args.next() {
            let value = args.next().ok_or_else(|| {
                HelperRunError::Usage(format!("missing value for {}", flag.to_string_lossy()))
            })?;
            match flag.to_string_lossy().as_ref() {
                "--protocol-version" => protocol = Some(parse_number(&value, "protocol version")?),
                "--hwnd" => hwnd = Some(parse_number(&value, "HWND")?),
                "--width" => width = Some(parse_number(&value, "width")?),
                "--height" => height = Some(parse_number(&value, "height")?),
                "--shm-name" => shm_name = Some(value.to_string_lossy().into_owned()),
                "--shm-id" => shm_id = Some(value.to_string_lossy().into_owned()),
                other => return Err(HelperRunError::Usage(format!("unknown argument {other}"))),
            }
        }

        let protocol: u32 = required(protocol, "--protocol-version")?;
        if protocol != HELPER_PROTOCOL_VERSION {
            return Err(HelperRunError::Usage(format!(
                "unsupported protocol version {protocol}; expected {HELPER_PROTOCOL_VERSION}"
            )));
        }
        let width: i32 = required(width, "--width")?;
        let height: i32 = required(height, "--height")?;
        pixel_len(width, height).map_err(|error| HelperRunError::Usage(error.to_string()))?;

        Ok(HelperRequest {
            hwnd: required(hwnd, "--hwnd")?,
            width,
            height,
            shm_name: required(shm_name, "--shm-name")?,
            shm_id: required(shm_id, "--shm-id")?,
        })
    }

    fn parse_number<T>(value: &OsStr, label: &str) -> Result<T, HelperRunError>
    where
        T: std::str::FromStr,
    {
        value
            .to_string_lossy()
            .parse()
            .map_err(|_| HelperRunError::Usage(format!("invalid {label}")))
    }

    fn required<T>(value: Option<T>, flag: &str) -> Result<T, HelperRunError> {
        value.ok_or_else(|| HelperRunError::Usage(format!("missing {flag}")))
    }

    fn pixel_len(width: i32, height: i32) -> CaptureResult<usize> {
        if width <= 0 || height <= 0 {
            return Err(CaptureError::InvalidConfig(format!(
                "capture dimensions must be positive, got {width}x{height}"
            )));
        }
        (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| CaptureError::InvalidConfig("capture dimensions overflow".to_string()))
    }

    fn encode_response(width: i32, height: i32, pixels: &[u8]) -> Result<Vec<u8>, String> {
        let expected = pixel_len(width, height).map_err(|error| error.to_string())?;
        if pixels.len() != expected {
            return Err(format!(
                "pixel payload is {} bytes, expected {expected}",
                pixels.len()
            ));
        }
        let stride = (width as u32)
            .checked_mul(4)
            .ok_or_else(|| "capture stride overflow".to_string())?;
        let mut response = Vec::with_capacity(RESPONSE_HEADER_LEN + pixels.len());
        response.extend_from_slice(RESPONSE_MAGIC);
        response.extend_from_slice(&HELPER_PROTOCOL_VERSION.to_le_bytes());
        response.extend_from_slice(&(width as u32).to_le_bytes());
        response.extend_from_slice(&(height as u32).to_le_bytes());
        response.extend_from_slice(&stride.to_le_bytes());
        response.extend_from_slice(&(pixels.len() as u64).to_le_bytes());
        response.extend_from_slice(pixels);
        Ok(response)
    }

    fn decode_response(response: &[u8], width: i32, height: i32) -> CaptureResult<Vec<u8>> {
        if response.len() < RESPONSE_HEADER_LEN || &response[..8] != RESPONSE_MAGIC {
            return Err(CaptureError::Platform(
                "capture helper returned an invalid response header".to_string(),
            ));
        }
        let version =
            u32::from_le_bytes(response[8..12].try_into().expect("header length checked"));
        let actual_width =
            u32::from_le_bytes(response[12..16].try_into().expect("header length checked"));
        let actual_height =
            u32::from_le_bytes(response[16..20].try_into().expect("header length checked"));
        let stride =
            u32::from_le_bytes(response[20..24].try_into().expect("header length checked"));
        let payload_len =
            u64::from_le_bytes(response[24..32].try_into().expect("header length checked"))
                as usize;
        let expected_len = pixel_len(width, height)?;
        let expected_stride = (width as u32)
            .checked_mul(4)
            .ok_or_else(|| CaptureError::InvalidConfig("capture stride overflow".to_string()))?;
        if version != HELPER_PROTOCOL_VERSION
            || actual_width != width as u32
            || actual_height != height as u32
            || stride != expected_stride
            || payload_len != expected_len
            || response.len() != RESPONSE_HEADER_LEN + payload_len
        {
            return Err(CaptureError::Platform(format!(
                "capture helper response contract mismatch: version={version}, dimensions={actual_width}x{actual_height}, stride={stride}, payload={payload_len}"
            )));
        }
        Ok(response[RESPONSE_HEADER_LEN..].to_vec())
    }

    #[derive(Clone, Debug, Eq, Hash, PartialEq)]
    struct HelperLaunch {
        executable: PathBuf,
        embedded: bool,
    }

    fn discover_helper() -> CaptureResult<HelperLaunch> {
        if let Some(override_path) = std::env::var_os("DCC_MCP_CAPTURE_HELPER") {
            let path = PathBuf::from(override_path);
            return if path.is_file() {
                Ok(HelperLaunch {
                    executable: path,
                    embedded: false,
                })
            } else {
                Err(missing_helper_error(&[path]))
            };
        }

        let mut dedicated_candidates = Vec::new();
        let mut embedded_candidates = Vec::new();
        if let Some(module) = module_file_path() {
            append_directory_candidates(module.parent(), &mut dedicated_candidates);
            append_embedded_server_candidate(&module, &mut embedded_candidates);
        }
        if let Ok(executable) = std::env::current_exe() {
            append_directory_candidates(executable.parent(), &mut dedicated_candidates);
            append_embedded_server_candidate(&executable, &mut embedded_candidates);
        }
        if let Some(path) = find_on_path(HELPER_BINARY_NAME) {
            dedicated_candidates.push(path);
        }

        let mut seen = HashSet::new();
        dedicated_candidates.retain(|path| seen.insert(path.clone()));
        if let Some(path) = dedicated_candidates
            .iter()
            .find(|path| path.is_file())
            .cloned()
        {
            return Ok(HelperLaunch {
                executable: path,
                embedded: false,
            });
        }
        embedded_candidates.retain(|path| seen.insert(path.clone()));
        if let Some(path) = embedded_candidates
            .iter()
            .find(|path| path.is_file())
            .cloned()
        {
            return Ok(HelperLaunch {
                executable: path,
                embedded: true,
            });
        }
        dedicated_candidates.extend(embedded_candidates);
        Err(missing_helper_error(&dedicated_candidates))
    }

    fn append_directory_candidates(directory: Option<&Path>, candidates: &mut Vec<PathBuf>) {
        if let Some(directory) = directory {
            candidates.push(directory.join(HELPER_BINARY_NAME));
            candidates.push(directory.join("bin").join(HELPER_BINARY_NAME));
        }
    }

    fn append_embedded_server_candidate(executable: &Path, candidates: &mut Vec<PathBuf>) {
        let is_server = executable
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.eq_ignore_ascii_case("dcc-mcp-server.exe"));
        if is_server {
            candidates.push(executable.to_path_buf());
        }
    }

    fn find_on_path(file_name: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|directory| directory.join(file_name))
            .find(|candidate| candidate.is_file())
    }

    fn missing_helper_error(candidates: &[PathBuf]) -> CaptureError {
        let searched = candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        CaptureError::Platform(format!(
            "Windows capture helper {HELPER_BINARY_NAME} was not found; searched [{searched}]. Reinstall the Windows wheel or set DCC_MCP_CAPTURE_HELPER to the helper executable"
        ))
    }

    fn module_file_path() -> Option<PathBuf> {
        unsafe {
            let mut module = HMODULE::default();
            let address = module_file_path as *const () as *const u16;
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                PCWSTR(address),
                &mut module,
            )
            .ok()?;

            let mut buffer = vec![0u16; 512];
            loop {
                let length = GetModuleFileNameW(Some(module), &mut buffer) as usize;
                if length == 0 {
                    return None;
                }
                if length < buffer.len() - 1 {
                    return Some(PathBuf::from(OsString::from_wide(&buffer[..length])));
                }
                if buffer.len() >= 32_768 {
                    return None;
                }
                buffer.resize(buffer.len() * 2, 0);
            }
        }
    }

    struct OwnedChild {
        child: Child,
        finished: bool,
    }

    impl OwnedChild {
        fn new(child: Child) -> Self {
            Self {
                child,
                finished: false,
            }
        }

        fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
            let status = self.child.try_wait()?;
            if status.is_some() {
                self.finished = true;
            }
            Ok(status)
        }

        fn read_stderr(&mut self) -> String {
            let mut text = String::new();
            if let Some(mut stderr) = self.child.stderr.take() {
                let _ = stderr.read_to_string(&mut text);
            }
            text
        }

        fn terminate_and_wait(&mut self) {
            if self.finished {
                return;
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.finished = true;
        }
    }

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            self.terminate_and_wait();
        }
    }

    #[derive(Clone, Copy)]
    enum CaptureMethod {
        PrintWindowThenBitBlt,
        BitBltOnly,
    }

    unsafe fn capture_bgra(
        hwnd: HWND,
        width: i32,
        height: i32,
        method: CaptureMethod,
    ) -> CaptureResult<Vec<u8>> {
        unsafe {
            let surface = GdiSurface::new(hwnd, width, height)?;
            let printed = match method {
                CaptureMethod::PrintWindowThenBitBlt => {
                    // Keep the quality-preserving call literal. This function
                    // is invoked by the isolated helper process only.
                    PrintWindow(hwnd, surface.memory_dc, PW_RENDERFULLCONTENT).as_bool()
                }
                CaptureMethod::BitBltOnly => false,
            };
            if !printed {
                BitBlt(
                    surface.memory_dc,
                    0,
                    0,
                    width,
                    height,
                    Some(surface.window_dc),
                    0,
                    0,
                    SRCCOPY,
                )
                .map_err(|error| CaptureError::Platform(format!("BitBlt: {error}")))?;
            }
            surface.read_bgra(width, height)
        }
    }

    struct GdiSurface {
        hwnd: HWND,
        window_dc: HDC,
        memory_dc: HDC,
        bitmap: HBITMAP,
        old_object: HGDIOBJ,
    }

    impl GdiSurface {
        unsafe fn new(hwnd: HWND, width: i32, height: i32) -> CaptureResult<Self> {
            let mut surface = Self {
                hwnd,
                window_dc: HDC::default(),
                memory_dc: HDC::default(),
                bitmap: HBITMAP::default(),
                old_object: HGDIOBJ::default(),
            };
            // Capture dimensions come from GetWindowRect, so the fallback DC
            // must include the non-client area and use the same window origin.
            surface.window_dc = unsafe { GetWindowDC(Some(hwnd)) };
            if surface.window_dc.is_invalid() {
                return Err(CaptureError::Platform(
                    "GetWindowDC returned null".to_string(),
                ));
            }
            surface.memory_dc = unsafe { CreateCompatibleDC(Some(surface.window_dc)) };
            if surface.memory_dc.is_invalid() {
                return Err(CaptureError::Platform(
                    "CreateCompatibleDC returned null".to_string(),
                ));
            }
            surface.bitmap = unsafe { CreateCompatibleBitmap(surface.window_dc, width, height) };
            if surface.bitmap.is_invalid() {
                return Err(CaptureError::Platform(
                    "CreateCompatibleBitmap returned null".to_string(),
                ));
            }
            surface.old_object = unsafe { SelectObject(surface.memory_dc, surface.bitmap.into()) };
            if surface.old_object.is_invalid() {
                return Err(CaptureError::Platform("SelectObject failed".to_string()));
            }
            Ok(surface)
        }

        unsafe fn read_bgra(&self, width: i32, height: i32) -> CaptureResult<Vec<u8>> {
            let mut pixels = vec![0u8; pixel_len(width, height)?];
            let mut bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let rows = unsafe {
                GetDIBits(
                    self.memory_dc,
                    self.bitmap,
                    0,
                    height as u32,
                    Some(pixels.as_mut_ptr().cast()),
                    &mut bitmap_info,
                    DIB_RGB_COLORS,
                )
            };
            if rows == 0 {
                return Err(CaptureError::Platform(
                    "GetDIBits returned 0 scanlines".to_string(),
                ));
            }
            Ok(pixels)
        }
    }

    impl Drop for GdiSurface {
        fn drop(&mut self) {
            unsafe {
                if !self.old_object.is_invalid() && !self.memory_dc.is_invalid() {
                    SelectObject(self.memory_dc, self.old_object);
                }
                if !self.bitmap.is_invalid() {
                    let _ = DeleteObject(self.bitmap.into());
                }
                if !self.memory_dc.is_invalid() {
                    let _ = DeleteDC(self.memory_dc);
                }
                if !self.window_dc.is_invalid() {
                    ReleaseDC(Some(self.hwnd), self.window_dc);
                }
            }
        }
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn helper_is_explicitly_unsupported_off_windows() {
        assert_eq!(run_dedicated(std::iter::empty()), EXIT_UNSUPPORTED);
    }
}
