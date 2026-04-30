"""Adapter-facing context, instruction, and policy helpers.

These helpers give DCC adapters a small shared vocabulary for session
instructions, post-tool context snapshots, visual feedback, response shaping,
toolset profiles, and searchable host API docs.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import json
from typing import Any
from typing import Callable
from typing import Iterable
from typing import Mapping

from dcc_mcp_core.docs_resources import register_docs_resource

__all__ = [
    "AdapterInstructionSet",
    "DccApiDocEntry",
    "DccApiDocIndex",
    "DccContextSnapshot",
    "DccToolsetProfile",
    "ResponseShapePolicy",
    "ToolsetProfileRegistry",
    "VisualFeedbackPolicy",
    "append_context_snapshot",
    "build_visual_feedback_context",
    "register_adapter_instruction_resources",
    "register_dcc_api_docs",
    "shape_response",
]


@dataclass(frozen=True)
class AdapterInstructionSet:
    """Concise adapter guidance exposed as MCP resources."""

    dcc: str
    instructions: str
    capabilities: Mapping[str, Any] = field(default_factory=dict)
    troubleshooting: str = ""
    adapter_version: str | None = None
    metadata: Mapping[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class DccContextSnapshot:
    """Bounded post-tool context snapshot for DCC adapters."""

    dcc: str
    document: Mapping[str, Any] | None = None
    selection: Mapping[str, Any] | None = None
    active_object: Mapping[str, Any] | None = None
    active_layer: Mapping[str, Any] | None = None
    counts: Mapping[str, int] = field(default_factory=dict)
    metadata: Mapping[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        data: dict[str, Any] = {"dcc": self.dcc}
        for key in ("document", "selection", "active_object", "active_layer"):
            value = getattr(self, key)
            if value is not None:
                data[key] = dict(value)
        if self.counts:
            data["counts"] = dict(self.counts)
        if self.metadata:
            data["metadata"] = dict(self.metadata)
        return data


@dataclass(frozen=True)
class VisualFeedbackPolicy:
    """Policy describing when and how an adapter returns visual previews."""

    mode: str = "manual"
    max_size: int = 800
    format: str = "png"
    resource_backed: bool = True

    def __post_init__(self) -> None:
        if self.mode not in {"manual", "after_mutation", "on_request"}:
            raise ValueError("mode must be 'manual', 'after_mutation', or 'on_request'")
        if self.max_size <= 0:
            raise ValueError("max_size must be > 0")


@dataclass(frozen=True)
class ResponseShapePolicy:
    """Reusable truncation policy for large DCC payloads."""

    max_bytes: int = 256_000
    max_items: int = 200
    summarize: bool = True
    include_truncation_notice: bool = True

    def __post_init__(self) -> None:
        if self.max_bytes <= 0:
            raise ValueError("max_bytes must be > 0")
        if self.max_items <= 0:
            raise ValueError("max_items must be > 0")


@dataclass(frozen=True)
class DccToolsetProfile:
    """High-level toolset profile spanning tools, resources, prompts, and policy."""

    name: str
    description: str = ""
    tools: tuple[str, ...] = ()
    resources: tuple[str, ...] = ()
    prompts: tuple[str, ...] = ()
    default: bool = False
    policy_hints: Mapping[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "description": self.description,
            "tools": list(self.tools),
            "resources": list(self.resources),
            "prompts": list(self.prompts),
            "default": self.default,
            "policy_hints": dict(self.policy_hints),
        }


class ToolsetProfileRegistry:
    """In-memory registry for adapter toolset profiles."""

    def __init__(self, profiles: Iterable[DccToolsetProfile] = ()) -> None:
        self._profiles = {profile.name: profile for profile in profiles}
        self._active = {profile.name for profile in self._profiles.values() if profile.default}

    def register(self, profile: DccToolsetProfile) -> None:
        self._profiles[profile.name] = profile
        if profile.default:
            self._active.add(profile.name)

    def list_profiles(self) -> list[dict[str, Any]]:
        return [
            {**profile.to_dict(), "active": profile.name in self._active}
            for profile in sorted(self._profiles.values(), key=lambda item: item.name)
        ]

    def activate(self, name: str) -> None:
        if name not in self._profiles:
            raise KeyError(f"unknown toolset profile: {name}")
        self._active.add(name)

    def deactivate(self, name: str) -> None:
        if name not in self._profiles:
            raise KeyError(f"unknown toolset profile: {name}")
        self._active.discard(name)

    def active_profiles(self) -> list[DccToolsetProfile]:
        return [self._profiles[name] for name in sorted(self._active)]


@dataclass(frozen=True)
class DccApiDocEntry:
    """Searchable DCC host API documentation entry."""

    symbol: str
    summary: str
    body: str = ""
    uri: str | None = None
    version: str | None = None
    tags: tuple[str, ...] = ()

    def to_dict(self) -> dict[str, Any]:
        return {
            "symbol": self.symbol,
            "summary": self.summary,
            "body": self.body,
            "uri": self.uri,
            "version": self.version,
            "tags": list(self.tags),
        }


class DccApiDocIndex:
    """Tiny searchable docs index for adapter-provided API references."""

    def __init__(self, dcc: str, entries: Iterable[DccApiDocEntry] = (), version: str | None = None) -> None:
        self.dcc = dcc
        self.version = version
        self._entries = {entry.symbol: entry for entry in entries}

    def add(self, entry: DccApiDocEntry) -> None:
        self._entries[entry.symbol] = entry

    def get(self, symbol: str) -> DccApiDocEntry | None:
        return self._entries.get(symbol)

    def search(self, query: str, *, limit: int = 10) -> list[dict[str, Any]]:
        needle = query.casefold()
        scored: list[tuple[int, DccApiDocEntry]] = []
        for entry in self._entries.values():
            haystack = " ".join([entry.symbol, entry.summary, entry.body, " ".join(entry.tags)]).casefold()
            if needle in haystack:
                score = 0 if entry.symbol.casefold() == needle else haystack.find(needle)
                scored.append((score, entry))
        return [entry.to_dict() for _, entry in sorted(scored, key=lambda item: (item[0], item[1].symbol))[:limit]]

    def to_resource_manifest(self) -> dict[str, Any]:
        return {
            "dcc": self.dcc,
            "version": self.version,
            "entries": [entry.to_dict() for entry in sorted(self._entries.values(), key=lambda item: item.symbol)],
        }


def register_adapter_instruction_resources(
    server: Any,
    instruction_set: AdapterInstructionSet,
    *,
    uri_prefix: str | None = None,
) -> list[str]:
    """Register standard adapter instruction/capability resources."""
    prefix = (uri_prefix or f"docs://adapter/{instruction_set.dcc}").rstrip("/")
    resources = [
        (
            f"{prefix}/instructions",
            f"{instruction_set.dcc} instructions",
            "Adapter instructions and recommended workflows.",
            instruction_set.instructions,
            "text/markdown",
        ),
        (
            f"{prefix}/capabilities",
            f"{instruction_set.dcc} capabilities",
            "Adapter capability and version metadata.",
            json.dumps(
                {
                    "dcc": instruction_set.dcc,
                    "adapter_version": instruction_set.adapter_version,
                    "capabilities": dict(instruction_set.capabilities),
                    "metadata": dict(instruction_set.metadata),
                },
                indent=2,
                sort_keys=True,
            ),
            "application/json",
        ),
    ]
    if instruction_set.troubleshooting:
        resources.append(
            (
                f"{prefix}/troubleshooting",
                f"{instruction_set.dcc} troubleshooting",
                "Adapter setup and troubleshooting guidance.",
                instruction_set.troubleshooting,
                "text/markdown",
            )
        )

    registered: list[str] = []
    for uri, name, description, content, mime in resources:
        register_docs_resource(server, uri=uri, name=name, description=description, content=content, mime=mime)
        registered.append(uri)
    return registered


def append_context_snapshot(
    result: Mapping[str, Any],
    snapshot: DccContextSnapshot | Mapping[str, Any] | Callable[[], DccContextSnapshot | Mapping[str, Any]],
    *,
    policy: ResponseShapePolicy | None = None,
) -> dict[str, Any]:
    """Return a copy of *result* with ``context.snapshot`` attached."""
    shaped_policy = policy or ResponseShapePolicy(max_bytes=64_000, max_items=100)
    raw_snapshot = snapshot() if callable(snapshot) else snapshot
    snapshot_dict = raw_snapshot.to_dict() if isinstance(raw_snapshot, DccContextSnapshot) else dict(raw_snapshot)
    shaped = shape_response(snapshot_dict, shaped_policy)
    output = dict(result)
    context = dict(output.get("context") or {})
    context["snapshot"] = shaped["data"]
    if shaped.get("truncated"):
        context["snapshot_truncation"] = shaped["_meta"]["dcc.response_shape"]
    output["context"] = context
    return output


def build_visual_feedback_context(
    *,
    resource: str,
    width: int | None = None,
    height: int | None = None,
    feedback_type: str = "image",
    policy: VisualFeedbackPolicy | None = None,
) -> dict[str, Any]:
    """Build the standard ``context.visual_feedback`` payload."""
    visual_policy = policy or VisualFeedbackPolicy()
    payload: dict[str, Any] = {
        "type": feedback_type,
        "resource": resource,
        "format": visual_policy.format,
        "mode": visual_policy.mode,
        "max_size": visual_policy.max_size,
    }
    if width is not None:
        payload["width"] = min(width, visual_policy.max_size)
    if height is not None:
        payload["height"] = min(height, visual_policy.max_size)
    return {"visual_feedback": payload}


def shape_response(data: Any, policy: ResponseShapePolicy | None = None) -> dict[str, Any]:
    """Shape a potentially large payload and return truncation metadata."""
    active_policy = policy or ResponseShapePolicy()
    shaped, omitted = _shape_value(data, active_policy)
    encoded = json.dumps(shaped, ensure_ascii=False, default=str)
    byte_len = len(encoded.encode("utf-8"))
    truncated = bool(omitted) or byte_len > active_policy.max_bytes
    if byte_len > active_policy.max_bytes:
        text = encoded.encode("utf-8")[: active_policy.max_bytes].decode("utf-8", errors="ignore")
        shaped = {"truncated_json": text}
        omitted.append({"reason": "max_bytes", "original_bytes": byte_len, "max_bytes": active_policy.max_bytes})
    result = {"data": shaped, "truncated": truncated}
    if truncated and active_policy.include_truncation_notice:
        result["_meta"] = {
            "dcc.response_shape": {
                "truncated": True,
                "omitted": omitted,
                "next_query": "Request a narrower path, filter, or page to inspect omitted data.",
            }
        }
    return result


def register_dcc_api_docs(server: Any, index: DccApiDocIndex, *, uri_prefix: str | None = None) -> list[str]:
    """Register a searchable API docs index as resource-backed Markdown/JSON."""
    prefix = (uri_prefix or f"docs://adapter/{index.dcc}/api").rstrip("/")
    manifest_uri = f"{prefix}/index"
    register_docs_resource(
        server,
        uri=manifest_uri,
        name=f"{index.dcc} API docs index",
        description="Searchable adapter-provided DCC API documentation index.",
        content=json.dumps(index.to_resource_manifest(), indent=2, sort_keys=True),
        mime="application/json",
    )
    registered = [manifest_uri]
    for entry in index.to_resource_manifest()["entries"]:
        uri = f"{prefix}/{entry['symbol']}"
        register_docs_resource(
            server,
            uri=uri,
            name=entry["symbol"],
            description=entry["summary"],
            content=entry["body"] or entry["summary"],
            mime="text/markdown",
        )
        registered.append(uri)
    return registered


def _shape_value(value: Any, policy: ResponseShapePolicy) -> tuple[Any, list[dict[str, Any]]]:
    omitted: list[dict[str, Any]] = []
    if isinstance(value, Mapping):
        shaped = {}
        for idx, (key, child) in enumerate(value.items()):
            if idx >= policy.max_items:
                omitted.append({"path": ".", "omitted_items": len(value) - policy.max_items})
                break
            shaped_child, child_omitted = _shape_value(child, policy)
            shaped[key] = shaped_child
            omitted.extend(child_omitted)
        return shaped, omitted
    if isinstance(value, (list, tuple)):
        items = list(value)
        shaped_items = []
        for child in items[: policy.max_items]:
            shaped_child, child_omitted = _shape_value(child, policy)
            shaped_items.append(shaped_child)
            omitted.extend(child_omitted)
        if len(items) > policy.max_items:
            omitted.append({"path": "[]", "omitted_items": len(items) - policy.max_items})
        return shaped_items, omitted
    if isinstance(value, str) and len(value.encode("utf-8")) > policy.max_bytes:
        truncated = value.encode("utf-8")[: policy.max_bytes].decode("utf-8", errors="ignore")
        omitted.append({"path": "$", "reason": "string_max_bytes"})
        return truncated, omitted
    return value, omitted
