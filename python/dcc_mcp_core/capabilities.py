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
import datetime
import logging
import re
from threading import RLock
import time
from typing import Any
from typing import Callable
from urllib.parse import quote

from dcc_mcp_core import json_dumps

logger = logging.getLogger(__name__)

__all__ = [
    "HOST_CAPABILITIES_SCHEMA_ID",
    "HOST_CAPABILITIES_SCHEMA_VERSION",
    "PROBE_RESULTS",
    "CapabilityManifestBuilder",
    "CapabilityRecord",
    "HostCapabilityCache",
    "HostCapabilityValidationError",
    "build_host_capabilities",
    "build_manifest_payload",
    "make_host_capability",
    "register_capability_mcp_tool",
    "register_host_capability_tool",
    "summarize_host_capabilities",
    "validate_host_capabilities",
]

_SKILL_STUB_PREFIX = "__skill__"
_GROUP_STUB_PREFIX = "__group__"
_MAX_TAGS_PER_RECORD = 8


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
            120,
        )

        tags: list[str] = []
        for source in (
            action.get("tags"),
            skill_info.get("tags"),
            [action.get("category")],
            [action.get("group")],
        ):
            tags.extend(_as_str_list(source))
        tags = sorted({t for t in tags if t})[:_MAX_TAGS_PER_RECORD]

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
        summary = _truncate(_first_nonempty(tool.get("summary"), tool.get("description"), ""), 120)

        tags: list[str] = []
        for source in (
            tool.get("tags"),
            skill_info.get("tags"),
            [tool.get("category")],
            [tool.get("group")],
        ):
            tags.extend(_as_str_list(source))
        tags = sorted({t for t in tags if t})[:_MAX_TAGS_PER_RECORD]

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


# ---------------------------------------------------------------------------
# Probed host capabilities (issue #2867)
# ---------------------------------------------------------------------------
#
# ``CapabilityRecord`` above answers "which actions exist". The records below
# answer a different question: "which host features actually work on this
# instance, and what proves it". They are deliberately separate from
# ``McpHttpConfig.declared_capabilities``: a declaration says what the adapter
# *claims*, a probe result says what the host actually *did*.
#
# The contract is published as
# ``python/dcc_mcp_core/schemas/host-capabilities-v1.schema.json`` and is
# validated here in pure Python so embedded DCC interpreters can check a
# manifest without the native extension.

HOST_CAPABILITIES_SCHEMA_ID = (
    "https://dcc-mcp.github.io/dcc-mcp-core/schemas/host-capabilities-v1.schema.json"
)
HOST_CAPABILITIES_SCHEMA_VERSION = 1

_SCHEMA_HOST_CAPABILITIES = "dcc-mcp/host-capabilities"

PROBE_PASS = "PASS"
PROBE_FAIL = "FAIL"
PROBE_UNSUPPORTED = "UNSUPPORTED"
PROBE_NOT_RUN = "NOT_RUN"

PROBE_RESULTS = (PROBE_PASS, PROBE_FAIL, PROBE_UNSUPPORTED, PROBE_NOT_RUN)

_CAPABILITY_KEY_RE = re.compile(r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$")

_MAX_CAPABILITY_KEY_CHARS = 96
_MAX_MANIFEST_VERSION_CHARS = 128
_MAX_REASON_CHARS = 512
_MAX_EVIDENCE_CHARS = 512
_MAX_PROBED_AT_CHARS = 64
_MAX_SUPPORTED_CHARS = 64
_MAX_CAPABILITIES = 256
_SUPPORTED_VERSION_RE = re.compile(
    r"^[0-9]+(?:\.[0-9]+)*(?:[+._-][0-9A-Za-z][0-9A-Za-z.+_-]*)?$"
)
_FALSE_LIKE_SUPPORT_VALUES = frozenset(
    {"false", "no", "off", "none", "null", "n/a", "na", "0", "-"}
)


def _hc_supported_schema() -> dict[str, Any]:
    """Return the exact ``supported`` schema used by the published contract."""
    false_like = sorted(_FALSE_LIKE_SUPPORT_VALUES)
    return {
        "description": (
            "true, false, or a bounded numeric version/enum string; "
            "false-like strings are not support claims."
        ),
        "oneOf": [
            {"type": "boolean"},
            {
                "type": "string",
                "minLength": 1,
                "maxLength": _MAX_SUPPORTED_CHARS,
                "pattern": _SUPPORTED_VERSION_RE.pattern,
                "not": {"enum": false_like},
            },
        ],
    }


def _hc_timestamp_schema() -> dict[str, Any]:
    """Return the exact ``probed_at`` schema used by the published contract."""
    year = r"(?!0000)[0-9]{4}"
    leap_year = (
        r"(?!0000)(?:[0-9]{2}(?:0[48]|[2468][048]|[13579][26])|"
        r"(?:0[48]|[2468][048]|[13579][26])00)"
    )
    time_and_zone = (
        r"[Tt](?:[01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]"
        r"(?:\.[0-9]+)?(?:[Zz]|[+-](?:0[0-9]|1[0-9]|2[0-3]):[0-5][0-9])"
        r"(?![\s\S])"
    )
    # Four mutually exclusive branches cover 29 February in leap years,
    # shorter February days, 31-day months, and 30-day months. This keeps the
    # JSON Schema self-contained because common jsonschema installations do
    # not register a semantic date-time checker.
    branches = (
        f"^{leap_year}-02-(?:0[1-9]|1[0-9]|2[0-9]){time_and_zone}",
        f"^(?!{leap_year}){year}-02-(?:0[1-9]|1[0-9]|2[0-8]){time_and_zone}",
        f"^{year}-(?:0[13578]|1[02])-(?:0[1-9]|[12][0-9]|3[01]){time_and_zone}",
        f"^{year}-(?:0[469]|11)-(?:0[1-9]|[12][0-9]|30){time_and_zone}",
    )
    pattern = "(?:" + "|".join(branches) + ")"
    return {
        "type": "string",
        "format": "date-time",
        "maxLength": _MAX_PROBED_AT_CHARS,
        "pattern": pattern,
    }


_HC_TIMESTAMP_RE = re.compile(_hc_timestamp_schema()["pattern"])


def _hc_consistency_rules() -> list[dict[str, Any]]:
    """Return the conditional rules shared with the JSON Schema document."""
    claimed = {"properties": {"supported": {"not": {"enum": [False]}}}}
    unclaimed = {"properties": {"supported": {"enum": [False]}}}
    declared = {
        "required": ["declaration_only"],
        "properties": {"declaration_only": {"const": True}},
    }
    not_declared = {"not": declared}
    return [
        {
            "if": declared,
            "then": {
                "properties": {"probe_result": {"not": {"enum": [PROBE_PASS]}}}
            },
        },
        {
            "if": {
                "allOf": [
                    claimed,
                    not_declared,
                    {
                        "properties": {
                            "probe_result": {"enum": [PROBE_FAIL, PROBE_UNSUPPORTED]}
                        }
                    },
                ]
            },
            "then": {"not": {}},
        },
        {
            "if": {
                "allOf": [
                    unclaimed,
                    not_declared,
                    {"properties": {"probe_result": {"enum": [PROBE_PASS]}}},
                ]
            },
            "then": {"not": {}},
        },
        {
            "if": {
                "allOf": [
                    claimed,
                    not_declared,
                    {"properties": {"probe_result": {"const": PROBE_NOT_RUN}}},
                ]
            },
            "then": {"required": ["reason"]},
        },
        {
            "if": {"allOf": [unclaimed, not_declared]},
            "then": {"required": ["reason"]},
        },
        {"if": declared, "then": {"required": ["reason"]}},
    ]


_MANIFEST_FIELDS = frozenset(
    {
        "schema_name",
        "schema_version",
        "dcc_type",
        "instance_id",
        "host_version",
        "adapter_version",
        "probed_at",
        "capabilities",
    }
)

_CAPABILITY_FIELDS = frozenset(
    {
        "key",
        "supported",
        "reason",
        "probe_result",
        "probed_at",
        "evidence_ref",
        "declaration_only",
    }
)


class HostCapabilityValidationError(ValueError):
    """Raised when a host capability manifest violates the v1 contract."""


def _hc_text(
    value: Any,
    field_name: str,
    *,
    max_chars: int,
    optional: bool = False,
) -> str | None:
    if value is None:
        if optional:
            return None
        raise HostCapabilityValidationError(f"{field_name} is required")
    if not isinstance(value, str):
        raise HostCapabilityValidationError(f"{field_name} must be a string")
    text = value.strip()
    if not text:
        if optional:
            return None
        raise HostCapabilityValidationError(f"{field_name} must not be empty")
    if len(text) > max_chars:
        raise HostCapabilityValidationError(
            f"{field_name} exceeds {max_chars} characters"
        )
    return text


def _hc_supported(value: Any, field_name: str) -> Any:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        text = value.strip()
        if not text:
            raise HostCapabilityValidationError(f"{field_name} must not be empty")
        if len(text) > _MAX_SUPPORTED_CHARS:
            raise HostCapabilityValidationError(
                f"{field_name} exceeds {_MAX_SUPPORTED_CHARS} characters"
            )
        lowered = text.lower()
        if lowered in _FALSE_LIKE_SUPPORT_VALUES:
            raise HostCapabilityValidationError(
                f"{field_name} must not use a false-like version string"
            )
        if not _SUPPORTED_VERSION_RE.match(text):
            raise HostCapabilityValidationError(
                f"{field_name} must be a boolean or a version string"
            )
        return text
    raise HostCapabilityValidationError(
        f"{field_name} must be a boolean or a bounded version string"
    )


def _hc_is_supported(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    return bool(value)


def _hc_timestamp(value: Any, field_name: str, *, optional: bool = False) -> str | None:
    text = _hc_text(
        value,
        field_name,
        max_chars=_MAX_PROBED_AT_CHARS,
        optional=optional,
    )
    if text is None:
        return None
    if not _HC_TIMESTAMP_RE.match(text):
        raise HostCapabilityValidationError(
            f"{field_name} must be an RFC 3339 date-time with a timezone"
        )
    normalized = text[:-1] + "+00:00" if text[-1:] in ("Z", "z") else text
    try:
        parsed = datetime.datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise HostCapabilityValidationError(
            f"{field_name} must be an RFC 3339 date-time with a timezone"
        ) from exc
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise HostCapabilityValidationError(
            f"{field_name} must be an RFC 3339 date-time with a timezone"
        )
    return text


def _now_utc_iso() -> str:
    moment = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
    return moment.isoformat().replace("+00:00", "Z")


def _validate_host_capability(record: Any, index: int) -> dict[str, Any]:
    path = f"capabilities[{index}]"
    if not isinstance(record, dict):
        raise HostCapabilityValidationError(f"{path} must be an object")

    unknown = sorted(set(record) - _CAPABILITY_FIELDS)
    if unknown:
        raise HostCapabilityValidationError(
            f"{path} has unknown field(s): {', '.join(unknown)}"
        )

    key = _hc_text(
        record.get("key"),
        f"{path}.key",
        max_chars=_MAX_CAPABILITY_KEY_CHARS,
    )
    assert key is not None  # required above
    if not _CAPABILITY_KEY_RE.match(key):
        raise HostCapabilityValidationError(
            f"{path}.key must be a namespaced lower-case key such as render.lpe"
        )

    supported = _hc_supported(record.get("supported"), f"{path}.supported")
    probe_result = record.get("probe_result")
    if probe_result not in PROBE_RESULTS:
        raise HostCapabilityValidationError(
            f"{path}.probe_result must be one of {', '.join(PROBE_RESULTS)}"
        )

    probed_at = _hc_timestamp(
        record.get("probed_at"),
        f"{path}.probed_at",
    )
    reason = _hc_text(
        record.get("reason"),
        f"{path}.reason",
        max_chars=_MAX_REASON_CHARS,
        optional=True,
    )
    evidence_ref = _hc_text(
        record.get("evidence_ref"),
        f"{path}.evidence_ref",
        max_chars=_MAX_EVIDENCE_CHARS,
        optional=True,
    )

    declaration_only = record.get("declaration_only", False)
    if not isinstance(declaration_only, bool):
        raise HostCapabilityValidationError(
            f"{path}.declaration_only must be a boolean"
        )

    claimed = _hc_is_supported(supported)

    if declaration_only and not claimed:
        raise HostCapabilityValidationError(
            f"{path} is declaration_only while declaring no support"
        )
    if declaration_only and probe_result == PROBE_PASS:
        raise HostCapabilityValidationError(
            f"{path} is declaration_only while reporting a PASS probe"
        )
    if probe_result in (PROBE_FAIL, PROBE_UNSUPPORTED) and claimed:
        raise HostCapabilityValidationError(
            f"{path} claims support while probe_result is {probe_result}"
        )
    if not claimed and probe_result == PROBE_PASS:
        raise HostCapabilityValidationError(
            f"{path} reports a passing probe while declaring no support"
        )
    if claimed and probe_result != PROBE_PASS and not declaration_only:
        raise HostCapabilityValidationError(
            f"{path} claims support without a PASS probe; "
            "set declaration_only and explain the reason, or run the probe"
        )
    if not claimed and not reason:
        raise HostCapabilityValidationError(
            f"{path} declares no support without a reason"
        )
    if declaration_only and not reason:
        raise HostCapabilityValidationError(
            f"{path} is declaration_only without a reason"
        )

    validated: dict[str, Any] = {
        "key": key,
        "supported": supported,
        "probe_result": probe_result,
        "probed_at": probed_at,
        "declaration_only": declaration_only,
    }
    if reason:
        validated["reason"] = reason
    if evidence_ref:
        validated["evidence_ref"] = evidence_ref
    return validated


def validate_host_capabilities(payload: Any) -> dict[str, Any]:
    """Validate a ``dcc-mcp/host-capabilities@1`` manifest and normalize it.

    Fails closed: unknown fields, duplicate keys, a claimed capability without a
    passing probe (or an explicit declaration), and an unsupported capability
    without a reason are all rejected. Returns a new dict; the input is not
    mutated.
    """
    if not isinstance(payload, dict):
        raise HostCapabilityValidationError(
            "host capabilities manifest must be an object"
        )

    unknown = sorted(set(payload) - _MANIFEST_FIELDS)
    if unknown:
        raise HostCapabilityValidationError(
            f"manifest has unknown field(s): {', '.join(unknown)}"
        )

    if payload.get("schema_name") != _SCHEMA_HOST_CAPABILITIES:
        raise HostCapabilityValidationError(
            f"schema_name must be {_SCHEMA_HOST_CAPABILITIES!r}"
        )
    version = payload.get("schema_version")
    if isinstance(version, bool) or version != HOST_CAPABILITIES_SCHEMA_VERSION:
        raise HostCapabilityValidationError(
            f"schema_version must be {HOST_CAPABILITIES_SCHEMA_VERSION}"
        )

    host_version = _hc_text(
        payload.get("host_version"),
        "host_version",
        max_chars=_MAX_MANIFEST_VERSION_CHARS,
    )
    adapter_version = _hc_text(
        payload.get("adapter_version"),
        "adapter_version",
        max_chars=_MAX_MANIFEST_VERSION_CHARS,
    )

    records = payload.get("capabilities")
    if not isinstance(records, list):
        raise HostCapabilityValidationError("capabilities must be an array")
    if len(records) > _MAX_CAPABILITIES:
        raise HostCapabilityValidationError(
            f"capabilities exceeds {_MAX_CAPABILITIES} entries"
        )

    capabilities: list[dict[str, Any]] = []
    seen: set[str] = set()
    for index, record in enumerate(records):
        validated = _validate_host_capability(record, index)
        if validated["key"] in seen:
            raise HostCapabilityValidationError(
                f"duplicate capability key: {validated['key']}"
            )
        seen.add(validated["key"])
        capabilities.append(validated)

    manifest: dict[str, Any] = {
        "schema_name": _SCHEMA_HOST_CAPABILITIES,
        "schema_version": HOST_CAPABILITIES_SCHEMA_VERSION,
        "host_version": host_version,
        "adapter_version": adapter_version,
        "capabilities": capabilities,
    }
    dcc_type = _hc_text(payload.get("dcc_type"), "dcc_type", max_chars=64, optional=True)
    instance_id = _hc_text(
        payload.get("instance_id"), "instance_id", max_chars=128, optional=True
    )
    probed_at = _hc_timestamp(payload.get("probed_at"), "probed_at", optional=True)
    for field_name, text in (
        ("dcc_type", dcc_type),
        ("instance_id", instance_id),
        ("probed_at", probed_at),
    ):
        if text:
            manifest[field_name] = text
    return manifest


def make_host_capability(
    key: str,
    supported: Any,
    probe_result: str,
    *,
    reason: str | None = None,
    evidence_ref: str | None = None,
    probed_at: str | None = None,
    declaration_only: bool = False,
) -> dict[str, Any]:
    """Build and validate one capability record."""
    record: dict[str, Any] = {
        "key": key,
        "supported": supported,
        "probe_result": probe_result,
        "probed_at": probed_at or _now_utc_iso(),
        "declaration_only": declaration_only,
    }
    if reason is not None:
        record["reason"] = reason
    if evidence_ref is not None:
        record["evidence_ref"] = evidence_ref
    return _validate_host_capability(record, 0)


def build_host_capabilities(
    *,
    host_version: str,
    adapter_version: str,
    capabilities: list[dict[str, Any]],
    dcc_type: str | None = None,
    instance_id: str | None = None,
    probed_at: str | None = None,
) -> dict[str, Any]:
    """Assemble and validate a complete manifest."""
    manifest: dict[str, Any] = {
        "schema_name": _SCHEMA_HOST_CAPABILITIES,
        "schema_version": HOST_CAPABILITIES_SCHEMA_VERSION,
        "host_version": host_version,
        "adapter_version": adapter_version,
        "capabilities": list(capabilities),
    }
    if dcc_type:
        manifest["dcc_type"] = dcc_type
    if instance_id:
        manifest["instance_id"] = instance_id
    manifest["probed_at"] = probed_at or _now_utc_iso()
    return validate_host_capabilities(manifest)


def summarize_host_capabilities(
    payload: Any,
    *,
    keys: list[str] | None = None,
) -> dict[str, Any]:
    """Partition a manifest into the planning answer an agent needs.

    Groups are ``available`` (proved by a passing probe), ``declared`` (claimed
    without proof), ``unavailable`` (probed and rejected), and ``not_run``
    (unknown). An agent planning a long job should treat ``declared`` and
    ``not_run`` as unverified rather than as support.
    """
    manifest = validate_host_capabilities(payload)
    wanted = set(keys) if keys else None

    available: list[dict[str, Any]] = []
    declared: list[dict[str, Any]] = []
    unavailable: list[dict[str, Any]] = []
    not_run: list[dict[str, Any]] = []

    for record in manifest["capabilities"]:
        if wanted is not None and record["key"] not in wanted:
            continue
        entry: dict[str, Any] = {
            "key": record["key"],
            "supported": record["supported"],
            "probe_result": record["probe_result"],
            "probed_at": record["probed_at"],
        }
        if "reason" in record:
            entry["reason"] = record["reason"]
        if record["declaration_only"]:
            entry["declaration_only"] = True

        if record["probe_result"] == PROBE_PASS and _hc_is_supported(
            record["supported"]
        ):
            available.append(entry)
        elif record["declaration_only"]:
            declared.append(entry)
        elif record["probe_result"] == PROBE_NOT_RUN:
            not_run.append(entry)
        else:
            unavailable.append(entry)

    return {
        "host_version": manifest["host_version"],
        "adapter_version": manifest["adapter_version"],
        "probed_at": manifest.get("probed_at"),
        "available": available,
        "declared": declared,
        "unavailable": unavailable,
        "not_run": not_run,
        "totals": {
            "available": len(available),
            "declared": len(declared),
            "unavailable": len(unavailable),
            "not_run": len(not_run),
        },
    }


class HostCapabilityCache:
    """TTL cache keyed by DCC type plus host and adapter version.

    A probe result is only valid for the exact host and adapter build that
    produced it, so both versions are part of the key. ``ttl_secs=0`` keeps
    entries until they are invalidated explicitly.
    """

    def __init__(
        self,
        ttl_secs: int = 300,
        *,
        clock: Callable[[], float] | None = None,
    ) -> None:
        if isinstance(ttl_secs, bool) or not isinstance(ttl_secs, int):
            raise HostCapabilityValidationError("ttl_secs must be an integer")
        if ttl_secs < 0:
            raise HostCapabilityValidationError("ttl_secs must not be negative")
        self._ttl = ttl_secs
        self._clock = clock or time.monotonic
        self._entries: dict[str, tuple[float, dict[str, Any]]] = {}
        self._lock = RLock()

    @property
    def ttl_secs(self) -> int:
        return self._ttl

    @staticmethod
    def cache_key(
        dcc_type: str | None,
        host_version: str,
        adapter_version: str,
    ) -> str:
        parts = (
            str(dcc_type or ""),
            str(host_version),
            str(adapter_version),
        )
        return "|".join(f"{len(part)}:{quote(part, safe='')}" for part in parts)

    def get(self, key: str) -> dict[str, Any] | None:
        with self._lock:
            entry = self._entries.get(key)
            if entry is None:
                return None
            stored_at, payload = entry
            if self._ttl and (self._clock() - stored_at) > self._ttl:
                del self._entries[key]
                return None
            return payload

    def put(self, key: str, payload: dict[str, Any]) -> None:
        with self._lock:
            self._entries[key] = (self._clock(), payload)

    def is_stale(self, key: str) -> bool:
        return self.get(key) is None

    def invalidate(self, key: str | None = None) -> None:
        with self._lock:
            if key is None:
                self._entries.clear()
            else:
                self._entries.pop(key, None)

    def __len__(self) -> int:
        with self._lock:
            return len(self._entries)


def register_host_capability_tool(
    server: Any,
    *,
    dcc_name: str,
    provider: Callable[[], dict[str, Any]],
    cache: HostCapabilityCache | None = None,
    provider_cache_key: Callable[[], tuple[str | None, str, str]] | None = None,
) -> bool:
    """Register ``dcc_host_capabilities`` as an MCP tool.

    ``provider`` is adapter-owned and returns a validated manifest. No probe is
    executed here: the adapter decides what it can prove, and this tool only
    exposes the result with optional version-keyed caching.
    """
    inner = getattr(server, "_server", None)
    if inner is None:
        logger.debug("host capabilities: server has no inner _server; skipping")
        return False

    tool_name = "dcc_host_capabilities"
    description = (
        f"Report probed {dcc_name} host feature capabilities with the probe result "
        "that proves each one. Call this before committing to an expensive job so "
        "unavailable features are found in planning rather than after a render."
    )
    input_schema = {
        "type": "object",
        "properties": {
            "force_refresh": {
                "type": "boolean",
                "description": "Ignore the version-keyed cache and re-read probes.",
                "default": False,
            },
            "keys": {
                "type": "array",
                "items": {"type": "string"},
                "maxItems": 64,
                "description": "Optional capability keys to keep in the summary.",
            },
        },
        "additionalProperties": False,
    }

    def handler(params: dict[str, Any]) -> dict[str, Any]:
        params = params or {}
        try:
            identity = provider_cache_key() if provider_cache_key is not None else None
            key: str | None = None
            cached = None
            if cache is not None and identity is not None and not params.get(
                "force_refresh"
            ):
                key = HostCapabilityCache.cache_key(*identity)
                cached = cache.get(key)
            if cached is not None:
                manifest = cached
            else:
                manifest = validate_host_capabilities(provider())
                if cache is not None:
                    if key is None:
                        key = HostCapabilityCache.cache_key(
                            manifest.get("dcc_type") or dcc_name,
                            manifest["host_version"],
                            manifest["adapter_version"],
                        )
                    cache.put(key, manifest)

            summary = summarize_host_capabilities(
                manifest, keys=params.get("keys") or None
            )
            return {
                "success": True,
                "message": f"{dcc_name} host capabilities",
                "context": summary,
            }
        except Exception as exc:
            logger.warning("host capabilities: provider failed: %s", exc)
            return {
                "success": False,
                "error": "host_capability_provider_failed",
                "message": "Host capabilities could not be read",
                "context": {
                    "dcc_name": dcc_name,
                    "exception_type": type(exc).__name__,
                    "detail": str(exc)[:512],
                },
            }

    registry = getattr(inner, "registry", None)
    if registry is None or not hasattr(registry, "register"):
        return False

    try:
        registry.register(
            tool_name,
            description=description,
            category="dcc",
            tags=["capability", "capabilities", "probe", "planning", dcc_name.lower()],
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
        logger.debug("host capabilities: registration failed: %s", exc)
        return False
