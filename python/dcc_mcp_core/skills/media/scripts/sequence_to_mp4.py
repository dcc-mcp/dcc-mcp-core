"""media__sequence_to_mp4 entry point."""

from __future__ import annotations

from pathlib import Path
import sys

_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

from _media_common import emit  # noqa: E402
from _media_common import read_params  # noqa: E402
from _media_common import run_tool  # noqa: E402
from _media_common import sequence_to_mp4  # noqa: E402


def main(**params):
    """Run the image-sequence conversion tool."""
    return run_tool(sequence_to_mp4, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    emit(main(**read_params()))
