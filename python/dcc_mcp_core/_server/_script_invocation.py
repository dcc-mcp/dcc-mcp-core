"""Calling conventions shared by in-process adapter script entry points."""

from __future__ import annotations

from collections.abc import Sequence
from collections.abc import Set
import inspect
import types
import typing
from typing import Any
from typing import Callable
from typing import Mapping
from typing import Union
from typing import get_type_hints

from dcc_mcp_core.cancellation import DccMcpCancelledError


def _rejects_mapping_payload(annotation: Any) -> bool:
    origin = getattr(annotation, "__origin__", None) or annotation
    literal = getattr(typing, "Literal", None)
    if literal is not None and origin is literal:
        values = getattr(annotation, "__args__", ())
        return bool(values) and all(_rejects_mapping_payload(type(value)) for value in values)
    return isinstance(origin, type) and issubclass(
        origin, (bool, int, float, complex, str, bytes, Sequence, Set, type(None))
    )


def call_script_main(main: Callable[..., Any], params: Mapping[str, Any]) -> Any:
    """Invoke a skill ``main`` using the best supported calling convention."""
    params_dict = {key: value for key, value in params.items() if not key.startswith("_")}
    try:
        signature = inspect.signature(main)
    except (TypeError, ValueError):
        return main(**params_dict)

    parameters = list(signature.parameters.values())
    if any(param.kind is inspect.Parameter.VAR_KEYWORD for param in parameters):
        return main(**params_dict)

    keyword_names = {
        param.name
        for param in parameters
        if param.kind in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY)
    }
    if set(params_dict).issubset(keyword_names):
        return main(**params_dict)

    positional = [
        param
        for param in parameters
        if param.kind in (inspect.Parameter.POSITIONAL_ONLY, inspect.Parameter.POSITIONAL_OR_KEYWORD)
    ]
    required = [
        param
        for param in parameters
        if param.default is inspect.Parameter.empty
        and param.kind
        in (
            inspect.Parameter.POSITIONAL_ONLY,
            inspect.Parameter.POSITIONAL_OR_KEYWORD,
            inspect.Parameter.KEYWORD_ONLY,
        )
    ]
    if len(positional) == 1 and len(required) <= 1:
        parameter = positional[0]
        annotation = parameter.annotation
        if isinstance(annotation, str):
            # shortcut: unresolved host hints keep legacy mapping until a complete annotation is available.
            try:
                target = inspect.unwrap(main)
                hints = type("_ParameterHint", (), {"__annotations__": {parameter.name: annotation}})
                annotation = get_type_hints(hints, globalns=getattr(target, "__globals__", None))[parameter.name]
            except DccMcpCancelledError:
                raise
            except Exception:
                pass
        options = (
            annotation.__args__
            if getattr(annotation, "__origin__", None) is Union
            or isinstance(annotation, getattr(types, "UnionType", ()))
            else (annotation,)
        )
        if options and all(_rejects_mapping_payload(option) for option in options):
            signature.bind_partial(**params_dict)
            return main(**params_dict)
        return main(params_dict)

    return main(**params_dict)
