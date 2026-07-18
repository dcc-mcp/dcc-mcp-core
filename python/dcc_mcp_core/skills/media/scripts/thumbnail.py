"""media__thumbnail entry point."""

from __future__ import annotations

from _media_common import emit
from _media_common import read_params
from _media_common import run_tool
from _media_common import thumbnail


def main(**params):
    """Run the thumbnail tool."""
    return run_tool(thumbnail, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    emit(main(**read_params()))
