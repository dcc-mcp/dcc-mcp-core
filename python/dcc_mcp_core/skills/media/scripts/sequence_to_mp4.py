"""media__sequence_to_mp4 entry point."""

from __future__ import annotations

from _media_common import emit
from _media_common import read_params
from _media_common import run_tool
from _media_common import sequence_to_mp4


def main(**params):
    """Run the image-sequence conversion tool."""
    return run_tool(sequence_to_mp4, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    emit(main(**read_params()))
