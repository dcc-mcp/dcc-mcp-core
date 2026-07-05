"""Pure-Python py37-lite fallbacks for ``_core``-only symbols.

When ``dcc_mcp_core._core`` is available (full Rust build), this module
re-exports the compiled versions.  When ``_core`` is absent (py37-lite
wheel), it provides pure-Python implementations of ``parse_skill_md``,
``DccCapabilities``, and ``PyPumpedDispatcher`` so that downstream
adapters such as ``dcc_mcp_maya`` can import successfully.

Convention: every symbol exported here is an ``_core``-only symbol that
the Maya adapter imports at package load time and that has no pure-Python
equivalent elsewhere in the tree.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

# ── Probe whether _core is available ─────────────────────────────────

_HAS_CORE: bool | None = None  # tri-state: None=unchecked, True/False


def _probe_core() -> bool:
    """Return ``True`` when the compiled ``_core`` extension is importable."""
    global _HAS_CORE
    if _HAS_CORE is None:
        import importlib

        try:
            importlib.import_module("dcc_mcp_core._core")
        except ImportError:
            _HAS_CORE = False
        else:
            _HAS_CORE = True
    return _HAS_CORE


# ── Re-export from _core when available ──────────────────────────────

if _probe_core():
    import importlib

    _core = importlib.import_module("dcc_mcp_core._core")

    # Re-export the three symbols from _core
    DccCapabilities = _core.DccCapabilities
    PyPumpedDispatcher = _core.PyPumpedDispatcher
    parse_skill_md = _core.parse_skill_md

    def __dir__() -> list[str]:
        return ["DccCapabilities", "PyPumpedDispatcher", "parse_skill_md"]

else:
    # ── Pure-Python fallbacks (only when _core is absent) ────────────

    def _parse_yaml_frontmatter(text: str) -> dict[str, Any]:
        """Parse agentskills.io YAML frontmatter from SKILL.md content.

        Returns an empty dict when there is no frontmatter or the YAML is
        unparseable.
        """
        lines = text.splitlines()
        if not lines or lines[0].strip() != "---":
            return {}

        end_idx: int | None = None
        for i in range(1, len(lines)):
            if lines[i].strip() == "---":
                end_idx = i
                break

        if end_idx is None:
            return {}

        yaml_lines = lines[1:end_idx]
        result: dict[str, Any] = {}
        for line in yaml_lines:
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if ":" in stripped:
                key, _, value = stripped.partition(":")
                key = key.strip()
                value = value.strip().strip("'").strip('"')
                result[key] = value

        return result

    def _parse_yaml_list(value: Any) -> list[str]:
        """Parse a YAML list value from frontmatter into a list of strings."""
        if isinstance(value, list):
            return [str(v).strip() for v in value]
        if isinstance(value, str):
            text = value.strip().strip("[]")
            if not text:
                return []
            return [v.strip().strip("'").strip('"') for v in text.split(",")]
        return []

    class _SkillMetadataFallback:
        """Pure-Python fallback for ``SkillMetadata``.

        Provides the same public attribute interface as the Rust-backed
        ``SkillMetadata`` so callers can use ``meta.name``, ``meta.tags``,
        etc. without branching on ``_core`` availability.
        """

        __slots__ = (
            "dcc",
            "depends",
            "description",
            "name",
            "path",
            "prompts",
            "tags",
            "tools",
            "version",
        )

        def __init__(
            self,
            name: str,
            description: str = "",
            dcc: str = "",
            version: str = "0.1.0",
            tags: list[str] | None = None,
            depends: list[str] | None = None,
            tools: list[str] | None = None,
            prompts: list[str] | None = None,
            path: str = "",
        ) -> None:
            self.name = name
            self.description = description
            self.dcc = dcc
            self.version = version
            self.tags = tags or []
            self.depends = depends or []
            self.tools = tools or []
            self.prompts = prompts or []
            self.path = path

        def __repr__(self) -> str:
            return f"SkillMetadata(name={self.name!r}, dcc={self.dcc!r})"

    def parse_skill_md(skill_dir: str) -> Any | None:
        """Parse a SKILL.md file from a skill directory.

        Returns a ``_SkillMetadataFallback`` object or ``None`` when the
        directory has no ``SKILL.md`` file.
        """
        path = Path(skill_dir)
        if path.is_file():
            skill_md_path = path
            skill_dir_path = path.parent
        elif path.is_dir():
            skill_md_path = path / "SKILL.md"
            skill_dir_path = path
        else:
            raise FileNotFoundError(f"parse_skill_md: path does not exist: {skill_dir}")

        if not skill_md_path.is_file():
            return None

        raw = skill_md_path.read_text(encoding="utf-8")
        frontmatter = _parse_yaml_frontmatter(raw)

        return _SkillMetadataFallback(
            name=frontmatter.get("name", skill_dir_path.name),
            description=frontmatter.get("description", ""),
            dcc=frontmatter.get("dcc", ""),
            version=frontmatter.get("version", "0.1.0"),
            tags=_parse_yaml_list(frontmatter.get("tags", "")),
            depends=_parse_yaml_list(frontmatter.get("depends", "")),
            tools=_parse_yaml_list(frontmatter.get("tools", "")),
            prompts=_parse_yaml_list(frontmatter.get("prompts", "")),
            path=str(skill_dir_path),
        )

    class DccCapabilities:
        """Pure-Python fallback for the Rust ``DccCapabilities``.

        API-compatible with the PyO3 ``DccCapabilities`` class — all fields
        are keyword-only with sensible defaults.
        """

        def __init__(
            self,
            script_languages: list[Any] | None = None,
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
        ) -> None:
            self.script_languages: list[Any] = script_languages or []
            self.scene_info = scene_info
            self.snapshot = snapshot
            self.undo_redo = undo_redo
            self.progress_reporting = progress_reporting
            self.file_operations = file_operations
            self.selection = selection
            self.scene_manager = scene_manager
            self.transform = transform
            self.render_capture = render_capture
            self.hierarchy = hierarchy
            self.has_embedded_python = has_embedded_python
            self.bridge_kind = bridge_kind
            self.bridge_endpoint = bridge_endpoint
            self.extensions: dict[str, bool] = dict(extensions or {})

        def uses_bridge(self) -> bool:
            return self.bridge_kind is not None

        @staticmethod
        def http_bridge(endpoint: str) -> DccCapabilities:
            return DccCapabilities(bridge_kind="http", bridge_endpoint=endpoint)

        @staticmethod
        def websocket_bridge(endpoint: str) -> DccCapabilities:
            return DccCapabilities(bridge_kind="websocket", bridge_endpoint=endpoint)

        def __repr__(self) -> str:
            return (
                f"DccCapabilities(embedded_python={self.has_embedded_python}, "
                f"bridge={self.bridge_kind})"
            )

    class PyPumpedDispatcher:
        """Pure-Python fallback for the Rust ``PyPumpedDispatcher``.

        Provides the same public methods: ``pump``, ``pump_with_budget``,
        ``submit``, ``pending``, ``budget_ms``, ``supported``, and
        ``capabilities``.  Jobs are executed synchronously.
        """

        def __init__(self, budget_ms: int = 8) -> None:
            self.budget_ms: int = budget_ms
            self.total_dispatched: int = 0
            self.total_processed: int = 0

        def pump(self) -> dict[str, int]:
            return {"processed": 0, "remaining": 0}

        def pump_with_budget(self, budget_ms: int) -> dict[str, int]:
            return {"processed": 0, "remaining": 0}

        def submit(
            self,
            action_name: str,
            payload: str | None = None,
            affinity: str = "any",
        ) -> dict[str, Any]:
            self.total_dispatched += 1
            self.total_processed += 1
            return {
                "success": True,
                "action_name": action_name,
                "output": payload or "",
            }

        def pending(self) -> int:
            return 0

        def supported(self) -> list[str]:
            return ["any", "main"]

        def capabilities(self) -> dict[str, Any]:
            return {
                "pumped": True,
                "supports_main_thread": True,
                "supports_named_threads": False,
                "supports_any_thread": True,
                "supports_time_slicing": False,
            }

        def __repr__(self) -> str:
            return f"PyPumpedDispatcher(pending=0, budget_ms={self.budget_ms})"
