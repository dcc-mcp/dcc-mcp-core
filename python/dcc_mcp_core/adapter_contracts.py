"""Python helpers for adapter runtime observation contracts.

The Rust crates own the canonical wire schemas. These dataclasses give Python
adapters a zero-dependency way to emit the same debug-session and UI automation
JSON shapes without hand-rolling dictionaries in every adapter.
"""

from __future__ import annotations

from dataclasses import asdict
from dataclasses import dataclass
from dataclasses import field
from typing import Any
from typing import Dict
from typing import List
from typing import Optional


def _drop_none(value: Any) -> Any:
    if isinstance(value, dict):
        return {k: _drop_none(v) for k, v in value.items() if v is not None and v != []}
    if isinstance(value, list):
        return [_drop_none(v) for v in value]
    return value


class DebugSessionStatus:
    """Stable debug-session status strings."""

    UNAVAILABLE = "unavailable"
    AVAILABLE = "available"
    LISTENING = "listening"
    CLIENT_CONNECTED = "client_connected"
    ERROR = "error"


@dataclass
class DebugPathMapping:
    """Path mapping hint for attach-based debuggers."""

    local_root: str
    remote_root: str

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class DebugSessionDescriptor:
    """Optional debug attach descriptor published by a DCC adapter."""

    debugger_kind: str
    status: str = DebugSessionStatus.UNAVAILABLE
    host: Optional[str] = None
    port: Optional[int] = None
    runtime: Optional[str] = None
    process_id: Optional[int] = None
    path_mappings: List[DebugPathMapping] = field(default_factory=list)
    log_uri: Optional[str] = None
    setup_instructions: Optional[str] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    @classmethod
    def unavailable(cls, debugger_kind: str, setup_instructions: str) -> "DebugSessionDescriptor":
        """Build an unavailable descriptor with adapter-provided guidance."""
        return cls(
            debugger_kind=debugger_kind,
            status=DebugSessionStatus.UNAVAILABLE,
            setup_instructions=setup_instructions,
        )

    @classmethod
    def listening(cls, debugger_kind: str, host: str, port: int) -> "DebugSessionDescriptor":
        """Build a listening attach descriptor."""
        return cls(
            debugger_kind=debugger_kind,
            status=DebugSessionStatus.LISTENING,
            host=host,
            port=port,
        )

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiBounds:
    """Rectangle in physical pixels or adapter-defined UI coordinates."""

    x: float
    y: float
    width: float
    height: float

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiPoint:
    """Point in snapshot-relative UI coordinates."""

    x: float
    y: float

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiArtifactRef:
    """Small resource/artifact reference included in UI results."""

    uri: str
    mime: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiControlNode:
    """Normalized UI control node."""

    id: str
    role: str
    label: Optional[str] = None
    text: Optional[str] = None
    object_name: Optional[str] = None
    tooltip: Optional[str] = None
    enabled: bool = True
    visible: bool = True
    bounds: Optional[UiBounds] = None
    value: Optional[str] = None
    checked: Optional[bool] = None
    children: List["UiControlNode"] = field(default_factory=list)
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiSnapshot:
    """Bounded UI tree snapshot."""

    root: UiControlNode
    session_id: Optional[str] = None
    focus_id: Optional[str] = None
    truncated: bool = False
    node_count: int = 1
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiFindRequest:
    """Request for locating controls in a bounded UI snapshot/backend."""

    query: Optional[str] = None
    role: Optional[str] = None
    label: Optional[str] = None
    object_name: Optional[str] = None
    limit: Optional[int] = None

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


class UiWaitConditionKind:
    """Stable UI wait condition kind strings."""

    CONTROL_EXISTS = "control_exists"
    CONTROL_MISSING = "control_missing"
    TEXT_EQUALS = "text_equals"
    VALUE_EQUALS = "value_equals"
    CHECKED_EQUALS = "checked_equals"
    ENABLED = "enabled"
    DISABLED = "disabled"
    FOCUSED = "focused"


@dataclass
class UiWaitCondition:
    """Condition that ``ui_control__wait_for`` evaluates inside one tool call."""

    kind: str
    control_id: Optional[str] = None
    query: Optional[str] = None
    role: Optional[str] = None
    label: Optional[str] = None
    text: Optional[str] = None
    value: Optional[str] = None
    checked: Optional[bool] = None
    timeout_ms: int = 5000
    interval_ms: int = 100

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


class UiActionKind:
    """Stable UI action kind strings."""

    CLICK = "click"
    MOVE = "move"
    DOUBLE_CLICK = "double_click"
    SCROLL = "scroll"
    DRAG = "drag"
    RAW_COORDINATE_CLICK = "raw_coordinate_click"
    TYPE = "type"
    KEYPRESS = "keypress"
    GAME_NAVIGATION = "game_navigation"
    SET_TEXT = "set_text"
    TOGGLE = "toggle"
    SET_CHECKED = "set_checked"
    SELECT_OPTION = "select_option"
    FOCUS = "focus"
    KEYBOARD_SHORTCUT = "keyboard_shortcut"
    GET_WINDOW_STATE = "get_window_state"
    RESTORE_WINDOW = "restore_window"
    SHOW_WINDOW = "show_window"
    ACTIVATE_WINDOW = "activate_window"


@dataclass
class UiControlPolicy:
    """Policy controls for scoped ``ui_control`` observation and actions."""

    allow_snapshot: bool = True
    allow_find: bool = True
    allow_mutating_actions: bool = True
    allow_text_entry: bool = True
    allow_keyboard_shortcuts: bool = False
    allow_raw_coordinates: bool = False
    require_scoped_window: bool = True
    allowed_window_titles: List[str] = field(default_factory=list)
    allowed_process_ids: List[int] = field(default_factory=list)
    audit_sensitive_values: bool = False
    scope_denied: bool = False

    def narrowed(self, overrides: Optional[Dict[str, Any]] = None) -> "UiControlPolicy":
        """Apply request overrides as restrictions; never widen this policy ceiling."""
        raw = overrides if isinstance(overrides, dict) else {}

        def allowed(name: str) -> bool:
            return bool(getattr(self, name)) and bool(raw.get(name, True))

        titles = raw.get("allowed_window_titles", self.allowed_window_titles)
        process_ids = raw.get("allowed_process_ids", self.allowed_process_ids)
        requested_titles = [str(item) for item in titles] if isinstance(titles, list) else []
        requested_process_ids = [int(item) for item in process_ids] if isinstance(process_ids, list) else []
        scope_denied = self.scope_denied
        if self.allowed_window_titles:
            requested_titles = requested_titles or list(self.allowed_window_titles)
            requested_titles = [item for item in requested_titles if item in self.allowed_window_titles]
            scope_denied = scope_denied or not requested_titles
        if self.allowed_process_ids:
            requested_process_ids = requested_process_ids or list(self.allowed_process_ids)
            requested_process_ids = [item for item in requested_process_ids if item in self.allowed_process_ids]
            scope_denied = scope_denied or not requested_process_ids

        return UiControlPolicy(
            allow_snapshot=allowed("allow_snapshot"),
            allow_find=allowed("allow_find"),
            allow_mutating_actions=allowed("allow_mutating_actions"),
            allow_text_entry=allowed("allow_text_entry"),
            allow_keyboard_shortcuts=allowed("allow_keyboard_shortcuts"),
            allow_raw_coordinates=allowed("allow_raw_coordinates"),
            require_scoped_window=self.require_scoped_window or bool(raw.get("require_scoped_window", False)),
            allowed_window_titles=requested_titles,
            allowed_process_ids=requested_process_ids,
            audit_sensitive_values=self.audit_sensitive_values and bool(raw.get("audit_sensitive_values", False)),
            scope_denied=scope_denied,
        )

    def allows_action(self, action: str) -> bool:
        """Return whether this policy permits an action kind."""
        if self.scope_denied:
            return False
        if action == UiActionKind.GET_WINDOW_STATE:
            return self.allow_snapshot
        if (
            action
            in (
                UiActionKind.MOVE,
                UiActionKind.DOUBLE_CLICK,
                UiActionKind.SCROLL,
                UiActionKind.DRAG,
                UiActionKind.RAW_COORDINATE_CLICK,
            )
            and not self.allow_raw_coordinates
        ):
            return False
        if (
            action
            in (
                UiActionKind.TYPE,
                UiActionKind.KEYPRESS,
                UiActionKind.GAME_NAVIGATION,
                UiActionKind.KEYBOARD_SHORTCUT,
            )
            and not self.allow_keyboard_shortcuts
        ):
            return False
        if action in (UiActionKind.TYPE, UiActionKind.SET_TEXT) and not self.allow_text_entry:
            return False
        if action in (
            UiActionKind.CLICK,
            UiActionKind.MOVE,
            UiActionKind.DOUBLE_CLICK,
            UiActionKind.SCROLL,
            UiActionKind.DRAG,
            UiActionKind.RAW_COORDINATE_CLICK,
            UiActionKind.TYPE,
            UiActionKind.KEYPRESS,
            UiActionKind.GAME_NAVIGATION,
            UiActionKind.SET_TEXT,
            UiActionKind.TOGGLE,
            UiActionKind.SET_CHECKED,
            UiActionKind.SELECT_OPTION,
            UiActionKind.FOCUS,
            UiActionKind.KEYBOARD_SHORTCUT,
            UiActionKind.RESTORE_WINDOW,
            UiActionKind.SHOW_WINDOW,
            UiActionKind.ACTIVATE_WINDOW,
        ):
            return self.allow_mutating_actions
        return False

    def allows_request(self, request: "UiActionRequest") -> bool:
        """Return whether policy permits an action payload, including its coordinate mode."""
        if self.scope_denied:
            return False
        if request.action == UiActionKind.CLICK and (request.x is not None or request.y is not None):
            return self.allow_mutating_actions and self.allow_raw_coordinates
        return self.allows_action(request.action)

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiActionRequest:
    """Request to perform one bounded UI action."""

    control_id: Optional[str]
    action: str
    text: Optional[str] = None
    checked: Optional[bool] = None
    option: Optional[str] = None
    x: Optional[float] = None
    y: Optional[float] = None
    button: Optional[str] = None
    scroll_x: Optional[int] = None
    scroll_y: Optional[int] = None
    path: List[UiPoint] = field(default_factory=list)
    keys: List[str] = field(default_factory=list)
    snapshot_id: Optional[str] = None
    duration_ms: Optional[int] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


class UiErrorCode:
    """Stable UI action error code strings."""

    STALE_CONTROL = "stale_control"
    STALE_OBSERVATION = "stale_observation"
    NOT_FOUND = "not_found"
    UNSUPPORTED_ACTION = "unsupported_action"
    DENIED = "denied"
    POLICY_DISABLED = "policy_disabled"
    MISSING_WINDOW = "missing_window"
    TIMEOUT = "timeout"
    INVALID_TARGET = "invalid_target"
    USER_INTERRUPTED = "user_interrupted"
    FOCUS_LOST = "focus_lost"
    DESKTOP_UNAVAILABLE = "desktop_unavailable"
    PERMISSION_DENIED = "permission_denied"
    BACKEND_UNAVAILABLE = "backend_unavailable"
    INVALID_ACTION = "invalid_action"
    INPUT_FAILED = "input_failed"
    CAPTURE_FAILED = "capture_failed"
    BACKEND_ERROR = "backend_error"


@dataclass
class UiActionResult:
    """Result of one bounded UI action."""

    success: bool
    control_id: str
    error_code: Optional[str] = None
    message: Optional[str] = None
    before_focus_id: Optional[str] = None
    after_focus_id: Optional[str] = None
    artifacts: List[UiArtifactRef] = field(default_factory=list)
    metadata: Dict[str, Any] = field(default_factory=dict)

    @classmethod
    def stale(cls, control_id: str) -> "UiActionResult":
        """Build a stale-control failure result."""
        return cls(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.STALE_CONTROL,
            message="control is stale; refresh the UI snapshot",
        )

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiWaitResult:
    """Result of evaluating one UI wait condition."""

    success: bool
    condition: UiWaitCondition
    elapsed_ms: float
    attempts: int
    snapshot: Optional[UiSnapshot] = None
    error_code: Optional[str] = None
    message: Optional[str] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiControlAuditRecord:
    """Small audit record for a ``ui_control`` action decision or outcome."""

    action_kind: str
    success: bool
    target_control_id: Optional[str] = None
    target_role: Optional[str] = None
    target_label: Optional[str] = None
    before_focus_id: Optional[str] = None
    after_focus_id: Optional[str] = None
    error_code: Optional[str] = None
    message: Optional[str] = None
    session_id: Optional[str] = None
    redacted_fields: List[str] = field(default_factory=list)
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


__all__ = [
    "DebugPathMapping",
    "DebugSessionDescriptor",
    "DebugSessionStatus",
    "UiActionKind",
    "UiActionRequest",
    "UiActionResult",
    "UiArtifactRef",
    "UiBounds",
    "UiControlAuditRecord",
    "UiControlNode",
    "UiControlPolicy",
    "UiErrorCode",
    "UiFindRequest",
    "UiPoint",
    "UiSnapshot",
    "UiWaitCondition",
    "UiWaitConditionKind",
    "UiWaitResult",
]
