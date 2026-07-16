use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::thread::JoinHandle;

use crate::ComputerUseError;
#[cfg(not(windows))]
use crate::{ComputerUseAction, ComputerUseErrorCode, ComputerUseObservation, ComputerUseResult};

#[cfg(windows)]
mod windows;

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct ControlBannerSignals {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) interrupted: Arc<AtomicBool>,
    pub(crate) visible: Arc<AtomicBool>,
    pub(crate) desktop_state: Arc<AtomicU64>,
    pub(crate) desktop_barrier: Arc<DesktopEventBarrier>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) target_available: Arc<AtomicBool>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) cleanup_pending: Arc<AtomicBool>,
}

#[derive(Default)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct DesktopEventBarrier {
    window_handle: AtomicUsize,
    next_sequence: AtomicU32,
    acknowledged_sequence: AtomicU32,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl DesktopEventBarrier {
    pub(crate) fn register_window(&self, window_handle: usize) {
        self.window_handle.store(window_handle, Ordering::Release);
    }

    pub(crate) fn clear_window(&self, window_handle: usize) {
        let _ = self.window_handle.compare_exchange(
            window_handle,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

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

#[cfg(windows)]
pub(crate) use windows::{
    ThreadDpiAwareness, clear_user_interrupt, desktop_interactive, flush_pending_input_releases,
    perform_action, prepare_target_for_input, prepare_target_window, start_control_banner,
    synchronize_desktop_events, user_interrupted, validate_target_policy, window_dpi,
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
    true
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
    _interrupted: &Arc<AtomicBool>,
    _desktop_state: &Arc<AtomicU64>,
    _desktop_barrier: &Arc<DesktopEventBarrier>,
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
