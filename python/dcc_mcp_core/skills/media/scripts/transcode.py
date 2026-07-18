"""media__transcode entry point."""

from __future__ import annotations

from _media_common import emit
from _media_common import read_params
from _media_common import run_tool
from _media_common import transcode


def main(**params):
    """Run the transcode tool."""
    return run_tool(transcode, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    emit(main(**read_params()))
