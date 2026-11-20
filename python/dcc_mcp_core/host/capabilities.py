"""Adapter-owned host capability probes and MCP registration.

The versioned contract and cache primitives live in
:mod:`dcc_mcp_core.host.capability_contract`. This module keeps the bounded
probe runner and the compatibility registration helpers.
"""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import TimeoutError as FutureTimeoutError
import logging
from threading import Thread
from typing import Any
from typing import Callable
from typing import Sequence

from dcc_mcp_core import json_dumps
from dcc_mcp_core.host.capability_contract import _CAPABILITY_KEY_RE
from dcc_mcp_core.host.capability_contract import HOST_CAPABILITIES_SCHEMA_ID
from dcc_mcp_core.host.capability_contract import HOST_CAPABILITIES_SCHEMA_VERSION
from dcc_mcp_core.host.capability_contract import INITIAL_CAPABILITY_KEYS
from dcc_mcp_core.host.capability_contract import PROBE_FAIL
from dcc_mcp_core.host.capability_contract import PROBE_NOT_RUN
from dcc_mcp_core.host.capability_contract import PROBE_PASS
from dcc_mcp_core.host.capability_contract import PROBE_RESULTS
from dcc_mcp_core.host.capability_contract import PROBE_UNSUPPORTED
from dcc_mcp_core.host.capability_contract import HostCapabilityCache
from dcc_mcp_core.host.capability_contract import HostCapabilityValidationError
from dcc_mcp_core.host.capability_contract import _hc_consistency_rules
from dcc_mcp_core.host.capability_contract import _hc_supported_schema
from dcc_mcp_core.host.capability_contract import _hc_timestamp_schema
from dcc_mcp_core.host.capability_contract import _now_utc_iso
from dcc_mcp_core.host.capability_contract import _validate_host_capability
from dcc_mcp_core.host.capability_contract import build_host_capabilities
from dcc_mcp_core.host.capability_contract import make_host_capability
from dcc_mcp_core.host.capability_contract import summarize_host_capabilities
from dcc_mcp_core.host.capability_contract import validate_host_capabilities

logger = logging.getLogger(__name__)

__all__ = [
    "CAPABILITY_PROBE_TOOL",
    "DESCRIBE_CAPABILITIES_TOOL",
    "HOST_CAPABILITIES_SCHEMA_ID",
    "HOST_CAPABILITIES_SCHEMA_VERSION",
    "INITIAL_CAPABILITY_KEYS",
    "LEGACY_HOST_CAPABILITIES_TOOL",
    "PROBE_RESULTS",
    "PROBE_UNSUPPORTED",
    "CapabilityProbeRunner",
    "HostCapabilityCache",
    "HostCapabilityValidationError",
    "_hc_consistency_rules",
    "_hc_supported_schema",
    "_hc_timestamp_schema",
    "build_host_capabilities",
    "make_host_capability",
    "register_capability_tools",
    "register_host_capability_tool",
    "summarize_host_capabilities",
    "validate_host_capabilities",
]

DESCRIBE_CAPABILITIES_TOOL = "describe_capabilities"
LEGACY_HOST_CAPABILITIES_TOOL = "dcc_host_capabilities"
CAPABILITY_PROBE_TOOL = "capability_probe"


class CapabilityProbeRunner:
    """Run adapter-provided probes with a bounded wait and fail-closed results.

    Core waits at most ``timeout_secs`` for each probe. It does not terminate
    adapter threads: cancellation of the underlying host operation remains the
    adapter's responsibility. A timeout, malformed record, or unsupported
    outcome becomes a ``FAIL`` record with a concrete reason.
    The callback signature is ``probe(key)`` when ``context`` is omitted and
    ``probe(key, context)`` when an adapter context is supplied. The context
    is passed through untouched so an adapter can supply its own host session,
    dispatcher, or cancellation token.
    """

    def __init__(
        self,
        probe: Callable[[str], Any],
        *,
        timeout_secs: float = 5.0,
        keys: Sequence[str] = INITIAL_CAPABILITY_KEYS,
        context: Any = None,
        cancel: Callable[[str], Any] | None = None,
        cancel_timeout_secs: float = 0.1,
    ) -> None:
        if not callable(probe):
            raise HostCapabilityValidationError("probe must be callable")
        if isinstance(timeout_secs, bool) or not isinstance(timeout_secs, (int, float)):
            raise HostCapabilityValidationError("timeout_secs must be a number")
        if timeout_secs <= 0:
            raise HostCapabilityValidationError("timeout_secs must be greater than zero")
        if isinstance(cancel_timeout_secs, bool) or not isinstance(cancel_timeout_secs, (int, float)):
            raise HostCapabilityValidationError("cancel_timeout_secs must be a number")
        if cancel_timeout_secs < 0:
            raise HostCapabilityValidationError("cancel_timeout_secs must not be negative")
        if isinstance(keys, str):
            raise HostCapabilityValidationError("keys must be a sequence of capability names")
        seen: set[str] = set()
        for key in keys:
            if not isinstance(key, str) or not _CAPABILITY_KEY_RE.fullmatch(key):
                raise HostCapabilityValidationError(f"invalid capability probe key: {key!r}")
            if key in seen:
                raise HostCapabilityValidationError(f"duplicate probe key: {key}")
            seen.add(key)
        self._probe = probe
        self._timeout = float(timeout_secs)
        self._keys = tuple(keys)
        self._context = context
        self._cancel = cancel
        self._cancel_timeout = float(cancel_timeout_secs)

    @property
    def keys(self) -> tuple[str, ...]:
        return self._keys

    def run_one(self, key: str) -> dict[str, Any]:
        """Run one probe and return a validated capability record."""
        if key not in self._keys:
            return make_host_capability(
                key,
                False,
                PROBE_FAIL,
                reason=f"Capability key {key!r} has no registered probe",
                probed_at=_now_utc_iso(),
            )
        return self._run_unchecked(key)

    def run_all(self) -> list[dict[str, Any]]:
        """Run every registered probe and return validated capability records."""
        return [self._run_unchecked(key) for key in self._keys]

    def _run_unchecked(self, key: str) -> dict[str, Any]:
        # A context manager would wait for a timed-out worker on exit, which
        # turns the advertised timeout into an unbounded handler call.
        executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix="dcc-capability-probe")
        try:
            future = executor.submit(self._call_probe, key)
            try:
                raw = future.result(timeout=self._timeout)
            except FutureTimeoutError:
                future.cancel()
                self._request_cancel(key)
                return self._failed(
                    key,
                    f"Probe exceeded the {self._timeout:g}s Core wait; adapter must cancel the host operation",
                )
            except Exception as exc:
                return self._failed(
                    key,
                    f"Probe raised {type(exc).__name__}: {str(exc)[:256]}",
                )
        finally:
            self._shutdown_executor(executor)

        try:
            record = dict(raw) if isinstance(raw, dict) else None
            if record is None:
                raise HostCapabilityValidationError("probe result must be an object")
            record.setdefault("key", key)
            if record["key"] != key:
                raise HostCapabilityValidationError("probe result key does not match the requested key")
            if record.get("probe_result") == PROBE_PASS and not (
                str(record.get("reason") or "").strip() or str(record.get("evidence_ref") or "").strip()
            ):
                raise HostCapabilityValidationError("a passing probe must include reason or evidence_ref")
            return _validate_host_capability(record, 0)
        except (HostCapabilityValidationError, TypeError, ValueError) as exc:
            return self._failed(key, f"Probe returned a malformed record: {str(exc)[:256]}")

    def _failed(self, key: str, reason: str) -> dict[str, Any]:
        return make_host_capability(key, False, PROBE_FAIL, reason=reason, probed_at=_now_utc_iso())

    def _call_probe(self, key: str) -> Any:
        if self._context is None:
            return self._probe(key)
        return self._probe(key, self._context)

    def _request_cancel(self, key: str) -> None:
        if self._cancel is None:
            return

        def request() -> None:
            try:
                self._cancel(key)
            except Exception as exc:
                logger.debug("host capability probe cancellation failed for %s: %s", key, exc)

        thread = Thread(target=request, name="dcc-capability-cancel", daemon=True)
        thread.start()
        if self._cancel_timeout:
            thread.join(self._cancel_timeout)

    @staticmethod
    def _shutdown_executor(executor: ThreadPoolExecutor) -> None:
        # ``cancel_futures`` arrived after the Python 3.7 floor. The running
        # probe cannot be cancelled, but pending work must never keep the
        # caller waiting after a bounded timeout.
        try:
            executor.shutdown(wait=False, cancel_futures=True)
        except TypeError:
            executor.shutdown(wait=False)


def _register_tool(
    registry: Any,
    inner: Any,
    tool_name: str,
    *,
    dcc_name: str,
    description: str,
    input_schema: dict[str, Any],
    handler: Callable[[dict[str, Any]], dict[str, Any]],
    tags: list[str],
) -> bool:
    get_action = getattr(registry, "get_action", None)
    already_registered = callable(get_action) and get_action(tool_name) is not None
    if not already_registered:
        registry.register(
            tool_name,
            description=description,
            category="dcc",
            tags=tags,
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


def _capability_input_schema(*, include_probe: bool) -> dict[str, Any]:
    properties: dict[str, Any] = {
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
    }
    if include_probe:
        properties["invalidate"] = {
            "type": "boolean",
            "description": "Clear this host/adapter cache entry before reading capabilities.",
            "default": False,
        }
    return {"type": "object", "properties": properties, "additionalProperties": False}


def _probe_input_schema() -> dict[str, Any]:
    return {
        "type": "object",
        "properties": {
            "key": {
                "type": "string",
                "description": "One namespaced capability key to probe.",
            },
            "keys": {
                "type": "array",
                "items": {"type": "string"},
                "minItems": 1,
                "maxItems": 64,
                "description": "Capability keys to probe. Omit when key is supplied.",
            },
            "invalidate": {
                "type": "boolean",
                "description": "Clear this host/adapter cache entry before running probes.",
                "default": False,
            },
            "force_refresh": {
                "type": "boolean",
                "description": "Ignore cached probe records and run the requested probes again.",
                "default": False,
            },
        },
        "additionalProperties": False,
    }


def _requested_keys(
    params: dict[str, Any],
    *,
    runner: CapabilityProbeRunner,
) -> list[str]:
    raw = params.get("keys")
    if isinstance(raw, str):
        raw = [raw]
    if raw:
        if not isinstance(raw, list) or any(not isinstance(item, str) or not item for item in raw):
            raise HostCapabilityValidationError("keys must be an array of non-empty strings")
        return list(dict.fromkeys(raw))
    key = params.get("key")
    if key:
        if not isinstance(key, str):
            raise HostCapabilityValidationError("key must be a string")
        return [key]
    return list(runner.keys)


def _manifest_with_records(
    manifest: dict[str, Any],
    records: list[dict[str, Any]],
) -> dict[str, Any]:
    merged: dict[str, dict[str, Any]] = {record["key"]: dict(record) for record in manifest.get("capabilities", [])}
    for record in records:
        merged[record["key"]] = dict(record)
    updated = dict(manifest)
    updated["capabilities"] = [merged[key] for key in sorted(merged)]
    return validate_host_capabilities(updated)


def _complete_initial_capabilities(manifest: dict[str, Any]) -> dict[str, Any]:
    present = {record["key"] for record in manifest.get("capabilities", [])}
    missing = [
        make_host_capability(
            key,
            False,
            PROBE_NOT_RUN,
            reason=f"No adapter probe or declaration is registered for {key}",
        )
        for key in INITIAL_CAPABILITY_KEYS
        if key not in present
    ]
    return _manifest_with_records(manifest, missing) if missing else manifest


def _invalidate_cache(cache: HostCapabilityCache | None, key: str | None) -> None:
    if cache is None:
        return
    cache.invalidate(key)


def register_capability_tools(
    server: Any,
    *,
    dcc_name: str,
    provider: Callable[[], dict[str, Any]],
    cache: HostCapabilityCache | None = None,
    provider_cache_key: Callable[[], tuple[str | None, str, str]] | None = None,
    probe_runner: CapabilityProbeRunner | None = None,
) -> bool:
    """Register the canonical describe tool, its alias, and ``capability_probe``.

    The provider remains the adapter-owned source of truth. The optional probe
    runner lets an adapter replace cached records with freshly probed ones.
    A probe that fails or times out is fail-closed and never cached as support.
    """
    inner = getattr(server, "_server", None)
    registry = getattr(inner, "registry", None)
    if registry is None or not hasattr(registry, "register"):
        logger.debug("host capabilities: server has no registry; skipping")
        return False

    fallback_cache_key: str | None = None

    def identity_key() -> str | None:
        nonlocal fallback_cache_key
        if cache is None:
            return None
        identity = provider_cache_key() if provider_cache_key is not None else None
        if identity is None:
            return fallback_cache_key
        return HostCapabilityCache.cache_key(*identity)

    def describe_handler(params: dict[str, Any]) -> dict[str, Any]:
        nonlocal fallback_cache_key
        params = params or {}
        try:
            key = identity_key()
            if params.get("invalidate"):
                _invalidate_cache(cache, key)
                key = identity_key()
            cached = None
            cache_version = cache.version(key) if cache is not None and key is not None else None
            if cache is not None and key is not None and not params.get("force_refresh"):
                cache_version, cached = cache.snapshot(key)
            if cached is not None:
                manifest = _complete_initial_capabilities(cached)
            else:
                manifest = _complete_initial_capabilities(validate_host_capabilities(provider()))
                if cache is not None:
                    if key is None:
                        key = HostCapabilityCache.cache_key(
                            manifest.get("dcc_type") or dcc_name,
                            manifest["host_version"],
                            manifest["adapter_version"],
                        )
                        fallback_cache_key = key
                    cache_version = cache.version(key)
                    cache.put_if_version(key, manifest, cache_version)
            if cached is not None and manifest is not cached and cache is not None and key is not None:
                if cache_version is None:
                    cache_version = cache.version(key)
                cache.put_if_version(key, manifest, cache_version, preserve_existing=True)
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

    def probe_handler(params: dict[str, Any]) -> dict[str, Any]:
        nonlocal fallback_cache_key
        params = params or {}
        if probe_runner is None:
            return {
                "success": False,
                "error": "capability_probe_not_configured",
                "message": "No adapter capability probe is configured",
                "context": {"dcc_name": dcc_name},
            }
        try:
            requested = _requested_keys(params, runner=probe_runner)
            cache_key = identity_key()
            if params.get("invalidate"):
                _invalidate_cache(cache, cache_key)
                cache_key = identity_key()
                cached = None
                cache_version = cache.version(cache_key) if cache is not None and cache_key is not None else None
            else:
                cache_version, cached = (
                    cache.snapshot(cache_key) if cache is not None and cache_key is not None else (None, None)
                )

            if cached is not None and not params.get("force_refresh"):
                cached_records = {record["key"]: record for record in cached["capabilities"]}
                missing = [
                    key
                    for key in requested
                    if key not in cached_records or cached_records[key]["probe_result"] == PROBE_NOT_RUN
                ]
                if not missing:
                    summary = summarize_host_capabilities(cached, keys=requested)
                    return {
                        "success": True,
                        "message": f"{dcc_name} capability probe",
                        "context": summary,
                    }
                base_manifest = _complete_initial_capabilities(cached)
            else:
                base_manifest = _complete_initial_capabilities(validate_host_capabilities(provider()))
                missing = requested

            records = [probe_runner.run_one(key) for key in missing]
            merged = _manifest_with_records(base_manifest, records)
            summary = summarize_host_capabilities(merged, keys=requested)
            if cache is not None:
                if cache_key is None:
                    cache_key = HostCapabilityCache.cache_key(
                        merged.get("dcc_type") or dcc_name,
                        merged["host_version"],
                        merged["adapter_version"],
                    )
                    fallback_cache_key = cache_key
                if cache_version is None:
                    cache_version = cache.version(cache_key)
                cache.put_if_version(cache_key, merged, cache_version, preserve_existing=True)
            return {
                "success": True,
                "message": f"{dcc_name} capability probe",
                "context": summary,
            }
        except Exception as exc:
            logger.warning("host capabilities: probe failed: %s", exc)
            return {
                "success": False,
                "error": "capability_probe_failed",
                "message": "Host capability probe could not be read",
                "context": {
                    "dcc_name": dcc_name,
                    "exception_type": type(exc).__name__,
                    "detail": str(exc)[:512],
                },
            }

    try:
        registered = [
            _register_tool(
                registry,
                inner,
                DESCRIBE_CAPABILITIES_TOOL,
                dcc_name=dcc_name,
                description=(
                    f"Describe probed {dcc_name} host feature capabilities before committing to an expensive job."
                ),
                input_schema=_capability_input_schema(include_probe=True),
                handler=describe_handler,
                tags=["capability", "capabilities", "describe", "probe", "planning", dcc_name.lower()],
            ),
            _register_tool(
                registry,
                inner,
                LEGACY_HOST_CAPABILITIES_TOOL,
                dcc_name=dcc_name,
                description=f"Compatibility alias for {DESCRIBE_CAPABILITIES_TOOL}.",
                input_schema=_capability_input_schema(include_probe=True),
                handler=describe_handler,
                tags=["capability", "capabilities", "probe", "planning", dcc_name.lower()],
            ),
        ]
        has_existing_probe_tool = (
            callable(getattr(registry, "get_action", None)) and registry.get_action(CAPABILITY_PROBE_TOOL) is not None
        )
        if probe_runner is not None or has_existing_probe_tool:
            registered.append(
                _register_tool(
                    registry,
                    inner,
                    CAPABILITY_PROBE_TOOL,
                    dcc_name=dcc_name,
                    description=f"Run bounded {dcc_name} adapter capability probes with fail-closed results.",
                    input_schema=_probe_input_schema(),
                    handler=probe_handler,
                    tags=["capability", "capabilities", "probe", dcc_name.lower()],
                )
            )
        return any(registered)
    except Exception as exc:
        logger.debug("host capabilities: registration failed: %s", exc)
        return False


def register_host_capability_tool(
    server: Any,
    *,
    dcc_name: str,
    provider: Callable[[], dict[str, Any]],
    cache: HostCapabilityCache | None = None,
    provider_cache_key: Callable[[], tuple[str | None, str, str]] | None = None,
    probe_runner: CapabilityProbeRunner | None = None,
) -> bool:
    """Register the canonical host capability tools and compatibility aliases."""
    return register_capability_tools(
        server,
        dcc_name=dcc_name,
        provider=provider,
        cache=cache,
        provider_cache_key=provider_cache_key,
        probe_runner=probe_runner,
    )
