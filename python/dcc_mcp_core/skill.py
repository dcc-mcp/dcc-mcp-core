"""Skill script utilities for DCC-MCP skill authors.

This module provides lightweight helpers that make it easy to write
skill scripts conforming to the DCC-MCP skill execution protocol.  It
is intentionally free of hard dependencies on the compiled ``_core``
extension so that scripts can import it inside DCC environments that
may not have the full wheel installed.

Typical usage inside a skill script
-------------------------------------

.. code-block:: python

    from dcc_mcp_core.skill import skill_entry, skill_success, skill_error

    @skill_entry
    def my_tool(name: str = "world", count: int = 1) -> dict:
        # ... do DCC work ...
        return skill_success(
            f"Created {count} objects named {name!r}",
            prompt="Inspect the viewport to verify placement.",
            names=[name] * count,
        )

The ``@skill_entry`` decorator:

* Forwards all ``**kwargs`` received by ``main()`` to your function.
* Catches ``ImportError`` (DCC module not available), ``Exception``, and
  bare ``BaseException``, returning a well-formed error dict in each case.
* Writes the JSON result to *stdout* when the script is executed directly
  (``__name__ == "__main__"``) so agents can capture it.

You can also call the helpers directly without the decorator:

.. code-block:: python

    def set_timeline(start_frame=1.0, end_frame=120.0, **kwargs):
        try:
            import maya.cmds as cmds
            cmds.playbackOptions(min=start_frame, max=end_frame)
            return skill_success("Timeline updated", start=start_frame, end=end_frame)
        except ImportError:
            return skill_error("Maya not available", "ImportError: maya.cmds not found")
        except Exception as exc:
            return skill_exception(exc)

    def main(**kwargs):
        return set_timeline(**kwargs)
"""

from __future__ import annotations

import functools
from pathlib import Path
import sys
import traceback
from typing import Any
from typing import Callable
from typing import Dict
from typing import TypeVar

from dcc_mcp_core import json_dumps

__all__ = [
    "get_bundled_skill_paths",
    "get_bundled_skills_dir",
    "run_main",
    "skill_entry",
    "skill_error",
    "skill_error_with_trace",
    "skill_exception",
    "skill_success",
    "skill_warning",
]

# ---------------------------------------------------------------------------
# Bundled skills directory helpers
# ---------------------------------------------------------------------------

# The ``skills/`` subdirectory is co-located with this module inside the
# installed wheel.  It contains the general-purpose reference skill packages
# (dcc-diagnostics, workflow, git-automation, etc.) that are bundled with
# dcc-mcp-core so users do not need to clone the repository.
_BUNDLED_SKILLS_DIR: Path = Path(__file__).parent / "skills"


def get_bundled_skills_dir() -> str:
    """Return the absolute path to the bundled skills directory.

    The directory contains the general-purpose skill packages shipped with
    ``dcc-mcp-core`` (``dcc-diagnostics``, ``workflow``, ``git-automation``,
    ``ffmpeg-media``, ``imagemagick-tools``).

    Returns:
        Absolute path string.  The directory is guaranteed to exist when the
        package is installed from a wheel; it may not exist in editable/source
        installs unless ``examples/skills/`` was copied to the package.

    Example::

        from dcc_mcp_core.skill import get_bundled_skills_dir
        print(get_bundled_skills_dir())
        # /path/to/site-packages/dcc_mcp_core/skills

    """
    return str(_BUNDLED_SKILLS_DIR)


def get_bundled_skill_paths(include_bundled: bool = True) -> list[str]:
    """Return a list containing the bundled skills directory (when it exists).

    Convenience wrapper used by DCC adapters to build their skill search path.
    Pass ``include_bundled=False`` to disable bundled skills entirely.

    Args:
        include_bundled: If ``False``, return an empty list so callers can
            easily opt-out of the bundled skills.

    Returns:
        A list with the bundled skills directory path, or ``[]`` if the
        directory does not exist or ``include_bundled`` is ``False``.

    Example::

        from dcc_mcp_core.skill import get_bundled_skill_paths

        # Default — include bundled skills
        paths = get_bundled_skill_paths()

        # Opt-out
        paths = get_bundled_skill_paths(include_bundled=False)

    """
    if not include_bundled:
        return []
    bundled = _BUNDLED_SKILLS_DIR
    return [str(bundled)] if bundled.is_dir() else []


# ---------------------------------------------------------------------------
# Type aliases
# ---------------------------------------------------------------------------

ResultDict = Dict[str, Any]
_F = TypeVar("_F", bound=Callable[..., ResultDict])


# ---------------------------------------------------------------------------
# Result builders
# ---------------------------------------------------------------------------


def skill_success(
    message: str,
    *,
    prompt: str | None = None,
    **context: Any,
) -> ResultDict:
    """Return a success result dict compatible with ``ToolResult``.

    Parameters
    ----------
    message:
        Human-readable summary of what was accomplished.
    prompt:
        Optional hint for the agent's next action (e.g.
        ``"Inspect the viewport to verify the result."``).
    **context:
        Arbitrary key/value pairs attached to ``context``.  Use these to
        return structured data (object names, frame counts, file paths …).

    Returns
    -------
    dict
        ``{"success": True, "message": ..., "prompt": ..., "error": None,
        "context": {...}}``

    Example
    -------
    .. code-block:: python

        return skill_success(
            "Timeline set",
            prompt="Check the timeline slider.",
            start_frame=1,
            end_frame=120,
        )

    """
    return {
        "success": True,
        "message": message,
        "prompt": prompt,
        "error": None,
        "context": context,
    }


def skill_error(
    message: str,
    error: str,
    *,
    prompt: str | None = None,
    possible_solutions: list[str] | None = None,
    **context: Any,
) -> ResultDict:
    """Return a failure result dict compatible with ``ToolResult``.

    Parameters
    ----------
    message:
        User-facing description of what went wrong.
    error:
        Technical error string (exception repr, error code …).
    prompt:
        Optional hint for recovery (defaults to a generic "check the error"
        message).
    possible_solutions:
        Optional list of actionable suggestions stored under
        ``context["possible_solutions"]``.
    **context:
        Additional key/value pairs attached to ``context``.

    Example
    -------
    .. code-block:: python

        return skill_error(
            "Failed to create object",
            "NameError: 'polyCube' is not defined",
            prompt="Ensure the Maya plugin is loaded.",
            possible_solutions=["Load plugin: loadPlugin('polyCube')"],
        )

    """
    if possible_solutions:
        context.setdefault("possible_solutions", possible_solutions)
    return {
        "success": False,
        "message": message,
        "prompt": prompt or "Check the error details and try again.",
        "error": error,
        "context": context,
    }


def skill_error_with_trace(
    message: str,
    error: str,
    *,
    underlying_call: str | None = None,
    recipe_hint: str | None = None,
    introspect_hint: str | None = None,
    tb: str | None = None,
    prompt: str | None = None,
    possible_solutions: list[str] | None = None,
    **context: Any,
) -> ResultDict:
    """Return a failure result dict enriched with a diagnostic ``_meta.dcc.raw_trace`` block.

    Designed for thin-harness ``execute_python`` skills and any handler that
    wraps a native DCC API call: the trace block gives the calling agent enough
    context to self-correct the call without asking for a new wrapper tool.

    The ``_meta.dcc.raw_trace`` block is included only when at least one of
    ``underlying_call``, ``recipe_hint``, or ``introspect_hint`` is non-empty.
    When ``McpHttpConfig.enable_error_raw_trace`` is ``False`` (the production
    default), the gateway strips this block before forwarding the response.

    Parameters
    ----------
    message:
        User-facing description of what went wrong.
    error:
        Technical error string (exception repr, error code …).
    underlying_call:
        The raw DCC API call that failed (e.g.
        ``"maya.cmds.polySphere(name='mySphere', radius=-1.0)"``).
        Truncated to 500 chars automatically.
    recipe_hint:
        Path + optional anchor to a recipe that covers this call
        (e.g. ``"references/RECIPES.md#create_sphere"``).
    introspect_hint:
        A ready-to-call ``dcc_introspect__*`` expression that reveals
        the live API contract
        (e.g. ``"dcc_introspect__signature(qualname='maya.cmds.polySphere')"``).
    tb:
        Full formatted traceback string (``traceback.format_exc()``).
        Stored in ``_meta.dcc.raw_trace.traceback``.
    prompt:
        Optional recovery hint for the agent.
    possible_solutions:
        Optional list of actionable suggestions.
    **context:
        Additional key/value pairs attached to ``context``.

    Returns
    -------
    dict
        Standard error dict with an additional ``_meta`` key::

            {
                "success": False,
                "message": ...,
                "error": ...,
                "_meta": {
                    "dcc.raw_trace": {
                        "underlying_call": "...",
                        "traceback": "...",
                        "recipe_hint": "...",
                        "introspect_hint": "...",
                    }
                }
            }

    Example
    -------
    .. code-block:: python

        import traceback as _tb

        try:
            result = cmds.polySphere(name="mySphere", radius=radius)
        except Exception as exc:
            return skill_error_with_trace(
                "Failed to create sphere",
                str(exc),
                underlying_call=f"maya.cmds.polySphere(name='mySphere', radius={radius})",
                recipe_hint="references/RECIPES.md#create_sphere",
                introspect_hint="dcc_introspect__signature(qualname='maya.cmds.polySphere')",
                tb=_tb.format_exc(),
            )

    """
    if possible_solutions:
        context.setdefault("possible_solutions", possible_solutions)

    raw_trace: dict[str, str] = {}
    if underlying_call:
        raw_trace["underlying_call"] = underlying_call[:500]
    if tb:
        raw_trace["traceback"] = tb
    if recipe_hint:
        raw_trace["recipe_hint"] = recipe_hint
    if introspect_hint:
        raw_trace["introspect_hint"] = introspect_hint

    result: ResultDict = {
        "success": False,
        "message": message,
        "prompt": prompt or "Check the error details and try again.",
        "error": error,
        "context": context,
    }
    if raw_trace:
        result["_meta"] = {"dcc.raw_trace": raw_trace}
    return result


def skill_warning(
    message: str,
    *,
    warning: str = "",
    prompt: str | None = None,
    **context: Any,
) -> ResultDict:
    """Return a success-but-with-warning result dict.

    The action succeeded, but there is something the user should be aware of.
    ``context["warning"]`` is set to *warning*.

    Parameters
    ----------
    message:
        Summary of what was done (success perspective).
    warning:
        Description of the condition that should be noted.
    prompt:
        Optional follow-up hint for the agent.
    **context:
        Additional context key/value pairs.

    Example
    -------
    .. code-block:: python

        return skill_warning(
            "Timeline set, but end_frame was clamped to scene length",
            warning="end_frame 9999 > scene length 240; clamped to 240",
            prompt="Verify the timeline slider shows the expected range.",
            actual_end=240,
        )

    """
    context["warning"] = warning
    return {
        "success": True,
        "message": message,
        "prompt": prompt,
        "error": None,
        "context": context,
    }


def skill_exception(
    exc: BaseException,
    *,
    message: str | None = None,
    prompt: str | None = None,
    include_traceback: bool = True,
    possible_solutions: list[str] | None = None,
    **context: Any,
) -> ResultDict:
    """Return a failure result dict built from an exception.

    Captures the exception type, repr, and optionally the full traceback
    and stores them in ``context``.

    Parameters
    ----------
    exc:
        The caught exception.
    message:
        Optional custom message.  Defaults to ``"Error: <exc>"``.
    prompt:
        Optional recovery hint.
    include_traceback:
        When ``True`` (default), attach the formatted traceback to
        ``context["traceback"]``.
    possible_solutions:
        Optional list of actionable suggestions.
    **context:
        Additional context key/value pairs.

    Example
    -------
    .. code-block:: python

        try:
            do_work()
        except Exception as exc:
            return skill_exception(exc, possible_solutions=["Check file path"])

    """
    error_str = repr(exc)
    error_type = type(exc).__name__
    context["error_type"] = error_type
    if include_traceback:
        context["traceback"] = traceback.format_exc()
    if possible_solutions:
        context.setdefault("possible_solutions", possible_solutions)
    return {
        "success": False,
        "message": message or f"Error: {exc}",
        "prompt": prompt or "Check the error details and try again.",
        "error": error_str,
        "context": context,
    }


# ---------------------------------------------------------------------------
# @skill_entry decorator
# ---------------------------------------------------------------------------


def skill_entry(func: _F) -> _F:
    """Wrap a skill function with standard error handling.

    The decorated function **must** accept ``**kwargs`` and return a
    ``ResultDict``.  The decorator:

    1. Creates a ``main(**kwargs)`` shim that forwards to *func*.
    2. Catches ``ImportError`` (DCC module missing), generic ``Exception``,
       and bare ``BaseException``, converting each to a proper error dict.
    3. When the module is executed directly (``__name__ == "__main__"``),
       prints the JSON result to stdout — ready for agent capture.

    Usage
    -----
    .. code-block:: python

        from dcc_mcp_core.skill import skill_entry, skill_success

        @skill_entry
        def set_timeline(start_frame: float = 1.0, end_frame: float = 120.0):
            import maya.cmds as cmds
            cmds.playbackOptions(min=start_frame, max=end_frame)
            return skill_success("Timeline updated", start=start_frame, end=end_frame)

        # main() is auto-generated — call it as the script entry point.
        # When run directly the JSON result is printed to stdout.

    The decorator preserves ``__name__``, ``__doc__``, and ``__module__`` of
    the original function via ``functools.wraps``.
    """

    @functools.wraps(func)
    def wrapper(**kwargs: Any) -> ResultDict:
        try:
            return func(**kwargs)
        except ImportError as exc:
            dcc_name = _guess_dcc_from_import_error(exc)
            return skill_error(
                f"{dcc_name} is not available in this environment",
                repr(exc),
                prompt=f"Ensure {dcc_name} is running and the plugin is loaded.",
            )
        except Exception as exc:
            return skill_exception(exc)
        except BaseException as exc:
            return skill_error(
                "Skill execution was interrupted",
                repr(exc),
                prompt="The skill was forcibly stopped; retry if needed.",
            )

    # Attach a `main` name alias so callers can use `main(**kwargs)` pattern.
    wrapper.__name__ = func.__name__  # keep original name on the wrapper

    # Expose a module-level main() at the call site via a sentinel attribute.
    wrapper._is_skill_entry = True  # type: ignore[attr-defined]

    return wrapper  # type: ignore[return-value]


# ---------------------------------------------------------------------------
# CLI runner
# ---------------------------------------------------------------------------


def run_main(main_fn: Callable[..., ResultDict], argv: list[str] | None = None) -> None:
    """Execute *main_fn* and print the serialized result to stdout.

    Intended for use in ``if __name__ == "__main__"`` blocks:

    .. code-block:: python

        if __name__ == "__main__":
            from dcc_mcp_core.skill import run_main
            run_main(main)

    Parameters
    ----------
    main_fn:
        The entry-point function (typically the ``main`` or ``@skill_entry``
        decorated function).
    argv:
        If given, overrides ``sys.argv[1:]`` for argument parsing.  When
        ``None`` (default) the function is called with no arguments, which
        causes each parameter's default value to be used.

    Notes
    -----
    * Serialization uses the Rust ``serialize_result()`` implementation when
      the compiled ``_core`` extension is available.  This is type-safe,
      format-agnostic (JSON now, MessagePack in the future), and validates the
      result through ``ToolResult``.
    * Falls back to ``json.dumps`` in DCC environments where only the pure-Python
      wheel is installed.
    * The function currently ignores *argv* (no CLI arg parser is bundled).
      Future versions may parse ``--key=value`` pairs into kwargs.
    * Exit code ``0`` on success, ``1`` on failure (``result["success"] is False``).

    """
    result: ResultDict = {}
    try:
        result = main_fn()
    except Exception as exc:
        result = skill_exception(exc)

    output = _serialize_result(result)
    sys.stdout.write(output + "\n")
    sys.stdout.flush()
    sys.exit(0 if result.get("success", False) else 1)


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _serialize_result(result: ResultDict) -> str:
    """Serialize a result dict to a JSON string.

    Tries the Rust ``serialize_result()`` path first (type-safe, validates via
    ``ToolResult``, format-extensible).  Falls back to ``json.dumps``
    when the compiled ``_core`` extension is not available (e.g. standalone
    DCC environment with only this module installed).

    Parameters
    ----------
    result:
        A dict conforming to the ``ToolResult`` schema
        (keys: success, message, prompt, error, context).

    Returns
    -------
    str
        JSON-encoded result string (no trailing newline).

    """
    try:
        # Import lazily so skill.py itself has no hard _core dependency.
        from dcc_mcp_core._core import SerializeFormat
        from dcc_mcp_core._core import serialize_result
        from dcc_mcp_core._core import validate_action_result

        arm = validate_action_result(result)
        return serialize_result(arm, SerializeFormat.Json)
    except ImportError:
        pass  # _core not available — fall back to pure Python

    # Pure-Python fallback: handles any extra keys in context gracefully.
    try:
        return json_dumps(result, ensure_ascii=False)
    except (TypeError, ValueError) as exc:
        return json_dumps(
            skill_error("Failed to serialize result", repr(exc)),
            ensure_ascii=False,
        )


_DCC_IMPORT_LABELS = {
    "maya": "Maya",
    "houdini": "Houdini",
    "nuke": "Nuke",
    "blender": "Blender",
    "cinema4d": "Cinema 4D",
    "c4d": "Cinema 4D",
    "3dsmax": "3ds Max",
    "unreal": "Unreal",
    "unity": "Unity",
    "photoshop": "Photoshop",
    "zbrush": "ZBrush",
    "figma": "Figma",
}


def _guess_dcc_from_import_error(exc: ImportError) -> str:
    """Best-effort guess of the DCC name from an ImportError message."""
    if exc.name:
        top = exc.name.split(".")[0].lower()
        return _DCC_IMPORT_LABELS.get(top, top)

    msg = str(exc).lower()
    for dcc, label in _DCC_IMPORT_LABELS.items():
        if dcc in msg:
            return label
    return "DCC"
