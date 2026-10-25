"""Calling conventions shared by in-process adapter script entry points."""

from __future__ import annotations

from contextlib import suppress
import inspect
import types
from typing import Any
from typing import Callable
from typing import Mapping
from typing import Union
from typing import get_type_hints


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
            with suppress(NameError, TypeError):
                annotation = get_type_hints(inspect.unwrap(main)).get(parameter.name, annotation)
        options = (
            annotation.__args__
            if getattr(annotation, "__origin__", None) is Union
            or isinstance(annotation, getattr(types, "UnionType", ()))
            else (annotation,)
        )
        if options and all(
            (getattr(option, "__origin__", None) or option)
            in (bool, int, float, str, bytes, list, tuple, set, frozenset, type(None))
            for option in options
        ):
            signature.bind_partial(**params_dict)
            return main(**params_dict)
        return main(params_dict)

    return main(**params_dict)
