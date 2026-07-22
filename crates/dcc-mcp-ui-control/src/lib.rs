//! DCC-agnostic UI Control observation and action contract types.
//!
//! These schemas describe what adapters may expose. They do not implement a
//! universal clicker; each adapter remains responsible for Qt, accessibility,
//! webview, OS automation, or host-specific backends and safety policy.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod host_protocol;

/// Rectangle in the coordinate space declared by its enclosing snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UiBounds {
    /// Left coordinate.
    pub x: f64,
    /// Top coordinate.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// Point in screenshot-relative UI coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UiPoint {
    /// Horizontal screenshot coordinate.
    pub x: f64,
    /// Vertical screenshot coordinate.
    pub y: f64,
}

/// Declarative perception provider used by guarded replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiRecognizerType {
    /// Accessibility tree query.
    Accessibility,
    /// Bounded image template match.
    Template,
    /// Optical character recognition inside the scoped region.
    Ocr,
    /// Difference against a reviewed reference image.
    ImageDifference,
}

/// Region used by a visual recognizer; never a global desktop rectangle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiRecognitionRegion {
    /// Entire exact authorized target window.
    ExactWindow,
    /// Target-window-relative rectangle whose values must remain inside 0..=1.
    NormalizedRect {
        /// Left fraction.
        x: f64,
        /// Top fraction.
        y: f64,
        /// Width fraction.
        width: f64,
        /// Height fraction.
        height: f64,
    },
    /// Rectangle resolved relative to a freshly recognized anchor.
    AnchorRelative {
        /// Hash-addressed or accessibility anchor identifier.
        anchor: String,
        /// Normalized offset from the anchor bounds.
        offset: UiBounds,
    },
}

/// DCC-agnostic guarded replay recognizer contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiRecognizer {
    /// Recognition implementation family.
    pub recognizer_type: UiRecognizerType,
    /// Exact-window-bounded search region.
    pub region: UiRecognitionRegion,
    /// Hash-addressed template/reference artifact when required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
    /// OCR or accessibility query when required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Minimum accepted confidence, inclusive.
    pub threshold: f64,
    /// Bounded recognition timeout.
    pub timeout_ms: u64,
    /// Consecutive matching frames required before success.
    pub stable_frames: u32,
}

/// Exact target calibration recorded for visual fallback drift checks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UiVisualCalibration {
    /// Exact target client width in physical pixels.
    pub width: u32,
    /// Exact target client height in physical pixels.
    pub height: u32,
    /// Effective horizontal DPI.
    pub dpi_x: u32,
    /// Effective vertical DPI.
    pub dpi_y: u32,
    /// Monitor/topology generation captured by the provider.
    pub topology_generation: u64,
}

impl UiRecognizer {
    /// Validate fail-closed bounds before a provider sees the recognizer.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(0.0..=1.0).contains(&self.threshold) || self.threshold == 0.0 {
            return Err("threshold must be within (0, 1]");
        }
        if self.timeout_ms == 0 || self.timeout_ms > 60_000 {
            return Err("timeout_ms must be within 1..=60000");
        }
        if self.stable_frames == 0 || self.stable_frames > 120 {
            return Err("stable_frames must be within 1..=120");
        }
        match (&self.recognizer_type, &self.asset, &self.query) {
            (UiRecognizerType::Template | UiRecognizerType::ImageDifference, Some(asset), _)
                if asset.starts_with("artefact://sha256/") => {}
            (UiRecognizerType::Template | UiRecognizerType::ImageDifference, _, _) => {
                return Err("visual reference must use an artefact://sha256/ URI");
            }
            (UiRecognizerType::Ocr | UiRecognizerType::Accessibility, _, Some(query))
                if !query.trim().is_empty() => {}
            (UiRecognizerType::Ocr | UiRecognizerType::Accessibility, _, _) => {
                return Err("OCR and accessibility recognizers require a query");
            }
        }
        if let UiRecognitionRegion::NormalizedRect {
            x,
            y,
            width,
            height,
        } = self.region
            && (x < 0.0
                || y < 0.0
                || width <= 0.0
                || height <= 0.0
                || x + width > 1.0
                || y + height > 1.0)
        {
            return Err("normalized region must remain within the exact target window");
        }
        Ok(())
    }
}

impl UiVisualCalibration {
    /// Return whether replay geometry is identical to the reviewed calibration.
    #[must_use]
    pub fn matches(self, current: Self) -> bool {
        self == current && self.width > 0 && self.height > 0 && self.dpi_x > 0 && self.dpi_y > 0
    }
}

/// Normalized UI control node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiControlNode {
    /// Stable-ish control id scoped to the current adapter session.
    pub id: String,
    /// Role/type, such as `button`, `text_field`, `checkbox`, or `combo_box`.
    pub role: String,
    /// Visible label or accessible name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Current visible text when safe to expose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Host object/control name when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_name: Option<String>,
    /// Tooltip/help text when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Whether the control can be interacted with.
    pub enabled: bool,
    /// Whether the control is visible.
    pub visible: bool,
    /// Control bounds when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<UiBounds>,
    /// Value/current text for value-bearing controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Checked state for checkboxes/toggles/radio buttons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Child controls included in this bounded snapshot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<UiControlNode>,
    /// Adapter-specific metadata behind a namespaced map.
    #[serde(default)]
    pub metadata: Value,
}

impl UiControlNode {
    /// Construct a visible, enabled node with no children.
    #[must_use]
    pub fn new(id: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: role.into(),
            label: None,
            text: None,
            object_name: None,
            tooltip: None,
            enabled: true,
            visible: true,
            bounds: None,
            value: None,
            checked: None,
            children: Vec::new(),
            metadata: Value::Null,
        }
    }
}

/// Bounded UI tree snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiSnapshot {
    /// Optional session id this snapshot belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Root of the captured UI subtree.
    pub root: UiControlNode,
    /// Focused control id when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_id: Option<String>,
    /// Whether the adapter truncated the tree.
    pub truncated: bool,
    /// Number of nodes represented in `root`.
    pub node_count: usize,
    /// Adapter-defined snapshot metadata.
    #[serde(default)]
    pub metadata: Value,
}

/// Request for locating controls in a bounded UI snapshot/backend.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UiFindRequest {
    /// Fuzzy or exact text query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Role/type filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Label filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Object-name filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_name: Option<String>,
    /// Maximum matches to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Condition kind evaluated by an `ui_control__wait_for` style tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiWaitConditionKind {
    /// A matching control must exist.
    ControlExists,
    /// A matching control must be absent.
    ControlMissing,
    /// The control text must equal the expected text.
    TextEquals,
    /// The control value must equal the expected value.
    ValueEquals,
    /// The checked state must equal the expected value.
    CheckedEquals,
    /// The control must be enabled.
    Enabled,
    /// The control must be disabled.
    Disabled,
    /// The control must have focus.
    Focused,
}

const fn default_wait_timeout_ms() -> u64 {
    5_000
}

const fn default_wait_interval_ms() -> u64 {
    100
}

const fn default_true() -> bool {
    true
}

const fn is_false(value: &bool) -> bool {
    !*value
}

/// Polling condition for a bounded UI backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiWaitCondition {
    /// Condition kind to evaluate.
    pub kind: UiWaitConditionKind,
    /// Exact control id to inspect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_id: Option<String>,
    /// Fuzzy/exact query used to resolve a control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Role/type filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Label filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Expected visible text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Expected value/current text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Expected checked state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Maximum time to poll inside the tool call.
    #[serde(default = "default_wait_timeout_ms")]
    pub timeout_ms: u64,
    /// Poll interval inside the tool call.
    #[serde(default = "default_wait_interval_ms")]
    pub interval_ms: u64,
}

/// Bounded UI action kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiActionKind {
    /// Click or activate a control.
    Click,
    /// Move the pointer to raw snapshot coordinates. Disabled by policy by default.
    Move,
    /// Double-click at raw snapshot coordinates. Disabled by policy by default.
    DoubleClick,
    /// Scroll at raw snapshot coordinates. Disabled by policy by default.
    Scroll,
    /// Drag through raw snapshot coordinates. Disabled by policy by default.
    Drag,
    /// Fallback click at raw coordinates. Disabled by policy by default.
    RawCoordinateClick,
    /// Type literal text through the keyboard. Disabled by policy by default.
    Type,
    /// Send one or more key presses. Disabled by policy by default.
    Keypress,
    /// Set text/value on an editable control.
    SetText,
    /// Toggle a binary control.
    Toggle,
    /// Set an explicit checked state.
    SetChecked,
    /// Select an option in a combo/list/menu.
    SelectOption,
    /// Move focus to a control.
    Focus,
    /// Send a keyboard shortcut. Disabled by policy by default.
    KeyboardShortcut,
    /// Read exact PID/HWND window state without requiring a screenshot.
    GetWindowState,
    /// Restore the exact scoped window when minimized.
    RestoreWindow,
    /// Show the exact scoped window without activating it.
    ShowWindow,
    /// Activate the exact visible, non-minimized scoped window.
    ActivateWindow,
}

/// Policy controls for scoped `ui_control` observation and actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiControlPolicy {
    /// Allow observing snapshots.
    pub allow_snapshot: bool,
    /// Allow finding controls.
    pub allow_find: bool,
    /// Allow any mutating UI action.
    pub allow_mutating_actions: bool,
    /// Allow text entry.
    pub allow_text_entry: bool,
    /// Allow keyboard shortcuts.
    pub allow_keyboard_shortcuts: bool,
    /// Allow raw-coordinate actions.
    pub allow_raw_coordinates: bool,
    /// Require the backend to target a scoped application window/process.
    #[serde(default = "default_true")]
    pub require_scoped_window: bool,
    /// Optional allow-list for window titles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_window_titles: Vec<String>,
    /// Optional allow-list for OS process ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_process_ids: Vec<u32>,
    /// Whether audit sinks may include sensitive values such as typed text.
    pub audit_sensitive_values: bool,
    /// Fail-closed marker when requested scope restrictions do not intersect
    /// the runtime allow-list.
    #[serde(default, skip_serializing_if = "is_false")]
    pub scope_denied: bool,
}

impl Default for UiControlPolicy {
    fn default() -> Self {
        Self {
            allow_snapshot: true,
            allow_find: true,
            allow_mutating_actions: true,
            allow_text_entry: true,
            allow_keyboard_shortcuts: false,
            allow_raw_coordinates: false,
            require_scoped_window: true,
            allowed_window_titles: Vec::new(),
            allowed_process_ids: Vec::new(),
            audit_sensitive_values: false,
            scope_denied: false,
        }
    }
}

impl UiControlPolicy {
    /// Return whether this policy permits an action kind.
    #[must_use]
    pub fn allows_action(&self, action: UiActionKind) -> bool {
        if self.scope_denied {
            return false;
        }
        match action {
            UiActionKind::GetWindowState => self.allow_snapshot,
            UiActionKind::Move
            | UiActionKind::DoubleClick
            | UiActionKind::Scroll
            | UiActionKind::Drag
            | UiActionKind::RawCoordinateClick => {
                self.allow_mutating_actions && self.allow_raw_coordinates
            }
            UiActionKind::Keypress | UiActionKind::KeyboardShortcut => {
                self.allow_mutating_actions && self.allow_keyboard_shortcuts
            }
            UiActionKind::Type => {
                self.allow_mutating_actions
                    && self.allow_text_entry
                    && self.allow_keyboard_shortcuts
            }
            UiActionKind::SetText => self.allow_mutating_actions && self.allow_text_entry,
            UiActionKind::Click
            | UiActionKind::Toggle
            | UiActionKind::SetChecked
            | UiActionKind::SelectOption
            | UiActionKind::Focus
            | UiActionKind::RestoreWindow
            | UiActionKind::ShowWindow
            | UiActionKind::ActivateWindow => self.allow_mutating_actions,
        }
    }

    /// Return whether this policy permits a complete action request.
    ///
    /// A `click` with screenshot coordinates is raw input, while a `click`
    /// with only a control id remains a semantic accessibility action.
    #[must_use]
    pub fn allows_request(&self, request: &UiActionRequest) -> bool {
        if self.scope_denied {
            return false;
        }
        if request.action == UiActionKind::Click && (request.x.is_some() || request.y.is_some()) {
            return self.allow_mutating_actions && self.allow_raw_coordinates;
        }
        self.allows_action(request.action)
    }
}

/// Request to perform one bounded UI action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiActionRequest {
    /// Control id resolved from a snapshot or find operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_id: Option<String>,
    /// Action to perform.
    pub action: UiActionKind,
    /// Text payload for `set_text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Checked payload for `set_checked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Option label/id for `select_option`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    /// X coordinate for raw-coordinate fallback actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Y coordinate for raw-coordinate fallback actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Mouse button for pointer actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
    /// Horizontal wheel delta for `scroll`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_x: Option<i32>,
    /// Vertical wheel delta for `scroll`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_y: Option<i32>,
    /// Ordered screenshot-relative points for `drag`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<UiPoint>,
    /// Keyboard shortcut keys for `keyboard_shortcut`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    /// Snapshot/observation id used to reject stale coordinates and controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    /// Optional action duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Adapter-defined action metadata.
    #[serde(default)]
    pub metadata: Value,
}

/// Structured UI action failure reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiErrorCode {
    /// The resolved control id is no longer valid.
    StaleControl,
    /// Screenshot coordinates no longer match the scoped window generation.
    StaleObservation,
    /// The control could not be found.
    NotFound,
    /// The backend does not support this action on this control.
    UnsupportedAction,
    /// Adapter-side safety policy denied the action.
    Denied,
    /// Runtime policy disabled the action category.
    PolicyDisabled,
    /// The scoped application window is missing or unavailable.
    MissingWindow,
    /// The backend timed out while performing the action.
    Timeout,
    /// The target exists but is not valid for this action.
    InvalidTarget,
    /// The user pressed the reserved stop key.
    UserInterrupted,
    /// The scoped application lost foreground focus before input injection.
    FocusLost,
    /// The Windows desktop is locked, disconnected, or otherwise not interactive.
    DesktopUnavailable,
    /// OS policy or process integrity denied the action.
    PermissionDenied,
    /// The native or semantic backend is unavailable in this runtime.
    BackendUnavailable,
    /// The action payload is invalid for the selected operation.
    InvalidAction,
    /// Native pointer or keyboard injection failed.
    InputFailed,
    /// The scoped window could not be captured.
    CaptureFailed,
    /// The backend failed for an adapter-specific reason.
    BackendError,
}

/// Small resource/artifact reference included in UI results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiArtifactRef {
    /// Resource URI for the artifact.
    pub uri: String,
    /// MIME type when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

/// Result of one bounded UI action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiActionResult {
    /// Whether the action succeeded.
    pub success: bool,
    /// Control id the action targeted.
    pub control_id: String,
    /// Structured error code on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<UiErrorCode>,
    /// Human-readable result or error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Focused control before the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_focus_id: Option<String>,
    /// Focused control after the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_focus_id: Option<String>,
    /// Screenshot/log/report artifacts produced by the action.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<UiArtifactRef>,
    /// Adapter-defined result metadata.
    #[serde(default)]
    pub metadata: Value,
}

impl UiActionResult {
    /// Build a stale-control failure result.
    #[must_use]
    pub fn stale(control_id: impl Into<String>) -> Self {
        Self {
            success: false,
            control_id: control_id.into(),
            error_code: Some(UiErrorCode::StaleControl),
            message: Some("control is stale; refresh the UI snapshot".to_owned()),
            before_focus_id: None,
            after_focus_id: None,
            artifacts: Vec::new(),
            metadata: Value::Null,
        }
    }
}

/// Result of evaluating a UI wait condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiWaitResult {
    /// Whether the condition became true.
    pub success: bool,
    /// Condition that was evaluated.
    pub condition: UiWaitCondition,
    /// Elapsed wall-clock time in milliseconds.
    pub elapsed_ms: f64,
    /// Number of polling attempts.
    pub attempts: u32,
    /// Latest snapshot when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<UiSnapshot>,
    /// Structured error code on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<UiErrorCode>,
    /// Human-readable result or error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Adapter-defined result metadata.
    #[serde(default)]
    pub metadata: Value,
}

/// Small audit record for a `ui_control` action decision or outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiControlAuditRecord {
    /// Action kind that was attempted.
    pub action_kind: String,
    /// Whether the decision/outcome succeeded.
    pub success: bool,
    /// Target control id when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_control_id: Option<String>,
    /// Target control role when safe to record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_role: Option<String>,
    /// Target control label when safe to record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_label: Option<String>,
    /// Focused control before the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_focus_id: Option<String>,
    /// Focused control after the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_focus_id: Option<String>,
    /// Structured error code on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<UiErrorCode>,
    /// Human-readable audit message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// UI Control session id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Fields intentionally redacted before audit storage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redacted_fields: Vec<String>,
    /// Adapter-defined audit metadata.
    #[serde(default)]
    pub metadata: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_snapshot_round_trips_with_adapter_metadata() {
        let mut button = UiControlNode::new("btn-save", "button");
        button.label = Some("Save".to_owned());
        button.metadata = serde_json::json!({"qt": {"class": "QPushButton"}});
        let snapshot = UiSnapshot {
            session_id: Some("session-1".to_owned()),
            root: button,
            focus_id: Some("btn-save".to_owned()),
            truncated: false,
            node_count: 1,
            metadata: serde_json::json!({"adapter": "maya"}),
        };

        let encoded = serde_json::to_string(&snapshot).unwrap();
        let decoded: UiSnapshot = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn visual_recognizer_requires_bounded_content_addressed_inputs() {
        let recognizer = UiRecognizer {
            recognizer_type: UiRecognizerType::Template,
            region: UiRecognitionRegion::NormalizedRect {
                x: 0.1,
                y: 0.2,
                width: 0.5,
                height: 0.5,
            },
            asset: Some(format!("artefact://sha256/{}", "a".repeat(64))),
            query: None,
            threshold: 0.92,
            timeout_ms: 5_000,
            stable_frames: 2,
        };

        assert!(recognizer.validate().is_ok());
        let mut unbounded = recognizer.clone();
        unbounded.region = UiRecognitionRegion::NormalizedRect {
            x: 0.8,
            y: 0.0,
            width: 0.3,
            height: 1.0,
        };
        assert!(unbounded.validate().is_err());
    }

    #[test]
    fn visual_calibration_rejects_dpi_geometry_and_topology_drift() {
        let recorded = UiVisualCalibration {
            width: 1920,
            height: 1080,
            dpi_x: 144,
            dpi_y: 144,
            topology_generation: 7,
        };
        assert!(recorded.matches(recorded));
        assert!(!recorded.matches(UiVisualCalibration {
            dpi_x: 96,
            dpi_y: 96,
            ..recorded
        }));
        assert!(!recorded.matches(UiVisualCalibration {
            topology_generation: 8,
            ..recorded
        }));
    }

    #[test]
    fn ui_action_result_can_represent_stale_control() {
        let result = UiActionResult::stale("old-id");
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["error_code"], "stale_control");
        assert_eq!(value["success"], false);
    }

    #[test]
    fn computer_use_error_codes_have_stable_wire_names() {
        for (code, expected) in [
            (UiErrorCode::BackendUnavailable, "backend_unavailable"),
            (UiErrorCode::DesktopUnavailable, "desktop_unavailable"),
            (UiErrorCode::InvalidAction, "invalid_action"),
            (UiErrorCode::InputFailed, "input_failed"),
            (UiErrorCode::CaptureFailed, "capture_failed"),
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), expected);
        }
    }

    #[test]
    fn ui_control_policy_blocks_high_risk_actions_by_default() {
        let policy = UiControlPolicy::default();

        assert!(policy.allows_action(UiActionKind::Click));
        assert!(policy.allows_action(UiActionKind::SetText));
        assert!(policy.allows_action(UiActionKind::GetWindowState));
        assert!(policy.allows_action(UiActionKind::RestoreWindow));
        assert!(!policy.allows_action(UiActionKind::RawCoordinateClick));
        assert!(!policy.allows_action(UiActionKind::KeyboardShortcut));
        assert!(policy.require_scoped_window);
    }

    #[test]
    fn denied_scope_blocks_even_semantic_actions() {
        let policy = UiControlPolicy {
            scope_denied: true,
            ..UiControlPolicy::default()
        };

        assert!(!policy.allows_action(UiActionKind::Click));
        assert_eq!(serde_json::to_value(policy).unwrap()["scope_denied"], true);
    }

    #[test]
    fn computer_use_actions_require_explicit_raw_input_policy() {
        let policy = UiControlPolicy::default();

        for action in [
            UiActionKind::Move,
            UiActionKind::DoubleClick,
            UiActionKind::Scroll,
            UiActionKind::Drag,
            UiActionKind::Type,
            UiActionKind::Keypress,
        ] {
            assert!(!policy.allows_action(action));
        }

        let enabled = UiControlPolicy {
            allow_raw_coordinates: true,
            allow_keyboard_shortcuts: true,
            ..UiControlPolicy::default()
        };
        for action in [
            UiActionKind::Move,
            UiActionKind::DoubleClick,
            UiActionKind::Scroll,
            UiActionKind::Drag,
            UiActionKind::Type,
            UiActionKind::Keypress,
        ] {
            assert!(enabled.allows_action(action));
        }

        assert_eq!(
            serde_json::to_value(UiActionKind::DoubleClick).unwrap(),
            "double_click"
        );
        assert_eq!(serde_json::to_value(UiActionKind::Type).unwrap(), "type");
    }

    #[test]
    fn coordinate_click_is_checked_as_raw_input() {
        let request = UiActionRequest {
            control_id: None,
            action: UiActionKind::Click,
            text: None,
            checked: None,
            option: None,
            x: Some(10.0),
            y: Some(20.0),
            button: Some("left".to_owned()),
            scroll_x: None,
            scroll_y: None,
            path: Vec::new(),
            keys: Vec::new(),
            snapshot_id: Some("godot:1".to_owned()),
            duration_ms: None,
            metadata: Value::Null,
        };

        assert!(!UiControlPolicy::default().allows_request(&request));
        assert!(
            UiControlPolicy {
                allow_raw_coordinates: true,
                ..UiControlPolicy::default()
            }
            .allows_request(&request)
        );
        assert!(
            !UiControlPolicy {
                allow_raw_coordinates: true,
                scope_denied: true,
                ..UiControlPolicy::default()
            }
            .allows_request(&request)
        );
        let encoded = serde_json::to_value(request).unwrap();
        assert_eq!(encoded["button"], "left");
        assert_eq!(encoded["snapshot_id"], "godot:1");
    }

    #[test]
    fn ui_control_policy_deserializes_old_payloads_as_scoped() {
        let policy: UiControlPolicy = serde_json::from_value(serde_json::json!({
            "allow_snapshot": true,
            "allow_find": true,
            "allow_mutating_actions": true,
            "allow_text_entry": true,
            "allow_keyboard_shortcuts": false,
            "allow_raw_coordinates": false,
            "audit_sensitive_values": false
        }))
        .unwrap();

        assert!(policy.require_scoped_window);
    }

    #[test]
    fn ui_wait_condition_serializes_polling_defaults() {
        let condition = UiWaitCondition {
            kind: UiWaitConditionKind::TextEquals,
            control_id: Some("status".to_owned()),
            query: None,
            role: None,
            label: None,
            text: Some("Ready".to_owned()),
            value: None,
            checked: None,
            timeout_ms: default_wait_timeout_ms(),
            interval_ms: default_wait_interval_ms(),
        };

        let value = serde_json::to_value(&condition).unwrap();
        assert_eq!(value["kind"], "text_equals");
        assert_eq!(value["timeout_ms"], 5000);
    }

    #[test]
    fn ui_control_audit_record_redacts_sensitive_fields() {
        let record = UiControlAuditRecord {
            action_kind: "set_text".to_owned(),
            success: false,
            target_control_id: Some("project-name".to_owned()),
            target_role: Some("text_field".to_owned()),
            target_label: Some("Project".to_owned()),
            before_focus_id: Some("project-name".to_owned()),
            after_focus_id: Some("project-name".to_owned()),
            error_code: Some(UiErrorCode::PolicyDisabled),
            message: Some("text entry disabled by policy".to_owned()),
            session_id: Some("session-1".to_owned()),
            redacted_fields: vec!["text".to_owned()],
            metadata: Value::Null,
        };

        let value = serde_json::to_value(record).unwrap();
        assert_eq!(value["error_code"], "policy_disabled");
        assert_eq!(value["redacted_fields"][0], "text");
    }

    #[test]
    fn ui_snapshot_can_mark_bounded_truncation() {
        let snapshot = UiSnapshot {
            session_id: None,
            root: UiControlNode::new("root", "window"),
            focus_id: None,
            truncated: true,
            node_count: 500,
            metadata: serde_json::json!({"limit": 500}),
        };

        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(value["truncated"], true);
        assert_eq!(value["node_count"], 500);
        assert_eq!(value["metadata"]["limit"], 500);
    }
}
