"""media__extract_frames entry point."""

from __future__ import annotations

from _media_common import emit
from _media_common import extract_frames
from _media_common import read_params
from _media_common import run_tool


def main(**params):
    """Run the frame extraction tool."""
    return run_tool(extract_frames, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    emit(main(**read_params()))
