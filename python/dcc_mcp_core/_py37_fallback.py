"""Pure-Python py37-lite fallbacks for ``_core``-only symbols.

When ``dcc_mcp_core._core`` is available (full Rust build), this module
re-exports the compiled versions.  When ``_core`` is absent (py37-lite
wheel), it provides pure-Python implementations of ``parse_skill_md``,
``DccCapabilities``, ``PyPumpedDispatcher``, ``scan_and_load_strict``,
``GuiExecutableHint``, ``is_gui_executable``, and ``correct_python_executable``
so that downstream adapters such as ``dcc_mcp_maya`` can import successfully.

Also provides ``ReadinessProbe`` so adapter readiness binders can start
without importing the compiled extension.

Convention: every symbol exported here is an ``_core``-only symbol that
the Maya adapter imports at package load time and that has no pure-Python
equivalent elsewhere in the tree.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any
from typing import Sequence

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

    # Re-export py37-lite symbols from _core when the extension is present.
    DccCapabilities = _core.DccCapabilities
    PyPumpedDispatcher = _core.PyPumpedDispatcher
    GuiExecutableHint = _core.GuiExecutableHint
    parse_skill_md = _core.parse_skill_md
    scan_and_load_strict = _core.scan_and_load_strict
    correct_python_executable = _core.correct_python_executable
    is_gui_executable = _core.is_gui_executable
    ReadinessProbe = _core.ReadinessProbe

    def __dir__() -> list[str]:
        return [
            "DccCapabilities",
            "GuiExecutableHint",
            "PyPumpedDispatcher",
            "ReadinessProbe",
            "correct_python_executable",
            "is_gui_executable",
            "parse_skill_md",
            "scan_and_load_strict",
        ]

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
            return f"DccCapabilities(embedded_python={self.has_embedded_python}, bridge={self.bridge_kind})"

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

    def _discover_skill_directories(extra_paths: Sequence[str] | None) -> list[str]:
        discovered: list[str] = []
        seen: set[str] = set()
        for raw in extra_paths or ():
            root = Path(raw)
            if not root.is_dir():
                continue
            if (root / "SKILL.md").is_file():
                candidates = [root]
            else:
                candidates = [child for child in root.iterdir() if child.is_dir()]
            for candidate in sorted(candidates, key=lambda path: path.name):
                if not (candidate / "SKILL.md").is_file():
                    continue
                key = str(candidate.resolve())
                if key in seen:
                    continue
                seen.add(key)
                discovered.append(str(candidate))
        return discovered

    def _missing_explicit_child_skill_md(extra_paths: Sequence[str] | None) -> list[str]:
        missing: list[str] = []
        for raw in extra_paths or ():
            root = Path(raw)
            if not root.is_dir() or (root / "SKILL.md").is_file():
                continue
            for child in root.iterdir():
                if child.is_dir() and not (child / "SKILL.md").is_file():
                    missing.append(str(child))
        return missing

    def _resolve_dependencies_ordered(skills: list[Any]) -> list[Any]:
        if not skills:
            return []
        by_name = {skill.name: skill for skill in skills}
        for skill in skills:
            for dep in skill.depends:
                if dep not in by_name:
                    raise ValueError(
                        f"Skill '{skill.name}' depends on '{dep}', but it was not found. "
                        f"Ensure '{dep}' is available in one of the skill search paths."
                    )
        in_degree = {skill.name: 0 for skill in skills}
        dependents = {skill.name: [] for skill in skills}
        for skill in skills:
            for dep in skill.depends:
                if dep in by_name:
                    in_degree[skill.name] += 1
                    dependents[dep].append(skill.name)
        queue = sorted(name for name, degree in in_degree.items() if degree == 0)
        ordered_names: list[str] = []
        while queue:
            name = queue.pop(0)
            ordered_names.append(name)
            for child in dependents[name]:
                in_degree[child] -= 1
                if in_degree[child] == 0:
                    queue.append(child)
            queue.sort()
        if len(ordered_names) != len(skills):
            raise ValueError("Circular dependency detected")
        return [by_name[name] for name in ordered_names]

    def scan_and_load_strict(
        extra_paths: Sequence[str] | None = None,
        dcc_name: str | None = None,
    ) -> tuple[list[Any], list[str]]:
        dirs = _discover_skill_directories(extra_paths)
        skills: list[Any] = []
        skipped: list[str] = []
        for dir_str in dirs:
            lines = (Path(dir_str) / "SKILL.md").read_text(encoding="utf-8").splitlines()
            if not lines or lines[0].strip() != "---":
                skipped.append(dir_str)
                continue
            try:
                meta = parse_skill_md(dir_str)
            except Exception:
                skipped.append(dir_str)
                continue
            if meta is None:
                skipped.append(dir_str)
                continue
            if dcc_name and meta.dcc and meta.dcc != dcc_name:
                continue
            skills.append(meta)

        ordered = _resolve_dependencies_ordered(skills)
        for dir_str in _missing_explicit_child_skill_md(extra_paths):
            if dir_str not in skipped:
                skipped.append(dir_str)
        if skipped:
            raise ValueError(
                "Strict scan rejected {} directory/directories that failed to load: {}. "
                "Inspect the SKILL.md files for missing/invalid YAML frontmatter or "
                "re-run with scan_and_load_lenient to tolerate them.".format(
                    len(skipped),
                    ", ".join(skipped),
                )
            )
        return ordered, []

    # ── DCC GUI executable detection (issue #524 / maya#125) ─────────

    _GUI_BINARY_ROWS: tuple[tuple[tuple[str, ...], str, tuple[str, ...]], ...] = (
        (("maya", "maya.bin"), "maya", ("mayapy",)),
        (("houdini", "houdinifx", "houdinicore"), "houdini", ("hython",)),
        (("unrealeditor",), "unreal", ("unrealeditor-cmd",)),
        (("blender",), "blender", ()),
        (("3dsmax",), "3dsmax", ()),
        (("nuke", "nukestudio"), "nuke", ()),
        (("modo",), "modo", ()),
        (("motionbuilder",), "motionbuilder", ()),
        (("cinema4d", "c4d"), "c4d", ()),
        (("katana",), "katana", ()),
    )

    def _lowercase_stem(path: Path) -> str | None:
        stem = path.stem
        if not stem:
            return None
        return stem.lower()

    def _real_case(parent: Path, candidate: Path) -> Path | None:
        target_name = candidate.name.lower()
        try:
            entries = [entry.name for entry in parent.iterdir()]
        except OSError:
            return None
        for entry in entries:
            if entry.lower() == target_name:
                return parent / entry
        return None

    def _locate_sibling(gui_path: Path, stems: Sequence[str]) -> Path | None:
        if not stems:
            return None
        parent = gui_path.parent
        if not parent.is_dir():
            return None
        extension = gui_path.suffix
        for stem in stems:
            candidate = parent / stem
            if extension:
                candidate = candidate.with_suffix(extension)
            if candidate.exists():
                return _real_case(parent, candidate) or candidate
            found = _real_case(parent, candidate)
            if found is not None:
                return found
        return None

    class GuiExecutableHint:
        """Pure-Python fallback for the Rust ``GuiExecutableHint``."""

        __slots__ = ("dcc_kind", "gui_path", "recommended_replacement")

        def __init__(
            self,
            gui_path: Path,
            dcc_kind: str,
            recommended_replacement: Path | None = None,
        ) -> None:
            self.gui_path = gui_path
            self.dcc_kind = dcc_kind
            self.recommended_replacement = recommended_replacement

    def is_gui_executable(path: str) -> GuiExecutableHint | None:
        """Return a hint when ``path`` looks like a known DCC GUI binary."""
        probe = Path(path)
        stem = _lowercase_stem(probe)
        if stem is None:
            return None
        for stems, dcc_kind, sibling_stems in _GUI_BINARY_ROWS:
            if stem in stems:
                replacement = _locate_sibling(probe, sibling_stems)
                return GuiExecutableHint(
                    gui_path=probe,
                    dcc_kind=dcc_kind,
                    recommended_replacement=replacement,
                )
        return None

    def correct_python_executable(path: str) -> Path:
        """Return a headless-Python sibling when one exists, else ``path``."""
        probe = Path(path)
        hint = is_gui_executable(path)
        if hint is not None and hint.recommended_replacement is not None:
            return hint.recommended_replacement
        return probe

    class ReadinessProbe:
        """Pure-Python fallback for the Rust ``ReadinessProbe``.

        Matches ``StaticReadiness`` defaults and ``is_ready()`` semantics from
        ``dcc_mcp_skill_rest::readiness``.
        """

        def __init__(
            self,
            *,
            process: bool = True,
            dcc: bool = False,
            skill_catalog: bool = True,
            dispatcher: bool = False,
            host_execution_bridge: bool = False,
            main_thread_executor: bool = False,
        ) -> None:
            self._process = process
            self._dcc = dcc
            self._skill_catalog = skill_catalog
            self._dispatcher = dispatcher
            self._host_execution_bridge = host_execution_bridge
            self._main_thread_executor = main_thread_executor

        @staticmethod
        def fully_ready() -> ReadinessProbe:
            return ReadinessProbe(
                process=True,
                dcc=True,
                skill_catalog=True,
                dispatcher=True,
                host_execution_bridge=True,
                main_thread_executor=True,
            )

        def set_dispatcher_ready(self, ready: bool) -> None:
            self._dispatcher = bool(ready)

        def set_dcc_ready(self, ready: bool) -> None:
            self._dcc = bool(ready)

        def set_skill_catalog_ready(self, ready: bool) -> None:
            self._skill_catalog = bool(ready)

        def set_host_execution_bridge_ready(self, ready: bool) -> None:
            self._host_execution_bridge = bool(ready)

        def set_main_thread_executor_ready(self, ready: bool) -> None:
            self._main_thread_executor = bool(ready)

        def is_ready(self) -> bool:
            return self._process and self._dcc and self._skill_catalog and self._dispatcher

        def report(self) -> dict[str, bool]:
            return {
                "process": self._process,
                "dcc": self._dcc,
                "skill_catalog": self._skill_catalog,
                "dispatcher": self._dispatcher,
                "host_execution_bridge": self._host_execution_bridge,
                "main_thread_executor": self._main_thread_executor,
            }

        def __repr__(self) -> str:
            report = self.report()
            return (
                "ReadinessProbe(process={process}, dcc={dcc}, skill_catalog={skill_catalog}, "
                "dispatcher={dispatcher}, host_execution_bridge={host_execution_bridge}, "
                "main_thread_executor={main_thread_executor})"
            ).format(**report)
