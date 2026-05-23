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
    """Condition that ``app_ui__wait_for`` evaluates inside one tool call."""

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
    RAW_COORDINATE_CLICK = "raw_coordinate_click"
    SET_TEXT = "set_text"
    TOGGLE = "toggle"
    SET_CHECKED = "set_checked"
    SELECT_OPTION = "select_option"
    FOCUS = "focus"
    KEYBOARD_SHORTCUT = "keyboard_shortcut"


@dataclass
class AppUiPolicy:
    """Policy controls for scoped ``app_ui`` observation and actions."""

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

    def allows_action(self, action: str) -> bool:
        """Return whether this policy permits an action kind."""
        if action == UiActionKind.RAW_COORDINATE_CLICK and not self.allow_raw_coordinates:
            return False
        if action == UiActionKind.KEYBOARD_SHORTCUT and not self.allow_keyboard_shortcuts:
            return False
        if action == UiActionKind.SET_TEXT and not self.allow_text_entry:
            return False
        if action in (
            UiActionKind.CLICK,
            UiActionKind.RAW_COORDINATE_CLICK,
            UiActionKind.SET_TEXT,
            UiActionKind.TOGGLE,
            UiActionKind.SET_CHECKED,
            UiActionKind.SELECT_OPTION,
            UiActionKind.FOCUS,
            UiActionKind.KEYBOARD_SHORTCUT,
        ):
            return self.allow_mutating_actions
        return False

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


@dataclass
class UiActionRequest:
    """Request to perform one bounded UI action."""

    control_id: str
    action: str
    text: Optional[str] = None
    checked: Optional[bool] = None
    option: Optional[str] = None
    x: Optional[float] = None
    y: Optional[float] = None
    keys: List[str] = field(default_factory=list)
    metadata: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return the wire dictionary."""
        return _drop_none(asdict(self))


class UiErrorCode:
    """Stable UI action error code strings."""

    STALE_CONTROL = "stale_control"
    NOT_FOUND = "not_found"
    UNSUPPORTED_ACTION = "unsupported_action"
    DENIED = "denied"
    POLICY_DISABLED = "policy_disabled"
    MISSING_WINDOW = "missing_window"
    TIMEOUT = "timeout"
    INVALID_TARGET = "invalid_target"
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
class AppUiAuditRecord:
    """Small audit record for an ``app_ui`` action decision or outcome."""

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
    "AppUiAuditRecord",
    "AppUiPolicy",
    "DebugPathMapping",
    "DebugSessionDescriptor",
    "DebugSessionStatus",
    "UiActionKind",
    "UiActionRequest",
    "UiActionResult",
    "UiArtifactRef",
    "UiBounds",
    "UiControlNode",
    "UiErrorCode",
    "UiFindRequest",
    "UiSnapshot",
    "UiWaitCondition",
    "UiWaitConditionKind",
    "UiWaitResult",
]
