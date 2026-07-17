//! Shared Windows per-monitor DPI awareness RAII guard for capture worker threads.
//!
//! Both the WGC and HWND capture backends run their pixel-reading work on a
//! dedicated thread. That thread must switch to `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`
//! so that `WindowFinder`, `GetWindowRect`, WGC pixels, and `SendInput`
//! observations are all measured in the same physical coordinate space.
//!
//! This module provides a single `ThreadDpiAwareness` type with a parameterised
//! `context` string (e.g. `"WGC capture worker"`) for error messages.  Both
//! backends hold an instance of this type for the duration of their capture
//! work; `Drop` restores the original DPI context.

#[cfg(target_os = "windows")]
pub(crate) use imp::ThreadDpiAwareness;

#[cfg(target_os = "windows")]
mod imp {
    use windows::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        SetThreadDpiAwarenessContext,
    };

    use crate::error::{CaptureError, CaptureResult};

    /// RAII guard: switches the current thread to per-monitor-v2 DPI awareness
    /// on construction and restores the previous context on drop.
    ///
    /// # Parameters
    ///
    /// `context` — a short label used in error messages (e.g. `"WGC capture worker"`).
    pub(crate) struct ThreadDpiAwareness {
        previous: DPI_AWARENESS_CONTEXT,
    }

    impl ThreadDpiAwareness {
        pub(crate) fn enter(context: &str) -> CaptureResult<Self> {
            let previous =
                unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            if previous.0.is_null() {
                return Err(CaptureError::Platform(format!(
                    "Windows refused per-monitor-v2 DPI awareness for the {context}"
                )));
            }
            Ok(Self { previous })
        }
    }

    impl Drop for ThreadDpiAwareness {
        fn drop(&mut self) {
            let _ = unsafe { SetThreadDpiAwarenessContext(self.previous) };
        }
    }
}
