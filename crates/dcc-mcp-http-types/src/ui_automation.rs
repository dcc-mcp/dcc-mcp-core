//! Cross-DCC UI automation observation/action contract wire types.
//!
//! These schemas describe what adapters may expose. They do not implement a
//! universal clicker; each adapter remains responsible for Qt, accessibility,
//! webview, or host-specific backends and safety policy.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Rectangle in physical pixels or adapter-defined UI coordinates.
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

/// Bounded UI action kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiActionKind {
    /// Click or activate a control.
    Click,
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
}

/// Request to perform one bounded UI action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiActionRequest {
    /// Control id resolved from a snapshot or find operation.
    pub control_id: String,
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
    /// The control could not be found.
    NotFound,
    /// The backend does not support this action on this control.
    UnsupportedAction,
    /// Adapter-side safety policy denied the action.
    Denied,
    /// The backend timed out while performing the action.
    Timeout,
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
    fn ui_action_result_can_represent_stale_control() {
        let result = UiActionResult::stale("old-id");
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["error_code"], "stale_control");
        assert_eq!(value["success"], false);
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
