"""write_temp_file skill - write content to a managed temp file."""

from __future__ import annotations

from dcc_mcp_core import skill_entry
from dcc_mcp_core import skill_success
from dcc_mcp_core.script_execution import write_temp_script


@skill_entry
def write_temp_file(
    content: str,
    *,
    suffix: str = ".py",
    prefix: str = "dcc_mcp_",
) -> dict:
    """Write *content* to a temp file and return the path.

    Use the returned ``file_path`` as the ``file_path`` argument of
    ``execute_python()``.  This avoids passing long code strings
    (and their escaping problems) inside the ``execute_python`` tool call.

    Args:
        content: Python source to write.
        suffix: Filename suffix (default ``.py``).
        prefix: Filename prefix (default ``dcc_mcp_``).

    Returns:
        skill_success with ``file_path`` key.

    """
    path = write_temp_script(content, suffix=suffix, prefix=prefix)
    return skill_success("Temp script written", file_path=path)
