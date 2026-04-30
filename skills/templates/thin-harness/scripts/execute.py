"""Thin-harness execute_python skill script — raw DCC Python execution."""

from __future__ import annotations

from dcc_mcp_core import normalize_script_execution_params
from dcc_mcp_core import skill_entry
from dcc_mcp_core import skill_error
from dcc_mcp_core import skill_success


@skill_entry
def execute_python(
    code: str | None = None,
    script: str | None = None,
    source: str | None = None,
    timeout_secs: int | None = None,
    timeout: int | None = None,
) -> dict:
    """Execute a Python script string in the live DCC interpreter.

    The script runs in an isolated namespace. Set ``result`` in the script
    to return a value to the caller::

        result = cmds.ls(selection=True)

    Args:
        code: Python source to execute.
        script: Alias for ``code``.
        source: Alias for ``code``.
        timeout_secs: Execution timeout hint in seconds.
        timeout: Alias for ``timeout_secs``.

    Returns:
        skill_success with ``output`` key on success, skill_error on failure.

    """
    import traceback

    params = normalize_script_execution_params(
        {
            "code": code,
            "script": script,
            "source": source,
            "timeout_secs": timeout_secs,
            "timeout": timeout,
        },
        default_timeout_secs=30,
    )
    local_ns: dict = {}
    try:
        exec(compile(params.code, "<execute_python>", "exec"), {}, local_ns)
        output = local_ns.get("result")
        return skill_success("Script executed", output=output, timeout_secs=params.timeout_secs)
    except Exception as exc:
        return skill_error(
            f"Script raised {type(exc).__name__}: {exc}",
            underlying_call=params.code[:300],
            traceback=traceback.format_exc(),
        )
