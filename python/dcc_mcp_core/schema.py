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

import ast
from dataclasses import MISSING
from dataclasses import fields as dataclass_fields
from dataclasses import is_dataclass
import datetime
import enum
import importlib.util
import inspect
import json
import pathlib
import sys
import types
import typing
from typing import Any
from typing import Callable
from typing import get_type_hints as _typing_get_type_hints
import uuid

try:
    from dcc_mcp_core._typing import Literal as _LITERAL_TYPE
except ImportError:  # pragma: no cover - standalone adapter bundle loading
    _typing_path = pathlib.Path(__file__).with_name("_typing.py")
    _typing_spec = importlib.util.spec_from_file_location("_dcc_mcp_core_local_typing", _typing_path)
    if _typing_spec is None or _typing_spec.loader is None:
        raise ImportError(f"cannot load local typing compatibility boundary from {_typing_path}") from None
    _typing_module = importlib.util.module_from_spec(_typing_spec)
    _typing_spec.loader.exec_module(_typing_module)
    _LITERAL_TYPE = _typing_module.Literal

try:
    from typing import get_args
    from typing import get_origin
except ImportError:  # pragma: no cover — Python 3.7 (Maya 2022) compatibility

    def get_args(tp: Any) -> tuple[Any, ...]:
        return getattr(tp, "__args__", ()) or ()

    def get_origin(tp: Any) -> Any | None:
        return getattr(tp, "__origin__", None)


_UNION_ORIGINS = (typing.Union,)
_union_type = getattr(types, "UnionType", None)
if _union_type is not None:  # pragma: no cover - Python 3.10+
    _UNION_ORIGINS = (*_UNION_ORIGINS, _union_type)

_TYPEDDICT_META = getattr(typing, "_TypedDictMeta", None)


def _require_json(value: Any, error: str, *, scalars: bool = False) -> Any:
    if scalars and not all(item is None or type(item) in (bool, float, int, str) for item in value):
        raise TypeError(error)
    try:
        json.dumps(value, allow_nan=False)
    except (TypeError, ValueError) as exc:
        raise TypeError(error) from exc
    return value


def _literal_origins() -> tuple[Any, ...]:
    """Return Core's local Literal origins by identity."""
    candidates = [_LITERAL_TYPE]

    origins: list[Any] = []
    for candidate in candidates:
        if candidate is None:
            continue
        if not any(candidate is known for known in origins):
            origins.append(candidate)
        try:
            alias_origin = get_origin(candidate[None])
        except (AttributeError, TypeError):
            continue
        if alias_origin is not None and not any(alias_origin is known for known in origins):
            origins.append(alias_origin)
    return tuple(origins)


def _get_type_hints(obj: Any, *, include_extras: bool = False) -> dict[str, Any]:
    if include_extras:
        try:
            return _typing_get_type_hints(obj, include_extras=True)
        except TypeError:  # pragma: no cover - Python 3.8 and earlier
            pass
    return _typing_get_type_hints(obj)


# JSON Schema draft + MCP protocol pair we target.
_JSON_SCHEMA_DRAFT = "https://json-schema.org/draft/2020-12/schema"


# ── Type-to-schema atoms ──────────────────────────────────────────────────


def _is_optional(tp: Any) -> tuple[bool, Any]:
    """Return ``(is_optional, inner_type)`` for ``Optional[X]`` / ``X | None``.

    For non-optional types returns ``(False, tp)``.
    """
    origin = get_origin(tp)
    if origin in _UNION_ORIGINS:
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
    origin = get_origin(tp)
    if any(origin is literal_origin for literal_origin in _literal_origins()):
        values = list(get_args(tp))
        _require_json(values, "Literal values must be JSON scalars", scalars=True)
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
        _require_json(values, "Enum values must be JSON scalars", scalars=True)
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
        if len(args) == 2:
            if args[0] is not str:
                raise TypeError("JSON object mappings require string keys")
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

    hints = _get_type_hints(tp)
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
    if _TYPEDDICT_META is not None and isinstance(tp, _TYPEDDICT_META):
        return True
    return isinstance(tp, type) and issubclass(tp, dict) and hasattr(tp, "__required_keys__")


def _typeddict_schema(tp: type, defs: dict[str, dict[str, Any]]) -> dict[str, Any]:
    title = tp.__name__
    if title in defs:
        return {"$ref": f"#/$defs/{title}"}
    defs[title] = {}  # placeholder for cycle safety

    hints = _get_type_hints(tp)
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
    required_keys = getattr(tp, "__required_keys__", None)
    if required_keys is None:  # pragma: no cover - Python 3.8 only
        required = sorted(hints) if getattr(tp, "__total__", True) else []
    else:
        required = sorted(required_keys)
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
    if origin in _UNION_ORIGINS:
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
        return _require_json(body, "derived schema contains a non-JSON value")

    # Primitive / container top-level.
    if defs:
        top = {**top, "$defs": defs}
    return _require_json(top, "derived schema contains a non-JSON value")


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
    hints = _get_type_hints(fn, include_extras=True)

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
    return _require_json(schema, "derived schema contains a non-JSON value")


_SCRIPT_ANNOTATION_ATOMS: dict[str, Any] = {
    "Any": Any,
    "None": type(None),
    "bool": bool,
    "float": float,
    "int": int,
    "str": str,
}
_SCRIPT_ANNOTATION_MODULES = frozenset({"typing", "typing_extensions"})
_SCRIPT_LITERAL_MISSING = object()


def _script_literal_value(node: ast.AST) -> Any:
    """Read a literal AST value across the supported Python versions."""
    if isinstance(node, ast.Constant):
        return node.value
    if sys.version_info >= (3, 8):
        return _SCRIPT_LITERAL_MISSING
    for node_name, attribute in (
        ("Str", "s"),
        ("Num", "n"),
        ("NameConstant", "value"),
        ("Bytes", "s"),
        ("Ellipsis", None),
    ):
        node_type = getattr(ast, node_name, None)
        if node_type is not None and isinstance(node, node_type):
            return Ellipsis if attribute is None else getattr(node, attribute)
    return _SCRIPT_LITERAL_MISSING


class _ScriptModuleBindingVisitor(ast.NodeVisitor):
    """Collect names bound while executing a module without entering scopes."""

    def __init__(self) -> None:
        self.bindings: list[tuple[str, ast.AST]] = []
        self.has_wildcard_import = False

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        self.bindings.append((node.name, node))

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        self.bindings.append((node.name, node))

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        self.bindings.append((node.name, node))

    def visit_Import(self, node: ast.Import) -> None:
        for alias in node.names:
            self.bindings.append((alias.asname or alias.name.split(".", 1)[0], node))

    def visit_ImportFrom(self, node: ast.ImportFrom) -> None:
        for alias in node.names:
            if alias.name == "*":
                self.has_wildcard_import = True
            else:
                self.bindings.append((alias.asname or alias.name, node))

    def visit_ExceptHandler(self, node: ast.ExceptHandler) -> None:
        if isinstance(node.name, str):
            self.bindings.append((node.name, node))
        self.generic_visit(node)

    def visit_Name(self, node: ast.Name) -> None:
        if isinstance(node.ctx, (ast.Store, ast.Del)):
            self.bindings.append((node.id, node))

    def visit_MatchAs(self, node: ast.AST) -> None:
        name = getattr(node, "name", None)
        if isinstance(name, str):
            self.bindings.append((name, node))
        self.generic_visit(node)

    def visit_MatchStar(self, node: ast.AST) -> None:
        name = getattr(node, "name", None)
        if isinstance(name, str):
            self.bindings.append((name, node))

    def visit_MatchMapping(self, node: ast.AST) -> None:
        rest = getattr(node, "rest", None)
        if isinstance(rest, str):
            self.bindings.append((rest, node))
        self.generic_visit(node)


def _script_annotation_name(node: ast.AST) -> str | None:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        parent = _script_annotation_name(node.value)
        return f"{parent}.{node.attr}" if parent else node.attr
    return None


def _script_subscript_args(node: ast.Subscript) -> list[ast.AST]:
    slice_node = node.slice
    index_type = getattr(ast, "Index", None)
    if index_type is not None and isinstance(slice_node, index_type):
        slice_node = slice_node.value
    if isinstance(slice_node, ast.Tuple):
        return list(slice_node.elts)
    return [slice_node]


def _script_annotation(node: ast.AST | None) -> Any:
    """Resolve a safe annotation AST without importing or executing source."""
    if node is None:
        return inspect.Parameter.empty
    literal_value = _script_literal_value(node)
    if literal_value is not _SCRIPT_LITERAL_MISSING:
        if literal_value is None:
            return type(None)
        if isinstance(literal_value, str):
            return _script_annotation(ast.parse(literal_value, mode="eval").body)
    if isinstance(node, (ast.Name, ast.Attribute)):
        name = _script_annotation_name(node)
        short_name = name.rsplit(".", 1)[-1] if name else ""
        module_name = name.split(".", 1)[0] if name and "." in name else None
        if short_name in _SCRIPT_ANNOTATION_ATOMS and (
            module_name is None or module_name in _SCRIPT_ANNOTATION_MODULES
        ):
            return _SCRIPT_ANNOTATION_ATOMS[short_name]
        raise TypeError(f"unsupported script annotation: {name or ast.dump(node)}")
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.BitOr):
        return typing.Union[(_script_annotation(node.left), _script_annotation(node.right))]
    if not isinstance(node, ast.Subscript):
        raise TypeError(f"unsupported script annotation: {ast.dump(node)}")

    name = _script_annotation_name(node.value)
    short_name = name.rsplit(".", 1)[-1] if name else ""
    module_name = name.split(".", 1)[0] if name and "." in name else None
    if module_name is not None and module_name not in _SCRIPT_ANNOTATION_MODULES:
        raise TypeError(f"unsupported script annotation: {name}")
    if short_name in {"list", "dict", "tuple"}:
        raise TypeError(f"script annotation requires Python 3.9+: {short_name}")
    args = _script_subscript_args(node)
    if short_name == "List" and len(args) == 1:
        return typing.List[_script_annotation(args[0])]
    if short_name == "Dict" and len(args) == 2:
        return typing.Dict[_script_annotation(args[0]), _script_annotation(args[1])]
    if short_name == "Tuple" and args:
        resolved = tuple(
            Ellipsis if _script_literal_value(arg) is Ellipsis else _script_annotation(arg) for arg in args
        )
        return typing.Tuple[resolved]
    if short_name == "Optional" and len(args) == 1:
        return typing.Optional[_script_annotation(args[0])]
    if short_name == "Union" and args:
        return typing.Union[tuple(_script_annotation(arg) for arg in args)]
    if short_name == "Literal" and args and _LITERAL_TYPE is not None:
        values: list[Any] = []
        for arg in args:
            value = _script_literal_value(arg)
            if value is _SCRIPT_LITERAL_MISSING:
                raise TypeError("Literal values must be constants")
            values.append(value)
        return _LITERAL_TYPE[tuple(values)]
    if short_name == "Annotated" and args:
        return _script_annotation(args[0])
    raise TypeError(f"unsupported script annotation: {name or ast.dump(node.value)}")


def derive_script_parameters_schema(
    source: str,
    *,
    function_name: str = "main",
) -> dict[str, Any] | None:
    """Derive ``main(**params)`` input schema without executing the script.

    Only a deliberately small, JSON-compatible annotation subset is accepted.
    Unsupported or partially untyped signatures return ``None`` so callers can
    keep legacy script semantics without presenting an unsafe inferred contract.
    """
    if not isinstance(source, str):
        raise TypeError("source must be a string")
    try:
        module = ast.parse(source)
    except SyntaxError:
        return None
    binding_visitor = _ScriptModuleBindingVisitor()
    binding_visitor.visit(module)
    if binding_visitor.has_wildcard_import:
        return None
    matching_bindings = [node for name, node in binding_visitor.bindings if name == function_name]
    if len(matching_bindings) != 1:
        return None
    function = matching_bindings[0]
    if not isinstance(function, ast.FunctionDef) or function not in module.body or function.decorator_list:
        return None
    if getattr(function.args, "posonlyargs", ()):
        return None

    try:
        positional = [*getattr(function.args, "posonlyargs", ()), *function.args.args]
        positional_defaults = [inspect.Parameter.empty] * (len(positional) - len(function.args.defaults)) + [
            object() for _ in function.args.defaults
        ]
        parameters: list[inspect.Parameter] = []
        positional_only = len(getattr(function.args, "posonlyargs", ()))
        for index, (argument, default) in enumerate(zip(positional, positional_defaults)):
            kind = (
                inspect.Parameter.POSITIONAL_ONLY
                if index < positional_only
                else inspect.Parameter.POSITIONAL_OR_KEYWORD
            )
            parameters.append(
                inspect.Parameter(
                    argument.arg,
                    kind,
                    default=default,
                    annotation=_script_annotation(argument.annotation),
                )
            )
        for argument, default_node in zip(function.args.kwonlyargs, function.args.kw_defaults):
            parameters.append(
                inspect.Parameter(
                    argument.arg,
                    inspect.Parameter.KEYWORD_ONLY,
                    default=inspect.Parameter.empty if default_node is None else object(),
                    annotation=_script_annotation(argument.annotation),
                )
            )

        defs: dict[str, dict[str, Any]] = {}
        properties: dict[str, Any] = {}
        required: list[str] = []
        descriptions: dict[str, str] = {}
        doc = ast.get_docstring(function)
        if doc:

            def _documented_main() -> None:
                return None

            _documented_main.__doc__ = doc
            descriptions = schema_from_doc(_documented_main)
        for parameter in parameters:
            is_opt, inner = _is_optional(parameter.annotation)
            prop = _derive(inner, defs)
            if is_opt:
                prop = {"anyOf": [prop, {"type": "null"}]}
            description = descriptions.get(parameter.name)
            if description:
                prop = {**prop, "description": description}
            properties[parameter.name] = prop
            if parameter.default is inspect.Parameter.empty:
                required.append(parameter.name)
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
        return _require_json(schema, "derived schema contains a non-JSON value")
    except (SyntaxError, TypeError, ValueError):
        return None


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


def _resolve_input_schema(
    handler: Callable[..., Any],
    sig: inspect.Signature,
    hints: dict[str, Any],
) -> dict[str, Any]:
    """Derive the ``inputSchema`` from *handler*'s signature.

    When the handler has exactly one parameter whose type is a dataclass or
    TypedDict, that type's schema is returned directly.  Otherwise, each
    parameter becomes a property of a containing ``object`` schema.
    """
    real_params = [
        p
        for p in sig.parameters.values()
        if p.name != "self" and p.kind not in (inspect.Parameter.VAR_POSITIONAL, inspect.Parameter.VAR_KEYWORD)
    ]

    if len(real_params) == 1:
        only = real_params[0]
        annotation = hints.get(only.name, only.annotation)
        if annotation is not inspect.Parameter.empty and _is_schema_object_type(annotation):
            return derive_schema(annotation)

    return derive_parameters_schema(handler)


def _resolve_output_schema(
    sig: inspect.Signature,
    hints: dict[str, Any],
) -> dict[str, Any] | None:
    """Derive the ``outputSchema`` from *handler*'s return annotation.

    Returns ``None`` when the return type is absent, ``None``, or not
    representable as a JSON Schema — MCP treats ``outputSchema`` as optional.
    """
    return_annotation = hints.get("return", sig.return_annotation)
    if return_annotation in (inspect.Parameter.empty, None, type(None)):
        return None
    try:
        return derive_schema(return_annotation)
    except TypeError:
        # Unsupported return type — leave outputSchema unset rather than
        # failing registration.
        return None


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
    hints = _get_type_hints(handler, include_extras=True)

    input_schema = _resolve_input_schema(handler, sig, hints)
    output_schema = _resolve_output_schema(sig, hints)

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
    "derive_script_parameters_schema",
    "schema_from_doc",
    "tool_spec_from_callable",
]
