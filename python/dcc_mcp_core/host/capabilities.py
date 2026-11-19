"""Probed host capability contract for connected DCC instances.

This pure-Python module publishes the versioned manifest shape, validation,
summary, TTL cache, and opt-in MCP registration helper for issue #2867. It is
separate from :mod:`dcc_mcp_core.capabilities`, which indexes which actions
exist; this module records which host features actually work and what proves
it.
"""

from __future__ import annotations

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
    "HostCapabilityCache",
    "HostCapabilityValidationError",
    "build_host_capabilities",
    "make_host_capability",
    "register_host_capability_tool",
    "summarize_host_capabilities",
    "validate_host_capabilities",
]


HOST_CAPABILITIES_SCHEMA_ID = "https://dcc-mcp.github.io/dcc-mcp-core/schemas/host-capabilities-v1.schema.json"
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
_SUPPORTED_VERSION_RE = re.compile(r"^[0-9]+(?:\.[0-9]+)*(?:[+._-][0-9A-Za-z][0-9A-Za-z.+_-]*)?$")
_FALSE_LIKE_SUPPORT_VALUES = frozenset({"false", "no", "off", "none", "null", "n/a", "na", "0", "-"})


def _hc_supported_schema() -> dict[str, Any]:
    """Return the exact ``supported`` schema used by the published contract."""
    false_like = sorted(_FALSE_LIKE_SUPPORT_VALUES)
    return {
        "description": (
            "true, false, or a bounded numeric version/enum string; false-like strings are not support claims."
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
            "then": {"properties": {"probe_result": {"not": {"enum": [PROBE_PASS]}}}},
        },
        {
            "if": {
                "allOf": [
                    claimed,
                    not_declared,
                    {"properties": {"probe_result": {"enum": [PROBE_FAIL, PROBE_UNSUPPORTED]}}},
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
        raise HostCapabilityValidationError(f"{field_name} exceeds {max_chars} characters")
    return text


def _hc_supported(value: Any, field_name: str) -> Any:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        text = value.strip()
        if not text:
            raise HostCapabilityValidationError(f"{field_name} must not be empty")
        if len(text) > _MAX_SUPPORTED_CHARS:
            raise HostCapabilityValidationError(f"{field_name} exceeds {_MAX_SUPPORTED_CHARS} characters")
        lowered = text.lower()
        if lowered in _FALSE_LIKE_SUPPORT_VALUES:
            raise HostCapabilityValidationError(f"{field_name} must not use a false-like version string")
        if not _SUPPORTED_VERSION_RE.match(text):
            raise HostCapabilityValidationError(f"{field_name} must be a boolean or a version string")
        return text
    raise HostCapabilityValidationError(f"{field_name} must be a boolean or a bounded version string")


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
        raise HostCapabilityValidationError(f"{field_name} must be an RFC 3339 date-time with a timezone")
    try:
        if text[-1:] in ("Z", "z"):
            offset = datetime.timedelta(0)
        else:
            zone_pos = text.index("+", 19) if "+" in text[19:] else text.index("-", 19)
            sign = 1 if text[zone_pos] == "+" else -1
            offset = sign * datetime.timedelta(
                hours=int(text[zone_pos + 1 : zone_pos + 3]),
                minutes=int(text[zone_pos + 4 : zone_pos + 6]),
            )
        datetime.datetime(
            int(text[:4]),
            int(text[5:7]),
            int(text[8:10]),
            int(text[11:13]),
            int(text[14:16]),
            int(text[17:19]),
            tzinfo=datetime.timezone(offset),
        )
    except ValueError as exc:
        raise HostCapabilityValidationError(f"{field_name} must be an RFC 3339 date-time with a timezone") from exc
    except (IndexError, TypeError) as exc:
        raise HostCapabilityValidationError(f"{field_name} must be an RFC 3339 date-time with a timezone") from exc
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
        raise HostCapabilityValidationError(f"{path} has unknown field(s): {', '.join(unknown)}")

    key = _hc_text(
        record.get("key"),
        f"{path}.key",
        max_chars=_MAX_CAPABILITY_KEY_CHARS,
    )
    assert key is not None  # required above
    if not _CAPABILITY_KEY_RE.match(key):
        raise HostCapabilityValidationError(f"{path}.key must be a namespaced lower-case key such as render.lpe")

    supported = _hc_supported(record.get("supported"), f"{path}.supported")
    probe_result = record.get("probe_result")
    if probe_result not in PROBE_RESULTS:
        raise HostCapabilityValidationError(f"{path}.probe_result must be one of {', '.join(PROBE_RESULTS)}")

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
        raise HostCapabilityValidationError(f"{path}.declaration_only must be a boolean")

    claimed = _hc_is_supported(supported)

    if declaration_only and not claimed:
        raise HostCapabilityValidationError(f"{path} is declaration_only while declaring no support")
    if declaration_only and probe_result == PROBE_PASS:
        raise HostCapabilityValidationError(f"{path} is declaration_only while reporting a PASS probe")
    if probe_result in (PROBE_FAIL, PROBE_UNSUPPORTED) and claimed:
        raise HostCapabilityValidationError(f"{path} claims support while probe_result is {probe_result}")
    if not claimed and probe_result == PROBE_PASS:
        raise HostCapabilityValidationError(f"{path} reports a passing probe while declaring no support")
    if claimed and probe_result != PROBE_PASS and not declaration_only:
        raise HostCapabilityValidationError(
            f"{path} claims support without a PASS probe; set declaration_only and explain the reason, or run the probe"
        )
    if not claimed and not reason:
        raise HostCapabilityValidationError(f"{path} declares no support without a reason")
    if declaration_only and not reason:
        raise HostCapabilityValidationError(f"{path} is declaration_only without a reason")

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
        raise HostCapabilityValidationError("host capabilities manifest must be an object")

    unknown = sorted(set(payload) - _MANIFEST_FIELDS)
    if unknown:
        raise HostCapabilityValidationError(f"manifest has unknown field(s): {', '.join(unknown)}")

    if payload.get("schema_name") != _SCHEMA_HOST_CAPABILITIES:
        raise HostCapabilityValidationError(f"schema_name must be {_SCHEMA_HOST_CAPABILITIES!r}")
    version = payload.get("schema_version")
    if isinstance(version, bool) or version != HOST_CAPABILITIES_SCHEMA_VERSION:
        raise HostCapabilityValidationError(f"schema_version must be {HOST_CAPABILITIES_SCHEMA_VERSION}")

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
        raise HostCapabilityValidationError(f"capabilities exceeds {_MAX_CAPABILITIES} entries")

    capabilities: list[dict[str, Any]] = []
    seen: set[str] = set()
    for index, record in enumerate(records):
        validated = _validate_host_capability(record, index)
        if validated["key"] in seen:
            raise HostCapabilityValidationError(f"duplicate capability key: {validated['key']}")
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
    instance_id = _hc_text(payload.get("instance_id"), "instance_id", max_chars=128, optional=True)
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

        if record["probe_result"] == PROBE_PASS and _hc_is_supported(record["supported"]):
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
            key: str | None = (
                HostCapabilityCache.cache_key(*identity) if cache is not None and identity is not None else None
            )
            cached = None
            if cache is not None and key is not None and not params.get("force_refresh"):
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

            summary = summarize_host_capabilities(manifest, keys=params.get("keys") or None)
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
        get_action = getattr(registry, "get_action", None)
        already_registered = callable(get_action) and get_action(tool_name) is not None
        if not already_registered:
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
        return not already_registered
    except Exception as exc:
        logger.debug("host capabilities: registration failed: %s", exc)
        return False
