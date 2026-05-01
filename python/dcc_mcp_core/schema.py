"""Zero-dependency type → JSON Schema derivation for MCP tool authors (issue #242).

MCP 2025-06-18 lets servers publish ``inputSchema`` / ``outputSchema`` alongside
every tool, and clients (LLM agents in particular) lean on those schemas to
construct valid arguments and to *trust* the shape of returned data.  Today,
Python tool authors in this repo must hand-write JSON schema strings — a
footgun both for agents generating actions and for humans evolving them.

This module provides a **pure-Python, stdlib-only** helper that derives a
JSON Schema (Draft 2020-12, MCP 2025-06-18 flavoured) from:

- ``@dataclass`` classes
- ``typing.TypedDict`` subclasses
- Plain ``typing`` annotations on function signatures

The emitted shape matches what ``pydantic`` would emit for an equivalent model
(same keys — ``title``, ``$defs``, ``$ref``, ``anyOf`` — same required-field
rules) so callers who later switch to pydantic do not need to migrate agents or
cached schemas.

Out of scope: complex pydantic features (discriminated unions, computed
fields, custom validators).  Callers who need those import pydantic themselves
and pass the result of ``MyModel.model_json_schema()`` into ``ToolSpec``.

See ``docs/guide/skills.md`` for usage examples.
"""

from __future__ import annotations

from dataclasses import MISSING
from dataclasses import fields as dataclass_fields
from dataclasses import is_dataclass
import datetime
import enum
import inspect
import pathlib
import types
import typing
from typing import Any
from typing import Callable
from typing import get_args
from typing import get_origin
from typing import get_type_hints
import uuid

# JSON Schema draft + MCP protocol pair we target.
_JSON_SCHEMA_DRAFT = "https://json-schema.org/draft/2020-12/schema"


# ── Type-to-schema atoms ──────────────────────────────────────────────────


def _is_optional(tp: Any) -> tuple[bool, Any]:
    """Return ``(is_optional, inner_type)`` for ``Optional[X]`` / ``X | None``.

    For non-optional types returns ``(False, tp)``.
    """
    origin = get_origin(tp)
    if origin is typing.Union or origin is types.UnionType:
        args = [a for a in get_args(tp) if a is not type(None)]
        had_none = len(args) != len(get_args(tp))
        if had_none:
            if len(args) == 1:
                return True, args[0]
            # Union[A, B, None] → treated as optional with anyOf over A, B.
            return True, typing.Union[tuple(args)]
    return False, tp


def _primitive_schema(tp: Any) -> dict[str, Any] | None:
    """Return the JSON Schema for a primitive leaf type.

    Returns ``None`` when *tp* is not a primitive this helper recognises.
    """
    if tp is bool:
        return {"type": "boolean"}
    if tp is int:
        return {"type": "integer"}
    if tp is float:
        return {"type": "number"}
    if tp is str:
        return {"type": "string"}
    if tp is bytes:
        return {"type": "string", "contentEncoding": "base64"}
    if tp is type(None):
        return {"type": "null"}
    if tp is datetime.datetime:
        return {"type": "string", "format": "date-time"}
    if tp is datetime.date:
        return {"type": "string", "format": "date"}
    if tp is pathlib.Path or (isinstance(tp, type) and issubclass(tp, pathlib.PurePath)):
        return {"type": "string"}
    if tp is uuid.UUID:
        return {"type": "string", "format": "uuid"}
    if tp is Any:
        # An empty schema accepts anything — this is JSON Schema's explicit
        # "any value" form.
        return {}
    return None


def _literal_schema(tp: Any) -> dict[str, Any] | None:
    """Return a schema for ``Literal[...]`` or ``None`` if *tp* is not one."""
    if get_origin(tp) is typing.Literal:
        values = list(get_args(tp))
        types_seen = {type(v) for v in values}
        # If all values share a single primitive type, pin it — this helps
        # validators short-circuit.
        if types_seen == {bool}:
            return {"enum": values, "type": "boolean"}
        if types_seen == {int}:
            return {"enum": values, "type": "integer"}
        if types_seen == {str}:
            return {"enum": values, "type": "string"}
        return {"enum": values}
    return None


def _enum_schema(tp: Any) -> dict[str, Any] | None:
    """Return a schema for an ``Enum`` subclass or ``None``."""
    if isinstance(tp, type) and issubclass(tp, enum.Enum):
        values = [member.value for member in tp]
        schema: dict[str, Any] = {"enum": values, "title": tp.__name__}
        types_seen = {type(v) for v in values}
        if types_seen == {str}:
            schema["type"] = "string"
        elif types_seen == {int}:
            schema["type"] = "integer"
        return schema
    return None


# ── Container recursion ───────────────────────────────────────────────────


def _container_schema(tp: Any, defs: dict[str, dict[str, Any]]) -> dict[str, Any] | None:
    """Return a schema for ``list[X]`` / ``tuple[...]`` / ``dict[str, V]``."""
    origin = get_origin(tp)
    args = get_args(tp)
    if origin in (list, set, frozenset):
        item = args[0] if args else Any
        return {"type": "array", "items": _derive(item, defs)}
    if origin is tuple:
        # ``tuple[X, ...]`` == homogeneous tuple
        if len(args) == 2 and args[1] is Ellipsis:
            return {"type": "array", "items": _derive(args[0], defs)}
        if args:
            return {
                "type": "array",
                "prefixItems": [_derive(a, defs) for a in args],
                "minItems": len(args),
                "maxItems": len(args),
            }
        return {"type": "array"}
    if origin is dict:
        # JSON objects only accept string keys.  We don't enforce the key type
        # in the schema because JSON pointers make that explicit anyway.
        if len(args) == 2:
            return {"type": "object", "additionalProperties": _derive(args[1], defs)}
        return {"type": "object"}
    return None


# ── Dataclass / TypedDict traversal ───────────────────────────────────────


def _field_description(field_metadata: Any) -> str | None:
    """Pull a ``description`` hint out of ``dataclasses.field(metadata=...)``."""
    if not field_metadata:
        return None
    description = field_metadata.get("description") if hasattr(field_metadata, "get") else None
    return description if isinstance(description, str) else None


def _dataclass_schema(tp: type, defs: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Derive a schema for a dataclass.

    Nested dataclasses are emitted into ``$defs`` and referenced via ``$ref``
    so the output shape matches pydantic's ``model_json_schema()``.
    """
    title = tp.__name__
    if title in defs:
        return {"$ref": f"#/$defs/{title}"}

    # Reserve the slot *before* recursing so cycles terminate.
    defs[title] = {}  # placeholder

    hints = get_type_hints(tp)
    properties: dict[str, Any] = {}
    required: list[str] = []
    for f in dataclass_fields(tp):
        annotation = hints.get(f.name, f.type)
        is_opt, inner = _is_optional(annotation)
        prop_schema = _derive(inner, defs)
        if is_opt:
            # Pydantic emits anyOf with a null branch for optional fields.
            prop_schema = {"anyOf": [prop_schema, {"type": "null"}]}
        description = _field_description(f.metadata)
        if description:
            prop_schema = {**prop_schema, "description": description}
        properties[f.name] = prop_schema
        # A field is "required" iff it has no default *and* no default_factory.
        if f.default is MISSING and f.default_factory is MISSING:
            required.append(f.name)

    schema: dict[str, Any] = {
        "type": "object",
        "title": title,
        "properties": properties,
    }
    if required:
        schema["required"] = required
    schema["additionalProperties"] = False

    defs[title] = schema
    return {"$ref": f"#/$defs/{title}"}


def _is_typeddict(tp: Any) -> bool:
    return isinstance(tp, type) and issubclass(tp, dict) and hasattr(tp, "__required_keys__")


def _typeddict_schema(tp: type, defs: dict[str, dict[str, Any]]) -> dict[str, Any]:
    title = tp.__name__
    if title in defs:
        return {"$ref": f"#/$defs/{title}"}
    defs[title] = {}  # placeholder for cycle safety

    hints = get_type_hints(tp)
    properties: dict[str, Any] = {}
    for key, annotation in hints.items():
        is_opt, inner = _is_optional(annotation)
        prop = _derive(inner, defs)
        if is_opt:
            prop = {"anyOf": [prop, {"type": "null"}]}
        properties[key] = prop

    schema: dict[str, Any] = {
        "type": "object",
        "title": title,
        "properties": properties,
    }
    required = sorted(tp.__required_keys__)
    if required:
        schema["required"] = required
    schema["additionalProperties"] = False

    defs[title] = schema
    return {"$ref": f"#/$defs/{title}"}


# ── Core dispatch ─────────────────────────────────────────────────────────


def _derive(tp: Any, defs: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Derive a schema for *tp*, populating *defs* with referenced types.

    Raises ``TypeError`` for unsupported types so callers never get a silent
    ``{"type": "object"}`` fallback.
    """
    # Optional first — unwrap the None branch before anything else.
    is_opt, inner = _is_optional(tp)
    if is_opt:
        sub = _derive(inner, defs)
        return {"anyOf": [sub, {"type": "null"}]}

    # Leaf primitives.
    prim = _primitive_schema(tp)
    if prim is not None:
        return prim

    # Literal / Enum.
    lit = _literal_schema(tp)
    if lit is not None:
        return lit
    en = _enum_schema(tp)
    if en is not None:
        return en

    # list / tuple / dict ...
    container = _container_schema(tp, defs)
    if container is not None:
        return container

    # Union (non-optional).
    origin = get_origin(tp)
    if origin is typing.Union or origin is types.UnionType:
        return {"anyOf": [_derive(a, defs) for a in get_args(tp)]}

    # Dataclass / TypedDict.
    if is_dataclass(tp) and isinstance(tp, type):
        return _dataclass_schema(tp, defs)
    if _is_typeddict(tp):
        return _typeddict_schema(tp, defs)

    raise TypeError(
        f"derive_schema: unsupported type {tp!r}. "
        f"Pass an explicit input_schema= / output_schema= dict for exotic types, "
        f"or import pydantic and use MyModel.model_json_schema()."
    )


# ── Public API ────────────────────────────────────────────────────────────


def derive_schema(tp: type, *, allow_additional: bool = False) -> dict[str, Any]:
    """Derive a JSON Schema dict from a dataclass / TypedDict / primitive.

    Parameters
    ----------
    tp:
        The type to describe.  Most commonly a ``@dataclass`` class.
    allow_additional:
        When ``True``, the emitted object schemas allow extra keys
        (``"additionalProperties": true``).  Default ``False`` mirrors
        pydantic's ``model_config.extra="forbid"`` so typos in arguments are
        rejected early.

    Returns
    -------
    dict
        A JSON Schema dict.  For dataclasses / TypedDicts the shape is an
        inline object schema with nested ``$defs`` when referenced multiple
        times; for primitives the leaf schema is returned unchanged.

    """
    defs: dict[str, dict[str, Any]] = {}
    top = _derive(tp, defs)

    # For object types we stored the body in $defs and the top-level is a
    # $ref — flatten that so agents see the full schema inline.  Any remaining
    # $defs (nested references, cycles) stay attached.
    if "$ref" in top and top["$ref"].startswith("#/$defs/"):
        name = top["$ref"].rsplit("/", 1)[-1]
        body = dict(defs.pop(name))
        if body.get("type") == "object":
            body["additionalProperties"] = bool(allow_additional)
        if defs:
            body["$defs"] = defs
        body.setdefault("$schema", _JSON_SCHEMA_DRAFT)
        return body

    # Primitive / container top-level.
    if defs:
        top = {**top, "$defs": defs}
    return top


def derive_parameters_schema(fn: Callable[..., Any]) -> dict[str, Any]:
    """Derive an object schema where each function parameter becomes a property.

    ``*args`` / ``**kwargs`` parameters are skipped — JSON Schema has no way to
    express them losslessly, and they are discouraged in MCP tool signatures.
    The return annotation is not inspected here; use :func:`derive_schema` on
    the return type if you want an ``outputSchema``.

    Descriptions sourced from a numpy-style or Google-style docstring are
    attached to properties when available (:func:`schema_from_doc`).

    Raises ``TypeError`` if any parameter is untyped — we never emit a silent
    "accept anything" fallback (the #588-era footgun).
    """
    sig = inspect.signature(fn)
    hints = get_type_hints(fn, include_extras=True)

    param_descriptions = schema_from_doc(fn)

    defs: dict[str, dict[str, Any]] = {}
    properties: dict[str, Any] = {}
    required: list[str] = []
    has_untyped = False
    for name, param in sig.parameters.items():
        if param.kind in (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD):
            continue
        if name == "self":
            continue
        annotation = hints.get(name, param.annotation)
        if annotation is inspect.Parameter.empty:
            has_untyped = True
            continue
        is_opt, inner = _is_optional(annotation)
        prop = _derive(inner, defs)
        if is_opt:
            prop = {"anyOf": [prop, {"type": "null"}]}
        description = param_descriptions.get(name)
        if description:
            prop = {**prop, "description": description}
        properties[name] = prop
        if param.default is inspect.Parameter.empty and not is_opt:
            required.append(name)

    if has_untyped:
        raise TypeError(
            f"derive_parameters_schema: {fn.__qualname__} has untyped parameters. "
            f"Annotate all params or pass an explicit input_schema=."
        )

    schema: dict[str, Any] = {
        "$schema": _JSON_SCHEMA_DRAFT,
        "type": "object",
        "properties": properties,
        "additionalProperties": False,
    }
    if required:
        schema["required"] = required
    if defs:
        schema["$defs"] = defs
    return schema


def schema_from_doc(fn: Callable[..., Any]) -> dict[str, str]:
    r"""Return a ``{param_name: description}`` map parsed from *fn*'s docstring.

    Recognises numpy-style (``Parameters\n----------``) and Google-style
    (``Args:``) docstrings without any dependency on a docstring library.
    Unknown formats return an empty mapping — the caller falls back to
    field-level ``metadata={"description": ...}`` on the dataclass or just
    omits descriptions.
    """
    doc = inspect.getdoc(fn) or ""
    if not doc:
        return {}

    out: dict[str, str] = {}

    lines = doc.splitlines()
    n = len(lines)
    i = 0
    while i < n:
        line = lines[i].strip()
        if line == "Parameters":
            i += 1
            if i < n and set(lines[i].strip()) == {"-"}:
                i += 1
            # numpy-style block after ``inspect.getdoc`` dedent: each
            # "name : type" starts at column 0, its description follows on
            # an indented line.  The block ends at a blank line followed by
            # a new section header (or EOF).
            while i < n:
                raw = lines[i]
                stripped = raw.strip()
                if not stripped:
                    j = i + 1
                    while j < n and not lines[j].strip():
                        j += 1
                    if j >= n or lines[j].strip() in (
                        "Returns",
                        "Raises",
                        "Examples",
                        "Notes",
                        "See Also",
                        "Yields",
                    ):
                        break
                    i += 1
                    continue
                # "name : type" line.
                if ":" in stripped:
                    name = stripped.split(":", 1)[0].strip()
                    desc_lines: list[str] = []
                    k = i + 1
                    # Collect indented continuation lines.
                    while k < n:
                        nxt = lines[k]
                        if not nxt.strip():
                            break
                        indent = len(nxt) - len(nxt.lstrip())
                        if indent == 0:
                            break
                        desc_lines.append(nxt.strip())
                        k += 1
                    if desc_lines and name.replace("_", "").isalnum():
                        out[name] = " ".join(desc_lines)
                    i = k
                    continue
                i += 1
            continue
        if line == "Args:":
            i += 1
            # Google-style block: "name: description" with wrap continuation.
            base_indent: int | None = None
            while i < n:
                raw = lines[i]
                stripped = raw.strip()
                if not stripped:
                    i += 1
                    continue
                indent = len(raw) - len(raw.lstrip())
                if base_indent is None:
                    base_indent = indent
                if indent < base_indent:
                    break
                if indent == base_indent and ":" in stripped:
                    name, _, rest = stripped.partition(":")
                    name = name.strip()
                    desc_lines = [rest.strip()] if rest.strip() else []
                    k = i + 1
                    while k < n:
                        next_indent = len(lines[k]) - len(lines[k].lstrip())
                        if lines[k].strip() and next_indent <= base_indent:
                            break
                        if lines[k].strip():
                            desc_lines.append(lines[k].strip())
                        k += 1
                    if desc_lines:
                        out[name] = " ".join(desc_lines)
                    i = k
                    continue
                i += 1
            continue
        i += 1

    return out


# ── ToolSpec bridge (issue #242 acceptance criterion) ─────────────────────


def _is_schema_object_type(tp: Any) -> bool:
    """Return True if *tp* should become a full object schema on its own."""
    if is_dataclass(tp) and isinstance(tp, type):
        return True
    return bool(_is_typeddict(tp))


def tool_spec_from_callable(
    handler: Callable[..., Any],
    *,
    name: str | None = None,
    description: str | None = None,
    category: str = "general",
    version: str = "1.0.0",
) -> Any:
    """Build a :class:`ToolSpec` by introspecting *handler*'s type annotations.

    The handler may use either of two signature styles:

    1. **Single dataclass / TypedDict parameter** — the whole parameter becomes
       the ``inputSchema``::

           @dataclass
           class ExportInput:
               scene_path: str
               format: Literal["fbx", "abc"] = "fbx"

           def export_scene(args: ExportInput) -> ExportResult: ...

    2. **Multiple primitive-typed parameters** — each parameter becomes a
       property of an ``object`` ``inputSchema``::

           def make_sphere(radius: float, segments: int = 16) -> SphereResult: ...

    The return annotation, if present and typed, becomes the ``outputSchema``.

    Refuses untyped handlers (``raise TypeError``) so authors get explicit
    feedback rather than a silently-too-permissive ``{"type": "object"}``
    fallback — the failure mode that #588 tracked.

    Parameters
    ----------
    handler:
        The tool's Python callable.
    name:
        MCP tool name; defaults to ``handler.__name__``.
    description:
        Tool description; defaults to the first non-empty line of the handler's
        docstring.
    category, version:
        Passed through to :class:`ToolSpec`.

    Returns
    -------
    ToolSpec
        Ready to pass to :func:`dcc_mcp_core._tool_registration.register_tools`.

    """
    # Local import to avoid a circular dependency at module-load time
    # (_tool_registration is itself a small module but shared with many callers).
    from dcc_mcp_core._tool_registration import ToolSpec

    resolved_name = name or getattr(handler, "__name__", None)
    if not resolved_name:
        raise TypeError("tool_spec_from_callable: handler has no __name__; pass name=...")

    doc = inspect.getdoc(handler) or ""
    first_line = next((line for line in doc.splitlines() if line.strip()), "") if doc else ""
    resolved_description = description if description is not None else first_line

    sig = inspect.signature(handler)
    hints = get_type_hints(handler, include_extras=True)
    real_params = [
        p
        for p in sig.parameters.values()
        if p.name != "self" and p.kind not in (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD)
    ]

    if len(real_params) == 1:
        only = real_params[0]
        annotation = hints.get(only.name, only.annotation)
        if annotation is not inspect.Parameter.empty and _is_schema_object_type(annotation):
            input_schema = derive_schema(annotation)
        else:
            input_schema = derive_parameters_schema(handler)
    else:
        input_schema = derive_parameters_schema(handler)

    output_schema: dict[str, Any] | None = None
    return_annotation = hints.get("return", sig.return_annotation)
    if return_annotation not in (inspect.Parameter.empty, None, type(None)):
        try:
            output_schema = derive_schema(return_annotation)
        except TypeError:
            # Unsupported return type — leave outputSchema unset rather than
            # failing registration. MCP treats outputSchema as optional.
            output_schema = None

    return ToolSpec(
        name=resolved_name,
        description=resolved_description,
        input_schema=input_schema,
        output_schema=output_schema,
        handler=handler,
        category=category,
        version=version,
    )


__all__ = [
    "derive_parameters_schema",
    "derive_schema",
    "schema_from_doc",
    "tool_spec_from_callable",
]
