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


class UiActionKind:
    """Stable UI action kind strings."""

    CLICK = "click"
    SET_TEXT = "set_text"
    TOGGLE = "toggle"
    SET_CHECKED = "set_checked"
    SELECT_OPTION = "select_option"
    FOCUS = "focus"


@dataclass
class UiActionRequest:
    """Request to perform one bounded UI action."""

    control_id: str
    action: str
    text: Optional[str] = None
    checked: Optional[bool] = None
    option: Optional[str] = None
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
    TIMEOUT = "timeout"
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


__all__ = [
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
]
