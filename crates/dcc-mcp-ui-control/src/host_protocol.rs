//! Versioned local protocol for the isolated DCC UI Control host.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Only wire version supported by `dcc-mcp-ui-control-host`.
pub const UI_CONTROL_HOST_PROTOCOL_VERSION: u32 = 2;

/// Maximum accepted JSON frame size. Screenshot pixels travel through shared memory.
pub const UI_CONTROL_HOST_MAX_FRAME_BYTES: u32 = 4 * 1024 * 1024;

/// Host capabilities implemented by protocol version 2.
pub const UI_CONTROL_HOST_CAPABILITIES: &[&str] = &[
    "exact_window_capabilities",
    "exact_window_state",
    "scoped_window_restore_show_activate",
    "shared_memory_snapshots",
    "exact_window_recording",
    "connection_scoped_sessions",
    "uia_snapshot_and_actions",
    "scoped_raw_input",
    "observation_fencing",
    "trusted_confirmation",
    "global_stop_latch",
    "redacted_audit",
    "typed_system_operations",
];

/// Client handshake required before any stateful request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiControlHostHello {
    /// Exact protocol version requested by the client.
    pub protocol_version: u32,
    /// Diagnostic-only client name.
    pub client_name: String,
}

/// Runtime-selected task scope used to open one host session.
///
/// The target identifiers come from the adapter/runtime boundary and never from
/// an agent-facing approval flag. The native host validates the target and
/// starts a visible, non-modal notice before minting a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiControlTaskGrant {
    /// Stable correlation id issued by the trusted adapter/runtime layer.
    pub task_grant_id: String,
    /// Selected DCC family, including custom DCC identifiers.
    pub dcc_type: String,
    /// Operator-bound process id, when the adapter owns one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u32>,
    /// Operator-bound top-level window handle, when the adapter owns one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_handle: Option<u64>,
    /// Whether raw pointer and keyboard input may be requested inside the target.
    #[serde(default)]
    pub allow_raw_input: bool,
}

/// One strongly typed, idempotent Windows configuration operation.
///
/// Registry keys are always relative to `HKEY_CURRENT_USER`. Commands,
/// executable paths, alternate registry hives, and overwrite semantics are
/// intentionally absent from this protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiControlSystemOperation {
    /// Ensure one `REG_SZ` value under `HKEY_CURRENT_USER`.
    EnsureRegistryString {
        /// HKCU-relative subkey.
        key: String,
        /// Registry value name. Empty selects the default value.
        value_name: String,
        /// Desired string value.
        value: String,
    },
    /// Ensure one `REG_DWORD` value under `HKEY_CURRENT_USER`.
    EnsureRegistryDword {
        /// HKCU-relative subkey.
        key: String,
        /// Registry value name. Empty selects the default value.
        value_name: String,
        /// Desired DWORD value.
        value: u32,
    },
    /// Ensure one file symbolic link without replacing an existing entry.
    EnsureFileSymlink {
        /// Absolute link path.
        link: String,
        /// Absolute existing file target.
        target: String,
    },
    /// Ensure one directory symbolic link without replacing an existing entry.
    EnsureDirectorySymlink {
        /// Absolute link path.
        link: String,
        /// Absolute existing directory target.
        target: String,
    },
}

impl UiControlSystemOperation {
    /// Static redacted name safe for prompts and audit logs.
    #[must_use]
    pub const fn audit_name(&self) -> &'static str {
        match self {
            Self::EnsureRegistryString { .. } => "ensure_hkcu_registry_string",
            Self::EnsureRegistryDword { .. } => "ensure_hkcu_registry_dword",
            Self::EnsureFileSymlink { .. } => "ensure_file_symlink",
            Self::EnsureDirectorySymlink { .. } => "ensure_directory_symlink",
        }
    }
}

/// One named operation in an operator-owned system grant.
///
/// Clients send only the non-sensitive identifier. The native host resolves
/// the typed operation locally so registry values and filesystem paths never
/// cross the agent-facing protocol boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiControlSystemGrantOperation {
    /// Stable, non-sensitive identifier exposed to the client.
    pub operation_id: String,
    /// Typed operation kept inside the trusted host configuration boundary.
    pub operation: UiControlSystemOperation,
}

/// Operator-owned catalog entry loaded by the host before accepting clients.
///
/// Clients open this grant by id; they never submit or widen its operation
/// allowlist over the host protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiControlSystemGrant {
    /// Stable id selected by trusted runtime/operator configuration.
    pub system_grant_id: String,
    /// DCC family associated with this setup task.
    pub dcc_type: String,
    /// Named operations resolved and executed only by the native host.
    pub operations: Vec<UiControlSystemGrantOperation>,
}

/// Idempotent result from a typed system operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiControlEnsureOutcome {
    /// The requested object did not exist and was created.
    Created,
    /// An existing registry value was changed to the requested value.
    Updated,
    /// The requested state already existed exactly.
    Unchanged,
}

/// Exact native window selected and validated by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiControlTarget {
    /// Owning process id.
    pub process_id: u32,
    /// Top-level native window handle.
    pub window_handle: u64,
    /// Current window title. This value is never written to host audit logs.
    pub window_title: String,
}

/// Observable state of the exact host-bound HWND.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiControlWindowState {
    /// Owning process id, revalidated for every query and transition.
    pub process_id: u32,
    /// Exact native window handle protected by the session capability.
    pub window_handle: u64,
    /// Whether the HWND still exists and belongs to the granted process.
    pub exists: bool,
    /// Whether Windows currently reports the target as visible.
    pub visible: bool,
    /// Whether Windows currently reports the target as minimized.
    pub minimized: bool,
    /// Whether this exact HWND is the current foreground window.
    pub foreground: bool,
}

/// Non-input state transition for the exact host-bound HWND.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiControlWindowOperation {
    /// Restore the exact target when it is minimized.
    Restore,
    /// Show the exact target without activating it.
    Show,
    /// Activate the exact visible, non-minimized target.
    Activate,
}

/// How an approved operation reaches the selected window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiControlInputKind {
    /// Accessibility or other structured application action.
    Semantic,
    /// Native pointer or keyboard input bounded to the selected window.
    RawInput,
}

/// Semantic intent used by the host to apply the confirmation policy.
///
/// The client hint may only raise the policy tier. The host independently
/// classifies the current UIA control, action, target, and keyboard chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiControlIntent {
    /// Read pixels, accessibility state, or non-sensitive window metadata.
    Observe,
    /// Bring the selected DCC window to the foreground.
    Activate,
    /// Navigate inside the selected DCC without submitting consequential work.
    Navigate,
    /// Make an ordinary reversible edit inside the named DCC/project.
    OrdinaryEdit,
    /// Interact with a login or permission prompt.
    LoginOrPermission,
    /// Upload an artefact.
    Upload,
    /// Move or rename user data.
    MoveOrRename,
    /// Transmit identified sensitive data to an identified destination.
    TransmitSensitiveData,
    /// Delete or overwrite material user data.
    DeleteOrOverwrite,
    /// Install software or execute newly downloaded software.
    InstallOrExecuteDownload,
    /// Perform a financial action.
    FinancialTransaction,
    /// Change an account, permission, or access relationship.
    AccountOrAccessChange,
    /// Communicate or submit content to a third party.
    ExternalCommunication,
    /// Target a terminal or the Windows Run dialog.
    TerminalOrRunDialog,
    /// Target a credential, authentication, or password-manager surface.
    CredentialOrAuthentication,
    /// Change Windows security or privacy settings.
    WindowsSecurityOrPrivacy,
    /// Bypass a safety interstitial or warning.
    SafetyBypass,
    /// Change a password.
    PasswordChange,
    /// Escape the selected process or window scope.
    EscapeScope,
}

impl UiControlIntent {
    /// Return the minimum host policy tier for this intent.
    #[must_use]
    pub const fn policy_tier(self) -> UiControlPolicyTier {
        match self {
            Self::Observe | Self::Activate | Self::Navigate | Self::OrdinaryEdit => {
                UiControlPolicyTier::TaskGrant
            }
            Self::LoginOrPermission
            | Self::Upload
            | Self::MoveOrRename
            | Self::TransmitSensitiveData => UiControlPolicyTier::PreApproval,
            Self::DeleteOrOverwrite
            | Self::InstallOrExecuteDownload
            | Self::FinancialTransaction
            | Self::AccountOrAccessChange
            | Self::ExternalCommunication => UiControlPolicyTier::ActionConfirmation,
            Self::TerminalOrRunDialog
            | Self::CredentialOrAuthentication
            | Self::WindowsSecurityOrPrivacy
            | Self::SafetyBypass
            | Self::PasswordChange
            | Self::EscapeScope => UiControlPolicyTier::HardDeny,
        }
    }
}

/// Host-enforced permission tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiControlPolicyTier {
    /// Allowed by the exact task grant.
    TaskGrant,
    /// Requires pre-approval or trusted action-time confirmation.
    PreApproval,
    /// Always requires trusted action-time confirmation.
    ActionConfirmation,
    /// Cannot be approved through UI Control.
    HardDeny,
}

/// One point in the latest screenshot coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UiControlPoint {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

/// Complete action descriptor. Sensitive fields are used only for execution
/// and are never included in host audit events or confirmation text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiControlAction {
    /// Semantic ui-control action name.
    pub action: String,
    /// UIA control id from the current accessibility state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_id: Option<String>,
    /// Structured or raw execution path.
    pub input_kind: UiControlInputKind,
    /// Client-declared lower bound for policy classification.
    pub intent: UiControlIntent,
    /// Horizontal screenshot coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    /// Vertical screenshot coordinate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Mouse button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
    /// Horizontal wheel delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_x: Option<i32>,
    /// Vertical wheel delta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_y: Option<i32>,
    /// Ordered drag path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<UiControlPoint>,
    /// Literal text. Always redacted from policy prompts and audit logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Keyboard keys or chords.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
    /// Checked state for accessibility toggle operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Action duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Shared-memory descriptor for screenshot pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiControlSharedImage {
    /// OS shared-memory name.
    pub name: String,
    /// Stable buffer id required by `PySharedBuffer.open`.
    pub id: String,
    /// Logical PNG byte length.
    pub length: usize,
    /// MIME type.
    pub mime_type: String,
}

/// Host-owned frame encoding used for one exact-window recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiControlClipFormat {
    /// One numbered JPEG file per sampled frame plus a JSON manifest.
    JpegSequence,
}

/// Hash-bearing host-owned artifact returned after exact-window recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiControlClipArtifact {
    /// Opaque recording id generated by the host.
    pub recording_id: String,
    /// Host-owned recording directory. Clients may copy but never select it.
    pub directory: String,
    /// JSON manifest containing target, timing, and per-frame hashes.
    pub manifest_path: String,
    /// Relative numbered-frame pattern inside `directory`.
    pub frame_pattern: String,
    /// Number of frames written and included in the manifest.
    pub frame_count: u32,
    /// Captured frame width in physical pixels.
    pub width: u32,
    /// Captured frame height in physical pixels.
    pub height: u32,
    /// Requested constant output frame rate.
    pub frames_per_second: u32,
    /// Unix epoch timestamp for the first accepted frame.
    pub started_at_ms: u64,
    /// Unix epoch timestamp after the final accepted frame.
    pub ended_at_ms: u64,
    /// Lowercase SHA-256 of the completed manifest bytes.
    pub manifest_sha256: String,
}

/// Request sent over the local host pipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum UiControlHostRequest {
    /// Negotiate one exact protocol version.
    Hello(UiControlHostHello),
    /// Resolve, visibly announce, and bind exactly one target window.
    OpenSession {
        /// Logical session id chosen by the adapter.
        session_id: String,
        /// Runtime-selected task scope.
        grant: UiControlTaskGrant,
    },
    /// Open an operator-configured system session without requiring a window.
    OpenSystemSession {
        /// Logical system session id chosen by the trusted runtime.
        session_id: String,
        /// Id from the host's operator-owned system grant catalog.
        system_grant_id: String,
    },
    /// Read state for the exact window without requiring a screenshot.
    GetWindowState {
        /// Logical session id.
        session_id: String,
        /// Exact task grant id.
        task_grant_id: String,
        /// Opaque window capability.
        window_capability: String,
    },
    /// Restore, show, or activate only the exact capability-bound window.
    ChangeWindowState {
        /// Logical session id.
        session_id: String,
        /// Exact task grant id.
        task_grant_id: String,
        /// Opaque window capability.
        window_capability: String,
        /// Bounded non-input state transition.
        operation: UiControlWindowOperation,
    },
    /// Capture pixels and accessibility state for the selected window.
    Snapshot {
        /// Logical session id.
        session_id: String,
        /// Exact task grant id.
        task_grant_id: String,
        /// Opaque window capability.
        window_capability: String,
        /// Maximum UIA tree depth.
        max_depth: u32,
        /// Maximum UIA nodes.
        max_nodes: u32,
    },
    /// Record a bounded frame sequence from the exact capability-bound window.
    RecordClip {
        /// Logical session id.
        session_id: String,
        /// Exact task grant id.
        task_grant_id: String,
        /// Opaque window capability.
        window_capability: String,
        /// Requested recording duration in milliseconds.
        duration_ms: u64,
        /// Requested constant output frame rate.
        frames_per_second: u32,
        /// Host-owned frame sequence format.
        format: UiControlClipFormat,
        /// JPEG quality in the inclusive range 70..=100.
        jpeg_quality: u8,
    },
    /// Execute one mutation atomically against the latest observation.
    ExecuteAction {
        /// Logical session id.
        session_id: String,
        /// Exact task grant id.
        task_grant_id: String,
        /// Opaque window capability.
        window_capability: String,
        /// Latest host-owned screenshot observation id.
        observation_id: String,
        /// Latest host-owned accessibility state id.
        accessibility_state_id: String,
        /// Action and sensitive execution fields.
        action: Box<UiControlAction>,
    },
    /// Execute one exact operator-allowed system configuration operation.
    ExecuteSystemOperation {
        /// Logical system session id.
        session_id: String,
        /// Exact operator-owned system grant id.
        system_grant_id: String,
        /// Opaque capability minted for this system session.
        system_capability: String,
        /// Non-sensitive id resolved against the already-open operator grant.
        operation_id: String,
    },
    /// Ask the trusted host UI to clear the global stop latch.
    ResumeSession {
        /// Logical session id.
        session_id: String,
        /// Exact task grant id.
        task_grant_id: String,
        /// Opaque window capability.
        window_capability: String,
    },
    /// Stop and forget one session.
    StopSession {
        /// Logical session id.
        session_id: String,
    },
    /// Stop and forget one system session.
    StopSystemSession {
        /// Logical system session id.
        session_id: String,
    },
}

/// Stable host error and denial codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiControlHostErrorCode {
    /// The client has not completed the handshake.
    HandshakeRequired,
    /// Client and host protocol versions differ.
    ProtocolMismatch,
    /// A required id or native scope value is invalid.
    InvalidRequest,
    /// The requested session already exists.
    SessionAlreadyExists,
    /// The requested session does not exist.
    SessionNotFound,
    /// The task grant id does not match the session.
    GrantMismatch,
    /// The opaque capability does not match the selected window.
    CapabilityMismatch,
    /// The observation or UIA state is missing or stale.
    StaleObservation,
    /// Raw input was not included in the trusted task grant.
    RawInputNotGranted,
    /// A trusted user confirmation was denied or unavailable.
    ApprovalRequired,
    /// A non-bypassable safety boundary denied the action.
    HardDenied,
    /// The selected target is invalid or no longer available.
    InvalidTarget,
    /// The interactive desktop is unavailable.
    DesktopUnavailable,
    /// Window capture failed.
    CaptureFailed,
    /// UI Automation failed.
    BackendUnavailable,
    /// The user stopped all UI Control with the reserved hotkey.
    UserInterrupted,
    /// The requested system operation was not present in the operator grant.
    SystemOperationNotGranted,
    /// An existing object conflicts with the requested idempotent state.
    Conflict,
    /// The operation is not supported by this platform or safety boundary.
    Unsupported,
    /// The target requires privileges UI Control does not acquire or elevate.
    ElevationRequired,
}

/// Response returned over the local host pipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiControlHostResponse {
    /// Successful protocol negotiation.
    Hello {
        /// Exact selected protocol version.
        protocol_version: u32,
        /// Host capabilities available at this version.
        capabilities: Vec<String>,
    },
    /// Session and opaque exact-window capability created.
    SessionOpened {
        /// Logical session id.
        session_id: String,
        /// Opaque capability, invalid after host restart or stop.
        window_capability: String,
        /// Exact window selected by the host.
        target: UiControlTarget,
    },
    /// Independent system session and capability opened from operator config.
    SystemSessionOpened {
        /// Logical system session id.
        session_id: String,
        /// Opaque capability, invalid after host restart or stop.
        system_capability: String,
        /// DCC family associated with the configured grant.
        dcc_type: String,
    },
    /// Current state of the exact capability-bound window.
    WindowState {
        /// Logical session id.
        session_id: String,
        /// Revalidated exact-window state.
        state: UiControlWindowState,
    },
    /// Exact-window state transition completed.
    WindowStateChanged {
        /// Logical session id.
        session_id: String,
        /// Transition performed.
        operation: UiControlWindowOperation,
        /// Revalidated state after the transition.
        state: UiControlWindowState,
    },
    /// Fresh window pixels and accessibility state.
    Snapshot {
        /// Native screenshot observation id.
        observation_id: String,
        /// Host accessibility state id.
        accessibility_state_id: String,
        /// Exact target observed with the frame.
        target: UiControlTarget,
        /// Native observation metadata used for coordinate mapping.
        observation: Value,
        /// Raw UIA root consumed by the adapter's portable contract mapper.
        root: Value,
        /// Runtime id of the focused control.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focus_runtime_id: Option<String>,
        /// UIA nodes collected.
        node_count: u32,
        /// Shared-memory PNG descriptor.
        image: Box<UiControlSharedImage>,
    },
    /// Exact-window frame sequence completed in host-owned storage.
    ClipRecorded {
        /// Exact target revalidated after recording completed.
        target: UiControlTarget,
        /// Hash-bearing frame sequence and manifest descriptor.
        artifact: UiControlClipArtifact,
    },
    /// One action completed or was rejected before input.
    ActionCompleted {
        /// Whether the requested mutation completed.
        success: bool,
        /// The exact capability-bound HWND closed after the action completed.
        /// The host revokes the session and never follows a replacement HWND.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        target_closed: bool,
        /// Host-enforced policy tier.
        policy_tier: UiControlPolicyTier,
        /// Safe result message with sensitive values removed.
        message: String,
        /// Stable error when unsuccessful.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<UiControlHostErrorCode>,
        /// Focus before a semantic UIA action.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before_focus_runtime_id: Option<String>,
        /// Focus after a semantic UIA action.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after_focus_runtime_id: Option<String>,
    },
    /// One typed system configuration ensure completed.
    SystemOperationCompleted {
        /// Static redacted operation kind resolved by the native host.
        operation_type: String,
        /// Whether state was created, updated, or already correct.
        outcome: UiControlEnsureOutcome,
        /// System operations always require trusted action-time confirmation.
        policy_tier: UiControlPolicyTier,
        /// Safe result message with values and paths removed.
        message: String,
    },
    /// Global stop latch was cleared through the trusted host UI.
    SessionResumed {
        /// Logical session id.
        session_id: String,
    },
    /// Session was stopped and all capabilities were invalidated.
    SessionStopped {
        /// Logical session id.
        session_id: String,
        /// Whether native overlay/input cleanup is still completing.
        cleanup_pending: bool,
    },
    /// System session stopped and its capability invalidated.
    SystemSessionStopped {
        /// Logical system session id.
        session_id: String,
    },
    /// Protocol or state error.
    Error {
        /// Stable machine-readable code.
        code: UiControlHostErrorCode,
        /// Safe human-readable message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_protocol_is_v2_only() {
        assert_eq!(UI_CONTROL_HOST_PROTOCOL_VERSION, 2);
        assert!(UI_CONTROL_HOST_CAPABILITIES.contains(&"connection_scoped_sessions"));
    }

    #[test]
    fn intent_tiers_match_the_confirmation_contract() {
        assert_eq!(
            UiControlIntent::OrdinaryEdit.policy_tier(),
            UiControlPolicyTier::TaskGrant
        );
        assert_eq!(
            UiControlIntent::Upload.policy_tier(),
            UiControlPolicyTier::PreApproval
        );
        assert_eq!(
            UiControlIntent::DeleteOrOverwrite.policy_tier(),
            UiControlPolicyTier::ActionConfirmation
        );
        assert_eq!(
            UiControlIntent::EscapeScope.policy_tier(),
            UiControlPolicyTier::HardDeny
        );
    }

    #[test]
    fn wire_protocol_has_no_client_approval_boolean() {
        let value = serde_json::to_value(UiControlHostRequest::Hello(UiControlHostHello {
            protocol_version: UI_CONTROL_HOST_PROTOCOL_VERSION,
            client_name: "maya-adapter".to_owned(),
        }))
        .unwrap();

        assert_eq!(value["method"], "hello");
        let schema = include_str!("host_protocol.rs").to_ascii_lowercase();
        assert!(!schema.contains(&["confirmed", ":"].concat()));
        assert!(!schema.contains(&["approved", ":"].concat()));
    }

    #[test]
    fn boxed_large_fields_preserve_the_flat_json_contract() {
        let request = UiControlHostRequest::ExecuteAction {
            session_id: "session".to_owned(),
            task_grant_id: "grant".to_owned(),
            window_capability: "window:opaque".to_owned(),
            observation_id: "observation".to_owned(),
            accessibility_state_id: "accessibility".to_owned(),
            action: Box::new(UiControlAction {
                action: "click".to_owned(),
                control_id: Some("uia:42.1".to_owned()),
                input_kind: UiControlInputKind::Semantic,
                intent: UiControlIntent::Navigate,
                x: None,
                y: None,
                button: None,
                scroll_x: None,
                scroll_y: None,
                path: Vec::new(),
                text: None,
                keys: Vec::new(),
                checked: None,
                duration_ms: None,
            }),
        };
        let request = serde_json::to_value(request).unwrap();
        assert_eq!(request["params"]["action"]["action"], "click");

        let response = UiControlHostResponse::Snapshot {
            observation_id: "observation".to_owned(),
            accessibility_state_id: "accessibility".to_owned(),
            target: UiControlTarget {
                process_id: 42,
                window_handle: 500,
                window_title: "DCC".to_owned(),
            },
            observation: serde_json::json!({"generation": 1}),
            root: serde_json::json!({"runtime_id": "42.1"}),
            focus_runtime_id: None,
            node_count: 1,
            image: Box::new(UiControlSharedImage {
                name: "shared".to_owned(),
                id: "image".to_owned(),
                length: 3,
                mime_type: "image/png".to_owned(),
            }),
        };
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["type"], "snapshot");
        assert_eq!(response["image"]["name"], "shared");
    }

    #[test]
    fn target_closed_is_an_additive_action_completion_field() {
        let legacy = serde_json::json!({
            "type": "action_completed",
            "success": true,
            "policy_tier": "task_grant",
            "message": "completed",
            "error": null,
            "before_focus_runtime_id": null,
            "after_focus_runtime_id": null
        });
        let decoded: UiControlHostResponse = serde_json::from_value(legacy).unwrap();
        assert!(matches!(
            decoded,
            UiControlHostResponse::ActionCompleted {
                target_closed: false,
                ..
            }
        ));

        let closed = serde_json::to_value(UiControlHostResponse::ActionCompleted {
            success: true,
            target_closed: true,
            policy_tier: UiControlPolicyTier::TaskGrant,
            message: "completed and closed".to_owned(),
            error: None,
            before_focus_runtime_id: None,
            after_focus_runtime_id: None,
        })
        .unwrap();
        assert_eq!(closed["target_closed"], true);
    }

    #[test]
    fn system_grants_are_opened_by_id_and_operations_use_a_separate_method() {
        let open = serde_json::to_value(UiControlHostRequest::OpenSystemSession {
            session_id: "setup".to_owned(),
            system_grant_id: "photoshop-remote".to_owned(),
        })
        .unwrap();
        assert_eq!(open["method"], "open_system_session");
        assert_eq!(open["params"]["system_grant_id"], "photoshop-remote");
        assert!(open["params"].get("operations").is_none());

        let execute = serde_json::to_value(UiControlHostRequest::ExecuteSystemOperation {
            session_id: "setup".to_owned(),
            system_grant_id: "photoshop-remote".to_owned(),
            system_capability: "system:opaque".to_owned(),
            operation_id: "enable-remote-control".to_owned(),
        })
        .unwrap();
        assert_eq!(execute["method"], "execute_system_operation");
        assert_eq!(execute["params"]["operation_id"], "enable-remote-control");
        assert!(execute["params"].get("operation").is_none());
        let mut injected = execute.clone();
        injected["params"]["operation"] = serde_json::json!({
            "type": "ensure_registry_string",
            "value": "must-not-cross-the-host-pipe"
        });
        assert!(serde_json::from_value::<UiControlHostRequest>(injected).is_err());
        assert!(
            serde_json::from_value::<UiControlSystemOperation>(serde_json::json!({
                "type": "ensure_registry_dword",
                "key": "Software\\Vendor\\Plugin",
                "value_name": "RemoteEnabled",
                "value": 1,
                "command": "not part of the protocol",
            }))
            .is_err()
        );
        assert!(UI_CONTROL_HOST_CAPABILITIES.contains(&"typed_system_operations"));
    }
}
