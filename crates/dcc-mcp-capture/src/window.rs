//! Window / process targeting utilities.
//!
//! Provides helpers to locate a specific window by process ID or title,
//! independent of the capture backend.

use crate::error::{CaptureError, CaptureResult};
use crate::types::CaptureTarget;

// ── WindowInfo ─────────────────────────────────────────────────────────────

/// Metadata about a discovered window.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// The platform window handle (e.g. HWND on Windows, XID on Linux).
    /// Stored as `u64` for cross-platform compatibility.
    pub handle: u64,
    /// The process ID that owns this window.
    pub pid: u32,
    /// The window title.
    pub title: String,
    /// Window position and size `[x, y, width, height]`.
    pub rect: [i32; 4],
}

// ── WindowFinder ───────────────────────────────────────────────────────────

/// Enumerates windows on the current system and resolves a [`CaptureTarget`]
/// to a [`WindowInfo`].
#[derive(Debug, Default)]
pub struct WindowFinder;

impl WindowFinder {
    /// Create a new [`WindowFinder`].
    pub fn new() -> Self {
        WindowFinder
    }

    /// Attempt to resolve `target` to the best matching window.
    ///
    /// Returns [`CaptureError::TargetNotFound`] if no matching window exists.
    pub fn find(&self, target: &CaptureTarget) -> CaptureResult<WindowInfo> {
        match target {
            CaptureTarget::PrimaryDisplay | CaptureTarget::MonitorIndex(_) => {
                // Display targets do not correspond to a specific window.
                Ok(WindowInfo {
                    handle: 0,
                    pid: 0,
                    title: "Primary Display".to_string(),
                    rect: [0, 0, 0, 0],
                })
            }
            CaptureTarget::ProcessId(pid) => self.find_by_pid(*pid),
            CaptureTarget::WindowTitle(title) => self.find_by_title(title),
        }
    }

    /// Enumerate all visible windows on the current desktop.
    ///
    /// Returns an empty list on unsupported platforms.
    pub fn enumerate(&self) -> Vec<WindowInfo> {
        platform_enumerate()
    }

    fn find_by_pid(&self, pid: u32) -> CaptureResult<WindowInfo> {
        let windows = self.enumerate();
        windows
            .into_iter()
            .find(|w| w.pid == pid)
            .ok_or_else(|| CaptureError::TargetNotFound(format!("no window for pid={pid}")))
    }

    fn find_by_title(&self, title: &str) -> CaptureResult<WindowInfo> {
        let title_lower = title.to_lowercase();
        let windows = self.enumerate();
        windows
            .into_iter()
            .find(|w| w.title.to_lowercase().contains(&title_lower))
            .ok_or_else(|| {
                CaptureError::TargetNotFound(format!("no window with title containing '{title}'"))
            })
    }
}

// ── Platform implementations ───────────────────────────────────────────────

/// Enumerate visible top-level windows.  Returns an empty list on
/// platforms where enumeration is not yet implemented.
fn platform_enumerate() -> Vec<WindowInfo> {
    #[cfg(target_os = "windows")]
    {
        windows_enumerate()
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec![]
    }
}

#[cfg(target_os = "windows")]
fn windows_enumerate() -> Vec<WindowInfo> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows::core::BOOL;

    let mut result: Vec<WindowInfo> = Vec::new();
    let result_ptr = &mut result as *mut Vec<WindowInfo> as isize;

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(lparam.0 as *mut Vec<WindowInfo>);

            if IsWindowVisible(hwnd).as_bool() {
                let mut title_buf = [0u16; 256];
                let len = GetWindowTextW(hwnd, &mut title_buf);
                if len > 0 {
                    let title = String::from_utf16_lossy(&title_buf[..len as usize]);
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    let mut rect = windows::Win32::Foundation::RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    list.push(WindowInfo {
                        handle: hwnd.0 as u64,
                        pid,
                        title,
                        rect: [
                            rect.left,
                            rect.top,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                        ],
                    });
                }
            }
            BOOL(1)
        }
    }

    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(result_ptr));
    }
    result
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finder_primary_display_always_ok() {
        let finder = WindowFinder::new();
        let info = finder.find(&CaptureTarget::PrimaryDisplay).unwrap();
        assert_eq!(info.handle, 0);
    }

    #[test]
    fn test_finder_monitor_index_always_ok() {
        let finder = WindowFinder::new();
        let info = finder.find(&CaptureTarget::MonitorIndex(0)).unwrap();
        assert_eq!(info.pid, 0);
    }

    #[test]
    fn test_finder_nonexistent_pid_returns_error() {
        let finder = WindowFinder::new();
        // PID 0x7FFF_FFFF is highly unlikely to have a visible window.
        let result = finder.find(&CaptureTarget::ProcessId(0x7FFF_FFFF));
        // On platforms that don't implement enumeration the list is empty,
        // so this should always return TargetNotFound.
        assert!(matches!(result, Err(CaptureError::TargetNotFound(_))));
    }

    #[test]
    fn test_finder_nonexistent_title_returns_error() {
        let finder = WindowFinder::new();
        let result = finder.find(&CaptureTarget::WindowTitle(
            "__NO_SUCH_DCC_WINDOW__".to_string(),
        ));
        assert!(matches!(result, Err(CaptureError::TargetNotFound(_))));
    }

    #[test]
    fn test_enumerate_does_not_panic() {
        let finder = WindowFinder::new();
        // We just check it doesn't panic.
        let _windows = finder.enumerate();
    }
}
