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
import json
import sys
import traceback
from typing import Any
from typing import Callable
from typing import Dict
from typing import TypeVar

__all__ = [
    # CLI runner
    "run_main",
    # Decorator
    "skill_entry",
    "skill_error",
    "skill_exception",
    # Result builders (return plain dict — no _core dependency required)
    "skill_success",
    "skill_warning",
]

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
    """Return a success result dict compatible with ``ActionResultModel``.

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
    """Return a failure result dict compatible with ``ActionResultModel``.

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
    """Execute *main_fn* and print the JSON result to stdout.

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
    * The function currently ignores *argv* (no CLI arg parser is bundled).
      Future versions may parse ``--key=value`` pairs into kwargs.
    * Exit code ``0`` on success, ``1`` on failure (``result["success"] is False``).

    """
    result: ResultDict = {}
    try:
        result = main_fn()
    except Exception as exc:
        result = skill_exception(exc)

    try:
        output = json.dumps(result, default=str, ensure_ascii=False)
    except (TypeError, ValueError) as exc:
        output = json.dumps(
            skill_error("Failed to serialize result", repr(exc)),
            ensure_ascii=False,
        )

    sys.stdout.write(output + "\n")
    sys.stdout.flush()
    sys.exit(0 if result.get("success", False) else 1)


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _guess_dcc_from_import_error(exc: ImportError) -> str:
    """Best-effort guess of the DCC name from an ImportError message."""
    msg = str(exc).lower()
    for dcc in ("maya", "houdini", "nuke", "blender", "cinema4d", "3dsmax", "unreal"):
        if dcc in msg:
            return dcc.capitalize()
    # Check module name if available (Python 3.6+)
    if exc.name:
        top = exc.name.split(".")[0]
        return top
    return "DCC"
