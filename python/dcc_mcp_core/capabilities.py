"""Generic DCC capability manifest for gateway indexing.

The gateway capability index wants a **compact** view of what each DCC
instance can do, without paying the cost of exploding every unloaded skill's
full MCP schema into ``tools/list``. This module provides that compact record
set for any DCC adapter.

SOLID notes
-----------
* :class:`CapabilityRecord` — value object; no behaviour.
* :class:`CapabilityManifestBuilder` — single responsibility: project the
  live :class:`SkillCatalog` into a list of :class:`CapabilityRecord`.
* :func:`build_manifest_payload` — pure function turning a builder output into
  the final manifest dict (adds instance metadata + capability flags).
* :func:`register_capability_mcp_tool` — side-effect: registers the
  ``dcc_capability_manifest`` MCP tool so agents can fetch the manifest
  without going through the gateway REST route.
"""

from __future__ import annotations

from dataclasses import asdict
from dataclasses import dataclass
from dataclasses import field
import logging
import re
from typing import Any
from typing import Callable

from dcc_mcp_core import json_dumps

logger = logging.getLogger(__name__)

__all__ = [
    "CapabilityManifestBuilder",
    "CapabilityRecord",
    "build_manifest_payload",
    "register_capability_mcp_tool",
]

_SKILL_STUB_PREFIX = "__skill__"
_GROUP_STUB_PREFIX = "__group__"


@dataclass(frozen=True)
class CapabilityRecord:
    """Compact per-action record."""

    tool_slug: str
    backend_tool: str
    skill_name: str
    summary: str
    loaded: bool
    tags: list[str] = field(default_factory=list)
    execution: str | None = None
    affinity: str | None = None
    timeout_hint_secs: int | None = None
    has_schema: bool = False
    group: str | None = None
    load_hint: dict[str, Any] = field(default_factory=dict)
    requires_load_skill: bool = False
    callable_id: str | None = None

    def to_dict(self) -> dict[str, Any]:
        """Plain dict form suitable for JSON serialisation."""
        payload = asdict(self)
        out = {k: v for k, v in payload.items() if v not in (None, [], "", {})}
        if out.get("requires_load_skill") is False:
            out.pop("requires_load_skill", None)
        if out.get("callable_id") == out.get("backend_tool"):
            out.pop("callable_id", None)
        return out


class CapabilityManifestBuilder:
    """Turn live catalog state into a list of :class:`CapabilityRecord`."""

    def __init__(
        self,
        dcc_name: str,
        *,
        skill_lister: Callable[[], list[Any]] | None = None,
        action_lister: Callable[[], list[Any]] | None = None,
        is_loaded: Callable[[str], bool] | None = None,
        skill_info_lister: Callable[[str], Any] | None = None,
    ) -> None:
        self._dcc_name = dcc_name
        self._skill_lister = skill_lister
        self._action_lister = action_lister
        self._is_loaded = is_loaded
        self._skill_info_lister = skill_info_lister

    def build(self) -> list[CapabilityRecord]:
        """Return records for every non-stub action in the catalog."""
        skills_by_name = self._collect_skill_info()
        records: list[CapabilityRecord] = []
        covered_tools: set[str] = set()

        for action in self._collect_actions():
            record = self._project_action(action, skills_by_name)
            if record is None:
                continue
            records.append(record)
            covered_tools.add(record.backend_tool)

        for skill_name, skill_info in skills_by_name.items():
            if self._is_loaded_safe(skill_name):
                continue
            for tool in self._collect_skill_tools(skill_name, skill_info):
                record = self._project_unloaded_action(
                    skill_name=skill_name,
                    tool=tool,
                    skill_info=skill_info,
                )
                if record is None:
                    continue
                if record.backend_tool in covered_tools:
                    continue
                records.append(record)
                covered_tools.add(record.backend_tool)

        return records

    def _collect_skill_info(self) -> dict[str, dict[str, Any]]:
        skills: dict[str, dict[str, Any]] = {}
        if self._skill_lister is None:
            return skills
        try:
            raw = self._skill_lister() or []
        except Exception as exc:
            logger.debug("capability manifest: skill_lister failed: %s", exc)
            return skills

        for item in raw:
            entry = _as_dict(item)
            name = entry.get("name") or entry.get("skill_name")
            if not name:
                continue
            skills[name] = entry
        return skills

    def _collect_actions(self) -> list[dict[str, Any]]:
        if self._action_lister is None:
            return []
        try:
            raw = self._action_lister() or []
        except Exception as exc:
            logger.debug("capability manifest: action_lister failed: %s", exc)
            return []
        return [_as_dict(a) for a in raw if a]

    def _project_action(
        self,
        action: dict[str, Any],
        skills_by_name: dict[str, dict[str, Any]],
    ) -> CapabilityRecord | None:
        name = action.get("name") or action.get("tool")
        if not name or _is_stub(name):
            return None

        skill_name = action.get("skill") or action.get("skill_name") or _derive_skill(name)
        skill_info = skills_by_name.get(skill_name or "", {})
        summary = _truncate(
            _first_nonempty(
                action.get("summary"),
                action.get("description"),
                skill_info.get("summary"),
                skill_info.get("description"),
                "",
            ),
            200,
        )

        tags: list[str] = []
        for source in (
            action.get("tags"),
            skill_info.get("tags"),
            [action.get("category")],
            [action.get("group")],
        ):
            tags.extend(_as_str_list(source))
        tags = sorted({t for t in tags if t})

        loaded = self._is_loaded_safe(skill_name) if skill_name else False
        execution = _maybe_str(action.get("execution"))
        affinity = _maybe_str(action.get("affinity"))
        timeout_hint = _maybe_int(action.get("timeout_hint_secs"))
        has_schema = bool(action.get("input_schema") or action.get("inputSchema"))

        slug = _slugify_tool_slug(self._dcc_name, name)
        return CapabilityRecord(
            tool_slug=slug,
            backend_tool=name,
            skill_name=skill_name or "",
            summary=summary or "",
            loaded=bool(loaded),
            tags=tags,
            execution=execution,
            affinity=affinity,
            timeout_hint_secs=timeout_hint,
            has_schema=has_schema,
            group=_maybe_str(action.get("group")),
            callable_id=name,
        )

    def _collect_skill_tools(self, skill_name: str, skill_info: dict[str, Any]) -> list[dict[str, Any]]:
        tools = skill_info.get("tools") or skill_info.get("actions")
        if isinstance(tools, list) and tools and isinstance(tools[0], dict):
            return [dict(t) for t in tools]

        if self._skill_info_lister is None:
            return []
        try:
            info = self._skill_info_lister(skill_name)
        except Exception as exc:
            logger.debug("capability manifest: skill_info(%r) raised %s", skill_name, exc)
            return []
        if not info:
            return []
        entry = _as_dict(info)
        collected = entry.get("tools") or entry.get("actions") or []
        return [_as_dict(t) for t in collected if t]

    def _project_unloaded_action(
        self,
        *,
        skill_name: str,
        tool: dict[str, Any],
        skill_info: dict[str, Any],
    ) -> CapabilityRecord | None:
        tool_name = tool.get("name") or tool.get("tool")
        if not tool_name or _is_stub(tool_name):
            return None

        backend_tool = "{}__{}".format(skill_name.replace("-", "_"), tool_name)
        summary = _truncate(_first_nonempty(tool.get("summary"), tool.get("description"), ""), 160)

        tags: list[str] = []
        for source in (
            tool.get("tags"),
            skill_info.get("tags"),
            [tool.get("category")],
            [tool.get("group")],
        ):
            tags.extend(_as_str_list(source))
        tags = sorted({t for t in tags if t})

        schema = tool.get("input_schema") or tool.get("inputSchema")
        has_schema = bool(schema) and not _is_stub(tool_name)

        return CapabilityRecord(
            tool_slug=_slugify_tool_slug(self._dcc_name, backend_tool),
            backend_tool=backend_tool,
            skill_name=skill_name,
            summary=summary,
            loaded=False,
            tags=tags,
            execution=_maybe_str(tool.get("execution")),
            affinity=_maybe_str(tool.get("affinity")),
            timeout_hint_secs=_maybe_int(tool.get("timeout_hint_secs")),
            has_schema=has_schema,
            group=_maybe_str(tool.get("group")),
            requires_load_skill=True,
            load_hint={"tool": "load_skill", "arguments": {"skill_name": skill_name}},
            callable_id=backend_tool,
        )

    def _is_loaded_safe(self, skill_name: str) -> bool:
        if self._is_loaded is None:
            return False
        try:
            return bool(self._is_loaded(skill_name))
        except Exception as exc:
            logger.debug("capability manifest: is_loaded(%r) raised %s", skill_name, exc)
            return False


def build_manifest_payload(
    records: list[CapabilityRecord],
    *,
    dcc_name: str,
    dcc_version: str | None = None,
    scene: str | None = None,
    display_name: str | None = None,
    instance_id: str | None = None,
    documents: list[str] | None = None,
    extra_metadata: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Wrap records with instance metadata for gateway ingestion."""
    loaded_records = [r for r in records if r.loaded]
    unloaded_records = [r for r in records if not r.loaded]
    manifest = {
        "schema_version": "1",
        "dcc_type": dcc_name,
        "metadata": {
            "instance_id": instance_id,
            "dcc_version": dcc_version,
            "scene": scene,
            "display_name": display_name,
            "documents": documents or [],
        },
        "totals": {
            "actions": len(records),
            "loaded_actions": len(loaded_records),
            "unloaded_actions": len(unloaded_records),
            "skills": len({r.skill_name for r in records if r.skill_name}),
            "loaded_skills": len({r.skill_name for r in loaded_records if r.skill_name}),
            "unloaded_skills": len({r.skill_name for r in unloaded_records if r.skill_name}),
        },
        "capabilities": [r.to_dict() for r in records],
    }
    if extra_metadata:
        manifest["metadata"].update({k: v for k, v in extra_metadata.items() if v is not None})
    manifest["metadata"] = {k: v for k, v in manifest["metadata"].items() if v not in (None, "")}
    return manifest


def register_capability_mcp_tool(
    server: Any,
    *,
    builder: CapabilityManifestBuilder,
    dcc_name: str,
    metadata_provider: Callable[[], dict[str, Any]] | None = None,
) -> bool:
    """Register ``dcc_capability_manifest`` as an MCP tool."""
    inner = getattr(server, "_server", None)
    if inner is None:
        logger.debug("capability manifest: server has no inner _server; skipping")
        return False

    tool_name = "dcc_capability_manifest"
    description = (
        f"Return a compact {dcc_name} capability manifest listing every discoverable "
        "action (loaded and unloaded), tagged by skill/group. Prefer this over "
        "tools/list when the caller only needs to decide which skill to load."
    )
    input_schema = {
        "type": "object",
        "properties": {
            "loaded_only": {
                "type": "boolean",
                "description": "When true, omit records for unloaded skills.",
                "default": False,
            },
        },
        "additionalProperties": False,
    }

    def handler(params: dict[str, Any]) -> dict[str, Any]:
        records = builder.build()
        if params.get("loaded_only"):
            records = [r for r in records if r.loaded]

        meta = metadata_provider() if metadata_provider else {}
        instance_id = meta.get("instance_id") or _extract_instance_id(server)
        payload = build_manifest_payload(
            records,
            dcc_name=dcc_name,
            dcc_version=meta.get("version") or meta.get("dcc_version"),
            scene=meta.get("scene"),
            display_name=meta.get("display_name"),
            instance_id=instance_id,
            documents=meta.get("documents"),
        )
        return {
            "success": True,
            "message": f"{dcc_name} capability manifest",
            "context": payload,
        }

    registry = getattr(inner, "registry", None)
    if registry is None or not hasattr(registry, "register"):
        return False

    try:
        registry.register(
            tool_name,
            description=description,
            category="dcc",
            tags=["capability", "manifest", "dcc", dcc_name.lower()],
            dcc=dcc_name,
            input_schema=json_dumps(input_schema),
            skill_name="dcc-adapter",
            group="capability",
            execution="sync",
            thread_affinity="any",
            enabled=True,
        )
        inner.register_handler(tool_name, handler)
        return True
    except Exception as exc:
        logger.debug("capability manifest: registration failed: %s", exc)
        return False


_SLUG_CLEAN_RE = re.compile(r"[^A-Za-z0-9_]")


def _slugify_tool_slug(dcc_name: str, tool_name: str) -> str:
    clean_dcc = _SLUG_CLEAN_RE.sub("_", dcc_name)
    clean_tool = _SLUG_CLEAN_RE.sub("_", tool_name)
    return f"{clean_dcc}.instance.{clean_tool}"


def _is_stub(name: str) -> bool:
    return name.startswith(_SKILL_STUB_PREFIX) or name.startswith(_GROUP_STUB_PREFIX)


def _derive_skill(tool_name: str) -> str | None:
    if "__" not in tool_name:
        return None
    return tool_name.split("__", 1)[0].replace("_", "-")


def _as_dict(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    to_dict = getattr(value, "to_dict", None)
    if callable(to_dict):
        try:
            result = to_dict()
            if isinstance(result, dict):
                return result
        except (TypeError, ValueError, AttributeError):
            pass
    out: dict[str, Any] = {}
    for attr in dir(value):
        if attr.startswith("_"):
            continue
        try:
            candidate = getattr(value, attr)
        except (AttributeError, TypeError):
            continue
        if callable(candidate):
            continue
        out[attr] = candidate
    return out


def _as_str_list(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        return [value]
    if isinstance(value, (list, tuple, set, frozenset)):
        return [str(v) for v in value if v]
    return [str(value)]


def _first_nonempty(*values: Any) -> str:
    for v in values:
        if v:
            return str(v)
    return ""


def _truncate(text: str, limit: int) -> str:
    if len(text) <= limit:
        return text
    return text[: limit - 1].rstrip() + "…"


def _maybe_str(value: Any) -> str | None:
    if value is None or value == "":
        return None
    return str(value)


def _maybe_int(value: Any) -> int | None:
    if value in (None, ""):
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _extract_instance_id(server: Any) -> str | None:
    for chain in (
        ("instance_id",),
        ("_config", "instance_id"),
        ("_server", "instance_id"),
        ("_handle", "instance_id"),
    ):
        target = server
        try:
            for part in chain:
                target = getattr(target, part)
            if target:
                return str(target)
        except AttributeError:
            continue
    return None
