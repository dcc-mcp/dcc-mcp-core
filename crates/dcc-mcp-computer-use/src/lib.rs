//! Scoped native computer-use sessions for one DCC application window.
//!
//! The crate owns OS input injection, foreground validation, a visible control
//! banner, and the Ctrl+Alt+Esc stop token. Screenshot encoding remains in
//! `dcc-mcp-capture`; UI semantics remain in `dcc-mcp-app-ui`.

mod platform;

#[cfg(feature = "python-bindings")]
pub mod python;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dcc_mcp_capture::{
    CaptureConfig, CaptureFormat, CaptureTarget, Capturer, WindowFinder, WindowInfo,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_SCREENSHOT_EDGE: f64 = 1_600.0;
const MAX_SCREENSHOT_PIXELS: f64 = 1_500_000.0;
const MAX_DRAG_POINTS: usize = 256;
const MAX_KEY_TOKENS: usize = 16;
const MAX_TEXT_UTF16_UNITS: usize = 4_096;
const CONTROL_THREAD_JOIN_TIMEOUT: Duration = Duration::from_millis(750);

#[cfg(all(test, not(windows)))]
static USER_INTERRUPT_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(all(test, windows))]
fn user_interrupt_test_guard() -> platform::TestIsolationGuard {
    platform::acquire_test_isolation_guard()
        .expect("acquire cross-process Computer Use test isolation")
}

#[cfg(all(test, not(windows)))]
fn user_interrupt_test_guard() -> MutexGuard<'static, ()> {
    USER_INTERRUPT_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn join_control_thread(thread: JoinHandle<()>) -> Option<JoinHandle<()>> {
    join_control_thread_with_timeout(thread, CONTROL_THREAD_JOIN_TIMEOUT)
}

fn join_control_thread_with_timeout(
    thread: JoinHandle<()>,
    timeout: Duration,
) -> Option<JoinHandle<()>> {
    let deadline = Instant::now() + timeout;
    while !thread.is_finished() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            // The caller can retain this handle for a later join or detach it.
            // Either way, a stuck banner keeps its named input-owner mutex.
            return Some(thread);
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    let _ = thread.join();
    None
}

/// One point in screenshot pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ComputerUsePoint {
    /// Horizontal screenshot coordinate.
    pub x: f64,
    /// Vertical screenshot coordinate.
    pub y: f64,
}

/// A single native computer-use action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerUseAction {
    /// Action name: move, click, double_click, scroll, drag, type, keypress, or wait.
    pub action: String,
    /// Observation id returned by the most recent screenshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
    /// Horizontal screenshot coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Vertical screenshot coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Mouse button: left, right, or middle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
    /// Horizontal wheel delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_x: Option<i32>,
    /// Vertical wheel delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_y: Option<i32>,
    /// Ordered drag path in screenshot coordinates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<ComputerUsePoint>,
    /// Literal Unicode text for the `type` action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Keys or key chords for the `keypress` action.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    /// Action duration, or wait time, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Metadata that binds model coordinates to one captured window generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputerUseObservation {
    /// Opaque observation generation id.
    pub observation_id: String,
    /// Native window handle.
    pub window_handle: u64,
    /// Owning process id.
    pub process_id: u32,
    /// Window title captured with the frame.
    pub window_title: String,
    /// Screenshot width in pixels.
    pub width: u32,
    /// Screenshot height in pixels.
    pub height: u32,
    /// Source window rectangle `[x, y, width, height]` in desktop pixels.
    pub source_rect: [i32; 4],
    /// Source DPI scale reported by the capture backend.
    pub dpi_scale: f32,
    /// Effective DPI of the scoped native window when the frame was captured.
    #[serde(default = "default_window_dpi")]
    pub window_dpi: u32,
    /// Capture backend name.
    pub capture_backend: String,
    /// Capture timestamp in Unix milliseconds.
    pub timestamp_ms: u64,
    /// User-desktop generation that produced this observation.
    #[serde(default)]
    pub desktop_generation: u64,
}

const fn default_window_dpi() -> u32 {
    96
}

/// PNG screenshot plus its coordinate-space metadata.
#[derive(Debug, Clone)]
pub struct ComputerUseScreenshot {
    /// PNG image bytes.
    pub data: Vec<u8>,
    /// Coordinate-space metadata.
    pub observation: ComputerUseObservation,
}

/// Stable failure codes returned to MCP skill adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseErrorCode {
    /// The platform does not provide the native backend.
    BackendUnavailable,
    /// No scoped window matched the configured target.
    MissingWindow,
    /// The target window identity changed.
    InvalidTarget,
    /// A new screenshot is required before the action.
    StaleObservation,
    /// The user pressed the Ctrl+Alt+Esc stop chord.
    UserInterrupted,
    /// The target could not be made the foreground window.
    FocusLost,
    /// The Windows user desktop is locked, disconnected, or otherwise not interactive.
    DesktopUnavailable,
    /// Windows integrity or desktop policy blocked input injection.
    PermissionDenied,
    /// The action payload is invalid or unsupported.
    InvalidAction,
    /// Native input injection failed.
    InputFailed,
    /// Window capture failed.
    CaptureFailed,
}

/// Native computer-use failure.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ComputerUseError {
    /// Machine-readable failure code.
    pub code: ComputerUseErrorCode,
    /// Human-readable failure message.
    pub message: String,
}

impl ComputerUseError {
    fn new(code: ComputerUseErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Serialize the failure as the skill-facing JSON envelope.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "success": false,
            "error": self.code,
            "message": self.message,
        })
    }
}

/// Result alias for native computer-use operations.
pub type ComputerUseResult<T> = Result<T, ComputerUseError>;

#[derive(Debug, Clone)]
struct TargetSpec {
    process_id: Option<u32>,
    window_handle: Option<u64>,
    window_title: Option<String>,
    app_name: String,
}

/// Exact target scope established by the adapter/runtime, not by an agent call.
///
/// At least one stable native identity is required. Request parameters may
/// narrow this scope, but can never replace or widen it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputerUseTargetScope {
    process_id: Option<u32>,
    window_handle: Option<u64>,
}

impl ComputerUseTargetScope {
    /// Bind Computer Use to an operator/runtime-owned process or window.
    pub fn new(process_id: Option<u32>, window_handle: Option<u64>) -> ComputerUseResult<Self> {
        if process_id.is_some_and(|value| value == 0)
            || window_handle.is_some_and(|value| value == 0)
        {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::PermissionDenied,
                "the trusted Computer Use PID/HWND scope must contain positive native identifiers",
            ));
        }
        if process_id.is_none() && window_handle.is_none() {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::PermissionDenied,
                "native Computer Use requires an adapter/runtime-bound process_id or window_handle",
            ));
        }
        Ok(Self {
            process_id,
            window_handle,
        })
    }

    fn validate_request(
        self,
        process_id: Option<u32>,
        window_handle: Option<u64>,
    ) -> ComputerUseResult<()> {
        if self
            .process_id
            .zip(process_id)
            .is_some_and(|(trusted, requested)| trusted != requested)
            || self
                .window_handle
                .zip(window_handle)
                .is_some_and(|(trusted, requested)| trusted != requested)
        {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::PermissionDenied,
                "the requested target is outside the adapter/runtime-bound Computer Use scope",
            ));
        }
        Ok(())
    }

    fn validate_target(self, target: &WindowInfo) -> ComputerUseResult<()> {
        if self.process_id.is_some_and(|trusted| target.pid != trusted)
            || self
                .window_handle
                .is_some_and(|trusted| target.handle != trusted)
        {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::PermissionDenied,
                "the resolved window is outside the adapter/runtime-bound Computer Use scope",
            ));
        }
        Ok(())
    }
}

const DESKTOP_INTERACTIVE_BIT: u64 = 1;
const MAX_DESKTOP_GENERATION: u64 = u64::MAX >> 1;

const fn desktop_state_value(generation: u64, interactive: bool) -> u64 {
    (generation << 1) | interactive as u64
}

pub(crate) fn desktop_state_snapshot(state: &AtomicU64) -> (bool, u64) {
    let value = state.load(Ordering::Acquire);
    (
        value & DESKTOP_INTERACTIVE_BIT != 0,
        value >> DESKTOP_INTERACTIVE_BIT,
    )
}

pub(crate) fn record_desktop_transition(state: &AtomicU64, next_interactive: bool) -> bool {
    let mut current = state.load(Ordering::Acquire);
    loop {
        let current_interactive = current & DESKTOP_INTERACTIVE_BIT != 0;
        if current_interactive == next_interactive {
            return false;
        }
        let generation = (current >> 1).saturating_add(1).min(MAX_DESKTOP_GENERATION);
        let next = desktop_state_value(generation, next_interactive);
        match state.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn record_desktop_environment_change(state: &AtomicU64) -> u64 {
    let mut current = state.load(Ordering::Acquire);
    loop {
        let interactive = current & DESKTOP_INTERACTIVE_BIT != 0;
        let generation = (current >> 1).saturating_add(1).min(MAX_DESKTOP_GENERATION);
        let next = desktop_state_value(generation, interactive);
        match state.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return generation,
            Err(observed) => current = observed,
        }
    }
}

struct SessionState {
    active: bool,
    target: Option<WindowInfo>,
    observation: Option<ComputerUseObservation>,
    interrupted: Arc<AtomicBool>,
    overlay_visible: Arc<AtomicBool>,
    desktop_state: Arc<AtomicU64>,
    desktop_barrier: Arc<platform::DesktopEventBarrier>,
    target_available: Arc<AtomicBool>,
    cleanup_pending: Arc<AtomicBool>,
    overlay_thread: Option<JoinHandle<()>>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            active: false,
            target: None,
            observation: None,
            interrupted: Arc::new(AtomicBool::new(false)),
            overlay_visible: Arc::new(AtomicBool::new(false)),
            desktop_state: Arc::new(AtomicU64::new(desktop_state_value(0, false))),
            desktop_barrier: Arc::new(platform::DesktopEventBarrier::default()),
            target_available: Arc::new(AtomicBool::new(false)),
            cleanup_pending: Arc::new(AtomicBool::new(false)),
            overlay_thread: None,
        }
    }
}

fn invalidate_observation_before_capture(state: &mut SessionState) {
    state.observation = None;
}

fn take_observation_for_action(
    state: &mut SessionState,
) -> ComputerUseResult<ComputerUseObservation> {
    state.observation.take().ok_or_else(|| {
        ComputerUseError::new(
            ComputerUseErrorCode::StaleObservation,
            "take a screenshot before performing computer-use actions",
        )
    })
}

fn retain_pending_control_thread(
    state: &mut SessionState,
    result: platform::ControlBannerStartResult,
) -> ComputerUseResult<JoinHandle<()>> {
    match result {
        Ok(thread) => Ok(thread),
        Err(failure) => {
            state.overlay_thread = failure.thread;
            Err(failure.error)
        }
    }
}

/// A long-lived, single-window computer-use session.
pub struct ComputerUseSession {
    trusted_scope: ComputerUseTargetScope,
    spec: TargetSpec,
    state: Mutex<SessionState>,
    stop_requested: Arc<AtomicBool>,
    generation: AtomicU64,
    capturer: Capturer,
}

impl std::fmt::Debug for ComputerUseSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComputerUseSession")
            .field("app_name", &self.spec.app_name)
            .finish_non_exhaustive()
    }
}

impl ComputerUseSession {
    /// Create a session inside an adapter/runtime-owned exact PID/HWND scope.
    pub fn new(
        trusted_scope: ComputerUseTargetScope,
        process_id: Option<u32>,
        window_handle: Option<u64>,
        window_title: Option<String>,
        app_name: Option<String>,
    ) -> ComputerUseResult<Self> {
        trusted_scope.validate_request(process_id, window_handle)?;
        let process_id = process_id.or(trusted_scope.process_id);
        let window_handle = window_handle.or(trusted_scope.window_handle);
        let window_title = window_title
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty());
        if process_id.is_none() && window_handle.is_none() && window_title.is_none() {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::MissingWindow,
                "computer use requires process_id, window_handle, or window_title",
            ));
        }
        Ok(Self {
            trusted_scope,
            spec: TargetSpec {
                process_id,
                window_handle,
                window_title,
                app_name: app_name.unwrap_or_else(|| "DCC application".to_string()),
            },
            state: Mutex::new(SessionState::default()),
            stop_requested: Arc::new(AtomicBool::new(false)),
            generation: AtomicU64::new(0),
            capturer: Capturer::new_window_auto(),
        })
    }

    /// Start the visible session banner and Ctrl+Alt+Esc watcher.
    pub fn start(&self) -> ComputerUseResult<Value> {
        let _dpi_awareness = platform::ThreadDpiAwareness::enter()?;
        let mut state = self.lock_state();
        if state.active {
            self.ensure_running(&state)?;
            return Ok(self.status_locked(&state));
        }
        Self::ensure_cleanup_complete(&mut state)?;
        self.stop_requested.store(false, Ordering::Release);
        check_action_cancellation(&self.stop_requested)?;
        let target = self.resolve_target()?;
        self.trusted_scope.validate_target(&target)?;
        platform::validate_target_policy(target.handle, target.pid, &target.title)?;
        platform::prepare_target_window(target.handle)?;
        let interrupted = Arc::new(AtomicBool::new(false));
        let overlay_visible = Arc::new(AtomicBool::new(false));
        let desktop_state = Arc::new(AtomicU64::new(desktop_state_value(1, true)));
        let desktop_barrier = Arc::new(platform::DesktopEventBarrier::default());
        let target_available = Arc::new(AtomicBool::new(true));
        let cleanup_pending = Arc::new(AtomicBool::new(false));
        state.cleanup_pending = Arc::clone(&cleanup_pending);
        let thread = retain_pending_control_thread(
            &mut state,
            platform::start_control_banner(
                target.handle,
                target.pid,
                self.spec.app_name.clone(),
                platform::ControlBannerSignals {
                    stop: Arc::clone(&self.stop_requested),
                    interrupted: Arc::clone(&interrupted),
                    visible: Arc::clone(&overlay_visible),
                    desktop_state: Arc::clone(&desktop_state),
                    desktop_barrier: Arc::clone(&desktop_barrier),
                    target_available: Arc::clone(&target_available),
                    cleanup_pending,
                },
            ),
        )?;
        state.active = true;
        state.target = Some(target);
        state.observation = None;
        state.interrupted = interrupted;
        state.overlay_visible = overlay_visible;
        state.desktop_state = desktop_state;
        state.desktop_barrier = desktop_barrier;
        state.target_available = target_available;
        state.overlay_thread = Some(thread);
        Ok(self.status_locked(&state))
    }

    /// Capture the target as PNG and establish the coordinate generation for the next action.
    pub fn screenshot(&self) -> ComputerUseResult<ComputerUseScreenshot> {
        let _dpi_awareness = platform::ThreadDpiAwareness::enter()?;
        let mut state = self.lock_state();
        self.ensure_running(&state)?;
        invalidate_observation_before_capture(&mut state);
        platform::synchronize_desktop_events(&state.desktop_barrier, &self.stop_requested)?;
        let _ = Self::refresh_desktop_state(&mut state)?;
        platform::flush_pending_input_releases()?;
        platform::synchronize_desktop_events(&state.desktop_barrier, &self.stop_requested)?;
        let desktop_generation = Self::refresh_desktop_state(&mut state)?;
        let target = self.revalidate_target(&state)?;
        platform::prepare_target_window(target.handle)?;
        let capture_window_dpi = platform::window_dpi(target.handle)?;
        let scale = screenshot_scale(target.rect);
        let config = CaptureConfig::builder()
            .target(CaptureTarget::WindowHandle(target.handle))
            .format(CaptureFormat::Png)
            .scale(scale)
            .build();
        let frame = match self.capturer.capture(&config) {
            Ok(frame) => frame,
            Err(error) => {
                check_action_cancellation(&self.stop_requested)?;
                return Err(resolve_capture_error(
                    error.to_string(),
                    Self::refresh_desktop_state(&mut state),
                ));
            }
        };
        check_action_cancellation(&self.stop_requested)?;
        platform::synchronize_desktop_events(&state.desktop_barrier, &self.stop_requested)?;
        let post_capture_desktop_generation = Self::refresh_desktop_state(&mut state)?;
        if desktop_generation != post_capture_desktop_generation {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                "the Windows desktop changed while capturing; take a fresh screenshot",
            ));
        }
        // The capture runs on another thread and an HWND can be destroyed,
        // reused, moved, or resized while pixels are being copied. Revalidate
        // after capture before returning any bytes to the caller.
        let captured_target = self.revalidate_target(&state)?;
        let rect = validate_captured_target(&target, &captured_target, frame.window_rect)?;
        let window_dpi = platform::window_dpi(captured_target.handle)?;
        if capture_window_dpi != window_dpi {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                "target window DPI changed while capturing; take a fresh screenshot",
            ));
        }
        platform::synchronize_desktop_events(&state.desktop_barrier, &self.stop_requested)?;
        let current_desktop_generation = Self::refresh_desktop_state(&mut state)?;
        if desktop_generation != current_desktop_generation {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                "the Windows desktop changed while capturing; take a fresh screenshot",
            ));
        }
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let observation = ComputerUseObservation {
            observation_id: format!(
                "{:x}:{generation}:{}:{}:{}:{}",
                captured_target.handle, rect[0], rect[1], rect[2], rect[3]
            ),
            window_handle: captured_target.handle,
            process_id: captured_target.pid,
            window_title: captured_target.title.clone(),
            width: frame.width,
            height: frame.height,
            source_rect: rect,
            dpi_scale: frame.dpi_scale,
            window_dpi,
            capture_backend: self.capturer.backend_kind().to_string(),
            timestamp_ms: frame.timestamp_ms,
            desktop_generation,
        };
        state.target = Some(captured_target);
        state.observation = Some(observation.clone());
        Ok(ComputerUseScreenshot {
            data: frame.data,
            observation,
        })
    }

    /// Perform one action against the most recent observation.
    pub fn perform(&self, request: &ComputerUseAction) -> ComputerUseResult<Value> {
        validate_action_limits(request)?;
        let _dpi_awareness = platform::ThreadDpiAwareness::enter()?;
        let mut state = self.lock_state();
        self.ensure_running(&state)?;
        let observation = take_observation_for_action(&mut state)?;
        let target_before_restore = self.revalidate_target(&state)?;
        // A user may minimize the scoped DCC after the screenshot. Verify its
        // immutable HWND/PID and hard policy first, restore it without sending
        // input before asking the banner thread to fence desktop events; the
        // banner intentionally hides while the target is minimized.
        platform::prepare_target_for_input(
            target_before_restore.handle,
            target_before_restore.pid,
        )?;
        platform::synchronize_desktop_events(&state.desktop_barrier, &self.stop_requested)?;
        let desktop_generation = Self::refresh_desktop_state(&mut state)?;
        let target = self.revalidate_target(&state)?;
        let expected = request.observation_id.as_deref().ok_or_else(|| {
            ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                "observation_id from the latest screenshot is required",
            )
        })?;
        validate_observation_desktop(&observation, desktop_generation)?;
        let current_window_dpi = platform::window_dpi(target.handle)?;
        if expected != observation.observation_id {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                "the observation was superseded; take a new screenshot",
            ));
        }
        if target.rect != observation.source_rect {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                format!(
                    "target window moved or resized after the screenshot (observed {:?}, current {:?}); take a new screenshot",
                    observation.source_rect, target.rect
                ),
            ));
        }
        if current_window_dpi != observation.window_dpi {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::StaleObservation,
                "target window DPI changed after the screenshot; take a new screenshot",
            ));
        }

        let action_result = platform::perform_action(
            target.handle,
            &observation,
            request,
            &self.stop_requested,
            &state.desktop_state,
            &state.desktop_barrier,
        );
        action_result?;
        Ok(json!({
            "success": true,
            "action": request.action,
            "observation_id": observation.observation_id,
            "requires_new_screenshot": true,
        }))
    }

    /// Request a stop without waiting for an in-flight action or state lock.
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Release);
    }

    /// Stop the session and remove the visible banner.
    pub fn stop(&self) -> Value {
        self.request_stop();
        let mut state = self.lock_state();
        let was_interrupted =
            state.interrupted.load(Ordering::Acquire) || platform::user_interrupted();
        let joined_control_thread = if let Some(thread) = state.overlay_thread.take() {
            state.overlay_thread = join_control_thread(thread);
            state.overlay_thread.is_none()
        } else {
            false
        };
        if joined_control_thread {
            state.cleanup_pending.store(false, Ordering::Release);
        }
        state.active = false;
        state.target = None;
        state.observation = None;
        let cleanup_pending =
            state.overlay_thread.is_some() || state.cleanup_pending.load(Ordering::Acquire);
        if !cleanup_pending {
            state.cleanup_pending.store(false, Ordering::Release);
            state.overlay_visible.store(false, Ordering::Release);
        }
        json!({
            "success": true,
            "active": false,
            "user_interrupted": was_interrupted,
            "cleanup_pending": cleanup_pending,
        })
    }

    /// Clear the Windows-logon-session stop latch after explicit user approval.
    pub fn resume_after_user_approval(&self) -> Value {
        let state = self.lock_state();
        if state.active {
            return ComputerUseError::new(
                ComputerUseErrorCode::InvalidAction,
                "stop the active Computer Use session before resuming after user approval",
            )
            .to_json();
        }
        if let Err(error) = platform::clear_user_interrupt() {
            return error.to_json();
        }
        state.interrupted.store(false, Ordering::Release);
        json!({
            "success": true,
            "user_interrupted": false,
            "message": "Computer Use may resume after explicit user approval",
        })
    }

    /// Return current session state.
    #[must_use]
    pub fn status(&self) -> Value {
        let state = self.lock_state();
        self.status_locked(&state)
    }

    fn status_locked(&self, state: &SessionState) -> Value {
        let active = state.active
            && !self.stop_requested.load(Ordering::Acquire)
            && state.target_available.load(Ordering::Acquire);
        let cleanup_pending = !active
            && (state.overlay_thread.is_some() || state.cleanup_pending.load(Ordering::Acquire));
        let (desktop_interactive, desktop_generation) =
            desktop_state_snapshot(&state.desktop_state);
        json!({
            "success": true,
            "active": active,
            "cleanup_pending": cleanup_pending,
            "user_interrupted": state.interrupted.load(Ordering::Acquire) || platform::user_interrupted(),
            "overlay_visible": state.overlay_visible.load(Ordering::Acquire),
            "desktop_interactive": desktop_interactive,
            "desktop_generation": desktop_generation,
            "input_suspended": active && !desktop_interactive,
            "app_name": self.spec.app_name,
            "window_handle": state.target.as_ref().map(|target| target.handle),
            "process_id": state.target.as_ref().map(|target| target.pid),
            "window_title": state.target.as_ref().map(|target| target.title.clone()),
            "hint": format!(
                "DCC MCP Computer Use is controlling {} - press Ctrl+Alt+Esc to stop",
                self.spec.app_name
            ),
        })
    }

    fn refresh_desktop_state(state: &mut SessionState) -> ComputerUseResult<u64> {
        let interactive = platform::desktop_interactive();
        if record_desktop_transition(&state.desktop_state, interactive) {
            state.observation = None;
        }
        if !interactive {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::DesktopUnavailable,
                "the Windows desktop is locked, disconnected, or not interactive; the Computer Use session is paused and no UI input was sent",
            ));
        }
        Ok(desktop_state_snapshot(&state.desktop_state).1)
    }

    fn ensure_running(&self, state: &SessionState) -> ComputerUseResult<()> {
        if !state.active {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "computer-use session is not active",
            ));
        }
        if !state.target_available.load(Ordering::Acquire) {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::MissingWindow,
                "the scoped DCC window is no longer available",
            ));
        }
        if state.interrupted.load(Ordering::Acquire) || platform::user_interrupted() {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::UserInterrupted,
                "the user pressed Ctrl+Alt+Esc; no further input was sent",
            ));
        }
        if self.stop_requested.load(Ordering::Acquire) {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "the Computer Use session was stopped; no further input was sent",
            ));
        }
        Ok(())
    }

    fn ensure_cleanup_complete(state: &mut SessionState) -> ComputerUseResult<()> {
        if let Some(thread) = state.overlay_thread.take() {
            if thread.is_finished() {
                let _ = thread.join();
                state.cleanup_pending.store(false, Ordering::Release);
                state.overlay_visible.store(false, Ordering::Release);
            } else {
                state.overlay_thread = Some(thread);
            }
        }
        if state.overlay_thread.is_some() || state.cleanup_pending.load(Ordering::Acquire) {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::BackendUnavailable,
                "the previous Computer Use control thread is still cleaning up; retry after cleanup completes",
            ));
        }
        Ok(())
    }

    fn resolve_target(&self) -> ComputerUseResult<WindowInfo> {
        let finder = WindowFinder::new();
        if let Some(handle) = self.spec.window_handle {
            let info = finder
                .find(&CaptureTarget::WindowHandle(handle))
                .map_err(|error| {
                    ComputerUseError::new(ComputerUseErrorCode::MissingWindow, error.to_string())
                })?;
            validate_target_constraints(&self.spec, &info)?;
            return Ok(info);
        }

        select_unique_target(&self.spec, finder.enumerate())
    }

    fn revalidate_target(&self, state: &SessionState) -> ComputerUseResult<WindowInfo> {
        let original = state.target.as_ref().ok_or_else(|| {
            ComputerUseError::new(
                ComputerUseErrorCode::MissingWindow,
                "session target is missing",
            )
        })?;
        let current = WindowFinder::new()
            .info_for_handle(original.handle)
            .map_err(|error| {
                ComputerUseError::new(ComputerUseErrorCode::MissingWindow, error.to_string())
            })?;
        if current.pid != original.pid {
            return Err(ComputerUseError::new(
                ComputerUseErrorCode::InvalidTarget,
                "target HWND was reused by another process",
            ));
        }
        validate_target_constraints(&self.spec, &current)?;
        self.trusted_scope.validate_target(&current)?;
        platform::validate_target_policy(current.handle, current.pid, &current.title)?;
        Ok(current)
    }

    fn lock_state(&self) -> MutexGuard<'_, SessionState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn select_unique_target(
    spec: &TargetSpec,
    candidates: impl IntoIterator<Item = WindowInfo>,
) -> ComputerUseResult<WindowInfo> {
    let mut matches = candidates
        .into_iter()
        .filter(|info| target_matches(spec, info));
    let info = matches.next().ok_or_else(|| {
        ComputerUseError::new(
            ComputerUseErrorCode::MissingWindow,
            "no visible window matched every computer-use target constraint",
        )
    })?;
    if matches.next().is_some() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "multiple visible windows matched the Computer Use scope; provide an exact window_handle",
        ));
    }
    Ok(info)
}

fn check_action_cancellation(stop_requested: &AtomicBool) -> ComputerUseResult<()> {
    if platform::user_interrupted() {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::UserInterrupted,
            "the user pressed Ctrl+Alt+Esc; input was stopped and held keys/buttons were released",
        ));
    }
    if stop_requested.load(Ordering::Acquire) {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::BackendUnavailable,
            "the Computer Use session was stopped; held keys/buttons were released",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn cancellable_wait(duration: Duration, stop_requested: &AtomicBool) -> ComputerUseResult<()> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        check_action_cancellation(stop_requested)?;
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(10)),
        );
    }
    check_action_cancellation(stop_requested)
}

fn screenshot_scale(rect: [i32; 4]) -> f32 {
    let width = f64::from(rect[2].max(1));
    let height = f64::from(rect[3].max(1));
    let edge_scale = MAX_SCREENSHOT_EDGE / width.max(height);
    let pixel_scale = (MAX_SCREENSHOT_PIXELS / (width * height)).sqrt();
    edge_scale.min(pixel_scale).min(1.0) as f32
}

fn resolve_capture_error(
    capture_error: impl Into<String>,
    desktop_state: ComputerUseResult<u64>,
) -> ComputerUseError {
    match desktop_state {
        Ok(_) => ComputerUseError::new(ComputerUseErrorCode::CaptureFailed, capture_error),
        Err(error) => error,
    }
}

fn validate_action_limits(request: &ComputerUseAction) -> ComputerUseResult<()> {
    if request.path.len() > MAX_DRAG_POINTS {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            format!("drag path exceeds the {MAX_DRAG_POINTS}-point safety limit"),
        ));
    }
    let key_count = request
        .keys
        .iter()
        .flat_map(|item| item.split('+'))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .count();
    if key_count > MAX_KEY_TOKENS {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            format!("keypress exceeds the {MAX_KEY_TOKENS}-key safety limit"),
        ));
    }
    if request
        .text
        .as_deref()
        .is_some_and(|text| text.encode_utf16().count() > MAX_TEXT_UTF16_UNITS)
    {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            format!("text exceeds the {MAX_TEXT_UTF16_UNITS}-UTF-16-unit safety limit"),
        ));
    }
    if request
        .duration_ms
        .is_some_and(|duration| duration > 60_000)
    {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidAction,
            "duration_ms exceeds the 60000 ms safety limit",
        ));
    }
    Ok(())
}

fn target_matches(spec: &TargetSpec, info: &WindowInfo) -> bool {
    spec.window_handle
        .is_none_or(|expected| info.handle == expected)
        && spec.process_id.is_none_or(|expected| info.pid == expected)
        && spec
            .window_title
            .as_ref()
            .is_none_or(|expected| info.title.to_lowercase().contains(&expected.to_lowercase()))
}

fn validate_target_constraints(spec: &TargetSpec, info: &WindowInfo) -> ComputerUseResult<()> {
    if target_matches(spec, info) {
        return Ok(());
    }
    Err(ComputerUseError::new(
        ComputerUseErrorCode::InvalidTarget,
        "resolved window does not satisfy every scoped PID, HWND, and title constraint",
    ))
}

#[cfg(any(windows, test))]
fn denied_target_reason(
    process_name: &str,
    class_name: &str,
    _window_title: &str,
) -> Option<&'static str> {
    let process_name = process_name.trim().to_ascii_lowercase();
    let class_name = class_name.trim().to_ascii_lowercase();
    const DENIED_PROCESSES: &[&str] = &[
        "1password",
        "authhost",
        "bitwarden",
        "cmd",
        "conhost",
        "consent",
        "credentialuibroker",
        "dashlane",
        "enpass",
        "keeperpasswordmanager",
        "keepass",
        "keepassxc",
        "lastpass",
        "lockapp",
        "logonui",
        "nordpass",
        "openconsole",
        "powershell",
        "powershell_ise",
        "pwsh",
        "roboform",
        "sechealthui",
        "securityhealthhost",
        "systemsettings",
        "windowsterminal",
        "wt",
    ];
    const DENIED_CLASSES: &[&str] = &[
        "cascadia_hosting_window_class",
        "consolewindowclass",
        "credential dialog xaml host",
        "lockscreenroot",
    ];
    if DENIED_PROCESSES.contains(&process_name.as_str()) {
        return Some(
            "system, terminal, authentication, or password-manager targets are not allowed",
        );
    }
    if DENIED_CLASSES.contains(&class_name.as_str()) {
        return Some("system, terminal, authentication, or lock-screen windows are not allowed");
    }
    // Explorer-owned #32770 dialogs include the localized Windows Run surface.
    // Class/process identity is stable across locales; titles are not.
    if process_name == "explorer" && class_name == "#32770" {
        return Some("the Windows Run dialog is not an allowed Computer Use target");
    }
    None
}

fn validate_captured_target(
    before: &WindowInfo,
    after: &WindowInfo,
    captured_rect: Option<[i32; 4]>,
) -> ComputerUseResult<[i32; 4]> {
    if before.handle != after.handle || before.pid != after.pid {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::InvalidTarget,
            "target HWND identity changed while capturing the screenshot",
        ));
    }
    let captured_rect = captured_rect.unwrap_or(after.rect);
    if before.rect != after.rect || captured_rect != after.rect {
        return Err(ComputerUseError::new(
            ComputerUseErrorCode::StaleObservation,
            "target window moved or resized while capturing; take a new screenshot",
        ));
    }
    Ok(captured_rect)
}

fn validate_observation_desktop(
    observation: &ComputerUseObservation,
    desktop_generation: u64,
) -> ComputerUseResult<()> {
    if observation.desktop_generation == desktop_generation {
        return Ok(());
    }
    Err(ComputerUseError::new(
        ComputerUseErrorCode::StaleObservation,
        "the Windows desktop changed after the screenshot; take a fresh screenshot before sending input",
    ))
}

impl Drop for ComputerUseSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted_pid_scope(process_id: u32) -> ComputerUseTargetScope {
        ComputerUseTargetScope::new(Some(process_id), None).unwrap()
    }

    fn test_session(process_id: u32) -> ComputerUseSession {
        ComputerUseSession::new(
            trusted_pid_scope(process_id),
            Some(process_id),
            None,
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn desktop_transitions_advance_generation_once_per_change() {
        let state = AtomicU64::new(desktop_state_value(1, true));

        assert!(!record_desktop_transition(&state, true));
        assert_eq!(desktop_state_snapshot(&state), (true, 1));

        assert!(record_desktop_transition(&state, false));
        assert_eq!(desktop_state_snapshot(&state), (false, 2));

        assert!(!record_desktop_transition(&state, false));
        assert_eq!(desktop_state_snapshot(&state), (false, 2));

        assert!(record_desktop_transition(&state, true));
        assert_eq!(desktop_state_snapshot(&state), (true, 3));
    }

    #[test]
    fn display_environment_changes_advance_generation_without_suspending() {
        let state = AtomicU64::new(desktop_state_value(4, true));

        assert_eq!(record_desktop_environment_change(&state), 5);
        assert_eq!(desktop_state_snapshot(&state), (true, 5));
    }

    #[test]
    fn desktop_barrier_acknowledges_every_earlier_fence() {
        let barrier = platform::DesktopEventBarrier::default();
        let first = barrier.request_sequence();
        let second = barrier.request_sequence();

        barrier.acknowledge(second);

        assert!(barrier.is_acknowledged(first));
        assert!(barrier.is_acknowledged(second));
        assert!(!barrier.is_acknowledged(barrier.request_sequence()));
    }

    #[test]
    fn capture_failure_prefers_desktop_unavailable_after_disconnect() {
        let desktop_error = ComputerUseError::new(
            ComputerUseErrorCode::DesktopUnavailable,
            "the remote desktop disconnected",
        );

        let error = resolve_capture_error("PrintWindow timed out", Err(desktop_error));

        assert_eq!(error.code, ComputerUseErrorCode::DesktopUnavailable);
        assert!(error.message.contains("disconnected"));
    }

    #[test]
    fn capture_failure_is_preserved_while_desktop_remains_interactive() {
        let error = resolve_capture_error("PrintWindow timed out", Ok(7));

        assert_eq!(error.code, ComputerUseErrorCode::CaptureFailed);
        assert!(error.message.contains("PrintWindow timed out"));
    }

    #[test]
    fn observations_expire_across_desktop_transitions() {
        let observation = ComputerUseObservation {
            observation_id: "window:7".to_string(),
            window_handle: 0x1234,
            process_id: 42,
            window_title: "Godot".to_string(),
            width: 800,
            height: 600,
            source_rect: [0, 0, 800, 600],
            dpi_scale: 1.0,
            window_dpi: 96,
            capture_backend: "window".to_string(),
            timestamp_ms: 1,
            desktop_generation: 7,
        };

        assert!(validate_observation_desktop(&observation, 7).is_ok());
        let error = validate_observation_desktop(&observation, 8).unwrap_err();
        assert_eq!(error.code, ComputerUseErrorCode::StaleObservation);
        assert!(error.message.contains("desktop changed"));
    }

    #[test]
    fn beginning_a_new_capture_invalidates_the_previous_observation() {
        let mut state = SessionState {
            observation: Some(ComputerUseObservation {
                observation_id: "window:7".to_string(),
                window_handle: 0x1234,
                process_id: 42,
                window_title: "Godot".to_string(),
                width: 800,
                height: 600,
                source_rect: [0, 0, 800, 600],
                dpi_scale: 1.0,
                window_dpi: 96,
                capture_backend: "window".to_string(),
                timestamp_ms: 1,
                desktop_generation: 7,
            }),
            ..SessionState::default()
        };

        invalidate_observation_before_capture(&mut state);

        assert!(state.observation.is_none());
    }

    #[test]
    fn beginning_an_action_consumes_the_observation_before_native_preflight() {
        let mut state = SessionState {
            observation: Some(ComputerUseObservation {
                observation_id: "window:8".to_string(),
                window_handle: 0x1234,
                process_id: 42,
                window_title: "Godot".to_string(),
                width: 800,
                height: 600,
                source_rect: [0, 0, 800, 600],
                dpi_scale: 1.0,
                window_dpi: 96,
                capture_backend: "window".to_string(),
                timestamp_ms: 1,
                desktop_generation: 8,
            }),
            ..SessionState::default()
        };

        let observation = take_observation_for_action(&mut state).unwrap();

        assert_eq!(observation.observation_id, "window:8");
        assert!(state.observation.is_none());
    }

    #[test]
    fn failed_banner_start_retains_a_pending_control_thread_for_cleanup() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = release_rx.recv();
        });
        let mut state = SessionState::default();

        let error = retain_pending_control_thread(
            &mut state,
            Err(platform::ControlBannerStartError {
                error: ComputerUseError::new(
                    ComputerUseErrorCode::BackendUnavailable,
                    "banner startup failed",
                ),
                thread: Some(thread),
            }),
        )
        .unwrap_err();

        assert_eq!(error.code, ComputerUseErrorCode::BackendUnavailable);
        assert!(state.overlay_thread.is_some());
        release_tx.send(()).unwrap();
        state.overlay_thread.take().unwrap().join().unwrap();
    }

    #[test]
    fn action_payload_round_trips_all_computer_use_fields() {
        let raw = json!({
            "action": "drag",
            "observation_id": "window:7",
            "button": "left",
            "path": [{"x": 10.0, "y": 20.0}, {"x": 40.0, "y": 60.0}],
            "duration_ms": 250,
        });
        let action: ComputerUseAction = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(action.action, "drag");
        assert_eq!(action.path.len(), 2);
        assert_eq!(serde_json::to_value(action).unwrap(), raw);
    }

    #[test]
    fn session_requires_an_adapter_runtime_bound_native_scope() {
        let error = ComputerUseTargetScope::new(None, None).unwrap_err();
        assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);

        let error = ComputerUseTargetScope::new(Some(0), None).unwrap_err();
        assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);

        let session = ComputerUseSession::new(
            trusted_pid_scope(7),
            None,
            None,
            None,
            Some("Godot".to_string()),
        )
        .unwrap();
        assert_eq!(session.spec.process_id, Some(7));

        let error = ComputerUseSession::new(
            trusted_pid_scope(7),
            Some(8),
            None,
            None,
            Some("Godot".to_string()),
        )
        .unwrap_err();
        assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);
    }

    #[test]
    fn sensitive_windows_are_denied_before_native_capture_or_input() {
        for process_name in [
            "PowerShell.exe",
            "powershell_ise.exe",
            "cmd",
            "Bitwarden",
            "SecHealthUI",
        ] {
            let process_name = process_name.trim_end_matches(".exe");
            assert!(
                denied_target_reason(process_name, "ApplicationFrameWindow", "Workspace").is_some()
            );
        }
        assert!(denied_target_reason("Godot", "ConsoleWindowClass", "Project").is_some());
        assert!(denied_target_reason("explorer", "#32770", "Run").is_some());
        assert!(denied_target_reason("explorer", "#32770", "\u{8fd0}\u{884c}").is_some());
        assert!(denied_target_reason("explorer", "#32770", "\u{30d5}\u{30a1}\u{30a4}\u{30eb}\u{540d}\u{3092}\u{6307}\u{5b9a}\u{3057}\u{3066}\u{5b9f}\u{884c}").is_some());
        for (process, class, title) in [
            ("Godot", "Qt5152QWindowIcon", "Project"),
            ("maya", "Qt5152QWindowIcon", "Autodesk Maya"),
            ("Photoshop", "Photoshop", "Adobe Photoshop"),
        ] {
            assert!(denied_target_reason(process, class, title).is_none());
        }
    }

    #[test]
    fn trusted_scope_is_rechecked_against_the_resolved_native_identity() {
        let target = WindowInfo {
            handle: 0x1234,
            pid: 42,
            title: "Project - Godot Engine".to_string(),
            rect: [0, 0, 800, 600],
        };
        let exact = ComputerUseTargetScope::new(Some(42), Some(0x1234)).unwrap();
        assert!(exact.validate_target(&target).is_ok());

        for scope in [
            ComputerUseTargetScope::new(Some(99), None).unwrap(),
            ComputerUseTargetScope::new(None, Some(0x5678)).unwrap(),
        ] {
            let error = scope.validate_target(&target).unwrap_err();
            assert_eq!(error.code, ComputerUseErrorCode::PermissionDenied);
        }
    }

    #[cfg(windows)]
    #[test]
    fn public_perform_restores_a_minimized_observed_window_before_rect_validation() {
        use windows::Win32::System::Threading::GetCurrentProcessId;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, IsIconic, SW_MINIMIZE, ShowWindow, WINDOW_EX_STYLE,
            WINDOW_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };
        use windows::core::PCWSTR;

        struct TestWindow(windows::Win32::Foundation::HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }

        let _interrupt_guard = user_interrupt_test_guard();
        let _dpi_awareness = platform::ThreadDpiAwareness::enter().unwrap();
        platform::clear_user_interrupt().unwrap();
        let class = "STATIC\0".encode_utf16().collect::<Vec<_>>();
        let title = "DCC MCP minimized restore target\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0),
                200,
                200,
                640,
                480,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let target_window = TestWindow(hwnd);
        let handle = hwnd.0 as usize as u64;
        let process_id = unsafe { GetCurrentProcessId() };
        let session = ComputerUseSession::new(
            ComputerUseTargetScope::new(Some(process_id), Some(handle)).unwrap(),
            Some(process_id),
            Some(handle),
            None,
            Some("test DCC".to_string()),
        )
        .unwrap();
        session.start().unwrap();

        let target = WindowFinder::new().info_for_handle(handle).unwrap();
        let desktop_generation = session.status()["desktop_generation"].as_u64().unwrap();
        let observed_dpi = platform::window_dpi(handle).unwrap();
        let observation = ComputerUseObservation {
            observation_id: "minimized-restore-observation".to_string(),
            window_handle: handle,
            process_id,
            window_title: target.title.clone(),
            width: target.rect[2] as u32,
            height: target.rect[3] as u32,
            source_rect: target.rect,
            dpi_scale: 1.0,
            window_dpi: observed_dpi,
            capture_backend: "test".to_string(),
            timestamp_ms: 1,
            desktop_generation,
        };
        session.lock_state().observation = Some(observation);

        let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
        assert!(unsafe { IsIconic(hwnd) }.as_bool());
        let result = session.perform(&ComputerUseAction {
            action: "wait".to_string(),
            observation_id: Some("minimized-restore-observation".to_string()),
            x: None,
            y: None,
            button: None,
            scroll_x: None,
            scroll_y: None,
            path: Vec::new(),
            text: None,
            keys: Vec::new(),
            duration_ms: Some(1),
        });

        let restored = WindowFinder::new().info_for_handle(handle).unwrap();
        let restored_dpi = platform::window_dpi(handle).unwrap();
        assert!(
            result.is_ok(),
            "{result:?}; observed_rect={:?}; restored_rect={:?}; observed_dpi={}; restored_dpi={restored_dpi}",
            target.rect,
            restored.rect,
            observed_dpi,
        );
        assert!(!unsafe { IsIconic(hwnd) }.as_bool());
        let _ = session.stop();
        drop(target_window);
        platform::clear_user_interrupt().unwrap();
    }

    #[test]
    fn target_constraints_are_an_intersection() {
        let spec = TargetSpec {
            process_id: Some(42),
            window_handle: Some(0x1234),
            window_title: Some("godot".to_string()),
            app_name: "Godot".to_string(),
        };
        let matching = WindowInfo {
            handle: 0x1234,
            pid: 42,
            title: "Project - Godot Engine".to_string(),
            rect: [0, 0, 800, 600],
        };
        assert!(target_matches(&spec, &matching));

        for rejected in [
            WindowInfo {
                pid: 99,
                ..matching.clone()
            },
            WindowInfo {
                handle: 0x5678,
                ..matching.clone()
            },
            WindowInfo {
                title: "Another application".to_string(),
                ..matching.clone()
            },
        ] {
            let error = validate_target_constraints(&spec, &rejected).unwrap_err();
            assert_eq!(error.code, ComputerUseErrorCode::InvalidTarget);
        }
    }

    #[test]
    fn pid_only_scope_rejects_multiple_visible_windows() {
        let spec = TargetSpec {
            process_id: Some(42),
            window_handle: None,
            window_title: None,
            app_name: "Godot".to_string(),
        };
        let windows = [
            WindowInfo {
                handle: 0x1234,
                pid: 42,
                title: "Godot Project Manager".to_string(),
                rect: [0, 0, 800, 600],
            },
            WindowInfo {
                handle: 0x5678,
                pid: 42,
                title: "Project - Godot Engine".to_string(),
                rect: [100, 100, 1200, 800],
            },
        ];

        let error = select_unique_target(&spec, windows).unwrap_err();

        assert_eq!(error.code, ComputerUseErrorCode::InvalidTarget);
        assert!(error.message.contains("exact window_handle"));
    }

    #[test]
    fn lost_target_is_not_reported_as_user_interruption() {
        let session = test_session(7);
        let state = SessionState {
            active: true,
            ..SessionState::default()
        };
        state.interrupted.store(true, Ordering::Release);

        let error = session.ensure_running(&state).unwrap_err();

        assert_eq!(error.code, ComputerUseErrorCode::MissingWindow);
    }

    #[test]
    fn stop_cancels_a_long_wait_before_taking_the_state_lock() {
        let _interrupt_guard = user_interrupt_test_guard();
        platform::clear_user_interrupt().unwrap();
        let session = Arc::new(test_session(7));
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
        let worker_session = Arc::clone(&session);
        let worker = std::thread::spawn(move || {
            let _state = worker_session.lock_state();
            started_tx.send(()).unwrap();
            cancellable_wait(Duration::from_secs(5), &worker_session.stop_requested)
        });
        started_rx.recv().unwrap();

        let started = Instant::now();
        let stopped = session.stop();
        let elapsed = started.elapsed();
        let error = worker.join().unwrap().unwrap_err();

        assert!(elapsed < Duration::from_secs(1), "stop took {elapsed:?}");
        assert_eq!(error.code, ComputerUseErrorCode::BackendUnavailable);
        assert_eq!(stopped["active"], false);
        platform::clear_user_interrupt().unwrap();
    }

    #[test]
    fn control_thread_join_is_bounded_when_a_worker_is_stuck() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = release_rx.recv();
        });

        let started = Instant::now();
        assert!(join_control_thread_with_timeout(thread, Duration::from_millis(30)).is_some());
        assert!(started.elapsed() < Duration::from_millis(250));

        release_tx.send(()).unwrap();
    }

    #[test]
    fn control_thread_join_waits_for_normal_cleanup() {
        let thread = std::thread::spawn(|| {});

        assert!(join_control_thread_with_timeout(thread, Duration::from_millis(250)).is_none());
    }

    #[test]
    fn start_does_not_reset_stop_while_previous_cleanup_is_pending() {
        let session = test_session(7);
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        {
            let mut state = session.lock_state();
            state.cleanup_pending.store(true, Ordering::Release);
            state.overlay_thread = Some(std::thread::spawn(move || {
                let _ = release_rx.recv();
            }));
        }
        session.stop_requested.store(true, Ordering::Release);

        let error = session.start().unwrap_err();

        assert_eq!(error.code, ComputerUseErrorCode::BackendUnavailable);
        assert!(session.stop_requested.load(Ordering::Acquire));
        assert_eq!(session.status()["cleanup_pending"], true);

        release_tx.send(()).unwrap();
        let _ = session.stop();
    }

    #[test]
    fn screenshot_scale_bounds_large_frames_without_upscaling() {
        assert_eq!(screenshot_scale([0, 0, 800, 600]), 1.0);

        let scale = screenshot_scale([0, 0, 2400, 1500]);
        assert!(scale < 0.65);
        assert!(scale > 0.64);
    }

    #[test]
    fn captured_target_identity_and_rect_must_remain_stable() {
        let before = WindowInfo {
            handle: 0x1234,
            pid: 42,
            title: "Godot".to_string(),
            rect: [100, 200, 800, 600],
        };
        assert_eq!(
            validate_captured_target(&before, &before, Some(before.rect)).unwrap(),
            before.rect
        );

        let reused = WindowInfo {
            pid: 99,
            ..before.clone()
        };
        assert_eq!(
            validate_captured_target(&before, &reused, Some(before.rect))
                .unwrap_err()
                .code,
            ComputerUseErrorCode::InvalidTarget
        );

        let moved = WindowInfo {
            rect: [110, 200, 800, 600],
            ..before.clone()
        };
        assert_eq!(
            validate_captured_target(&before, &moved, Some(before.rect))
                .unwrap_err()
                .code,
            ComputerUseErrorCode::StaleObservation
        );
    }

    #[test]
    fn native_action_limits_are_enforced_beyond_the_schema() {
        let mut request = ComputerUseAction {
            action: "drag".to_string(),
            observation_id: None,
            x: None,
            y: None,
            button: None,
            scroll_x: None,
            scroll_y: None,
            path: vec![ComputerUsePoint { x: 1.0, y: 1.0 }; MAX_DRAG_POINTS + 1],
            text: None,
            keys: Vec::new(),
            duration_ms: None,
        };
        assert_eq!(
            validate_action_limits(&request).unwrap_err().code,
            ComputerUseErrorCode::InvalidAction
        );

        request.path.clear();
        request.keys = vec!["CTRL+SHIFT+ALT+A+B+C+D+E+F+G+H+I+J+K+L+M+N".to_string()];
        assert_eq!(
            validate_action_limits(&request).unwrap_err().code,
            ComputerUseErrorCode::InvalidAction
        );

        request.keys.clear();
        request.text = Some("x".repeat(MAX_TEXT_UTF16_UNITS + 1));
        assert_eq!(
            validate_action_limits(&request).unwrap_err().code,
            ComputerUseErrorCode::InvalidAction
        );
    }
}
