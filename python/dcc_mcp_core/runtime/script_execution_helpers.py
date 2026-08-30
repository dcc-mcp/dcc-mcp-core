"""Small helpers shared by file-backed script execution contracts."""

from __future__ import annotations

from pathlib import Path
from typing import Any


def file_ref_for_script_path(
    path: Path,
    *,
    sha256: str | None,
    bytes_: int | None,
) -> dict[str, Any]:
    """Build the portable FileRef projection for one script path."""
    resolved = path.resolve()
    file_ref = {
        "uri": resolved.as_uri(),
        "mime": mime_for_script_path(resolved),
        "size_bytes": bytes_,
        "display_name": resolved.name,
        "digest": f"sha256:{sha256}" if sha256 else None,
        "metadata": {
            "materialization_kind": "script",
            "source": "file_path",
        },
    }
    return {key: value for key, value in file_ref.items() if value is not None}


def mime_for_script_path(path: Path) -> str:
    """Return the stable script MIME type inferred from a file suffix."""
    suffix = path.suffix.lower()
    if suffix == ".py":
        return "text/x-python"
    if suffix == ".mel":
        return "text/x-mel"
    if suffix == ".js":
        return "text/javascript"
    if suffix == ".ps1":
        return "text/x-powershell"
    return "text/plain"
