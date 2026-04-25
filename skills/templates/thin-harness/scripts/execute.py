"""Thin-harness execute_python skill script — raw DCC Python execution."""

from __future__ import annotations

from dcc_mcp_core import skill_entry
from dcc_mcp_core import skill_error
from dcc_mcp_core import skill_success


@skill_entry
def execute_python(code: str, timeout_secs: int = 30) -> dict:
    """Execute a Python script string in the live DCC interpreter.

    The script runs in an isolated namespace. Set ``result`` in the script
    to return a value to the caller::

        result = cmds.ls(selection=True)

    Args:
        code: Python source to execute.
        timeout_secs: Execution timeout in seconds. Default 30.

    Returns:
        skill_success with ``output`` key on success, skill_error on failure.

    """
    import traceback

    local_ns: dict = {}
    try:
        exec(compile(code, "<execute_python>", "exec"), {}, local_ns)
        output = local_ns.get("result")
        return skill_success("Script executed", output=output)
    except Exception as exc:
        return skill_error(
            f"Script raised {type(exc).__name__}: {exc}",
            underlying_call=code[:300],
            traceback=traceback.format_exc(),
        )
