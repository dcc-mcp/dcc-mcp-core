"""Managed temp-script files for legacy callers that only need a path.

``write_temp_script(...)`` is the legacy convenience wrapper for callers that
only need a path string. New code should use
``script_materialization.materialize_script(...)`` directly so audit, cleanup,
reuse, and FileRef-compatible metadata stay available.

Cleanup happens automatically on interpreter exit; adapters can also call
``cleanup_temp_scripts()`` explicitly when the DCC server shuts down.
"""

from __future__ import annotations

import contextlib
from pathlib import Path

from dcc_mcp_core.script_materialization import cleanup_materialized_scripts
from dcc_mcp_core.script_materialization import materialize_script

_TEMP_SCRIPT_DIR = Path.home() / ".dcc-mcp-core" / "temp_scripts"


def write_temp_script(
    content: str,
    *,
    suffix: str = ".py",
    prefix: str = "dcc_mcp_",
) -> str:
    """Write *content* to a managed temp file and return the absolute path.

    The file is created through the script materialization store so that
    both the AI agent (producer) and the in-process executor (consumer)
    agree on the location without passing the code over JSON.

    Args:
        content: Python source to write.
        suffix: Filename suffix (default ``.py``).
        prefix: Filename prefix (default ``dcc_mcp_``).

    Returns:
        Absolute path of the created temp file.

    """
    descriptor = materialize_script(
        content,
        dcc_type="generic",
        instance_id="local",
        session_id="default",
        suffix=suffix,
        root=_TEMP_SCRIPT_DIR,
        prefix=prefix,
    )
    return descriptor.file_path


def cleanup_temp_scripts() -> None:
    """Delete all files under the managed temp-script directory.

    Safe to call multiple times; missing directory is ignored.
    Adapters should call this on server shutdown (``register_quit_hook``).
    """
    if not _TEMP_SCRIPT_DIR.is_dir():
        return
    cleanup_materialized_scripts(root=_TEMP_SCRIPT_DIR, include_unexpired=True)
    for p in _TEMP_SCRIPT_DIR.iterdir():
        with contextlib.suppress(Exception):
            if p.is_file() or p.is_symlink():
                p.unlink()
