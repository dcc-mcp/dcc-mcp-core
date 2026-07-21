use std::sync::Arc;
#[cfg(windows)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, AtomicU64};
#[cfg(any(windows, test))]
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;

use crate::ComputerUseError;
#[cfg(not(windows))]
use crate::{
    ComputerUseAction, ComputerUseErrorCode, ComputerUseObservation, ComputerUseResult,
    PreInputFence,
};

/// Type alias for the last pointer-action coordinate + timestamp,
/// shared between the session owner and the control-banner thread.
pub(crate) type LastActionPoint = Arc<std::sync::Mutex<Option<(i32, i32, std::time::Instant)>>>;

#[cfg(windows)]
mod windows;

/// Signals shared between the session owner and the control-banner thread.
///
/// This struct is only meaningful on Windows: all fields are Windows-only
/// atomic/Arc values used to coordinate the banner thread's lifecycle,
/// visibility, desktop state, and input-owner safety. On non-Windows builds
/// the struct is a ZST (zero-sized type) provided by the stub below.
#[cfg(windows)]
pub(crate) struct ControlBannerSignals {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) interrupted: Arc<AtomicBool>,
    pub(crate) visible: Arc<AtomicBool>,
    pub(crate) desktop_state: Arc<AtomicU64>,
    pub(crate) desktop_barrier: Arc<DesktopEventBarrier>,
    pub(crate) target_available: Arc<AtomicBool>,
    pub(crate) cleanup_pending: Arc<AtomicBool>,
    pub(crate) session_id: Option<String>,
    pub(crate) last_action_point: LastActionPoint,
}

/// Non-Windows stub: `ControlBannerSignals` is a ZST that satisfies the type
/// system without carrying any state. All platform functions that accept it on
/// non-Windows return `BackendUnavailable` immediately.
#[cfg(not(windows))]
pub(crate) struct ControlBannerSignals;

#[derive(Default)]
pub(crate) struct DesktopEventBarrier {
    #[cfg(windows)]
    window_handle: AtomicUsize,
    #[cfg(any(windows, test))]
    next_sequence: AtomicU32,
    #[cfg(any(windows, test))]
    acknowledged_sequence: AtomicU32,
}

#[cfg(any(windows, test))]
impl DesktopEventBarrier {
    #[cfg(windows)]
    pub(crate) fn register_window(&self, window_handle: usize) {
        self.window_handle.store(window_handle, Ordering::Release);
    }

    #[cfg(windows)]
    pub(crate) fn clear_window(&self, window_handle: usize) {
        let _ = self.window_handle.compare_exchange(
            window_handle,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    #[cfg(windows)]
    pub(crate) fn window_handle(&self) -> usize {
        self.window_handle.load(Ordering::Acquire)
    }

    pub(crate) fn request_sequence(&self) -> u32 {
        self.next_sequence
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    pub(crate) fn acknowledge(&self, sequence: u32) {
        self.acknowledged_sequence
            .store(sequence, Ordering::Release);
    }

    pub(crate) fn is_acknowledged(&self, sequence: u32) -> bool {
        self.acknowledged_sequence
            .load(Ordering::Acquire)
            .wrapping_sub(sequence)
            < (1 << 31)
    }
}

pub(crate) struct ControlBannerStartError {
    pub(crate) error: ComputerUseError,
    pub(crate) thread: Option<JoinHandle<()>>,
}

impl From<ComputerUseError> for ControlBannerStartError {
    fn from(error: ComputerUseError) -> Self {
        Self {
            error,
            thread: None,
        }
    }
}

pub(crate) type ControlBannerStartResult = Result<JoinHandle<()>, ControlBannerStartError>;

/// Session-level signals that `perform_action` reads during execution.
/// Grouped to stay under clippy's `too_many_arguments` limit.
pub(crate) struct ActionSessionState {
    pub(crate) stop_requested: Arc<AtomicBool>,
    pub(crate) desktop_state: Arc<AtomicU64>,
    pub(crate) desktop_barrier: Arc<DesktopEventBarrier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(windows)]
pub(crate) struct ScopedWindowState {
    pub(crate) process_id: u32,
    pub(crate) window_handle: u64,
    pub(crate) exists: bool,
    pub(crate) visible: bool,
    pub(crate) minimized: bool,
    pub(crate) foreground: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(windows)]
pub(crate) enum ScopedWindowOperation {
    Restore,
    Show,
    Activate,
}

#[cfg(windows)]
pub(crate) use windows::{
    ThreadDpiAwareness, clear_user_interrupt, desktop_interactive, flush_pending_input_releases,
    perform_action, prepare_target_for_input, prepare_target_window, scoped_window_state,
    start_control_banner, synchronize_desktop_events, transition_scoped_window, user_interrupted,
    validate_target_policy, window_dpi,
};

#[cfg(all(test, windows))]
pub(crate) use windows::{TestIsolationGuard, acquire_test_isolation_guard};

#[cfg(not(windows))]
pub(crate) struct ThreadDpiAwareness;

#[cfg(not(windows))]
impl ThreadDpiAwareness {
    pub(crate) fn enter() -> ComputerUseResult<Self> {
        Ok(Self)
    }
}

#[cfg(not(windows))]
pub(crate) fn prepare_target_window(_window_handle: u64) -> ComputerUseResult<()> {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn prepare_target_for_input(
    _window_handle: u64,
    _expected_process_id: u32,
) -> ComputerUseResult<()> {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn validate_target_policy(
    _window_handle: u64,
    _process_id: u32,
    _window_title: &str,
) -> ComputerUseResult<()> {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn desktop_interactive() -> bool {
    false
}

#[cfg(not(windows))]
pub(crate) fn synchronize_desktop_events(
    _barrier: &DesktopEventBarrier,
    _stop_requested: &Arc<AtomicBool>,
) -> ComputerUseResult<()> {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn flush_pending_input_releases() -> ComputerUseResult<()> {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn window_dpi(_window_handle: u64) -> ComputerUseResult<u32> {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn start_control_banner(
    _window_handle: u64,
    _process_id: u32,
    _app_name: String,
    _signals: ControlBannerSignals,
) -> ControlBannerStartResult {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    )
    .into())
}

#[cfg(not(windows))]
pub(crate) fn perform_action(
    _window_handle: u64,
    _observation: &ComputerUseObservation,
    _request: &ComputerUseAction,
    _session: &ActionSessionState,
    _pre_input_fence: Option<&mut PreInputFence<'_>>,
    _last_action_point: &LastActionPoint,
) -> ComputerUseResult<()> {
    Err(ComputerUseError::new(
        ComputerUseErrorCode::BackendUnavailable,
        "native DCC MCP Computer Use is currently available on Windows",
    ))
}

#[cfg(not(windows))]
pub(crate) fn user_interrupted() -> bool {
    false
}

#[cfg(not(windows))]
pub(crate) fn clear_user_interrupt() -> ComputerUseResult<()> {
    Ok(())
}
