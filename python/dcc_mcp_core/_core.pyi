"""Type stubs for dcc_mcp_core._core (PyO3 native extension).

Auto-generated from Rust source definitions. Provides IDE auto-completion
and static type checking for all public APIs.
"""

from __future__ import annotations

from typing import Any

# ── Metadata ──

__version__: str
__author__: str

# ── Constants ──

APP_NAME: str
APP_AUTHOR: str
DEFAULT_DCC: str
DEFAULT_VERSION: str
DEFAULT_MIME_TYPE: str
DEFAULT_LOG_LEVEL: str
ENV_LOG_LEVEL: str
ENV_SKILL_PATHS: str
SKILL_METADATA_FILE: str
SKILL_SCRIPTS_DIR: str
SKILL_METADATA_DIR: str

# ── Models ──

class ToolResult:
    """Unified result type for all Skill executions.

    JSON Serialization
    ------------------
    ``json.dumps(result)`` will **not** work directly because ``ToolResult``
    is not a subclass of ``dict``.  Use one of these instead::

        import json
        result = success_result("done")

        # Option 1 — preferred, no extra allocation:
        json_str = result.to_json()

        # Option 2 — explicit dict conversion:
        json_str = json.dumps(result.to_dict())

        # Option 3 — via mapping protocol (also works):
        json_str = json.dumps(dict(result))

    """

    success: bool
    message: str
    prompt: str | None
    error: str | None
    context: dict[str, Any]

    def __init__(
        self,
        success: bool = True,
        message: str = "",
        prompt: str | None = None,
        error: str | None = None,
        context: dict[str, Any] | None = None,
    ) -> None: ...
    def with_error(self, error: str) -> ToolResult: ...
    def with_context(self, **kwargs: Any) -> ToolResult: ...
    def to_dict(self) -> dict[str, Any]:
        """Convert to a plain Python dict.

        The returned dict is always JSON-serializable via ``json.dumps(result.to_dict())``.
        """
        ...
    def to_json(self) -> str:
        """Serialize to a JSON string (UTF-8).

        Preferred over ``json.dumps(result.to_dict())`` — avoids an intermediate dict
        allocation and is slightly faster.

        Example::

            result = success_result("sphere created", name="sphere1")
            print(result.to_json())
            # {"success": true, "message": "sphere created", ...}
        """
        ...
    def keys(self) -> list[str]:
        """Return the field names (mapping protocol support).

        Enables ``dict(result)`` and thus ``json.dumps(dict(result))``.
        """
        ...
    def __iter__(self) -> Any:
        """Iterate over ``(key, value)`` pairs (mapping protocol).

        Enables ``dict(result)`` so that ``json.dumps(dict(result))`` works.
        """
        ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class ToolDeclaration:
    """Declaration of a tool provided by a skill, parsed from SKILL.md frontmatter.

    Lightweight declaration for agent-side discovery without loading the skill.
    """

    name: str
    description: str
    input_schema: str
    output_schema: str
    read_only: bool
    destructive: bool
    idempotent: bool
    defer_loading: bool
    source_file: str

    def __init__(
        self,
        name: str,
        description: str = "",
        input_schema: str | None = None,
        output_schema: str | None = None,
        read_only: bool = False,
        destructive: bool = False,
        idempotent: bool = False,
        defer_loading: bool = False,
        source_file: str = "",
    ) -> None: ...
    def __repr__(self) -> str: ...

class SkillGroup:
    """Declaration of a tool group within a skill (progressive exposure).

    Groups bundle multiple tools behind a single stub entry in ``tools/list``
    so agents only pay the context cost for the tools they actually use.
    """

    name: str
    description: str
    tools: list[str]
    default_active: bool

    def __init__(
        self,
        name: str,
        description: str = "",
        tools: list[str] | None = None,
        default_active: bool = False,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class SkillMetadata:
    """Metadata parsed from a SKILL.md frontmatter.

    Supports agentskills.io / Anthropic Skills, ClawHub / OpenClaw, and
    dcc-mcp-core extension fields simultaneously.
    """

    name: str
    description: str
    tools: list[ToolDeclaration]
    dcc: str
    tags: list[str]
    scripts: list[str]
    skill_path: str
    version: str
    depends: list[str]
    metadata_files: list[str]
    license: str
    compatibility: str
    allowed_tools: list[str]
    groups: list[SkillGroup]

    def __init__(
        self,
        name: str,
        description: str = "",
        tools: list[ToolDeclaration] | None = None,
        dcc: str = "python",
        tags: list[str] | None = None,
        scripts: list[str] | None = None,
        skill_path: str = "",
        version: str = "1.0.0",
        depends: list[str] | None = None,
        metadata_files: list[str] | None = None,
        license: str = "",
        compatibility: str = "",
        allowed_tools: list[str] | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

# ── Skills ──

class ToolRegistry:
    """Thread-safe registry for DCC skills."""

    def __init__(self) -> None: ...
    def register(
        self,
        name: str,
        description: str = "",
        category: str = "",
        tags: list[str] | None = None,
        dcc: str = "python",
        version: str = "1.0.0",
        input_schema: str = "",
        output_schema: str = "",
        source_file: str | None = None,
    ) -> None: ...
    def get_action(self, name: str, dcc_name: str | None = None) -> dict[str, Any] | None: ...
    def list_actions(self, dcc_name: str | None = None) -> list[dict[str, Any]]: ...
    def list_actions_for_dcc(self, dcc_name: str) -> list[str]: ...
    def get_all_dccs(self) -> list[str]: ...
    def search_actions(
        self,
        category: str | None = None,
        tags: list[str] = [],
        dcc_name: str | None = None,
    ) -> list[dict[str, Any]]:
        """Search skills by category, tags, and/or DCC name.

        All filters are AND-ed:

        - ``category``: exact match (``None`` / empty = no filter)
        - ``tags``: skill must contain **all** requested tags (empty = no filter)
        - ``dcc_name``: limit to a specific DCC (``None`` = all DCCs)

        Example::

            reg.register(name="create_sphere", category="geometry",
                         tags=["create", "mesh"], dcc="maya")
            results = reg.search_actions(category="geometry", tags=["create"])
        """
        ...
    def get_categories(self, dcc_name: str | None = None) -> list[str]:
        """Return all unique skill categories in sorted order.

        Optionally scoped to a specific DCC.
        """
        ...
    def get_tags(self, dcc_name: str | None = None) -> list[str]:
        """Return all unique skill tags in sorted order.

        Optionally scoped to a specific DCC.
        """
        ...
    def count_actions(
        self,
        category: str | None = None,
        tags: list[str] = [],
        dcc_name: str | None = None,
    ) -> int:
        """Count skills matching the given search criteria.

        Convenience wrapper around :meth:`search_actions` that returns the count
        rather than the full list of matching skills.

        Example::

            reg.register(name="create_sphere", category="geometry", dcc="maya")
            assert reg.count_actions(category="geometry") == 1
            assert reg.count_actions(category="export") == 0
        """
        ...
    def reset(self) -> None: ...
    def register_batch(self, actions: list[dict[str, Any]]) -> None:
        """Register multiple skills at once from a list of dicts.

        Each dict may contain the same keys as :meth:`register`.
        Entries without a ``"name"`` key (or empty name) are silently skipped.

        Example::

            reg.register_batch([
                {"name": "create_sphere", "category": "geometry", "dcc": "maya"},
                {"name": "delete_object", "category": "edit",     "dcc": "maya"},
            ])
        """
        ...
    def unregister(self, name: str, dcc_name: str | None = None) -> bool:
        """Remove a skill from the registry.

        If ``dcc_name`` is ``None`` (default), the skill is removed from the
        global registry and every per-DCC map.

        If ``dcc_name`` is provided, only that DCC's entry is removed; the
        global entry is cleared only when no other DCC still references it.

        Returns ``True`` if the skill was found and removed, ``False`` otherwise.

        Example::

            reg.register(name="create_sphere", dcc="maya")
            assert reg.unregister("create_sphere") is True
            assert reg.unregister("create_sphere") is False  # already gone
        """
        ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...

class EventBus:
    """Publish/subscribe event bus for DCC events."""

    def __init__(self) -> None: ...
    def subscribe(self, event: str, callback: Any) -> int: ...
    def unsubscribe(self, event: str, subscription_id: int) -> bool: ...
    def publish(self, event: str, **kwargs: Any) -> None: ...
    def __repr__(self) -> str: ...

class ToolValidator:
    """Validates JSON-encoded skill parameters against a JSON Schema.

    Example::

        import json
        from dcc_mcp_core import ToolRegistry, ToolValidator

        schema = json.dumps({
            "type": "object",
            "required": ["radius"],
            "properties": {"radius": {"type": "number", "minimum": 0.0}}
        })
        v = ToolValidator.from_schema_json(schema)
        ok, errors = v.validate('{"radius": 1.0}')
        assert ok
        ok, errors = v.validate("{}")
        assert not ok

    """

    @staticmethod
    def from_schema_json(schema_json: str) -> ToolValidator:
        """Create a validator from a JSON Schema string.

        Raises:
            ValueError: If ``schema_json`` is not valid JSON.

        """
        ...

    @staticmethod
    def from_action_registry(
        registry: ToolRegistry,
        action_name: str,
        dcc_name: str | None = None,
    ) -> ToolValidator:
        """Create a validator from a skill in an :class:`ToolRegistry`.

        Raises:
            KeyError: If the skill is not found in the registry.

        """
        ...

    def validate(self, params_json: str) -> tuple[bool, list[str]]:
        """Validate JSON-encoded parameters.

        Returns:
            ``(True, [])`` on success; ``(False, [error_msg, ...])`` on failure.

        Raises:
            ValueError: If ``params_json`` is not valid JSON.

        """
        ...

    def __repr__(self) -> str: ...

class ToolDispatcher:
    """Routes skill calls to registered Python callables with automatic validation.

    Example::

        import json
        from dcc_mcp_core import ToolRegistry, ToolDispatcher

        reg = ToolRegistry()
        reg.register(
            "create_sphere",
            input_schema=json.dumps({
                "type": "object",
                "required": ["radius"],
                "properties": {"radius": {"type": "number", "minimum": 0.0}},
            }),
        )
        dispatcher = ToolDispatcher(reg)

        def create_sphere(params):
            return {"created": True, "radius": params["radius"]}

        dispatcher.register_handler("create_sphere", create_sphere)
        result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
        assert result["output"]["created"] is True

    """

    def __init__(self, registry: ToolRegistry) -> None: ...
    def register_handler(self, action_name: str, handler: Any) -> None:
        """Register a Python callable ``(params: dict) -> Any`` for the skill ``action_name``.

        Raises:
            TypeError: If ``handler`` is not callable.

        """
        ...

    def remove_handler(self, action_name: str) -> bool:
        """Remove the handler for ``action_name``.

        Returns ``True`` if a handler existed and was removed.
        """
        ...

    def has_handler(self, action_name: str) -> bool:
        """Return ``True`` if a handler is registered for ``action_name``."""
        ...

    def handler_count(self) -> int:
        """Return the number of registered handlers."""
        ...

    def handler_names(self) -> list[str]:
        """Alphabetically sorted list of registered handler names."""
        ...

    @property
    def skip_empty_schema_validation(self) -> bool:
        """Whether to skip validation when the skill schema is empty (``{}``)."""
        ...

    @skip_empty_schema_validation.setter
    def skip_empty_schema_validation(self, value: bool) -> None: ...
    def dispatch(
        self,
        action_name: str,
        params_json: str = "null",
    ) -> dict[str, Any]:
        """Dispatch a skill call.

        Validates ``params_json`` against the skill schema, calls the registered
        Python handler, and returns a result dict.

        Returns:
            A dict with keys:

            - ``"action"`` (str): Skill name.
            - ``"output"`` (Any): Handler return value.
            - ``"validation_skipped"`` (bool): Whether schema validation was skipped.

        Raises:
            KeyError:     No handler registered for ``action_name``.
            ValueError:   Invalid JSON or validation failure.
            RuntimeError: Handler raised an exception.

        """
        ...

    def __repr__(self) -> str: ...

# ── Skill Version Management ──

class SemVer:
    """A semantic version (major.minor.patch).

    Example::

        from dcc_mcp_core import SemVer
        v = SemVer(1, 2, 3)
        assert str(v) == "1.2.3"
        assert SemVer.parse("2.0.0") > v

    """

    major: int
    minor: int
    patch: int

    def __init__(self, major: int, minor: int, patch: int) -> None: ...
    @staticmethod
    def parse(s: str) -> SemVer:
        """Parse a semver string such as ``"1.2.3"``, ``"v2.0"``, or ``"1.0.0-alpha"``.

        Raises:
            ValueError: If the string cannot be parsed.

        """
        ...

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __lt__(self, other: SemVer) -> bool: ...
    def __le__(self, other: SemVer) -> bool: ...
    def __gt__(self, other: SemVer) -> bool: ...
    def __ge__(self, other: SemVer) -> bool: ...

class VersionConstraint:
    """A version constraint for matching against registered action versions.

    Supported syntax:

    - ``"*"``         — any version
    - ``"=1.2.3"``   — exact match
    - ``">=1.2.0"``  — at least 1.2.0
    - ``">1.2.0"``   — strictly greater than
    - ``"<=2.0.0"``  — at most 2.0.0
    - ``"<2.0.0"``   — strictly less than
    - ``"^1.2.3"``   — same major, at least minor.patch (caret range)
    - ``"~1.2.3"``   — same major.minor, at least patch (tilde range)

    Example::

        from dcc_mcp_core import VersionConstraint, SemVer
        c = VersionConstraint.parse("^1.0.0")
        assert c.matches(SemVer(1, 5, 0))
        assert not c.matches(SemVer(2, 0, 0))

    """

    @staticmethod
    def parse(s: str) -> VersionConstraint:
        """Parse a constraint string.

        Raises:
            ValueError: If the string uses an unrecognised operator.

        """
        ...

    def matches(self, version: SemVer) -> bool:
        """Return ``True`` if ``version`` satisfies this constraint."""
        ...

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

class VersionedRegistry:
    """Multi-version skill registry.

    Allows multiple versions of the same ``(skill_name, dcc_name)`` pair to coexist.
    Use :meth:`router` to obtain a :class:`CompatibilityRouter` that resolves the
    best-matching version given a client constraint.

    Example::

        from dcc_mcp_core import VersionedRegistry, VersionConstraint

        vr = VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "maya", "1.5.0")
        vr.register_versioned("create_sphere", "maya", "2.0.0")

        result = vr.resolve("create_sphere", "maya", "^1.0.0")
        assert result is not None
        assert result["version"] == "1.5.0"

    """

    def __init__(self) -> None: ...
    def register_versioned(
        self,
        name: str,
        dcc: str,
        version: str,
        description: str = "",
        category: str = "",
        tags: list[str] | None = None,
    ) -> None:
        """Register a skill version.

        If the same ``(name, dcc, version)`` triple already exists it is overwritten.

        """
        ...

    def versions(self, name: str, dcc: str) -> list[str]:
        """Return all registered versions for ``(name, dcc)``, sorted ascending."""
        ...

    def latest_version(self, name: str, dcc: str) -> str | None:
        """Return the highest registered version string, or ``None`` if not registered."""
        ...

    def resolve(
        self,
        name: str,
        dcc: str,
        constraint: str,
    ) -> dict[str, Any] | None:
        """Resolve the best-matching version given a constraint string.

        Returns the skill metadata dict, or ``None`` if no version satisfies the
        constraint.

        """
        ...

    def resolve_all(
        self,
        name: str,
        dcc: str,
        constraint: str,
    ) -> list[dict[str, Any]]:
        """Return all skill metadata dicts that satisfy ``constraint``, sorted ascending."""
        ...

    def total_entries(self) -> int:
        """Return the total number of registered versioned entries."""
        ...

    def remove(self, name: str, dcc: str, constraint: str) -> int:
        """Remove all versions of ``(name, dcc)`` that satisfy the constraint string.

        Returns the number of versions removed.

        Raises:
            ValueError: If the constraint string is invalid.

        """
        ...

    def keys(self) -> list[tuple[str, str]]:
        """Return all registered ``(name, dcc)`` keys."""
        ...

    def __repr__(self) -> str: ...

# ── DCC Adapter Types ──

class ScriptLanguage:
    """Enum for DCC script languages."""

    PYTHON: ScriptLanguage
    MEL: ScriptLanguage
    MAXSCRIPT: ScriptLanguage
    HSCRIPT: ScriptLanguage
    VEX: ScriptLanguage
    LUA: ScriptLanguage
    CSHARP: ScriptLanguage
    BLUEPRINT: ScriptLanguage

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class DccErrorCode:
    """Enum for DCC error codes."""

    CONNECTION_FAILED: DccErrorCode
    TIMEOUT: DccErrorCode
    SCRIPT_ERROR: DccErrorCode
    NOT_RESPONDING: DccErrorCode
    UNSUPPORTED: DccErrorCode
    PERMISSION_DENIED: DccErrorCode
    INVALID_INPUT: DccErrorCode
    SCENE_ERROR: DccErrorCode
    INTERNAL: DccErrorCode

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class DccInfo:
    """Information about a DCC application instance."""

    dcc_type: str
    version: str
    python_version: str | None
    platform: str
    pid: int
    metadata: dict[str, str]

    def __init__(
        self,
        dcc_type: str,
        version: str,
        platform: str,
        pid: int,
        python_version: str | None = None,
        metadata: dict[str, str] | None = None,
    ) -> None: ...
    def to_dict(self) -> dict[str, Any]: ...
    def __repr__(self) -> str: ...

class ScriptResult:
    """Result of executing a script in a DCC application."""

    success: bool
    output: str | None
    error: str | None
    execution_time_ms: int
    context: dict[str, str]

    def __init__(
        self,
        success: bool,
        execution_time_ms: int,
        output: str | None = None,
        error: str | None = None,
        context: dict[str, str] | None = None,
    ) -> None: ...
    def to_dict(self) -> dict[str, Any]: ...
    def __repr__(self) -> str: ...

class SceneStatistics:
    """Basic scene statistics."""

    object_count: int
    vertex_count: int
    polygon_count: int
    material_count: int
    texture_count: int
    light_count: int
    camera_count: int

    def __init__(
        self,
        object_count: int = 0,
        vertex_count: int = 0,
        polygon_count: int = 0,
        material_count: int = 0,
        texture_count: int = 0,
        light_count: int = 0,
        camera_count: int = 0,
    ) -> None: ...
    def __repr__(self) -> str: ...

class SceneInfo:
    """Information about the currently open scene in a DCC application."""

    file_path: str
    name: str
    modified: bool
    format: str
    frame_range: tuple[float, float] | None
    current_frame: float | None
    fps: float | None
    up_axis: str | None
    units: str | None
    statistics: SceneStatistics
    metadata: dict[str, str]

    def __init__(
        self,
        file_path: str = "",
        name: str = "untitled",
        modified: bool = False,
        format: str = "",
        frame_range: tuple[float, float] | None = None,
        current_frame: float | None = None,
        fps: float | None = None,
        up_axis: str | None = None,
        units: str | None = None,
        statistics: SceneStatistics | None = None,
        metadata: dict[str, str] | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class DccCapabilities:
    """Capabilities that a DCC adapter supports.

    The four cross-DCC protocol flags (``scene_manager``, ``transform``,
    ``render_capture``, ``hierarchy``) indicate whether the adapter implements
    the corresponding universal protocol trait.  Check these instead of calling
    ``as_*()`` when you only need a quick feature check.

    Bridge mode
    -----------
    DCCs without an embedded Python interpreter (ZBrush, Photoshop) use a
    *bridge* process.  Set ``has_embedded_python=False``, ``bridge_kind``, and
    ``bridge_endpoint`` to describe the communication channel:

    .. code-block:: python

        # ZBrush HTTP bridge
        caps = DccCapabilities(
            has_embedded_python=False,
            bridge_kind="http",
            bridge_endpoint="http://localhost:8765",
        )

        # Photoshop UXP WebSocket bridge
        caps = DccCapabilities(
            has_embedded_python=False,
            bridge_kind="websocket",
            bridge_endpoint="ws://localhost:3000",
        )
    """

    script_languages: list[ScriptLanguage]
    scene_info: bool
    snapshot: bool
    undo_redo: bool
    progress_reporting: bool
    file_operations: bool
    selection: bool
    scene_manager: bool
    """Whether the adapter implements DccSceneManager (scene/file management)."""
    transform: bool
    """Whether the adapter implements DccTransform (object TRS transforms)."""
    render_capture: bool
    """Whether the adapter implements DccRenderCapture (viewport capture + render)."""
    hierarchy: bool
    """Whether the adapter implements DccHierarchy (parent/child hierarchy)."""
    has_embedded_python: bool
    """Whether the DCC has an embedded Python interpreter (False for ZBrush, Photoshop)."""
    bridge_kind: str | None
    """Bridge communication kind: ``"http"``, ``"websocket"``, ``"named_pipe"``, or ``None``."""
    bridge_endpoint: str | None
    """Bridge endpoint address, e.g. ``"http://localhost:8765"`` or ``"ws://localhost:3000"``."""
    extensions: dict[str, bool]

    def __init__(
        self,
        script_languages: list[ScriptLanguage] | None = None,
        scene_info: bool = False,
        snapshot: bool = False,
        undo_redo: bool = False,
        progress_reporting: bool = False,
        file_operations: bool = False,
        selection: bool = False,
        scene_manager: bool = False,
        transform: bool = False,
        render_capture: bool = False,
        hierarchy: bool = False,
        has_embedded_python: bool = True,
        bridge_kind: str | None = None,
        bridge_endpoint: str | None = None,
        extensions: dict[str, bool] | None = None,
    ) -> None: ...
    def uses_bridge(self) -> bool:
        """Return ``True`` if this DCC uses a bridge connection (bridge_kind is set)."""
        ...
    @staticmethod
    def http_bridge(endpoint: str) -> DccCapabilities:
        """Create a DccCapabilities for an HTTP bridge-based DCC.

        Args:
            endpoint: HTTP endpoint URL (e.g. ``"http://localhost:8765"``).

        Example::

            caps = DccCapabilities.http_bridge("http://localhost:8765")
            assert caps.bridge_kind == "http"
            assert caps.uses_bridge()

        """
        ...
    @staticmethod
    def websocket_bridge(endpoint: str) -> DccCapabilities:
        """Create a DccCapabilities for a WebSocket bridge-based DCC.

        Args:
            endpoint: WebSocket endpoint URL (e.g. ``"ws://localhost:9001"``).

        Example::

            caps = DccCapabilities.websocket_bridge("ws://localhost:9001")
            assert caps.bridge_kind == "websocket"
            assert caps.uses_bridge()

        """
        ...
    def __repr__(self) -> str: ...

class DccError:
    """Error type for DCC adapter operations."""

    code: DccErrorCode
    message: str
    details: str | None
    recoverable: bool

    def __init__(
        self,
        code: DccErrorCode,
        message: str,
        details: str | None = None,
        recoverable: bool = False,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

class CaptureResult:
    """Captured screenshot/viewport image data."""

    data: bytes
    width: int
    height: int
    format: str
    viewport: str | None

    def __init__(
        self,
        data: bytes,
        width: int,
        height: int,
        format: str,
        viewport: str | None = None,
    ) -> None: ...
    def data_size(self) -> int: ...
    def __repr__(self) -> str: ...

# ── Cross-DCC Protocol Data Models ──

class ObjectTransform:
    """3D object transform (TRS) in right-hand Y-up world space.

    - ``translate``: [x, y, z] in centimeters
    - ``rotate``: Euler XYZ in degrees [rx, ry, rz]
    - ``scale``: [sx, sy, sz]

    All DCC adapters convert from their native coordinate system to this convention.

    Example::

        from dcc_mcp_core import ObjectTransform

        t = ObjectTransform(translate=[0.0, 10.0, 0.0], rotate=[0.0, 45.0, 0.0])
        print(t.translate)  # [0.0, 10.0, 0.0]
        d = t.to_dict()
    """

    translate: list[float]  # [x, y, z] in cm
    rotate: list[float]  # Euler XYZ in degrees
    scale: list[float]  # [sx, sy, sz]

    def __init__(
        self,
        translate: list[float] | None = None,
        rotate: list[float] | None = None,
        scale: list[float] | None = None,
    ) -> None: ...
    @staticmethod
    def identity() -> ObjectTransform: ...
    def to_dict(self) -> dict[str, list[float]]: ...
    def __repr__(self) -> str: ...

class BoundingBox:
    """Axis-aligned bounding box in world space (centimeters).

    Example::

        from dcc_mcp_core import BoundingBox

        bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
        print(bb.center())  # [0.0, 1.0, 0.0]
        print(bb.size())    # [2.0, 2.0, 2.0]
    """

    min: list[float]  # [x, y, z]
    max: list[float]  # [x, y, z]

    def __init__(
        self,
        min: list[float] | None = None,
        max: list[float] | None = None,
    ) -> None: ...
    def center(self) -> list[float]: ...
    def size(self) -> list[float]: ...
    def to_dict(self) -> dict[str, list[float]]: ...
    def __repr__(self) -> str: ...

class SceneObject:
    """Lightweight descriptor of a scene object/layer/node.

    Applies to all DCC tools:
    - Maya/Blender/3dsMax: mesh, light, camera, transform nodes
    - Unreal Engine: actors, components
    - Unity: GameObjects
    - Photoshop: layers and layer groups
    - Figma: frames, groups, components

    Example::

        from dcc_mcp_core import SceneObject

        obj = SceneObject(name="pCube1", long_name="|group1|pCube1", object_type="mesh")
        print(obj.visible)   # True
    """

    name: str
    long_name: str
    object_type: str
    parent: str | None
    visible: bool
    metadata: dict[str, str]

    def __init__(
        self,
        name: str,
        long_name: str | None = None,
        object_type: str = "transform",
        parent: str | None = None,
        visible: bool = True,
        metadata: dict[str, str] | None = None,
    ) -> None: ...
    def to_dict(self) -> dict[str, Any]: ...
    def __repr__(self) -> str: ...

class FrameRange:
    """Animation frame range and timing.

    Example::

        from dcc_mcp_core import FrameRange

        fr = FrameRange(start=1.0, end=240.0, fps=24.0, current=1.0)
        duration_seconds = (fr.end - fr.start + 1) / fr.fps
    """

    start: float
    end: float
    fps: float
    current: float

    def __init__(
        self,
        start: float = 1.0,
        end: float = 100.0,
        fps: float = 24.0,
        current: float = 1.0,
    ) -> None: ...
    def to_dict(self) -> dict[str, float]: ...
    def __repr__(self) -> str: ...

class RenderOutput:
    """Metadata for a completed render operation.

    Example::

        from dcc_mcp_core import RenderOutput

        out = RenderOutput(file_path="/renders/frame001.png", width=1920,
                           height=1080, format="png", render_time_ms=5000)
        print(f"Rendered in {out.render_time_ms / 1000:.1f}s")
    """

    file_path: str
    width: int
    height: int
    format: str
    render_time_ms: int

    def __init__(
        self,
        file_path: str,
        width: int,
        height: int,
        format: str,
        render_time_ms: int = 0,
    ) -> None: ...
    def to_dict(self) -> dict[str, Any]: ...
    def __repr__(self) -> str: ...

class SceneNode:
    """Scene hierarchy node with recursive children.

    Represents a node in the DCC scene tree — Maya DAG node, Blender collection item,
    Unreal actor, Unity GameObject, Photoshop layer group, or Figma frame.

    Example::

        from dcc_mcp_core import SceneNode, SceneObject

        leaf = SceneNode(
            object=SceneObject(name="pSphere1", object_type="mesh"),
        )
        root = SceneNode(
            object=SceneObject(name="group1", object_type="transform"),
            children=[leaf],
        )
        print(len(root.children))    # 1
        print(root.children[0].object.name)  # "pSphere1"
    """

    object: SceneObject
    children: list[SceneNode]

    def __init__(
        self,
        object: SceneObject,
        children: list[SceneNode] | None = None,
    ) -> None: ...
    def to_dict(self) -> dict[str, Any]: ...
    def __repr__(self) -> str: ...

# ── Bridge Registry ──

class BridgeContext:
    """Information about a bridge connection.

    Returned by :func:`get_bridge_context` and :class:`BridgeRegistry` queries.

    Example::

        from dcc_mcp_core import register_bridge, get_bridge_context

        register_bridge("photoshop", "ws://localhost:9001")
        ctx = get_bridge_context("photoshop")
        if ctx:
            print(ctx.dcc_type, ctx.bridge_url, ctx.connected)
    """

    dcc_type: str
    """DCC type identifier (e.g. ``"photoshop"``, ``"zbrush"``)."""
    bridge_url: str
    """Bridge endpoint URL (e.g. ``"ws://localhost:9001"``)."""
    connected: bool
    """Whether the bridge is currently connected."""

    def __repr__(self) -> str: ...

class BridgeRegistry:
    """Thread-safe registry for bridge connections available in gateway mode.

    Bridge plugins register their connection info, and skill scripts query
    it to discover available bridges.

    Example::

        from dcc_mcp_core import BridgeRegistry

        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        registry.register("zbrush", "http://localhost:8765")

        ctx = registry.get("photoshop")
        if ctx:
            print(ctx.bridge_url, ctx.connected)

        for ctx in registry.list_all():
            print(ctx.dcc_type, ctx.connected)

        registry.set_disconnected("photoshop")
        registry.unregister("zbrush")
    """

    def __init__(self) -> None: ...
    def register(self, dcc_type: str, url: str) -> None:
        """Register or update a bridge connection.

        Args:
            dcc_type: DCC type identifier (e.g. ``"photoshop"``).
            url: Bridge endpoint URL (e.g. ``"ws://localhost:9001"``).

        Raises:
            ValueError: If ``dcc_type`` or ``url`` is empty.

        """
        ...
    def get(self, dcc_type: str) -> BridgeContext | None:
        """Get bridge context for a specific DCC type.

        Returns ``None`` if no bridge is registered for the given DCC type.
        """
        ...
    def get_url(self, dcc_type: str) -> str | None:
        """Get bridge URL for a specific DCC type (convenience method).

        Returns ``None`` if no bridge is registered.
        """
        ...
    def list_all(self) -> list[BridgeContext]:
        """List all registered bridges."""
        ...
    def set_disconnected(self, dcc_type: str) -> None:
        """Mark a bridge as disconnected without removing it.

        Raises:
            ValueError: If the bridge is not found.

        """
        ...
    def unregister(self, dcc_type: str) -> None:
        """Remove a bridge from the registry.

        Raises:
            ValueError: If the bridge is not found.

        """
        ...
    def clear(self) -> None:
        """Clear all registered bridges."""
        ...
    def contains(self, dcc_type: str) -> bool:
        """Check if a bridge is registered for the given DCC type."""
        ...
    def __len__(self) -> int: ...
    def is_empty(self) -> bool:
        """Check if the registry is empty."""
        ...
    def __repr__(self) -> str: ...

# ── MCP Protocol Types ──

class ToolAnnotations:
    """Annotations for MCP Tool behavior hints."""

    title: str | None
    read_only_hint: bool | None
    destructive_hint: bool | None
    idempotent_hint: bool | None
    open_world_hint: bool | None
    deferred_hint: bool | None

    def __init__(
        self,
        title: str | None = None,
        read_only_hint: bool | None = None,
        destructive_hint: bool | None = None,
        idempotent_hint: bool | None = None,
        open_world_hint: bool | None = None,
        deferred_hint: bool | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class ToolDefinition:
    """MCP Tool definition schema."""

    name: str
    description: str
    input_schema: str
    output_schema: str | None
    annotations: ToolAnnotations | None

    def __init__(
        self,
        name: str,
        description: str,
        input_schema: str,
        output_schema: str | None = None,
        annotations: ToolAnnotations | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class ResourceAnnotations:
    """Annotations for MCP Resource behavior hints."""

    audience: list[str]
    priority: float | None

    def __init__(
        self,
        audience: list[str] | None = None,
        priority: float | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ResourceDefinition:
    """MCP Resource definition."""

    uri: str
    name: str
    description: str
    mime_type: str
    annotations: ResourceAnnotations | None

    def __init__(
        self,
        uri: str,
        name: str,
        description: str,
        mime_type: str = "text/plain",
        annotations: ResourceAnnotations | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ResourceTemplateDefinition:
    """MCP Resource Template definition."""

    uri_template: str
    name: str
    description: str
    mime_type: str
    annotations: ResourceAnnotations | None

    def __init__(
        self,
        uri_template: str,
        name: str,
        description: str,
        mime_type: str = "text/plain",
        annotations: ResourceAnnotations | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class PromptArgument:
    """MCP Prompt argument."""

    name: str
    description: str
    required: bool

    def __init__(
        self,
        name: str,
        description: str,
        required: bool = False,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class PromptDefinition:
    """MCP Prompt definition."""

    name: str
    description: str
    arguments: list[PromptArgument]

    def __init__(
        self,
        name: str,
        description: str,
        arguments: list[PromptArgument] | None = None,
    ) -> None: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

# ── Skills ──

class SkillScanner:
    """Scanner for discovering Skill packages in directories."""

    discovered_skills: list[str]

    def __init__(self) -> None: ...
    def scan(
        self,
        extra_paths: list[str] | None = None,
        dcc_name: str | None = None,
        force_refresh: bool = False,
    ) -> list[str]: ...
    def clear_cache(self) -> None: ...
    def __repr__(self) -> str: ...

class SkillSummary:
    """Lightweight summary of a skill for search/list results."""

    name: str
    description: str
    version: str
    dcc: str
    tags: list[str]
    tool_count: int
    tool_names: list[str]
    loaded: bool

    def __repr__(self) -> str: ...

class SkillCatalog:
    """Manages discovered skills and their progressive loading.

    Thread-safe: all state is stored in DashMap / DashSet.

    When a dispatcher is attached (via ``with_dispatcher``), loading a skill
    also registers a subprocess-based handler for each action — enabling the
    Skills-First workflow where agents never need to register handlers manually.

    Args:
        scanner: A :class:`SkillScanner` instance to use for discovery.

    Example:
        >>> registry = ToolRegistry()
        >>> catalog = SkillCatalog(registry)
        >>> catalog.discover(dcc_name="maya")
        >>> skills = catalog.list_skills()

    """

    def __init__(self, registry: ToolRegistry) -> None: ...
    def discover(
        self,
        extra_paths: list[str] | None = None,
        dcc_name: str | None = None,
    ) -> int:
        """Scan for skills and populate the catalog.

        Args:
            extra_paths: Additional directories to scan beyond the scanner's
                         configured paths.
            dcc_name:    If given, filter discovery to skills for this DCC.

        """
        ...
    def find_skills(
        self,
        query: str | None = None,
        tags: list[str] | None = None,
        dcc: str | None = None,
    ) -> list[SkillSummary]:
        """Search the catalog by name, tags, or DCC.

        Args:
            query: Case-insensitive substring match on skill name/description.
            tags:  Return only skills that have ALL of the given tags.
            dcc:   Return only skills targeting this DCC.

        Returns:
            List of :class:`SkillSummary` matching all supplied filters.

        """
        ...
    def get_skill_info(self, skill_name: str) -> SkillMetadata | None:
        """Return the full :class:`SkillMetadata` for *skill_name*, or ``None``."""
        ...
    def is_loaded(self, skill_name: str) -> bool:
        """Return ``True`` if *skill_name* has been loaded."""
        ...
    def list_skills(self, status: str | None = None) -> list[SkillSummary]:
        """List all skills in the catalog.

        Args:
            status: Optional filter — ``"loaded"`` or ``"unloaded"``.

        Returns:
            List of :class:`SkillSummary`.

        """
        ...
    def load_skill(self, skill_name: str) -> list[str]:
        """Load a skill by name.

        Returns:
            ``True`` on success, ``False`` if already loaded or not found.

        """
        ...
    def loaded_count(self) -> int:
        """Return the number of currently loaded skills."""
        ...
    def unload_skill(self, skill_name: str) -> int:
        """Unload a skill by name.

        Returns:
            ``True`` on success, ``False`` if not loaded.

        """
        ...
    def __repr__(self) -> str: ...

class SkillWatcher:
    """Hot-reload watcher for skill directories.

    Monitors filesystem events using platform-native APIs (inotify on Linux,
    FSEvents on macOS, ReadDirectoryChangesW on Windows) and automatically
    re-loads skill metadata when SKILL.md files or companion scripts change.

    Args:
        debounce_ms: Milliseconds to wait before reloading after a change
                     (default: 300). Multiple rapid events within this window
                     are coalesced into a single reload.

    Example:
        >>> watcher = SkillWatcher(debounce_ms=300)
        >>> watcher.watch("/path/to/skills")
        >>> skills = watcher.skills()

    """

    def __init__(self, debounce_ms: int = 300) -> None: ...
    def watch(self, path: str) -> None:
        """Start watching *path* for skill changes.

        An immediate reload is performed so skills are available without
        waiting for a filesystem event.

        Args:
            path: Directory path to watch recursively.

        Raises:
            RuntimeError: If the path cannot be watched (e.g. does not exist).

        """
        ...
    def unwatch(self, path: str) -> bool:
        """Stop watching *path*.

        Returns:
            ``True`` if the path was being watched and has been removed,
            ``False`` if it was not in the watch list.

        """
        ...
    def skills(self) -> list[SkillMetadata]:
        """Return a snapshot of all currently loaded skills.

        This is a cloned, immutable snapshot — it does not block any
        background reload activity.
        """
        ...
    def skill_count(self) -> int:
        """Return the number of skills currently loaded."""
        ...
    def watched_paths(self) -> list[str]:
        """Return the list of directory paths currently being watched."""
        ...
    def reload(self) -> None:
        """Manually trigger a reload without waiting for a filesystem event."""
        ...
    def __repr__(self) -> str: ...

class TransportAddress:
    """Protocol-agnostic transport endpoint for DCC communication.

    Supports TCP, Named Pipes (Windows), and Unix Domain Sockets (macOS/Linux).
    """

    @staticmethod
    def tcp(host: str, port: int) -> TransportAddress:
        """Create a TCP transport address."""
        ...
    @staticmethod
    def named_pipe(name: str) -> TransportAddress:
        """Create a Named Pipe transport address (Windows)."""
        ...
    @staticmethod
    def unix_socket(path: str) -> TransportAddress:
        """Create a Unix Domain Socket transport address."""
        ...
    @staticmethod
    def default_local(dcc_type: str, pid: int) -> TransportAddress:
        """Generate optimal local transport for the current platform."""
        ...
    @staticmethod
    def default_pipe_name(dcc_type: str, pid: int) -> TransportAddress:
        """Generate a default Named Pipe name for a DCC instance."""
        ...
    @staticmethod
    def default_unix_socket(dcc_type: str, pid: int) -> TransportAddress:
        """Generate a default Unix Socket path for a DCC instance."""
        ...
    @staticmethod
    def parse(s: str) -> TransportAddress:
        """Parse a transport address from a URI string (tcp://, pipe://, unix://).

        Raises:
            ValueError: If the string is not a valid transport address URI.

        """
        ...

    @property
    def scheme(self) -> str:
        """Transport scheme name: "tcp", "pipe", or "unix"."""
        ...
    @property
    def is_local(self) -> bool:
        """Whether this is a local (same-machine) transport."""
        ...
    @property
    def is_tcp(self) -> bool: ...
    @property
    def is_named_pipe(self) -> bool: ...
    @property
    def is_unix_socket(self) -> bool: ...
    def to_connection_string(self) -> str:
        """Get the connection string (e.g. "tcp://127.0.0.1:18812")."""
        ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

class TransportScheme:
    """Transport selection strategy for choosing optimal communication channel."""

    AUTO: TransportScheme
    TCP_ONLY: TransportScheme
    PREFER_NAMED_PIPE: TransportScheme
    PREFER_UNIX_SOCKET: TransportScheme
    PREFER_IPC: TransportScheme

    def select_address(
        self,
        dcc_type: str,
        host: str,
        port: int,
        pid: int | None = None,
    ) -> TransportAddress:
        """Select the optimal transport address for a connection."""
        ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class RoutingStrategy:
    """Strategy for selecting DCC instances when multiple are available."""

    FIRST_AVAILABLE: RoutingStrategy
    ROUND_ROBIN: RoutingStrategy
    LEAST_BUSY: RoutingStrategy
    SPECIFIC: RoutingStrategy
    SCENE_MATCH: RoutingStrategy
    RANDOM: RoutingStrategy

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class ServiceStatus:
    """Enum for DCC service status."""

    AVAILABLE: ServiceStatus
    BUSY: ServiceStatus
    UNREACHABLE: ServiceStatus
    SHUTTING_DOWN: ServiceStatus

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class ServiceEntry:
    """Service entry representing a discovered DCC instance."""

    dcc_type: str
    instance_id: str
    host: str
    port: int
    version: str | None
    scene: str | None
    documents: list[str]
    pid: int | None
    display_name: str | None
    metadata: dict[str, str]
    status: ServiceStatus
    transport_address: TransportAddress | None
    last_heartbeat_ms: int
    @property
    def extras(self) -> dict[str, Any]:
        """Arbitrary DCC-specific extras with JSON-typed values.

        Unlike :attr:`metadata` (string-only), ``extras`` allows nested
        objects / arrays / numbers / booleans / ``None``.  Round-trips
        losslessly through ``services.json``.  Useful for WebView / bridge
        specific fields such as ``cdp_port``, ``url``, ``window_title``,
        ``host_dcc``.

        Returns:
            A fresh ``dict`` — mutating it does **not** update the registry;
            use :meth:`TransportManager.register_service(..., extras=...)`
            to register with extras.

        """
        ...
    """Last heartbeat timestamp in milliseconds since Unix epoch.

    Useful for ``LazySessionPool`` implementations to evict idle sessions:

    .. code-block:: python

        import time
        entry = mgr.get_service("maya", instance_id)
        idle_sec = (time.time() * 1000 - entry.last_heartbeat_ms) / 1000
        if idle_sec > 300:
            mgr.deregister_service("maya", instance_id)

    Updated by :meth:`TransportManager.heartbeat`.
    """

    @property
    def is_ipc(self) -> bool:
        """Whether this service uses an IPC transport."""
        ...
    def effective_address(self) -> TransportAddress:
        """Get the effective transport address (transport_address or TCP fallback)."""
        ...
    def to_dict(self) -> dict[str, Any]: ...
    def __repr__(self) -> str: ...

class TransportManager:
    """Transport layer manager with service discovery, sessions, and connection pooling."""

    def __init__(
        self,
        registry_dir: str,
        max_connections_per_dcc: int = 10,
        idle_timeout: int = 300,
        heartbeat_interval: int = 5,
        connect_timeout: int = 10,
        reconnect_max_retries: int = 3,
    ) -> None: ...

    # Service Discovery
    def register_service(
        self,
        dcc_type: str,
        host: str,
        port: int,
        version: str | None = None,
        scene: str | None = None,
        documents: list[str] | None = None,
        pid: int | None = None,
        display_name: str | None = None,
        metadata: dict[str, str] | None = None,
        transport_address: TransportAddress | None = None,
        extras: dict[str, Any] | None = None,
    ) -> str:
        """Register a DCC service instance.

        Args:
            dcc_type:           DCC application type (e.g. "maya").
            host:               Host address (e.g. "127.0.0.1").
            port:               TCP port number.
            version:            DCC version string (optional).
            scene:              Currently open scene/file (optional).
            documents:          All open documents for multi-document apps (optional).
            pid:                OS process ID (optional, disambiguates two
                                instances with the same scene).
            display_name:       Human-readable label, e.g. ``"Maya-Rigging"`` (optional).
            metadata:           Arbitrary string metadata dict (optional).
            transport_address:  Preferred IPC transport address (optional).
                                When provided, enables Named Pipe or Unix Socket
                                for lower latency same-machine communication.
                                Use ``TransportAddress.default_local(dcc_type, pid)``
                                to auto-select the optimal IPC transport.
            extras:             Arbitrary JSON-compatible extras dict (optional).
                                Values may be ``dict`` / ``list`` / ``int`` /
                                ``float`` / ``str`` / ``bool`` / ``None``.
                                Useful for WebView / bridge specific fields
                                (``cdp_port``, ``url``, ``window_title``, ...).

        Returns:
            The instance_id (UUID string) of the registered service.

        Example::

            import os
            from dcc_mcp_core import TransportManager, TransportAddress

            mgr = TransportManager(registry_dir="/tmp/dcc-mcp")
            addr = TransportAddress.default_local("maya", os.getpid())
            instance_id = mgr.register_service(
                "maya", "127.0.0.1", 18812,
                transport_address=addr,
                extras={"cdp_port": 9222, "url": "http://localhost:3000"},
            )

        """
        ...
    def deregister_service(self, dcc_type: str, instance_id: str) -> bool: ...
    def list_instances(self, dcc_type: str) -> list[ServiceEntry]: ...
    def list_all_services(self) -> list[ServiceEntry]: ...
    def list_all_instances(self) -> list[ServiceEntry]:
        """List all registered instances across all DCC types.

        Alias for :meth:`list_all_services` using the naming convention expected
        by smart-routing integrations.

        Returns:
            List of ServiceEntry objects for all registered DCC instances.

        Example::

            mgr = TransportManager("/tmp/dcc-mcp")
            for entry in mgr.list_all_instances():
                print(entry.dcc_type, entry.instance_id, entry.status)

        """
        ...
    def get_service(self, dcc_type: str, instance_id: str) -> ServiceEntry | None: ...
    def heartbeat(self, dcc_type: str, instance_id: str) -> bool: ...
    def update_service_status(self, dcc_type: str, instance_id: str, status: ServiceStatus) -> bool: ...
    def update_scene(
        self,
        dcc_type: str,
        instance_id: str,
        scene: str | None = None,
        version: str | None = None,
    ) -> bool:
        """Update scene and/or version metadata for a registered service.

        This is the primary way for a running DCC instance (e.g. Photoshop
        bridge plugin) to report that the user has opened a different scene
        or that the DCC version has changed.  The update is written to the
        shared FileRegistry so the gateway and other processes can see it.

        Args:
            dcc_type: DCC application type.
            instance_id: Instance UUID string.
            scene: New scene name (None = leave unchanged, "" = clear).
            version: New DCC version (None = leave unchanged, "" = clear).

        Returns:
            True if the service was found and updated.

        """
        ...

    # High-level auto-registration & discovery

    def bind_and_register(
        self,
        dcc_type: str,
        version: str | None = None,
        metadata: dict[str, str] | None = None,
    ) -> tuple[str, IpcListener]:
        """Bind a listener on the optimal transport and register this DCC instance.

        One-call replacement for the manual ``IpcListener.bind`` →
        ``local_address`` → ``register_service`` sequence.

        Transport selection (in priority order):

        1. **Named Pipe** (Windows) / **Unix Socket** (macOS/Linux) — PID-unique,
           zero-config, sub-millisecond latency.
        2. **TCP on ephemeral port** (``:0``) — OS assigns a free port; falls back
           to TCP when IPC is unavailable.

        Args:
            dcc_type: DCC application type (e.g. ``"maya"``).
            version:  DCC version string (optional).
            metadata: Arbitrary metadata dict (optional).

        Returns:
            ``(instance_id, listener)`` — the UUID string of the registered
            instance and the bound :class:`IpcListener` ready to accept
            connections.

        Example::

            from dcc_mcp_core import TransportManager

            mgr = TransportManager("/tmp/dcc-mcp")
            instance_id, listener = mgr.bind_and_register("maya", version="2025")
            local_addr = listener.local_address()
            print(f"Listening on {local_addr}")  # e.g. unix:///tmp/dcc-mcp-maya-12345.sock

            # Hand the listener to a serve loop (DCC plugin thread)
            channel = listener.accept()

        """
        ...

    def find_best_service(self, dcc_type: str) -> ServiceEntry:
        """Discover the best available service instance for the given DCC type.

        Returns the highest-priority *live* ``ServiceEntry`` based on:

        1. **Local IPC** (Named Pipe / Unix Socket) — lowest latency, same machine
        2. **Local TCP** (``127.0.0.1`` / ``localhost``) — same machine
        3. **Remote TCP** — cross-machine

        Within the same tier, ``AVAILABLE`` instances are preferred over ``BUSY``.
        ``UNREACHABLE`` and ``SHUTTING_DOWN`` instances are excluded.

        When **multiple instances share the same best score** (e.g. two local AVAILABLE
        IPC Maya instances), selection is **round-robin** across successive calls —
        load is automatically spread across all equivalent instances.

        Args:
            dcc_type: DCC application type (e.g. ``"maya"``).

        Returns:
            Best :class:`ServiceEntry`.

        Raises:
            RuntimeError: If no live instances are registered.

        Example::

            from dcc_mcp_core import TransportManager

            mgr = TransportManager("/tmp/dcc-mcp")

            # Works whether maya is local (IPC) or remote (TCP)
            # With 3 local Maya instances open, successive calls round-robin:
            entry1 = mgr.find_best_service("maya")  # → instance A
            entry2 = mgr.find_best_service("maya")  # → instance B
            entry3 = mgr.find_best_service("maya")  # → instance C
            session_id = mgr.get_or_create_session("maya", entry1.instance_id)

        """
        ...

    def rank_services(self, dcc_type: str) -> list[ServiceEntry]:
        """Return all live instances for `dcc_type`, sorted by connection preference.

        List-form companion to :meth:`find_best_service`. Use when you need all
        viable candidates — e.g. to dispatch work to every running Maya instance,
        implement a fallback chain, or show an instance picker in a UI.

        Sort order (lower score = more preferred):

        +-------+-----------------------------------+
        | Score | Tier                              |
        +=======+===================================+
        | 0     | Local IPC, AVAILABLE              |
        +-------+-----------------------------------+
        | 1     | Local IPC, BUSY                   |
        +-------+-----------------------------------+
        | 2     | Local TCP, AVAILABLE              |
        +-------+-----------------------------------+
        | 3     | Local TCP, BUSY                   |
        +-------+-----------------------------------+
        | 4     | Remote TCP, AVAILABLE             |
        +-------+-----------------------------------+
        | 5     | Remote TCP, BUSY                  |
        +-------+-----------------------------------+

        ``UNREACHABLE`` and ``SHUTTING_DOWN`` instances are excluded.

        Args:
            dcc_type: DCC application type (e.g. ``"maya"``).

        Returns:
            List of :class:`ServiceEntry` sorted by preference (best first).

        Raises:
            RuntimeError: If no live instances are registered.

        Example — broadcast a command to all open Maya instances::

            from dcc_mcp_core import TransportManager

            mgr = TransportManager("/tmp/dcc-mcp")

            # 3 Maya instances open locally
            for entry in mgr.rank_services("maya"):
                print(entry.instance_id, entry.status, entry.effective_address())
                sid = mgr.get_or_create_session("maya", entry.instance_id)
                # dispatch work to this specific instance via session sid

        """
        ...

    # Session Management
    def get_or_create_session(self, dcc_type: str, instance_id: str | None = None) -> str: ...
    def get_or_create_session_routed(
        self,
        dcc_type: str,
        strategy: RoutingStrategy | None = None,
        hint: str | None = None,
    ) -> str: ...
    def get_session(self, session_id: str) -> dict[str, Any] | None: ...
    def record_success(self, session_id: str, latency_ms: int) -> None: ...
    def record_error(self, session_id: str, latency_ms: int, error: str) -> None: ...
    def begin_reconnect(self, session_id: str) -> int: ...
    def reconnect_success(self, session_id: str) -> None: ...
    def close_session(self, session_id: str) -> bool: ...
    def list_sessions(self) -> list[dict[str, Any]]: ...
    def list_sessions_for_dcc(self, dcc_type: str) -> list[dict[str, Any]]: ...
    def session_count(self) -> int: ...

    # Connection Pool
    def acquire_connection(self, dcc_type: str, instance_id: str | None = None) -> str: ...
    def release_connection(self, dcc_type: str, instance_id: str) -> None: ...
    def pool_size(self) -> int: ...
    def pool_count_for_dcc(self, dcc_type: str) -> int: ...

    # Lifecycle
    def cleanup(self) -> tuple[int, int, int]: ...
    def shutdown(self) -> None: ...
    def is_shutdown(self) -> bool: ...
    def __repr__(self) -> str: ...
    def __len__(self) -> int: ...

class IpcListener:
    """Async IPC listener for DCC server-side applications.

    Supports TCP, Windows Named Pipes, and Unix Domain Sockets.
    Async operations are bridged to synchronous Python calls.

    Example:
        >>> addr = TransportAddress.tcp("127.0.0.1", 0)
        >>> listener = IpcListener.bind(addr)
        >>> local = listener.local_address()
        >>> print(f"Listening on {local}")
        >>> handle = listener.into_handle()

    """

    @staticmethod
    def bind(addr: TransportAddress) -> IpcListener:
        """Bind a listener to the given transport address.

        Raises:
            RuntimeError: If binding fails (port in use, permission denied, etc.).

        """
        ...
    def local_address(self) -> TransportAddress:
        """Get the local address that this listener is bound to.

        Raises:
            RuntimeError: If the listener has already been consumed by into_handle().

        """
        ...
    @property
    def transport_name(self) -> str:
        """Transport type: "tcp", "named_pipe", or "unix_socket"."""
        ...
    def into_handle(self) -> ListenerHandle:
        """Wrap in a ListenerHandle for connection tracking.

        Consumes the IpcListener. Can only be called once.

        Raises:
            RuntimeError: If called more than once.

        """
        ...
    def accept(self, timeout_ms: int | None = None) -> FramedChannel:
        """Accept the next incoming connection (blocking).

        Blocks until a client connects and returns a :class:`FramedChannel`
        for full-duplex framed communication with the connected client.

        Args:
            timeout_ms: Maximum wait time in ms. None = wait indefinitely.

        Returns:
            A :class:`FramedChannel` connected to the newly accepted client.

        Raises:
            RuntimeError: If no listener is bound, timeout expires, or I/O error.

        """
        ...
    def __repr__(self) -> str: ...

class ListenerHandle:
    """IPC listener handle with connection tracking and shutdown control.

    Example:
        >>> addr = TransportAddress.tcp("127.0.0.1", 0)
        >>> listener = IpcListener.bind(addr)
        >>> handle = listener.into_handle()
        >>> print(handle.accept_count)   # 0
        >>> print(handle.is_shutdown)    # False
        >>> handle.shutdown()

    """

    @property
    def accept_count(self) -> int:
        """Number of connections accepted so far."""
        ...
    @property
    def is_shutdown(self) -> bool:
        """Whether shutdown has been requested."""
        ...
    @property
    def transport_name(self) -> str:
        """Transport type: "tcp", "named_pipe", or "unix_socket"."""
        ...
    def local_address(self) -> TransportAddress:
        """Get the local address of the listener."""
        ...
    def shutdown(self) -> None:
        """Request the listener to stop accepting new connections. Idempotent."""
        ...
    def __repr__(self) -> str: ...

class FramedChannel:
    """Channel-based multiplexed I/O for DCC communication.

    Wraps a framed TCP/IPC connection with a background reader loop that
    automatically handles Ping/Pong heartbeats and Shutdown messages.
    Data messages (Request/Response/Notify) are buffered and returned by recv().

    Obtain instances via:
    - ``IpcListener.accept()`` — server-side, waits for client to connect
    - ``connect_ipc(addr)`` — client-side connection to DCC server

    Example (server):
        >>> addr = TransportAddress.tcp("127.0.0.1", 0)
        >>> listener = IpcListener.bind(addr)
        >>> channel = listener.accept()
        >>> msg = channel.recv()   # {"type": "request", ...}

    Example (client):
        >>> addr = TransportAddress.tcp("127.0.0.1", 18812)
        >>> channel = connect_ipc(addr)
        >>> rtt = channel.ping()
        >>> channel.shutdown()

    """

    @property
    def is_running(self) -> bool:
        """Whether the background reader task is still running."""
        ...
    def recv(self, timeout_ms: int | None = None) -> dict[str, Any] | None:
        """Receive the next data envelope (blocking).

        Receives Request, Response, or Notify messages. Ping/Pong/Shutdown
        are handled automatically and are NOT returned here.

        Args:
            timeout_ms: Maximum wait time in ms. None = wait indefinitely.

        Returns:
            A dict with "type" and variant-specific fields, or None if
            the connection was closed or the timeout expired.

        Raises:
            RuntimeError: If the channel has been shut down.

        """
        ...
    def try_recv(self) -> dict[str, Any] | None:
        """Try to receive without blocking. Returns None if buffer is empty.

        Raises:
            RuntimeError: If the channel has been shut down.

        """
        ...
    def ping(self, timeout_ms: int = 5000) -> int:
        """Send a heartbeat ping and return the round-trip time in ms.

        Data messages that arrive during the wait are NOT lost — they remain
        available via recv().

        Args:
            timeout_ms: Timeout in milliseconds. Defaults to 5000.

        Returns:
            Round-trip time in milliseconds.

        Raises:
            RuntimeError: If the channel is shut down or the timeout expires.

        """
        ...
    def shutdown(self) -> None:
        """Gracefully shut down the channel. Idempotent."""
        ...
    def send_request(
        self,
        method: str,
        params: bytes | None = None,
    ) -> str:
        """Send a Request to the peer.

        Args:
            method: Method name (e.g. ``"execute_python"``).
            params: Serialised parameters as bytes (optional).

        Returns:
            The request ID as a UUID string.

        Raises:
            RuntimeError: If the channel is shut down or the connection was lost.

        """
        ...
    def send_response(
        self,
        request_id: str,
        success: bool,
        payload: bytes | None = None,
        error: str | None = None,
    ) -> None:
        """Send a Response to the peer.

        Args:
            request_id: UUID string of the correlated request.
            success:    Whether the request succeeded.
            payload:    Serialised result bytes (optional).
            error:      Optional error message.

        Raises:
            RuntimeError: If the channel is shut down or the connection was lost.
            ValueError:   If ``request_id`` is not a valid UUID.

        """
        ...
    def send_notify(
        self,
        topic: str,
        data: bytes | None = None,
    ) -> None:
        """Send a one-way Notification to the peer.

        Args:
            topic: Event topic (e.g. ``"scene_changed"``).
            data:  Serialised event data bytes (optional).

        Raises:
            RuntimeError: If the channel is shut down or the connection was lost.

        """
        ...
    def call(
        self,
        method: str,
        params: bytes | None = None,
        timeout_ms: int = 30000,
    ) -> dict[str, Any]:
        """Send a Request and wait for the matching Response — the primary RPC helper.

        Atomically sends a ``Request`` and waits for the correlated ``Response``
        identified by UUID. Unrelated data messages (Notifications, other Responses)
        that arrive during the wait are **not lost** — they remain available via
        :meth:`recv`.

        This is the recommended way to invoke DCC commands:

        .. code-block:: python

            result = channel.call("execute_python", b'print("hello")')
            if result["success"]:
                print(result["payload"])
            else:
                raise RuntimeError(result["error"])

        Args:
            method:     Method name string (e.g. ``"execute_python"``).
            params:     Serialised parameters as bytes (optional, defaults to empty).
            timeout_ms: Timeout in milliseconds. Defaults to 30000 (30 s).

        Returns:
            A dict with keys:

            - ``"id"`` (str): UUID of the correlated request.
            - ``"success"`` (bool): Whether the DCC executed successfully.
            - ``"payload"`` (bytes): Serialised result data.
            - ``"error"`` (str | None): Error message when ``success`` is ``False``.

        Raises:
            RuntimeError: On timeout (``"call '<method>' timed out after <N>ms"``),
                peer error response (``"call '<method>' failed: <reason>"``),
                connection failure, or if the channel is shut down.

        """
        ...
    def __repr__(self) -> str: ...
    def __bool__(self) -> bool: ...

class BooleanWrapper:
    """Boolean wrapper for safe Python interop via PyO3."""

    value: bool

    def __init__(self, value: bool) -> None: ...
    def __bool__(self) -> bool: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class IntWrapper:
    """Integer wrapper for safe Python interop via PyO3."""

    value: int

    def __init__(self, value: int) -> None: ...
    def __int__(self) -> int: ...
    def __index__(self) -> int: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class FloatWrapper:
    """Float wrapper for safe Python interop via PyO3."""

    value: float

    def __init__(self, value: float) -> None: ...
    def __float__(self) -> float: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class StringWrapper:
    """String wrapper for safe Python interop via PyO3."""

    value: str

    def __init__(self, value: str) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

# ── Action Pipeline ──

class LoggingMiddleware:
    """Logging middleware — emits tracing log lines before/after each action."""

    def __init__(self, log_params: bool = False) -> None: ...
    @property
    def log_params(self) -> bool: ...
    def __repr__(self) -> str: ...

class TimingMiddleware:
    """Timing middleware — measures per-action latency."""

    def __init__(self) -> None: ...
    def last_elapsed_ms(self, action: str) -> int | None:
        """Return last elapsed time in ms for ``action``, or ``None`` if not dispatched yet."""
        ...
    def __repr__(self) -> str: ...

class AuditMiddleware:
    """Audit middleware — accumulates an in-memory log of all dispatched actions.

    Example::

        pipeline = ToolPipeline(dispatcher)
        audit = pipeline.add_audit()
        pipeline.dispatch("create_sphere", "{}")
        for r in audit.records():
            print(r["action"], r["success"], r["timestamp_ms"])

    """

    def __init__(self, record_params: bool = True) -> None: ...
    def records(self) -> list[dict[str, Any]]:
        """Return all audit records as dicts with keys: action, success, error, output_preview, timestamp_ms."""
        ...
    def records_for_action(self, action: str) -> list[dict[str, Any]]: ...
    def record_count(self) -> int: ...
    def clear(self) -> None: ...
    def __repr__(self) -> str: ...

class RateLimitMiddleware:
    """Rate limiting middleware — limits calls per action per time window.

    Example::

        rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
        print(rl.call_count("create_sphere"))

    """

    def __init__(self, max_calls: int, window_ms: int) -> None: ...
    def call_count(self, action: str) -> int: ...
    @property
    def max_calls(self) -> int: ...
    @property
    def window_ms(self) -> int: ...
    def __repr__(self) -> str: ...

class ToolPipeline:
    """Middleware-wrapped ToolDispatcher.

    Example::

        from dcc_mcp_core import ToolRegistry, ToolDispatcher, ToolPipeline

        reg = ToolRegistry()
        reg.register("ping", category="util")
        dispatcher = ToolDispatcher(reg)
        dispatcher.register_handler("ping", lambda params: "pong")

        pipeline = ToolPipeline(dispatcher)
        pipeline.add_logging()
        audit = pipeline.add_audit()
        timing = pipeline.add_timing()

        result = pipeline.dispatch("ping", "{}")
        assert result["output"] == "pong"

    """

    def __init__(self, dispatcher: ToolDispatcher) -> None: ...
    def add_logging(self, log_params: bool = False) -> None:
        """Add a LoggingMiddleware to the pipeline."""
        ...
    def add_timing(self) -> TimingMiddleware:
        """Add a TimingMiddleware and return the instance."""
        ...
    def add_audit(self, record_params: bool = True) -> AuditMiddleware:
        """Add an AuditMiddleware and return the instance."""
        ...
    def add_rate_limit(self, max_calls: int, window_ms: int) -> RateLimitMiddleware:
        """Add a RateLimitMiddleware and return the instance."""
        ...
    def add_callable(
        self,
        before_fn: Any | None = None,
        after_fn: Any | None = None,
    ) -> None:
        """Add a custom Python callable middleware.

        Args:
            before_fn: Optional ``(action_name: str) -> None``.
            after_fn:  Optional ``(action_name: str, success: bool) -> None``.

        Raises:
            TypeError: If before_fn or after_fn is not callable.

        """
        ...
    def register_handler(self, action_name: str, handler: Any) -> None:
        """Register a Python callable handler for action_name."""
        ...
    def dispatch(
        self,
        action_name: str,
        params_json: str = "null",
    ) -> dict[str, Any]:
        """Dispatch an action through the middleware pipeline.

        Returns:
            Dict with ``"action"``, ``"output"``, ``"validation_skipped"``.

        Raises:
            KeyError: No handler for action_name.
            ValueError: Invalid JSON or schema validation failure.
            RuntimeError: Handler error or rate-limit exceeded.

        """
        ...
    def middleware_count(self) -> int: ...
    def middleware_names(self) -> list[str]: ...
    def handler_count(self) -> int: ...
    def __repr__(self) -> str: ...

# ── Factory Functions ──

def success_result(
    message: str,
    prompt: str | None = None,
    **context: Any,
) -> ToolResult: ...
def error_result(
    message: str,
    error: str,
    prompt: str | None = None,
    possible_solutions: list[str] | None = None,
    **context: Any,
) -> ToolResult: ...
def from_exception(
    error_message: str,
    message: str | None = None,
    prompt: str | None = None,
    include_traceback: bool = True,
    possible_solutions: list[str] | None = None,
    **context: Any,
) -> ToolResult: ...
def validate_action_result(result: Any) -> ToolResult: ...
def get_bridge_context(dcc_type: str) -> BridgeContext | None:
    """Get bridge context for a specific DCC type from the global registry.

    In gateway mode, external bridge plugins register their connection info
    via :func:`register_bridge`, allowing skill scripts to access bridges
    from other processes.

    Args:
        dcc_type: DCC type identifier (e.g. ``"photoshop"``, ``"zbrush"``).

    Returns:
        A :class:`BridgeContext` if registered, or ``None``.

    Example::

        from dcc_mcp_core import get_bridge_context, register_bridge

        register_bridge("photoshop", "ws://localhost:9001")
        ctx = get_bridge_context("photoshop")
        if ctx:
            print(ctx.bridge_url, ctx.connected)

    """
    ...

def register_bridge(dcc_type: str, url: str) -> None:
    """Register a bridge connection in the global registry.

    Called by bridge plugins to register their connection info so that
    skill scripts can discover and use them via :func:`get_bridge_context`.

    Args:
        dcc_type: DCC type identifier (e.g. ``"photoshop"``).
        url: Bridge endpoint URL (e.g. ``"ws://localhost:9001"``).

    Raises:
        ValueError: If ``dcc_type`` or ``url`` is empty.

    Example::

        from dcc_mcp_core import register_bridge

        register_bridge("photoshop", "ws://localhost:9001")
        register_bridge("zbrush", "http://localhost:8765")

    """
    ...

# ── Skill Functions ──

def parse_skill_md(skill_dir: str) -> SkillMetadata | None: ...
def scan_skill_paths(
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> list[str]: ...
def resolve_dependencies(
    skills: list[SkillMetadata],
) -> list[SkillMetadata]:
    """Topologically sort skills by dependency order.

    Returns skills ordered so that every skill appears after its dependencies.
    Raises ValueError if a dependency is missing or a cycle is detected.
    """
    ...

def validate_dependencies(
    skills: list[SkillMetadata],
) -> list[str]:
    """Validate that all declared dependencies exist.

    Returns a list of error messages for each missing dependency.
    """
    ...

def expand_transitive_dependencies(
    skills: list[SkillMetadata],
    skill_name: str,
) -> list[str]:
    """Expand all transitive dependencies for a skill.

    Returns the names of all skills that skill_name transitively depends on.
    Raises ValueError if a dependency is missing or a cycle is detected.
    """
    ...

def scan_and_load(
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> tuple[list[SkillMetadata], list[str]]:
    """Full pipeline: scan directories, load skills, and resolve dependencies.

    Scans ``extra_paths`` + env + platform paths for skill directories, parses
    each SKILL.md, and topologically sorts by declared dependencies.

    Returns a tuple of (ordered_skills, skipped_dirs).
    Raises ValueError on missing dependencies or cycles.
    """
    ...

def scan_and_load_lenient(
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> tuple[list[SkillMetadata], list[str]]:
    """Lenient pipeline: scan, load, and resolve — skipping unresolvable skills.

    Unlike :func:`scan_and_load`, skills with missing dependencies are silently
    skipped (with a warning log) instead of raising an error. Only cyclic
    dependencies cause a failure.

    Returns a tuple of (ordered_skills, skipped_dirs).
    Raises ValueError only on cyclic dependencies.
    """
    ...

# ── Filesystem Functions ──

def get_config_dir() -> str: ...
def get_data_dir() -> str: ...
def get_log_dir() -> str: ...
def get_platform_dir(dir_type: str) -> str: ...
def get_tools_dir(dcc_name: str) -> str: ...
def get_skills_dir(dcc_name: str | None = None) -> str: ...
def get_skill_paths_from_env() -> list[str]: ...
def get_app_skill_paths_from_env(app_name: str) -> list[str]:
    """Return skill paths from the ``DCC_MCP_{APP_NAME}_SKILL_PATHS`` environment variable.

    Args:
        app_name: DCC application name, e.g. ``"maya"``, ``"blender"``.
                  The lookup is case-insensitive; the actual env var key is
                  upper-cased automatically (e.g. ``DCC_MCP_MAYA_SKILL_PATHS``).

    Returns:
        List of directory paths extracted from the env var, or ``[]`` if not set.

    """
    ...

# ── Type Wrapper Functions ──

def unwrap_value(value: Any) -> Any: ...
def unwrap_parameters(params: dict[str, Any]) -> dict[str, Any]: ...
def wrap_value(
    value: bool | int | float | str | Any,
) -> BooleanWrapper | IntWrapper | FloatWrapper | StringWrapper | Any: ...

# ── Transport Functions ──

def connect_ipc(
    addr: TransportAddress,
    timeout_ms: int = 10000,
) -> FramedChannel:
    """Connect to a DCC server and return a FramedChannel.

    Client-side counterpart to ``IpcListener.accept()``.

    Args:
        addr:       Transport address to connect to.
        timeout_ms: Connection timeout in milliseconds. Defaults to 10000.

    Returns:
        A :class:`FramedChannel` ready for use.

    Raises:
        RuntimeError: If the connection cannot be established within the timeout.

    Example:
        >>> from dcc_mcp_core import connect_ipc, TransportAddress
        >>> addr = TransportAddress.tcp("127.0.0.1", 18812)
        >>> channel = connect_ipc(addr)
        >>> rtt = channel.ping()
        >>> channel.shutdown()

    """
    ...

def encode_request(method: str, params: bytes | None = None) -> bytes:
    """Encode a Request message into a length-prefixed frame.

    Returns ``bytes`` in the format ``[4-byte BE length][MessagePack payload]``
    ready to write directly to a socket.

    Args:
        method: Method name (e.g. ``"execute_python"``).
        params: Serialised parameters as bytes. Defaults to empty bytes.

    Returns:
        ``bytes`` — the framed message.

    Raises:
        RuntimeError: If serialisation fails.

    Example:
        >>> frame = encode_request("execute_python", b'cmds.sphere()')
        >>> len(frame) >= 4
        True

    """
    ...

def encode_response(
    request_id: str,
    success: bool,
    payload: bytes | None = None,
    error: str | None = None,
) -> bytes:
    """Encode a Response message into a length-prefixed frame.

    Args:
        request_id: UUID string of the correlated request.
        success:    Whether the request succeeded.
        payload:    Serialised result bytes. Defaults to empty bytes.
        error:      Optional error message (use when ``success=False``).

    Returns:
        ``bytes`` — the framed message.

    Raises:
        RuntimeError: If serialisation fails.
        ValueError:   If ``request_id`` is not a valid UUID string.

    Example:
        >>> frame = encode_response("00000000-0000-0000-0000-000000000000", True, b"ok")
        >>> len(frame) >= 4
        True

    """
    ...

def encode_notify(topic: str, data: bytes | None = None) -> bytes:
    """Encode a Notify (one-way event) message into a length-prefixed frame.

    Args:
        topic: Event topic string (e.g. ``"scene_changed"``).
        data:  Serialised event data bytes. Defaults to empty bytes.

    Returns:
        ``bytes`` — the framed message.

    Raises:
        RuntimeError: If serialisation fails.

    Example:
        >>> frame = encode_notify("render_complete", b"")
        >>> len(frame) >= 4
        True

    """
    ...

def decode_envelope(data: bytes) -> dict[str, object]:
    """Decode a MessagePack payload (WITHOUT length prefix) into a message dict.

    The caller must strip the 4-byte length prefix before passing data here.

    The returned dict always has a ``"type"`` key. Additional fields depend on
    the variant:

    - ``"request"``:  ``"id"`` (str), ``"method"`` (str), ``"params"`` (bytes)
    - ``"response"``: ``"id"`` (str), ``"success"`` (bool), ``"payload"`` (bytes), ``"error"`` (str|None)
    - ``"notify"``:   ``"id"`` (str|None), ``"topic"`` (str), ``"data"`` (bytes)
    - ``"ping"``:     ``"id"`` (str), ``"timestamp_ms"`` (int)
    - ``"pong"``:     ``"id"`` (str), ``"timestamp_ms"`` (int)
    - ``"shutdown"``: ``"reason"`` (str|None)

    Args:
        data: Raw MessagePack bytes (length prefix already stripped).

    Returns:
        ``dict`` with ``"type"`` and variant-specific fields.

    Raises:
        RuntimeError: If ``data`` cannot be decoded as a valid envelope.

    Example:
        >>> frame = encode_request("ping", b"")
        >>> msg = decode_envelope(frame[4:])  # strip 4-byte prefix
        >>> msg["type"]
        'request'
        >>> msg["method"]
        'ping'

    """
    ...

# ── Process Management ──

class PyProcessMonitor:
    """Cross-platform DCC process monitor.

    Uses ``sysinfo`` to periodically sample CPU/memory for tracked PIDs.

    Example::

        import os
        mon = PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        if info:
            print(info["status"], info["cpu_usage_percent"])

    """

    def __init__(self) -> None: ...
    def track(self, pid: int, name: str) -> None:
        """Register a PID to monitor."""
        ...
    def untrack(self, pid: int) -> None:
        """Stop monitoring a PID."""
        ...
    def refresh(self) -> None:
        """Refresh underlying system data.  Must be called before querying."""
        ...
    def query(self, pid: int) -> dict[str, Any] | None:
        """Return a snapshot dict for ``pid``, or ``None`` if not found.

        Returned dict keys: ``pid``, ``name``, ``status``, ``cpu_usage_percent``,
        ``memory_bytes``, ``restart_count``.
        """
        ...
    def list_all(self) -> list[dict[str, Any]]:
        """Return snapshots for all tracked PIDs."""
        ...
    def is_alive(self, pid: int) -> bool:
        """Return ``True`` if ``pid`` is present in the OS process table."""
        ...
    def tracked_count(self) -> int:
        """Return the number of currently tracked PIDs."""
        ...
    def __repr__(self) -> str: ...

class PyDccLauncher:
    """Async DCC process launcher (spawn / terminate / kill).

    Example::

        launcher = PyDccLauncher()
        info = launcher.launch("maya-2025", "/usr/autodesk/maya/bin/maya")
        print(info["pid"])
        launcher.terminate("maya-2025")

    """

    def __init__(self) -> None: ...
    def launch(
        self,
        name: str,
        executable: str,
        args: list[str] | None = None,
        launch_timeout_ms: int = 30000,
    ) -> dict[str, Any]:
        """Spawn a DCC process.

        Returns a dict with ``pid``, ``name``, and ``status``.
        """
        ...
    def terminate(self, name: str, timeout_ms: int = 5000) -> None:
        """Gracefully terminate the named process."""
        ...
    def kill(self, name: str) -> None:
        """Kill the named process forcefully."""
        ...
    def pid_of(self, name: str) -> int | None:
        """Return the PID of the named running child, or ``None``."""
        ...
    def running_count(self) -> int:
        """Return the number of currently tracked live children."""
        ...
    def restart_count(self, name: str) -> int:
        """Return the restart count for the given name."""
        ...
    def __repr__(self) -> str: ...

class PyCrashRecoveryPolicy:
    """Crash recovery policy for DCC processes.

    Example::

        policy = PyCrashRecoveryPolicy(max_restarts=3)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        assert policy.should_restart("crashed")
        delay = policy.next_delay_ms("maya", attempt=0)

    """

    max_restarts: int

    def __init__(self, max_restarts: int = 3) -> None: ...
    def use_exponential_backoff(self, initial_ms: int, max_delay_ms: int) -> None:
        """Switch to exponential back-off."""
        ...
    def use_fixed_backoff(self, delay_ms: int) -> None:
        """Switch to fixed back-off."""
        ...
    def should_restart(self, status: str) -> bool:
        """Return ``True`` if the status warrants a restart.

        Recognised values: ``"crashed"``, ``"unresponsive"``.
        """
        ...
    def next_delay_ms(self, name: str, attempt: int) -> int:
        """Return the delay before attempt ``attempt`` (0-indexed).

        Raises ``RuntimeError`` if ``max_restarts`` has been exceeded.
        """
        ...
    def __repr__(self) -> str: ...

class PyProcessWatcher:
    """Async background process watcher with event polling.

    Spawns a background loop that periodically polls all tracked processes and
    accumulates events.  Python code calls ``poll_events()`` to drain the queue.

    Event dict keys: ``type``, ``pid``, ``name``, plus type-specific fields:

    - ``"heartbeat"`` → ``new_status``, ``cpu_usage_percent``, ``memory_bytes``
    - ``"status_changed"`` → ``old_status``, ``new_status``
    - ``"exited"`` → no extra fields

    Example::

        import os, time
        watcher = PyProcessWatcher(poll_interval_ms=200)
        watcher.track(os.getpid(), "self")
        watcher.start()
        time.sleep(0.5)
        for event in watcher.poll_events():
            print(event["type"], event["name"])
        watcher.stop()

    """

    def __init__(self, poll_interval_ms: int = 500) -> None: ...
    def track(self, pid: int, name: str) -> None:
        """Register a PID to monitor."""
        ...
    def untrack(self, pid: int) -> None:
        """Stop monitoring a PID."""
        ...
    def start(self) -> None:
        """Start the background watch loop.  No-op if already running."""
        ...
    def stop(self) -> None:
        """Stop the background watch loop.  No-op if not running."""
        ...
    def poll_events(self) -> list[dict[str, Any]]:
        """Drain and return all pending events as a list of dicts."""
        ...
    def is_running(self) -> bool:
        """Return ``True`` if the background loop is running."""
        ...
    def tracked_count(self) -> int:
        """Return the number of currently tracked PIDs."""
        ...
    def __repr__(self) -> str: ...

# ── Telemetry ──

class ToolMetrics:
    """Read-only snapshot of per-Action performance metrics."""

    @property
    def action_name(self) -> str: ...
    @property
    def invocation_count(self) -> int: ...
    @property
    def success_count(self) -> int: ...
    @property
    def failure_count(self) -> int: ...
    @property
    def avg_duration_ms(self) -> float: ...
    @property
    def p95_duration_ms(self) -> float: ...
    @property
    def p99_duration_ms(self) -> float: ...
    def success_rate(self) -> float:
        """Return success rate as a fraction in [0.0, 1.0]."""
        ...
    def __repr__(self) -> str: ...

class RecordingGuard:
    """RAII guard returned by ``ToolRecorder.start()``.

    Call :meth:`finish` or use as a context manager.

    Example::

        guard = recorder.start("create_sphere", "maya")
        try:
            # ... do work ...
            guard.finish(success=True)
        except Exception:
            guard.finish(success=False)
            raise

    Context manager usage::

        with recorder.start("create_sphere", "maya") as guard:
            # success=True if no exception, success=False otherwise
            pass

    """

    def finish(self, success: bool) -> None:
        """Finish recording with the given success flag."""
        ...
    def __enter__(self) -> RecordingGuard: ...
    def __exit__(
        self,
        exc_type: type | None,
        exc_value: BaseException | None,
        traceback: object | None,
    ) -> None: ...
    def __repr__(self) -> str: ...

class ToolRecorder:
    """Records per-Action execution time and success/failure counters.

    Example::

        recorder = ToolRecorder("my-service")
        guard = recorder.start("create_sphere", "maya")
        # ... do work ...
        guard.finish(success=True)

        metrics = recorder.metrics("create_sphere")
        print(metrics.invocation_count, metrics.success_rate())

    """

    def __init__(self, scope: str) -> None:
        """Create a new recorder for the given scope name."""
        ...
    def start(self, action_name: str, dcc_name: str) -> RecordingGuard:
        """Start timing an action and return a guard object."""
        ...
    def metrics(self, action_name: str) -> ToolMetrics | None:
        """Get aggregated metrics for a specific action.

        Returns ``None`` if no data exists for this action.
        """
        ...
    def all_metrics(self) -> list[ToolMetrics]:
        """Get aggregated metrics for all recorded actions."""
        ...
    def reset(self) -> None:
        """Reset all in-memory statistics."""
        ...

class TelemetryConfig:
    """Builder and initialiser for the global telemetry provider.

    Example::

        cfg = (TelemetryConfig("my-service")
                .with_stdout_exporter()
                .with_attribute("dcc.name", "maya"))
        cfg.init()
        cfg.shutdown()

    """

    def __init__(self, service_name: str) -> None: ...
    @property
    def service_name(self) -> str: ...
    @property
    def enable_metrics(self) -> bool: ...
    @property
    def enable_tracing(self) -> bool: ...
    def with_stdout_exporter(self) -> TelemetryConfig:
        """Use the stdout exporter (prints spans/metrics to stdout)."""
        ...
    def with_noop_exporter(self) -> TelemetryConfig:
        """Use the no-op exporter (discard all telemetry — useful in tests)."""
        ...
    def with_json_logs(self) -> TelemetryConfig:
        """Use JSON log format."""
        ...
    def with_text_logs(self) -> TelemetryConfig:
        """Use text log format (default)."""
        ...
    def with_attribute(self, key: str, value: str) -> TelemetryConfig:
        """Add an extra resource attribute."""
        ...
    def with_service_version(self, version: str) -> TelemetryConfig:
        """Set the service version string."""
        ...
    def set_enable_metrics(self, enabled: bool) -> TelemetryConfig:
        """Enable or disable metrics collection."""
        ...
    def set_enable_tracing(self, enabled: bool) -> TelemetryConfig:
        """Enable or disable distributed tracing."""
        ...
    def init(self) -> None:
        """Install this configuration as the global telemetry provider.

        Raises:
            RuntimeError: If a provider is already installed.

        """
        ...
    def __repr__(self) -> str: ...

# ── Telemetry Functions ──

def is_telemetry_initialized() -> bool:
    """Return ``True`` if the global telemetry provider has been initialised."""
    ...

def shutdown_telemetry() -> None:
    """Shut down the global telemetry provider, flushing all pending data."""
    ...

# ── Sandbox ──

class AuditEntry:
    """A single audit record produced by the sandbox for one action invocation.

    Read-only data class; all attributes are properties.
    """

    @property
    def timestamp_ms(self) -> int:
        """Unix timestamp in milliseconds when the action was recorded."""
        ...

    @property
    def actor(self) -> str | None:
        """Actor / caller identity, or ``None``."""
        ...

    @property
    def action(self) -> str:
        """Name of the action that was invoked."""
        ...

    @property
    def params_json(self) -> str:
        """Parameters as a JSON string."""
        ...

    @property
    def duration_ms(self) -> int:
        """Duration of the execution in milliseconds."""
        ...

    @property
    def outcome(self) -> str:
        """Outcome string: ``"success"``, ``"denied"``, ``"error"``, or ``"timeout"``."""
        ...

    @property
    def outcome_detail(self) -> str | None:
        """Outcome detail (denial reason or error message), or ``None``."""
        ...

    def __repr__(self) -> str: ...

class AuditLog:
    """Read-only Python view of the sandbox audit log.

    Example::

        ctx = SandboxContext(policy)
        ctx.execute_json("echo", "{}")
        log = ctx.audit_log
        print(len(log))             # 1
        for entry in log.entries():
            print(entry.action, entry.outcome)

    """

    def __len__(self) -> int: ...
    def entries(self) -> list[AuditEntry]:
        """Return all recorded entries."""
        ...
    def successes(self) -> list[AuditEntry]:
        """Return only entries with outcome ``"success"``."""
        ...
    def denials(self) -> list[AuditEntry]:
        """Return only entries with outcome ``"denied"``."""
        ...
    def entries_for_action(self, action: str) -> list[AuditEntry]:
        """Return all entries for the given action name."""
        ...
    def to_json(self) -> str:
        """Return all entries serialised as a JSON array string."""
        ...
    def __repr__(self) -> str: ...

class SandboxPolicy:
    """Sandbox policy: API whitelist, path allowlist, execution constraints.

    Example::

        policy = SandboxPolicy()
        policy.allow_actions(["get_scene_info", "list_objects"])
        policy.deny_actions(["delete_scene"])
        policy.set_timeout_ms(5000)
        policy.set_max_actions(100)
        policy.set_read_only(False)

    """

    def __init__(self) -> None: ...
    def allow_actions(self, actions: list[str]) -> None:
        """Restrict execution to only the listed actions (replaces any previous whitelist)."""
        ...
    def deny_actions(self, actions: list[str]) -> None:
        """Deny these actions even if listed in the whitelist."""
        ...
    def allow_paths(self, paths: list[str]) -> None:
        """Allow file-system access inside these directory paths."""
        ...
    def set_timeout_ms(self, ms: int) -> None:
        """Set the execution timeout in milliseconds."""
        ...
    def set_max_actions(self, count: int) -> None:
        """Set the maximum number of actions allowed per session."""
        ...
    def set_read_only(self, read_only: bool) -> None:
        """Enable (``True``) or disable (``False``) read-only mode."""
        ...
    @property
    def is_read_only(self) -> bool:
        """``True`` if the policy is in read-only mode."""
        ...
    def __repr__(self) -> str: ...

class SandboxContext:
    """Sandbox execution context for a single session.

    Bundles a :class:`SandboxPolicy` with an :class:`AuditLog` and an action
    counter.  All action invocations pass through policy checks, optional
    input validation, and are recorded in the audit log.

    Example::

        policy = SandboxPolicy()
        policy.allow_actions(["echo"])
        ctx = SandboxContext(policy)
        ctx.set_actor("my-agent")
        result_json = ctx.execute_json("echo", '{"x": 1}')
        print(ctx.action_count, ctx.audit_log)

    """

    def __init__(self, policy: SandboxPolicy) -> None: ...
    def set_actor(self, actor: str) -> None:
        """Set the caller identity attached to audit entries."""
        ...
    def execute_json(self, action: str, params_json: str) -> str:
        """Execute *action* with parameters supplied as a JSON string.

        Runs the full sandbox pipeline (policy check, validation).
        Returns the result as a JSON string.

        Raises:
            RuntimeError: If the action is denied, validation fails, or
                          any other sandbox error occurs.

        """
        ...
    @property
    def action_count(self) -> int:
        """Number of actions successfully executed in this session."""
        ...
    @property
    def audit_log(self) -> AuditLog:
        """The :class:`AuditLog` for this context."""
        ...
    def is_allowed(self, action: str) -> bool:
        """Return ``True`` if *action* is permitted by the current policy."""
        ...
    def is_path_allowed(self, path: str) -> bool:
        """Return ``True`` if *path* is within an allowed directory."""
        ...
    def __repr__(self) -> str: ...

class InputValidator:
    """Schema-based input validator for sandbox action parameters.

    Example::

        v = InputValidator()
        v.require_string("name", max_length=50)
        v.require_number("count", min_value=0, max_value=1000)
        ok, error = v.validate('{"name": "sphere", "count": 5}')
        assert ok

        # Injection guard
        v.forbid_substrings("script", ["__import__", "exec(", "eval("])
        ok, err = v.validate('{"script": "__import__(os)"}')
        assert not ok

    """

    def __init__(self) -> None: ...
    def require_string(
        self,
        field: str,
        max_length: int | None = None,
        min_length: int | None = None,
    ) -> None:
        """Register a required string field with optional length constraints."""
        ...
    def require_number(
        self,
        field: str,
        min_value: float | None = None,
        max_value: float | None = None,
    ) -> None:
        """Register a required numeric field with optional range constraints."""
        ...
    def forbid_substrings(self, field: str, substrings: list[str]) -> None:
        """Add an injection-guard rule: the string field must not contain any of these substrings."""
        ...
    def validate(self, params_json: str) -> tuple[bool, str | None]:
        """Validate *params_json* against all registered schemas.

        Returns:
            ``(True, None)`` on success.
            ``(False, error_message)`` on failure.

        Raises:
            RuntimeError: If *params_json* is not valid JSON.

        """
        ...

# ── Shared Memory (dcc-mcp-shm) ──

class PySharedBuffer:
    """A named, fixed-capacity shared memory buffer backed by a memory-mapped file.

    Zero-copy: the DCC side writes data directly into the mapped region; the
    consumer reads from the same mapping without any copying or serialisation.

    Example::

        buf = PySharedBuffer.create(capacity=1024 * 1024)  # 1 MiB
        n = buf.write(b"vertex data")
        data = buf.read()
        assert data == b"vertex data"

        # Cross-process handoff
        desc_json = buf.descriptor_json()
        # ... send desc_json to consumer via IPC ...
        buf2 = PySharedBuffer.open(path=buf.path(), id=buf.id)
        assert buf2.read() == b"vertex data"

    """

    @staticmethod
    def create(capacity: int) -> PySharedBuffer:
        """Create a new buffer with the given capacity in bytes."""
        ...
    @staticmethod
    def open(path: str, id: str) -> PySharedBuffer:
        """Open an existing buffer from a file path and id."""
        ...
    def write(self, data: bytes) -> int:
        """Write bytes into the buffer. Returns the number of bytes written.

        Raises:
            RuntimeError: If data is larger than the buffer capacity.

        """
        ...
    def read(self) -> bytes:
        """Read the current data from the buffer."""
        ...
    def data_len(self) -> int:
        """Return the number of bytes currently stored."""
        ...
    def capacity(self) -> int:
        """Return the maximum number of bytes this buffer can hold."""
        ...
    def clear(self) -> None:
        """Clear the buffer (reset data_len to 0)."""
        ...
    @property
    def id(self) -> str:
        """Buffer id (string)."""
        ...
    def path(self) -> str:
        """File path of the backing memory-mapped file."""
        ...
    def descriptor_json(self) -> str:
        """Return a JSON descriptor string for cross-process handoff."""
        ...
    def __repr__(self) -> str: ...

class PyBufferPool:
    """A fixed-capacity pool of reusable shared memory buffers.

    Amortises the cost of allocating memory-mapped files for high-frequency
    use-cases such as 30 fps scene snapshots.

    Example::

        pool = PyBufferPool(capacity=4, buffer_size=1024 * 1024)
        buf = pool.acquire()
        buf.write(b"scene snapshot")
        # Buffer returned to pool when `buf` is garbage-collected.

    """

    def __init__(self, capacity: int, buffer_size: int) -> None:
        """Create a pool of ``capacity`` buffers, each holding ``buffer_size`` bytes."""
        ...
    def acquire(self) -> PySharedBuffer:
        """Acquire a free buffer.

        Raises:
            RuntimeError: If all slots are in use.

        """
        ...
    def available(self) -> int:
        """Return the number of currently available (free) slots."""
        ...
    def capacity(self) -> int:
        """Return the total pool capacity."""
        ...
    def buffer_size(self) -> int:
        """Return the per-buffer size in bytes."""
        ...
    def __repr__(self) -> str: ...

class PySceneDataKind:
    """Kind of DCC scene data stored in a shared scene buffer."""

    Geometry: PySceneDataKind
    AnimationCache: PySceneDataKind
    Screenshot: PySceneDataKind
    Arbitrary: PySceneDataKind

class PySharedSceneBuffer:
    """High-level shared scene buffer for zero-copy DCC ↔ Agent data exchange.

    Automatically selects inline (single buffer) vs chunked storage based
    on data size.  Data larger than 256 MiB is split into chunks.

    Example::

        ssb = PySharedSceneBuffer.write(
            data=vertex_bytes,
            kind=PySceneDataKind.Geometry,
            source_dcc="Maya",
            use_compression=True,
        )
        desc_json = ssb.descriptor_json()
        # Send desc_json to consumer via IPC …

        # Consumer side:
        recovered = ssb.read()
        assert recovered == vertex_bytes

    """

    @staticmethod
    def write(
        data: bytes,
        kind: PySceneDataKind = ...,
        source_dcc: str | None = None,
        use_compression: bool = False,
    ) -> PySharedSceneBuffer:
        """Write data into a new shared scene buffer.

        Parameters
        ----------
        data:
            Raw payload to store.
        kind:
            Semantic kind of the data (default: ``Arbitrary``).
        source_dcc:
            Name of the originating DCC application.
        use_compression:
            Whether to apply LZ4 compression before writing.

        """
        ...
    def read(self) -> bytes:
        """Read the stored data back (decompresses automatically if needed)."""
        ...
    @property
    def id(self) -> str:
        """Transfer id (UUID string)."""
        ...
    @property
    def total_bytes(self) -> int:
        """Total original byte count."""
        ...
    @property
    def is_inline(self) -> bool:
        """Whether data is stored in a single inline buffer."""
        ...
    @property
    def is_chunked(self) -> bool:
        """Whether data spans multiple chunks."""
        ...
    def descriptor_json(self) -> str:
        """JSON descriptor for cross-process handoff."""
        ...
    def __repr__(self) -> str: ...

# ── GPU Capture (dcc-mcp-capture) ──

class CaptureFrame:
    """A single captured frame from a DCC viewport or display.

    Returned by :class:`Capturer`.capture().

    Example::

        capturer = Capturer.new_mock(1920, 1080)
        frame = capturer.capture(format="png")
        print(f"{frame.width}x{frame.height} — {frame.byte_len()} bytes")
        # Write PNG to disk:
        with open("screenshot.png", "wb") as f:
            f.write(frame.data)

    """

    @property
    def data(self) -> bytes:
        """Encoded image bytes (PNG, JPEG) or raw BGRA32 data."""
        ...

    @property
    def width(self) -> int:
        """Frame width in pixels (after scaling/crop)."""
        ...

    @property
    def height(self) -> int:
        """Frame height in pixels (after scaling/crop)."""
        ...

    @property
    def format(self) -> str:
        """Format string: ``"png"``, ``"jpeg"``, or ``"raw_bgra"``."""
        ...

    @property
    def mime_type(self) -> str:
        """MIME type for the encoded bytes (e.g. ``"image/png"``)."""
        ...

    @property
    def timestamp_ms(self) -> int:
        """Milliseconds since Unix epoch at capture time."""
        ...

    @property
    def dpi_scale(self) -> float:
        """Display scale factor (1.0 standard, 2.0 HiDPI)."""
        ...

    @property
    def window_rect(self) -> tuple[int, int, int, int] | None:
        """Source window bounds ``(x, y, width, height)`` or ``None`` for full-screen captures."""
        ...

    @property
    def window_title(self) -> str | None:
        """Source window title or ``None`` for full-screen captures."""
        ...

    def byte_len(self) -> int:
        """Byte length of the encoded image data."""
        ...

    def __repr__(self) -> str: ...

class CaptureTarget:
    """Capture target specifier.

    Construct via the static factory methods; the value is opaque to Python.
    """

    @staticmethod
    def primary_display() -> CaptureTarget: ...
    @staticmethod
    def monitor_index(index: int) -> CaptureTarget: ...
    @staticmethod
    def process_id(pid: int) -> CaptureTarget: ...
    @staticmethod
    def window_title(title: str) -> CaptureTarget: ...
    @staticmethod
    def window_handle(handle: int) -> CaptureTarget:
        """Platform-opaque native window handle (HWND on Windows, XID on X11)."""
        ...

    def __repr__(self) -> str: ...

class CaptureBackendKind:
    """Enum-like handle identifying the active capture backend."""

    DxgiDesktopDuplication: CaptureBackendKind
    ScreenCaptureKit: CaptureBackendKind
    X11Xshm: CaptureBackendKind
    PipeWire: CaptureBackendKind
    HwndPrintWindow: CaptureBackendKind
    Mock: CaptureBackendKind

    @property
    def name(self) -> str: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

class WindowInfo:
    """Metadata for a discovered top-level window."""

    @property
    def handle(self) -> int: ...
    @property
    def pid(self) -> int: ...
    @property
    def title(self) -> str: ...
    @property
    def rect(self) -> tuple[int, int, int, int]: ...

class WindowFinder:
    """Resolve :class:`CaptureTarget` values to concrete top-level windows."""

    def __init__(self) -> None: ...
    def find(self, target: CaptureTarget) -> WindowInfo | None:
        """Return the matching window or ``None`` if not found."""
        ...
    def enumerate(self) -> list[WindowInfo]:
        """Return all visible top-level windows."""
        ...

class Capturer:
    """High-level DCC screenshot / frame-capture entry point.

    Automatically selects the best available backend on the current platform:

    - Windows: DXGI Desktop Duplication API (GPU framebuffer, <16ms per frame)
    - Linux: X11 XShmGetImage
    - Fallback: Mock synthetic backend (for CI / headless environments)

    Example::

        capturer = Capturer.new_auto()
        frame = capturer.capture(format="png")
        print(f"Backend: {capturer.backend_name()}")
        print(f"Captured {frame.width}x{frame.height}, {frame.byte_len()} bytes")
        count, total_bytes, errors = capturer.stats()

    Mock backend (headless / testing)::

        capturer = Capturer.new_mock(width=1920, height=1080)
        frame = capturer.capture(format="raw_bgra")

    """

    @staticmethod
    def new_auto() -> Capturer:
        """Create a capturer using the best available backend on this platform."""
        ...

    @staticmethod
    def new_mock(width: int = 1920, height: int = 1080) -> Capturer:
        """Create a capturer backed by the mock (synthetic checkerboard) backend.

        Safe to use in headless CI and testing environments without a GPU.
        """
        ...

    @staticmethod
    def new_window_auto() -> Capturer:
        """Create a capturer configured for single-window capture.

        Uses the GDI ``PrintWindow`` backend on Windows; falls back to the
        mock backend on other platforms until window-target backends are added.
        """
        ...

    def capture(
        self,
        format: str = "png",
        jpeg_quality: int = 85,
        scale: float = 1.0,
        timeout_ms: int = 5000,
        process_id: int | None = None,
        window_title: str | None = None,
    ) -> CaptureFrame:
        """Capture a single frame.

        Parameters
        ----------
        format:
            Output format: ``"png"`` (default), ``"jpeg"``, or ``"raw_bgra"``.
        jpeg_quality:
            JPEG quality 0-100 (default 85). Ignored for PNG / raw_bgra.
        scale:
            Scale factor 0.0-1.0 (default 1.0 = native resolution).
        timeout_ms:
            Maximum milliseconds to wait for a frame (default 5000).
        process_id:
            Capture the window belonging to this PID.
        window_title:
            Capture the window whose title contains this substring.

        Returns
        -------
        CaptureFrame:
            Captured frame with image data and metadata.

        Raises
        ------
        RuntimeError:
            If the capture backend fails, the target window is not found, or
            the operation times out.

        """
        ...

    def capture_window(
        self,
        *,
        process_id: int | None = None,
        window_handle: int | None = None,
        window_title: str | None = None,
        format: str = "png",
        jpeg_quality: int = 85,
        scale: float = 1.0,
        timeout_ms: int = 5000,
        include_decorations: bool = True,
    ) -> CaptureFrame:
        """Capture a single top-level window.

        At least one of ``process_id``, ``window_handle``, or ``window_title``
        must be provided. Returns a :class:`CaptureFrame` with
        :attr:`CaptureFrame.window_rect` and :attr:`CaptureFrame.window_title`
        populated.

        Raises
        ------
        ValueError:
            If none of the three target kwargs are supplied.
        RuntimeError:
            If the target window cannot be resolved or the backend fails.

        """
        ...

    def backend_name(self) -> str:
        """Return the name of the active backend (e.g. ``"DXGI Desktop Duplication"``)."""
        ...

    def backend_kind(self) -> CaptureBackendKind:
        """Return the active backend kind enum."""
        ...

    def stats(self) -> tuple[int, int, int]:
        """Return running statistics as ``(capture_count, total_bytes, error_count)``."""
        ...

    @staticmethod
    def capture_window_png(pid: int, *, timeout_ms: int = 1000) -> bytes | None:
        """Capture a window as PNG bytes, looked up by process ID.

        Sugar wrapper for the "grab a window snapshot, attach to chat" flow —
        no ``Capturer`` instance needed.  Internally creates a window-auto
        capturer, captures the first visible top-level window for ``pid``,
        and returns the PNG-encoded bytes.

        Args:
            pid:         OS process ID of the DCC to capture.
            timeout_ms:  Max milliseconds to wait for the frame (default 1000).

        Returns:
            PNG-encoded bytes on success; ``None`` when the process has no
            visible top-level window or the backend is unavailable.  Never
            raises on capture errors — use :meth:`capture_window` when you
            need exceptions.

        Example::

            from dcc_mcp_core import Capturer

            png = Capturer.capture_window_png(pid=12345)
            if png is not None:
                attach_to_chat(png)

        """
        ...

    @staticmethod
    def capture_region_png(
        pid: int,
        x: int,
        y: int,
        w: int,
        h: int,
        *,
        timeout_ms: int = 1000,
    ) -> bytes | None:
        """Capture a cropped window region as PNG bytes, looked up by PID.

        The window is captured first (same backend as
        :meth:`capture_window_png`) and the ``(x, y, w, h)`` rectangle is
        cropped in CPU before re-encoding.  Coordinates are in window-local
        pixels relative to the window's top-left corner.

        Args:
            pid:         OS process ID of the DCC to capture.
            x, y:        Top-left corner of the crop rectangle (window-local).
            w, h:        Width / height of the crop rectangle.
            timeout_ms:  Max milliseconds to wait for the frame (default 1000).

        Returns:
            PNG-encoded cropped bytes on success; ``None`` on any failure
            (window not found, crop out of bounds, decode error, ...).

        """
        ...

    def __repr__(self) -> str: ...

# ── USD Scene Description (dcc-mcp-usd) ──

class SdfPath:
    """A USD scene description path (e.g. ``/World/Cube``).

    Paths use forward slashes and start with ``/`` for absolute paths.

    Example::

        path = SdfPath("/World")
        child = path.child("Cube")   # SdfPath("/World/Cube")
        assert child.name == "Cube"
        assert child.is_absolute

    """

    def __init__(self, path: str) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
    def child(self, name: str) -> SdfPath:
        """Append a child segment and return a new path."""
        ...

    def parent(self) -> SdfPath | None:
        """Parent path, or ``None`` for the root path."""
        ...

    @property
    def is_absolute(self) -> bool:
        """Whether this is an absolute path (starts with ``/``)."""
        ...

    @property
    def name(self) -> str:
        """Last path element (e.g. ``"Cube"`` for ``/World/Cube``)."""
        ...

class VtValue:
    """A USD variant value (bool, int, float, string, vec3f, …).

    Use the static factory methods to create values::

        v_float = VtValue.from_float(1.0)
        v_vec3  = VtValue.from_vec3f(1.0, 2.0, 3.0)
        v_str   = VtValue.from_string("hello")

    """

    @property
    def type_name(self) -> str:
        """USD type name string (e.g. ``"float3"``, ``"token"``)."""
        ...

    @staticmethod
    def from_bool(v: bool) -> VtValue: ...
    @staticmethod
    def from_int(v: int) -> VtValue: ...
    @staticmethod
    def from_float(v: float) -> VtValue: ...
    @staticmethod
    def from_string(v: str) -> VtValue: ...
    @staticmethod
    def from_token(v: str) -> VtValue: ...
    @staticmethod
    def from_asset(v: str) -> VtValue: ...
    @staticmethod
    def from_vec3f(x: float, y: float, z: float) -> VtValue: ...
    def to_python(self) -> bool | int | float | str | tuple[float, ...] | list[float] | list[int] | list[str] | None:
        """Convert to a Python primitive.  Returns ``None`` for matrix/unsupported types."""
        ...

    def __repr__(self) -> str: ...

class UsdPrim:
    """A prim (primitive) within a USD stage.

    Example::

        stage = UsdStage("test")
        prim = stage.define_prim("/World/Cube", "Mesh")
        prim.set_attribute("radius", VtValue.from_float(1.0))
        print(prim.get_attribute("radius").to_python())  # 1.0

    """

    @property
    def path(self) -> SdfPath: ...
    @property
    def type_name(self) -> str: ...
    @property
    def active(self) -> bool: ...
    @property
    def name(self) -> str: ...
    def set_attribute(self, name: str, value: VtValue) -> None: ...
    def get_attribute(self, name: str) -> VtValue | None: ...
    def attribute_names(self) -> list[str]: ...
    def attributes_summary(self) -> dict[str, str]: ...
    def has_api(self, schema: str) -> bool: ...
    def __repr__(self) -> str: ...

class UsdStage:
    """A composed USD stage — primary unit of cross-DCC scene exchange.

    Example::

        stage = UsdStage("my_scene")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        stage.set_attribute("/World/Cube", "extent", VtValue.from_vec3f(1, 1, 1))
        usda = stage.export_usda()
        json_str = stage.to_json()
        back = UsdStage.from_json(json_str)

    """

    def __init__(self, name: str) -> None: ...
    def __repr__(self) -> str: ...
    @property
    def name(self) -> str: ...
    @property
    def id(self) -> str: ...
    @property
    def default_prim(self) -> str | None: ...
    @default_prim.setter
    def default_prim(self, value: str | None) -> None: ...
    @property
    def up_axis(self) -> str: ...
    @up_axis.setter
    def up_axis(self, axis: str) -> None: ...
    @property
    def meters_per_unit(self) -> float: ...
    @meters_per_unit.setter
    def meters_per_unit(self, mpu: float) -> None: ...
    @property
    def fps(self) -> float | None: ...
    @fps.setter
    def fps(self, fps: float | None) -> None: ...
    @property
    def start_time_code(self) -> float | None: ...
    @start_time_code.setter
    def start_time_code(self, v: float | None) -> None: ...
    @property
    def end_time_code(self) -> float | None: ...
    @end_time_code.setter
    def end_time_code(self, v: float | None) -> None: ...
    def define_prim(self, path: str, type_name: str) -> UsdPrim: ...
    def get_prim(self, path: str) -> UsdPrim | None: ...
    def has_prim(self, path: str) -> bool: ...
    def remove_prim(self, path: str) -> bool: ...
    def traverse(self) -> list[UsdPrim]: ...
    def prims_of_type(self, type_name: str) -> list[UsdPrim]: ...
    def set_attribute(self, prim_path: str, attr_name: str, value: VtValue) -> None: ...
    def get_attribute(self, prim_path: str, attr_name: str) -> VtValue | None: ...
    def metrics(self) -> dict[str, int]: ...
    def to_json(self) -> str: ...
    @staticmethod
    def from_json(json: str) -> UsdStage: ...
    def export_usda(self) -> str: ...

# ── USD bridge functions ──

def scene_info_json_to_stage(scene_info_json: str, dcc_type: str) -> UsdStage:
    """Convert a DCC ``SceneInfo`` JSON string to a ``UsdStage``."""
    ...

def stage_to_scene_info_json(stage: UsdStage) -> str:
    """Convert a ``UsdStage`` to a ``SceneInfo`` JSON string (best-effort)."""
    ...

def units_to_mpu(units: str) -> float:
    """Convert a unit string to USD ``metersPerUnit`` (e.g. ``"cm"`` → 0.01)."""
    ...

def mpu_to_units(mpu: float) -> str:
    """Convert ``metersPerUnit`` to a unit string (e.g. 0.01 → ``"cm"``)."""
    ...

# ── MCP HTTP Server ──

class McpHttpConfig:
    """Configuration for the MCP Streamable HTTP server.

    Args:
        port: TCP port to listen on. Use ``0`` for a random available port.
        server_name: Name reported in MCP ``initialize`` response.
        server_version: Version reported in MCP ``initialize`` response.
        enable_cors: Enable CORS headers (for browser clients).
        request_timeout_ms: Request timeout in milliseconds.

    Example::

        from dcc_mcp_core import McpHttpConfig
        cfg = McpHttpConfig(port=8765, server_name="maya-mcp")

    """

    def __init__(
        self,
        port: int = 8765,
        server_name: str | None = None,
        server_version: str | None = None,
        enable_cors: bool = False,
        request_timeout_ms: int = 30000,
    ) -> None: ...
    @property
    def port(self) -> int: ...
    @property
    def host(self) -> str: ...
    @property
    def endpoint_path(self) -> str: ...
    @property
    def server_name(self) -> str: ...
    @property
    def server_version(self) -> str: ...
    @property
    def max_sessions(self) -> int: ...
    @property
    def request_timeout_ms(self) -> int: ...
    @property
    def enable_cors(self) -> bool: ...
    @property
    def session_ttl_secs(self) -> int:
        """Idle session TTL in seconds.

        Sessions not touched within this window are automatically evicted.
        Default: 3600 (1 hour). Set to 0 to disable.
        """
        ...
    @session_ttl_secs.setter
    def session_ttl_secs(self, secs: int) -> None: ...
    def __repr__(self) -> str: ...

class McpServerHandle:
    """Handle returned by :meth:`McpHttpServer.start`.

    Example::

        handle = server.start()
        print(handle.mcp_url())   # http://127.0.0.1:8765/mcp
        handle.shutdown()
    """

    @property
    def port(self) -> int:
        """The actual port the server is listening on."""
        ...
    @property
    def bind_addr(self) -> str:
        """The bind address, e.g. ``127.0.0.1:8765``."""
        ...
    @property
    def is_gateway(self) -> bool:
        """Whether this process won the gateway port competition."""
        ...
    def mcp_url(self) -> str:
        """Full MCP endpoint URL, e.g. ``http://127.0.0.1:8765/mcp``."""
        ...
    def shutdown(self) -> None:
        """Gracefully shut down the server (blocks until stopped)."""
        ...
    def signal_shutdown(self) -> None:
        """Signal shutdown without blocking."""
        ...
    def __repr__(self) -> str: ...

class McpHttpServer:
    """MCP Streamable HTTP server (2025-03-26 spec).

    Embeds an axum/Tokio HTTP server. Safe to call from DCC main threads —
    the server runs in a background thread and never blocks the caller.

    Example::

        from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

        registry = ToolRegistry()
        registry.register("get_scene_info", description="Get scene info",
                          category="scene", tags=[], dcc="maya",
                          version="1.0.0")

        server = McpHttpServer(registry, McpHttpConfig(port=8765))
        handle = server.start()
        # MCP host connects to handle.mcp_url()
        handle.shutdown()
    """

    def __init__(
        self,
        registry: ToolRegistry,
        config: McpHttpConfig | None = None,
    ) -> None: ...
    def start(self) -> McpServerHandle:
        """Start the server and return a :class:`McpServerHandle`.

        Returns immediately; the server runs in a background thread.
        """
        ...
    def register_handler(self, action_name: str, handler: object) -> None:
        """Register a Python callable as the handler for ``action_name``.

        The callable receives a single Python object converted from the JSON
        params dict (typically a `dict`).
        It must return a JSON-serialisable value.

        Raises:
            TypeError: If ``handler`` is not callable.

        """
        ...
    def has_handler(self, action_name: str) -> bool:
        """Return ``True`` if a handler is registered for ``action_name``."""
        ...
    @property
    def catalog(self) -> str:
        """Debug representation of the SkillCatalog state (total/loaded counts)."""
        ...
    def discover(
        self,
        extra_paths: list[str] | None = None,
        dcc_name: str | None = None,
    ) -> int:
        """Discover skills from standard scan paths.

        Args:
            extra_paths: Additional directories to scan.
            dcc_name: DCC name filter (e.g. ``"maya"``).

        Returns:
            Number of newly discovered skills.

        """
        ...
    def load_skill(self, skill_name: str) -> list[str]:
        """Load a skill by name — registers its tools in the ToolRegistry.

        Returns the list of registered action names.

        Raises:
            ValueError: If the skill is not found.

        """
        ...
    def unload_skill(self, skill_name: str) -> int:
        """Unload a skill — removes its tools from the ToolRegistry.

        Returns the number of actions removed.

        Raises:
            ValueError: If the skill is not loaded.

        """
        ...
    def find_skills(
        self,
        query: str | None = None,
        tags: list[str] | None = None,
        dcc: str | None = None,
    ) -> list[SkillSummary]:
        """Search for skills matching the given criteria.

        Returns a list of skill summary dicts.
        """
        ...
    def list_skills(self, status: str | None = None) -> list[SkillSummary]:
        """List all skills with their load status.

        Args:
            status: Optional filter — ``"loaded"`` or ``"unloaded"``.

        Returns a list of skill summary dicts.

        """
        ...
    def get_skill_info(self, skill_name: str) -> object | None:
        """Get detailed info about a specific skill as a Python dict.

        Returns ``None`` if the skill is not found.
        """
        ...
    def is_loaded(self, skill_name: str) -> bool:
        """Check if a skill is loaded."""
        ...
    def loaded_count(self) -> int:
        """Return the number of loaded skills."""
        ...
    def __repr__(self) -> str: ...

def create_skill_server(
    app_name: str,
    config: McpHttpConfig | None = None,
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> McpHttpServer:
    """Create a pre-configured ``McpHttpServer`` for a specific DCC application.

    This is the recommended entry-point for the **Skills-First** workflow.
    It automatically:

    1. Creates a ``ToolRegistry`` and ``ToolDispatcher``.
    2. Creates a ``SkillCatalog`` wired to the dispatcher.
    3. Discovers skills from **both** env vars (per-app + global):
       - ``DCC_MCP_{APP}_SKILL_PATHS`` — e.g. ``DCC_MCP_MAYA_SKILL_PATHS``
       - ``DCC_MCP_SKILL_PATHS`` — global fallback
    4. Returns a ready-to-start ``McpHttpServer``.

    Args:
        app_name: DCC application name (e.g. ``"maya"``, ``"blender"``).
                  Used to derive the per-app env var and as the MCP server name.
        config:   Optional ``McpHttpConfig``; defaults to port 8765.
        extra_paths: Extra skill directories to scan in addition to env var paths.
        dcc_name: Override the DCC filter for skill scanning (defaults to ``app_name``).

    Example::

        import os
        os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

        from dcc_mcp_core import create_skill_server, McpHttpConfig

        server = create_skill_server("maya", McpHttpConfig(port=8765))
        handle = server.start()
        print(f"Maya MCP server at {handle.mcp_url()}")

    """
    ...

# ── Serialization ──

class SerializeFormat:
    """Supported serialization formats for ``ToolResult``.

    Use ``SerializeFormat.Json`` (default) for human-readable JSON text.
    Use ``SerializeFormat.MsgPack`` for compact binary MessagePack encoding.

    Future formats (CBOR, Bincode …) may be added as new class attributes
    without breaking existing callers.
    """

    Json: SerializeFormat
    """JSON: UTF-8 text, human-readable, widely compatible (default)."""
    MsgPack: SerializeFormat
    """MessagePack: compact binary encoding via rmp-serde."""

    def __eq__(self, other: object) -> bool: ...
    def __repr__(self) -> str: ...

def serialize_result(
    result: ToolResult,
    format: SerializeFormat = ...,  # default: SerializeFormat.Json
) -> str | bytes:
    """Serialize a ``ToolResult`` to a string or bytes.

    Parameters
    ----------
    result:
        The ``ToolResult`` to serialize.
    format:
        Serialization format.  Defaults to ``SerializeFormat.Json``.

    Returns
    -------
    str
        When *format* is ``SerializeFormat.Json``: a UTF-8 JSON string.
    bytes
        When *format* is ``SerializeFormat.MsgPack``: raw MessagePack bytes.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import serialize_result, deserialize_result, SerializeFormat, success_result

        arm = success_result("done", count=3)

        # JSON (default)
        json_str = serialize_result(arm)
        assert isinstance(json_str, str)

        # MessagePack
        msgpack_bytes = serialize_result(arm, SerializeFormat.MsgPack)
        assert isinstance(msgpack_bytes, bytes)

    """
    ...

def deserialize_result(
    data: str | bytes,
    format: SerializeFormat = ...,  # default: SerializeFormat.Json
) -> ToolResult:
    """Deserialize a ``str`` (JSON) or ``bytes`` (MsgPack) into a ``ToolResult``.

    The *format* must match what was used during serialization.

    Parameters
    ----------
    data:
        Serialized data — ``str`` for JSON, ``bytes`` for MessagePack.
    format:
        The format that was used to serialize *data*.
        Defaults to ``SerializeFormat.Json``.

    Raises
    ------
    TypeError
        If *data* is neither ``str`` nor ``bytes``.
    ValueError
        If deserialization fails (corrupt data or format mismatch).

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import serialize_result, deserialize_result, success_result

        original = success_result("render complete", frame_count=240)
        roundtrip = deserialize_result(serialize_result(original))
        assert roundtrip.success
        assert roundtrip.message == "render complete"

    """
    ...
