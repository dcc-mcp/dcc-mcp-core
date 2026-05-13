"""Thin-harness execute_python skill - supports file_path and IDE-like context."""

from __future__ import annotations

from pathlib import Path

from dcc_mcp_core import normalize_script_execution_params
from dcc_mcp_core import skill_entry
from dcc_mcp_core import skill_error
from dcc_mcp_core import skill_success
from dcc_mcp_core.script_execution import execute_with_context


@skill_entry
def execute_python(
    code: str | None = None,
    *,
    file_path: str | None = None,
    timeout_secs: int | None = None,
) -> dict:
    """Execute a Python script in the live DCC interpreter.

    Two calling modes:

    *Temp-file mode* (recommended):
      Call ``write_temp_file(content=...)`` first; pass the returned
      ``file_path`` here.  This avoids JSON-escaping and long-string
      transfer problems.

    *Inline-code mode* (legacy, do not use for new workflows):
      Pass the code directly as ``code=...``.  A deprecation warning
      is emitted.

    The script runs with access to:
      - DCC application globals (``cmds``, ``hou``, ``bpy``, ...)
        registered via
        ``script_execution.register_dcc_namespace(vars(__main__))``.
      - Persistent variables from earlier ``execute_python`` calls
        (IDE-style variable sharing).

    Set ``result`` in the script to return a value to the caller::

        result = cmds.ls(selection=True)

    Args:
        code: Python source string (legacy, avoid).
        file_path: Path returned by ``write_temp_file()`` (recommended).
        timeout_secs: Execution timeout hint in seconds.

    Returns:
        skill_success with ``output`` key on success, skill_error on failure.

    """
    import traceback
    import warnings

    # Resolve the code string and display name
    if file_path is not None:
        path = Path(file_path)
        if not path.is_file():
            return skill_error(f"File not found: {file_path}")
        code_str = path.read_text(encoding="utf-8")
        filename = str(path)
    elif code is not None:
        code_str = code
        filename = "<execute_python>"
        warnings.warn(
            "Passing 'code' as a string is deprecated. Use write_temp_file() + file_path=... instead.",
            DeprecationWarning,
            stacklevel=2,
        )
    else:
        return skill_error("execute_python requires either 'code' or 'file_path'")

    params = normalize_script_execution_params(
        {"code": code_str, "timeout_secs": timeout_secs},
        default_timeout_secs=30,
    )

    try:
        output = execute_with_context(code_str, filename=filename)
        return skill_success(
            "Script executed",
            output=output,
            timeout_secs=params.timeout_secs,
        )
    except Exception as exc:
        return skill_error(
            f"Script raised {type(exc).__name__}: {exc}",
            underlying_call=code_str[:300],
            traceback=traceback.format_exc(),
        )
