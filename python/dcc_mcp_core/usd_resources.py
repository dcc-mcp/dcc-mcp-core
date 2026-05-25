"""USD project resource conventions for headless adapters.

The helpers in this module are intentionally DCC-agnostic. A pure OpenUSD
adapter, Houdini Solaris bridge, Maya USD exporter, Blender USD workflow, or
Unreal/Omniverse-style integration can expose the same project concepts without
inventing a different URI and metadata shape.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import json
from pathlib import Path
import re
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Mapping
from typing import Optional
from typing import Union

USD_RESOURCE_SCHEME = "openusd"
USD_STAGE_URI = "openusd://stage"
USD_LAYERS_URI = "openusd://layers"
USD_ASSETS_URI = "openusd://assets"
USD_MATERIALS_URI = "openusd://materials"
USD_VALIDATION_URI = "openusd://validation"
USD_SNAPSHOTS_URI = "openusd://snapshots"
USD_PACKAGES_URI = "openusd://packages"

USD_TEXT_MIME = "model/vnd.usd.usda+text"
USD_BINARY_MIME = "model/vnd.usd.usdc"
USD_PACKAGE_MIME = "model/vnd.usdz+zip"
USD_JSON_MIME = "application/json"
USD_MARKDOWN_MIME = "text/markdown"

ResourceSpec = Union[str, Path, Mapping[str, Any]]
ResourceContent = Union[str, bytes, Mapping[str, Any], List[Any]]

_USD_FAMILY_ROOTS: Dict[str, str] = {
    "layer": USD_LAYERS_URI,
    "asset": USD_ASSETS_URI,
    "material": USD_MATERIALS_URI,
    "validation": USD_VALIDATION_URI,
    "snapshot": USD_SNAPSHOTS_URI,
    "package": USD_PACKAGES_URI,
}
_TEXT_SUFFIXES = {".usda", ".json", ".md", ".txt", ".log"}


@dataclass(frozen=True)
class UsdProjectResource:
    """One MCP resource record in the canonical USD project convention."""

    uri: str
    name: str
    description: str
    kind: str
    mime_type: str = USD_JSON_MIME
    path: Optional[str] = None
    content: Optional[ResourceContent] = None
    file_ref: Optional[Mapping[str, Any]] = None
    metadata: Mapping[str, Any] = field(default_factory=dict)

    def list_entry(self) -> Dict[str, str]:
        """Return the MCP ``resources/list`` entry for this record."""
        return {
            "uri": self.uri,
            "name": self.name,
            "description": self.description,
            "mimeType": self.mime_type,
        }

    def manifest_entry(self) -> Dict[str, Any]:
        """Return the stable JSON metadata entry for family root resources."""
        entry: Dict[str, Any] = {
            "uri": self.uri,
            "name": self.name,
            "description": self.description,
            "kind": self.kind,
            "mimeType": self.mime_type,
        }
        if self.path:
            entry["path"] = self.path
        if self.file_ref:
            entry["file_ref"] = dict(self.file_ref)
        if self.metadata:
            entry["metadata"] = dict(self.metadata)
        return entry


class UsdProjectResourceProvider:
    """Callable resource producer registered through ``server.resources()``."""

    def __init__(self, records: Iterable[UsdProjectResource]) -> None:
        self._records = {record.uri: record for record in records}

    @property
    def records(self) -> List[UsdProjectResource]:
        """Return resource records in deterministic URI order."""
        return [self._records[uri] for uri in sorted(self._records)]

    def list_resources(self) -> List[Dict[str, str]]:
        """Return rich MCP ``resources/list`` metadata for the Python producer."""
        return [record.list_entry() for record in self.records]

    def __call__(self, uri: str) -> Dict[str, Any]:
        record = self._records.get(uri)
        if record is None:
            return {
                "mimeType": USD_JSON_MIME,
                "text": json.dumps({"error": "unknown-usd-resource", "uri": uri}, sort_keys=True),
            }
        return _producer_content(record)


def register_usd_project_resources(
    server: Any,
    *,
    project_root: Union[str, Path],
    stage: Optional[ResourceSpec] = None,
    layers: Iterable[ResourceSpec] = (),
    assets: Iterable[ResourceSpec] = (),
    materials: Iterable[ResourceSpec] = (),
    validation: Optional[ResourceSpec] = None,
    snapshots: Iterable[ResourceSpec] = (),
    packages: Iterable[ResourceSpec] = (),
    scheme: str = USD_RESOURCE_SCHEME,
    project_label: Optional[str] = None,
) -> UsdProjectResourceProvider:
    """Register canonical USD project resources on ``server``.

    The returned provider is also the callable installed into
    ``server.resources().register_producer``. Keep it if the adapter wants to
    inspect the deterministic records in tests or diagnostics.
    """
    root = Path(project_root)
    label = _safe_display_name(project_label or root.name or "USD project")
    records = build_usd_project_resources(
        project_root=root,
        stage=stage,
        layers=layers,
        assets=assets,
        materials=materials,
        validation=validation,
        snapshots=snapshots,
        packages=packages,
        scheme=scheme,
        project_label=label,
    )
    provider = UsdProjectResourceProvider(records)
    handle = server.resources()
    handle.register_producer(f"{scheme}://", provider)
    return provider


def build_usd_project_resources(
    *,
    project_root: Union[str, Path],
    stage: Optional[ResourceSpec] = None,
    layers: Iterable[ResourceSpec] = (),
    assets: Iterable[ResourceSpec] = (),
    materials: Iterable[ResourceSpec] = (),
    validation: Optional[ResourceSpec] = None,
    snapshots: Iterable[ResourceSpec] = (),
    packages: Iterable[ResourceSpec] = (),
    scheme: str = USD_RESOURCE_SCHEME,
    project_label: Optional[str] = None,
) -> List[UsdProjectResource]:
    """Build deterministic USD resource records without registering them."""
    root = Path(project_root)
    label = _safe_display_name(project_label or root.name or "USD project")
    records: List[UsdProjectResource] = []

    if stage is not None:
        records.append(
            _record_from_spec(stage, kind="stage", root=root, scheme=scheme, uri=f"{scheme}://stage", label=label)
        )

    family_specs = {
        "layer": layers,
        "asset": assets,
        "material": materials,
        "snapshot": snapshots,
        "package": packages,
    }
    for kind, specs in family_specs.items():
        family_records = [_record_from_spec(spec, kind=kind, root=root, scheme=scheme, label=label) for spec in specs]
        records.extend(family_records)
        records.append(_family_root_record(kind, family_records, scheme=scheme, label=label))

    if validation is not None:
        validation_record = _record_from_spec(
            validation,
            kind="validation",
            root=root,
            scheme=scheme,
            uri=f"{scheme}://validation/report",
            label=label,
        )
        records.append(validation_record)
        records.append(_family_root_record("validation", [validation_record], scheme=scheme, label=label))
    else:
        records.append(_family_root_record("validation", [], scheme=scheme, label=label))

    return _dedupe_records(records)


def _record_from_spec(
    spec: ResourceSpec,
    *,
    kind: str,
    root: Path,
    scheme: str,
    label: str,
    uri: Optional[str] = None,
) -> UsdProjectResource:
    if isinstance(spec, Mapping):
        raw_path = spec.get("path")
        path = _coerce_path(raw_path, root) if raw_path else None
        name = _safe_display_name(str(spec.get("name") or (path.name if path else kind.title())))
        mime = str(spec.get("mimeType") or spec.get("mime_type") or _mime_for_path(path, kind))
        record_uri = str(spec.get("uri") or uri or _item_uri(scheme, kind, name))
        content = spec.get("content")
        file_ref = spec.get("file_ref") or _file_ref(path, mime, name)
        metadata = _base_metadata(root, label)
        metadata.update(dict(spec.get("metadata") or {}))
        return UsdProjectResource(
            uri=record_uri,
            name=name,
            description=str(spec.get("description") or _description(kind, name)),
            kind=kind,
            mime_type=mime,
            path=str(path) if path else None,
            content=content,
            file_ref=file_ref,
            metadata=metadata,
        )

    path = _coerce_path(spec, root)
    name = _safe_display_name(path.name)
    mime = _mime_for_path(path, kind)
    return UsdProjectResource(
        uri=uri or _item_uri(scheme, kind, path.stem),
        name=name,
        description=_description(kind, name),
        kind=kind,
        mime_type=mime,
        path=str(path),
        file_ref=_file_ref(path, mime, name),
        metadata=_base_metadata(root, label),
    )


def _family_root_record(
    kind: str,
    children: List[UsdProjectResource],
    *,
    scheme: str,
    label: str,
) -> UsdProjectResource:
    root_uri = _USD_FAMILY_ROOTS[kind].replace(f"{USD_RESOURCE_SCHEME}://", f"{scheme}://", 1)
    title = {
        "layer": "USD layers",
        "asset": "USD assets",
        "material": "USD materials",
        "validation": "USD validation reports",
        "snapshot": "USD snapshots",
        "package": "USD packages",
    }[kind]
    payload = {
        "kind": kind,
        "project_label": label,
        "count": len(children),
        "resources": [child.manifest_entry() for child in children],
    }
    return UsdProjectResource(
        uri=root_uri,
        name=f"{label} {title}",
        description=f"Manifest of {title.lower()} for {label}.",
        kind=f"{kind}-manifest",
        mime_type=USD_JSON_MIME,
        content=payload,
        metadata={"project_root_label": label},
    )


def _producer_content(record: UsdProjectResource) -> Dict[str, Any]:
    if isinstance(record.content, bytes):
        return {"mimeType": record.mime_type, "blob": record.content}
    if isinstance(record.content, (dict, list)):
        return {"mimeType": record.mime_type, "text": json.dumps(record.content, sort_keys=True)}
    if isinstance(record.content, str):
        return {"mimeType": record.mime_type, "text": record.content}
    if record.path:
        path = Path(record.path)
        if path.suffix.lower() in _TEXT_SUFFIXES:
            return {"mimeType": record.mime_type, "text": path.read_text(encoding="utf-8")}
        return {"mimeType": record.mime_type, "blob": path.read_bytes()}
    return {"mimeType": record.mime_type, "text": json.dumps(record.manifest_entry(), sort_keys=True)}


def _item_uri(scheme: str, kind: str, value: str) -> str:
    if kind == "stage":
        return f"{scheme}://stage"
    family = _USD_FAMILY_ROOTS[kind].split("://", 1)[1]
    return f"{scheme}://{family}/{_slug(value)}"


def _mime_for_path(path: Optional[Path], kind: str) -> str:
    if path is None:
        return USD_JSON_MIME
    suffix = path.suffix.lower()
    if suffix == ".usda":
        return USD_TEXT_MIME
    if suffix in {".usd", ".usdc"}:
        return USD_BINARY_MIME
    if suffix == ".usdz" or kind == "package":
        return USD_PACKAGE_MIME
    if suffix == ".md":
        return USD_MARKDOWN_MIME
    return USD_JSON_MIME if kind in {"validation", "snapshot"} else "application/octet-stream"


def _coerce_path(value: Union[str, Path], root: Path) -> Path:
    path = Path(value)
    return path if path.is_absolute() else root / path


def _file_ref(path: Optional[Path], mime: str, display_name: str) -> Optional[Dict[str, Any]]:
    if path is None:
        return None
    resolved = path.resolve()
    ref: Dict[str, Any] = {
        "uri": resolved.as_uri(),
        "mime": mime,
        "display_name": display_name,
    }
    if resolved.exists():
        ref["size_bytes"] = resolved.stat().st_size
    return ref


def _base_metadata(root: Path, label: str) -> Dict[str, Any]:
    return {
        "project_root_label": label,
        "project_root": str(root),
    }


def _description(kind: str, name: str) -> str:
    return {
        "stage": f"Primary USD stage resource: {name}.",
        "layer": f"USD layer resource: {name}.",
        "asset": f"USD asset dependency: {name}.",
        "material": f"USD material resource: {name}.",
        "validation": f"USD validation report: {name}.",
        "snapshot": f"USD project snapshot: {name}.",
        "package": f"USD package archive: {name}.",
    }.get(kind, f"USD project resource: {name}.")


def _safe_display_name(value: str) -> str:
    cleaned = re.sub(r"[\x00-\x1f/\\]+", " ", value).strip()
    return cleaned[:96] or "USD resource"


def _slug(value: str) -> str:
    stem = Path(value).stem if value else "resource"
    slug = re.sub(r"[^A-Za-z0-9._-]+", "-", stem).strip("-._")
    return slug or "resource"


def _dedupe_records(records: List[UsdProjectResource]) -> List[UsdProjectResource]:
    deduped: Dict[str, UsdProjectResource] = {}
    for record in records:
        deduped[record.uri] = record
    return [deduped[uri] for uri in sorted(deduped)]


__all__ = [
    "USD_ASSETS_URI",
    "USD_BINARY_MIME",
    "USD_JSON_MIME",
    "USD_LAYERS_URI",
    "USD_MARKDOWN_MIME",
    "USD_MATERIALS_URI",
    "USD_PACKAGES_URI",
    "USD_PACKAGE_MIME",
    "USD_RESOURCE_SCHEME",
    "USD_SNAPSHOTS_URI",
    "USD_STAGE_URI",
    "USD_TEXT_MIME",
    "USD_VALIDATION_URI",
    "UsdProjectResource",
    "UsdProjectResourceProvider",
    "build_usd_project_resources",
    "register_usd_project_resources",
]
