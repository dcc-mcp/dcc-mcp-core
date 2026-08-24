"""Runtime namespace discovery tools for DCC host interpreters (issue #426).

Provides four read-only MCP tools that let AI agents inspect the live DCC
Python namespace without burning tokens on web searches or relying on stale
training data:

- ``dcc_introspect__list_module`` — list exported names in a module
- ``dcc_introspect__signature``   — get the signature and docstring of a callable
- ``dcc_introspect__search``      — regex-search names across a module
- ``dcc_introspect__eval``        — evaluate a short read-only expression

All tools are registered with ``read_only_hint=True, idempotent_hint=True``
and hard-cap their output to avoid blowing the agent's context window.

Usage::

    from dcc_mcp_core.introspect import register_introspect_tools

    # Attach tools before server.start()
    register_introspect_tools(server, dcc_name="maya")

"""

from __future__ import annotations

import ast
import builtins
import contextlib
import importlib
import inspect
import logging
import re
import traceback
from typing import Any

from dcc_mcp_core import json_loads
from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools
from dcc_mcp_core.constants import CATEGORY_INTROSPECT
from dcc_mcp_core.result_envelope import ToolResultEnvelope

logger = logging.getLogger(__name__)

# Hard output caps
_MAX_NAMES = 200  # max entries from list_module
_MAX_HITS = 50  # max hits from search
_DOC_MAX_CHARS = 800  # max chars for docstring truncation
_REPR_MAX_CHARS = 500  # max chars for eval repr
_EVAL_MAX_CHARS = 2_000
_EVAL_MAX_AST_NODES = 256
_EVAL_MAX_CONTAINER_ITEMS = 10_000
_EVAL_MAX_INT_BITS = 4_096

_SAFE_EVAL_BUILTINS: dict[str, Any] = {
    name: getattr(builtins, name)
    for name in (
        "abs",
        "all",
        "any",
        "bool",
        "dict",
        "float",
        "int",
        "len",
        "list",
        "max",
        "min",
        "range",
        "repr",
        "round",
        "set",
        "sorted",
        "str",
        "sum",
        "tuple",
        "type",
    )
}


class _UnsafeExpression(ValueError):
    """Raised when a purported read-only expression leaves the safe subset."""


def _constant_value(node: ast.AST) -> tuple[bool, Any]:
    if isinstance(node, ast.Constant):
        return True, node.value
    legacy_attribute = {
        "Num": "n",
        "Str": "s",
        "Bytes": "s",
        "NameConstant": "value",
    }.get(type(node).__name__)
    if legacy_attribute is not None:
        return True, getattr(node, legacy_attribute)
    return False, None


def _literal_int(node: ast.AST) -> int | None:
    is_constant, value = _constant_value(node)
    if is_constant and isinstance(value, int) and not isinstance(value, bool):
        return value
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.UAdd, ast.USub)):
        operand = _literal_int(node.operand)
        if operand is not None:
            return operand if isinstance(node.op, ast.UAdd) else -operand
    return None


def _is_numeric_expression(node: ast.AST) -> bool:
    is_constant, value = _constant_value(node)
    if is_constant:
        return isinstance(value, (int, float, complex)) and not isinstance(value, bool)
    if isinstance(node, ast.UnaryOp):
        return isinstance(node.op, (ast.UAdd, ast.USub, ast.Invert)) and _is_numeric_expression(node.operand)
    if isinstance(node, ast.BinOp):
        return _is_numeric_expression(node.left) and _is_numeric_expression(node.right)
    return False


def _integer_bit_bound(node: ast.AST) -> int | None:
    value = _literal_int(node)
    if value is not None:
        return max(1, abs(value).bit_length())
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.UAdd, ast.USub, ast.Invert)):
        return _integer_bit_bound(node.operand)
    if not isinstance(node, ast.BinOp):
        return None
    left = _integer_bit_bound(node.left)
    right = _integer_bit_bound(node.right)
    if left is None or right is None:
        return None
    if isinstance(node.op, (ast.Add, ast.Sub)):
        return max(left, right) + 1
    if isinstance(node.op, ast.Mult):
        return left + right
    if isinstance(node.op, ast.Pow):
        exponent = _literal_int(node.right)
        if exponent is not None and exponent >= 0:
            return left * exponent
        return None
    if isinstance(node.op, ast.LShift):
        shift = _literal_int(node.right)
        return left + shift if shift is not None and shift >= 0 else None
    if isinstance(node.op, ast.RShift):
        shift = _literal_int(node.right)
        return max(1, left - shift) if shift is not None and shift >= 0 else None
    if isinstance(node.op, (ast.FloorDiv, ast.Mod, ast.BitAnd, ast.BitOr, ast.BitXor)):
        return max(left, right)
    return None


class _ReadOnlyExpressionValidator(ast.NodeVisitor):
    """Validate the literal-only expression subset accepted by introspect_eval."""

    def __init__(self) -> None:
        self._materialized_items = 0

    def reject(self, node: ast.AST, reason: str) -> None:
        raise _UnsafeExpression(f"{reason} ({type(node).__name__})")

    def generic_visit(self, node: ast.AST) -> None:
        self.reject(node, "construct is not allowed")

    def visit_Expression(self, node: ast.Expression) -> None:
        self.visit(node.body)

    def visit_Constant(self, node: ast.Constant) -> None:
        self._validate_constant(node, node.value)

    def visit_Num(self, node: ast.AST) -> None:
        self._validate_constant(node, _constant_value(node)[1])

    def visit_Str(self, node: ast.AST) -> None:
        self._validate_constant(node, _constant_value(node)[1])

    def visit_Bytes(self, node: ast.AST) -> None:
        self._validate_constant(node, _constant_value(node)[1])

    def visit_NameConstant(self, node: ast.AST) -> None:
        self._validate_constant(node, _constant_value(node)[1])

    def _validate_constant(self, node: ast.AST, value: Any) -> None:
        if value is None or isinstance(value, (bool, float, complex)):
            return
        if isinstance(value, int):
            if abs(value).bit_length() > _EVAL_MAX_INT_BITS:
                self.reject(node, "integer literal exceeds the size limit")
            return
        if isinstance(value, (str, bytes)) and len(value) <= _EVAL_MAX_CHARS:
            return
        self.reject(node, "constant type or size is not allowed")

    def visit_List(self, node: ast.List) -> None:
        self._visit_values(node, node.elts)

    def visit_Tuple(self, node: ast.Tuple) -> None:
        self._visit_values(node, node.elts)

    def visit_Set(self, node: ast.Set) -> None:
        self._visit_values(node, node.elts)

    def _visit_values(self, node: ast.AST, values: list[ast.expr]) -> None:
        self._reserve_items(node, len(values))
        for value in values:
            self.visit(value)

    def _reserve_items(self, node: ast.AST, count: int) -> None:
        self._materialized_items += count
        if self._materialized_items > _EVAL_MAX_CONTAINER_ITEMS:
            self.reject(node, "expression exceeds the materialized-item limit")

    def visit_Dict(self, node: ast.Dict) -> None:
        self._reserve_items(node, len(node.keys))
        for key, value in zip(node.keys, node.values):
            if key is None:
                self.reject(node, "dictionary unpacking is not allowed")
            self.visit(key)
            self.visit(value)

    def visit_Name(self, node: ast.Name) -> None:
        if node.id not in _SAFE_EVAL_BUILTINS:
            self.reject(node, f"name {node.id!r} is not available")

    def visit_Call(self, node: ast.Call) -> None:
        if not isinstance(node.func, ast.Name) or node.func.id not in _SAFE_EVAL_BUILTINS:
            self.reject(node, "only direct calls to safe builtins are allowed")
        if len(node.args) + len(node.keywords) > _EVAL_MAX_CONTAINER_ITEMS:
            self.reject(node, "call exceeds the argument limit")
        allowed_keywords = {"reverse"} if node.func.id == "sorted" else set()
        for keyword in node.keywords:
            if keyword.arg is None or keyword.arg not in allowed_keywords:
                self.reject(keyword, f"keyword argument is not allowed for {node.func.id}")
            self.visit(keyword.value)
        for argument in node.args:
            self.visit(argument)
        if node.func.id == "range":
            self._validate_range(node)

    def _validate_range(self, node: ast.Call) -> None:
        values = [_literal_int(argument) for argument in node.args]
        if not 1 <= len(values) <= 3 or any(value is None for value in values):
            self.reject(node, "range arguments must be one to three integer literals")
        try:
            item_count = len(range(*values))  # type: ignore[arg-type]
        except (OverflowError, ValueError) as exc:
            raise _UnsafeExpression(f"invalid range: {exc}") from exc
        if item_count > _EVAL_MAX_CONTAINER_ITEMS:
            self.reject(node, "range exceeds the item limit")
        self._reserve_items(node, item_count)

    def visit_UnaryOp(self, node: ast.UnaryOp) -> None:
        if not isinstance(node.op, (ast.UAdd, ast.USub, ast.Not, ast.Invert)):
            self.reject(node, "unary operator is not allowed")
        self.visit(node.operand)

    def visit_BinOp(self, node: ast.BinOp) -> None:
        if not isinstance(
            node.op,
            (
                ast.Add,
                ast.Sub,
                ast.Mult,
                ast.Div,
                ast.FloorDiv,
                ast.Mod,
                ast.Pow,
                ast.BitAnd,
                ast.BitOr,
                ast.BitXor,
                ast.LShift,
                ast.RShift,
            ),
        ):
            self.reject(node, "binary operator is not allowed")
        self.visit(node.left)
        self.visit(node.right)
        if isinstance(node.op, (ast.Mult, ast.Pow, ast.LShift, ast.RShift)):
            self._validate_expanding_operation(node)
        bit_bound = _integer_bit_bound(node)
        if bit_bound is not None and bit_bound > _EVAL_MAX_INT_BITS:
            self.reject(node, "integer result exceeds the size limit")

    def _validate_expanding_operation(self, node: ast.BinOp) -> None:
        if not _is_numeric_expression(node.left) or not _is_numeric_expression(node.right):
            self.reject(node, "sequence expansion is not allowed")
        if isinstance(node.op, ast.Pow):
            exponent = _literal_int(node.right)
            base_bits = _integer_bit_bound(node.left)
            if exponent is None or not -1_000 <= exponent <= 1_000:
                self.reject(node, "power exponent must be a bounded integer literal")
            if exponent > 0 and base_bits is not None and base_bits * exponent > _EVAL_MAX_INT_BITS:
                self.reject(node, "power result exceeds the size limit")
        if isinstance(node.op, (ast.LShift, ast.RShift)):
            shift = _literal_int(node.right)
            if shift is None or not 0 <= shift <= _EVAL_MAX_INT_BITS:
                self.reject(node, "shift count must be a bounded integer literal")

    def visit_BoolOp(self, node: ast.BoolOp) -> None:
        if not isinstance(node.op, (ast.And, ast.Or)):
            self.reject(node, "boolean operator is not allowed")
        self._visit_values(node, node.values)

    def visit_Compare(self, node: ast.Compare) -> None:
        allowed = (ast.Eq, ast.NotEq, ast.Lt, ast.LtE, ast.Gt, ast.GtE, ast.Is, ast.IsNot, ast.In, ast.NotIn)
        if not all(isinstance(operator, allowed) for operator in node.ops):
            self.reject(node, "comparison operator is not allowed")
        self.visit(node.left)
        for comparator in node.comparators:
            self.visit(comparator)

    def visit_IfExp(self, node: ast.IfExp) -> None:
        self.visit(node.test)
        self.visit(node.body)
        self.visit(node.orelse)

    def visit_Subscript(self, node: ast.Subscript) -> None:
        self.visit(node.value)
        self.visit(node.slice)

    def visit_Index(self, node: ast.AST) -> None:
        self.visit(node.value)  # type: ignore[attr-defined]

    def visit_Slice(self, node: ast.Slice) -> None:
        for value in (node.lower, node.upper, node.step):
            if value is not None:
                self.visit(value)


# ── Core introspection helpers ────────────────────────────────────────────


def _import_module(module_name: str) -> tuple[Any, str | None]:
    """Return (module, error_str) — error_str is None on success."""
    try:
        return importlib.import_module(module_name), None
    except ImportError as exc:
        return None, f"Cannot import module '{module_name}': {exc}"
    except Exception as exc:
        return None, f"Error importing module '{module_name}': {exc}"


def introspect_list_module(module_name: str, *, limit: int = _MAX_NAMES) -> dict[str, Any]:
    """Return exported names from *module_name*.

    Parameters
    ----------
    module_name:
        Dotted module path (e.g. ``"maya.cmds"`` or ``"bpy.ops.object"``).
    limit:
        Maximum number of names to return (default :data:`_MAX_NAMES`).

    Returns
    -------
    dict
        ``{"names": [...], "count": N, "truncated": bool}``

    """
    mod, err = _import_module(module_name)
    if err:
        return ToolResultEnvelope(success=False, message=err).to_dict()

    names = list(mod.__all__) if hasattr(mod, "__all__") else [n for n in dir(mod) if not n.startswith("_")]

    names.sort()
    truncated = len(names) > limit
    return ToolResultEnvelope.ok(
        f"{len(names)} names in {module_name}" + (" (truncated)" if truncated else ""),
        module=module_name,
        names=names[:limit],
        count=len(names),
        truncated=truncated,
    ).to_dict()


def introspect_signature(qualname: str) -> dict[str, Any]:
    """Return signature and docstring for *qualname*.

    Parameters
    ----------
    qualname:
        Fully-qualified name such as ``"maya.cmds.polyCube"`` or
        ``"bpy.ops.object.join"``.

    Returns
    -------
    dict
        ``{"signature": str, "doc": str, "source_file": str|None}``

    """
    parts = qualname.rsplit(".", 1)
    if len(parts) == 1:
        module_name, attr = "builtins", parts[0]
    else:
        module_name, attr = parts

    mod, err = _import_module(module_name)
    if err:
        return ToolResultEnvelope(success=False, message=err).to_dict()

    obj = getattr(mod, attr, None)
    if obj is None:
        return ToolResultEnvelope(success=False, message=f"'{attr}' not found in '{module_name}'").to_dict()

    # Signature
    sig_str = ""
    try:
        sig = inspect.signature(obj)
        sig_str = f"{attr}{sig}"
    except (ValueError, TypeError):
        sig_str = f"{attr}(...)"

    # Docstring
    doc = inspect.getdoc(obj) or ""
    if len(doc) > _DOC_MAX_CHARS:
        doc = doc[:_DOC_MAX_CHARS] + "\n...(truncated)"

    # Source file
    source_file: str | None = None
    with contextlib.suppress(TypeError, OSError):
        source_file = inspect.getfile(obj)

    return ToolResultEnvelope.ok(
        f"Signature for {qualname}",
        qualname=qualname,
        signature=sig_str,
        doc=doc,
        source_file=source_file,
        kind=type(obj).__name__,
    ).to_dict()


def introspect_search(
    pattern: str,
    module_name: str,
    *,
    limit: int = _MAX_HITS,
) -> dict[str, Any]:
    """Regex-search exported names in *module_name*.

    Parameters
    ----------
    pattern:
        Regular expression (case-insensitive).
    module_name:
        Dotted module path to search.
    limit:
        Maximum hits to return (default :data:`_MAX_HITS`).

    Returns
    -------
    dict
        ``{"hits": [{"qualname": str, "summary": str}, ...], "count": int}``

    """
    try:
        regex = re.compile(pattern, re.IGNORECASE)
    except re.error as exc:
        return ToolResultEnvelope(success=False, message=f"Invalid regex '{pattern}': {exc}").to_dict()

    mod, err = _import_module(module_name)
    if err:
        return ToolResultEnvelope(success=False, message=err).to_dict()

    all_names: list[str] = list(getattr(mod, "__all__", None) or [n for n in dir(mod) if not n.startswith("_")])
    hits: list[dict[str, str]] = []

    for name in all_names:
        if not regex.search(name):
            continue
        obj = getattr(mod, name, None)
        summary = ""
        if obj is not None:
            raw_doc = inspect.getdoc(obj) or ""
            first_line = raw_doc.split("\n", 1)[0].strip()
            summary = first_line[:120] if first_line else type(obj).__name__
        hits.append({"qualname": f"{module_name}.{name}", "summary": summary})
        if len(hits) >= limit:
            break

    return ToolResultEnvelope.ok(
        f"{len(hits)} matches for '{pattern}' in '{module_name}'",
        pattern=pattern,
        module=module_name,
        hits=hits,
        count=len(hits),
        truncated=len(hits) >= limit,
    ).to_dict()


def introspect_eval(expression: str) -> dict[str, Any]:
    """Evaluate a bounded literal expression and return its repr.

    The accepted subset contains literals, bounded operators, indexing, and a
    small allowlist of pure builtins. Attribute access, comprehensions,
    imports, assignments, lambdas, and dynamic builtin lookup are unavailable.

    Parameters
    ----------
    expression:
        A short expression string (for example ``"type([1, 2, 3])"`` or
        ``"sorted([3, 1, 2], reverse=True)"``).

    Returns
    -------
    dict
        ``{"repr": str}`` on success, or ``{"success": False, "message": err}``
        on any error.

    """
    stripped = expression.strip()
    if not stripped:
        return ToolResultEnvelope(success=False, message="Expression must not be empty.").to_dict()
    if len(stripped) > _EVAL_MAX_CHARS:
        return ToolResultEnvelope(
            success=False,
            message=f"Expression exceeds the {_EVAL_MAX_CHARS}-character limit.",
        ).to_dict()

    try:
        tree = ast.parse(stripped, mode="eval")
        if sum(1 for _ in ast.walk(tree)) > _EVAL_MAX_AST_NODES:
            raise _UnsafeExpression(f"expression exceeds the {_EVAL_MAX_AST_NODES}-node limit")
        _ReadOnlyExpressionValidator().visit(tree)
        compiled = compile(tree, "<dcc-introspect>", "eval")
        result = eval(compiled, {"__builtins__": {}}, _SAFE_EVAL_BUILTINS)
        repr_str = repr(result)
        if len(repr_str) > _REPR_MAX_CHARS:
            repr_str = repr_str[:_REPR_MAX_CHARS] + "...(truncated)"
    except _UnsafeExpression as exc:
        return ToolResultEnvelope(
            success=False,
            message=f"Expression is outside the read-only subset: {exc}",
        ).to_dict()
    except Exception:
        tb = traceback.format_exc()
        return ToolResultEnvelope(
            success=False,
            message=f"Evaluation failed: {tb.splitlines()[-1]}",
            context={"traceback": tb},
        ).to_dict()

    return ToolResultEnvelope.ok("Expression evaluated.", expression=expression, repr=repr_str).to_dict()


# ── JSON schemas for MCP tools ─────────────────────────────────────────────

_LIST_MODULE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "module": {
            "type": "string",
            "description": "Dotted module path, e.g. 'maya.cmds' or 'bpy.ops.object'.",
        },
        "limit": {
            "type": "integer",
            "description": f"Max names to return (default {_MAX_NAMES}).",
            "default": _MAX_NAMES,
        },
    },
    "required": ["module"],
    "additionalProperties": False,
}

_SIGNATURE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "qualname": {
            "type": "string",
            "description": "Fully-qualified name, e.g. 'maya.cmds.polyCube'.",
        },
    },
    "required": ["qualname"],
    "additionalProperties": False,
}

_SEARCH_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "pattern": {
            "type": "string",
            "description": "Case-insensitive regex to match against exported names.",
        },
        "module": {
            "type": "string",
            "description": "Module to search within, e.g. 'maya.cmds'.",
        },
        "limit": {
            "type": "integer",
            "description": f"Max hits to return (default {_MAX_HITS}).",
            "default": _MAX_HITS,
        },
    },
    "required": ["pattern", "module"],
    "additionalProperties": False,
}

_EVAL_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "expression": {
            "type": "string",
            "description": "Short read-only Python expression to evaluate in the DCC interpreter.",
        },
    },
    "required": ["expression"],
    "additionalProperties": False,
}

_LIST_MODULE_DESCRIPTION = (
    "List exported names in a Python module loaded in the DCC interpreter. "
    "When to use: before writing a script — discover what functions are available "
    "without browsing offline docs. "
    "How to use: pass the dotted module path; use dcc_introspect__search to narrow results."
)

_SIGNATURE_DESCRIPTION = (
    "Return the signature and docstring for a callable in the live DCC interpreter. "
    "When to use: when you have a function name from dcc_introspect__list_module or "
    "dcc_introspect__search and need parameter names and defaults. "
    "How to use: pass the fully-qualified name, e.g. 'maya.cmds.polyCube'."
)

_SEARCH_DESCRIPTION = (
    "Regex-search exported names in a DCC module. "
    "When to use: when you need to find a function but only remember part of its name. "
    "How to use: pass a regex pattern + module; use dcc_introspect__signature on hits."
)

_EVAL_DESCRIPTION = (
    "Evaluate a bounded expression built from literals and allowlisted pure builtins, then return its repr. "
    "When to use: for small type, shape, ordering, or arithmetic checks. "
    "Use list_module, search, and signature for DCC APIs; attributes, imports, comprehensions, and dynamic builtins "
    "are unavailable."
)


# ── MCP tool registration ─────────────────────────────────────────────────


def register_introspect_tools(
    server: Any,
    *,
    dcc_name: str = "dcc",
) -> None:
    """Register the four ``dcc_introspect__*`` tools on *server*.

    All tools are annotated ``read_only_hint=True, idempotent_hint=True``.
    Register them **before** calling ``server.start()``.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` compatible object with ``server.registry``
        and ``server.register_handler(name, fn)``.
    dcc_name:
        DCC name string for tool metadata tagging.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import McpHttpServer, McpHttpConfig
        from dcc_mcp_core.introspect import register_introspect_tools

        server = McpHttpServer(registry, McpHttpConfig(port=8765))
        register_introspect_tools(server, dcc_name="maya")
        handle = server.start()

    """

    def _handler(fn):
        """Wrap a function to accept JSON string or dict params."""

        def wrapper(params: Any) -> Any:
            args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
            args.pop("_meta", None)
            return fn(**args)

        return wrapper

    specs = [
        ToolSpec(
            name="dcc_introspect__list_module",
            description=_LIST_MODULE_DESCRIPTION,
            input_schema=_LIST_MODULE_SCHEMA,
            handler=_handler(lambda module, limit=_MAX_NAMES: introspect_list_module(module, limit=limit)),
            category=CATEGORY_INTROSPECT,
        ),
        ToolSpec(
            name="dcc_introspect__signature",
            description=_SIGNATURE_DESCRIPTION,
            input_schema=_SIGNATURE_SCHEMA,
            handler=_handler(lambda qualname: introspect_signature(qualname)),
            category=CATEGORY_INTROSPECT,
        ),
        ToolSpec(
            name="dcc_introspect__search",
            description=_SEARCH_DESCRIPTION,
            input_schema=_SEARCH_SCHEMA,
            handler=_handler(lambda pattern, module, limit=_MAX_HITS: introspect_search(pattern, module, limit=limit)),
            category=CATEGORY_INTROSPECT,
        ),
        ToolSpec(
            name="dcc_introspect__eval",
            description=_EVAL_DESCRIPTION,
            input_schema=_EVAL_SCHEMA,
            handler=_handler(lambda expression: introspect_eval(expression)),
            category=CATEGORY_INTROSPECT,
        ),
    ]

    register_tools(
        server,
        specs,
        dcc_name=dcc_name,
        log_prefix="register_introspect_tools",
        logger=logger,
    )


# ── Public API ─────────────────────────────────────────────────────────────

__all__ = [
    "introspect_eval",
    "introspect_list_module",
    "introspect_search",
    "introspect_signature",
    "register_introspect_tools",
]
