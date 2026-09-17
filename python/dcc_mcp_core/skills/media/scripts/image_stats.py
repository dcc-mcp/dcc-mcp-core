"""media__image_stats entry point."""

from __future__ import annotations

from _media_common import emit
from _media_common import image_stats
from _media_common import read_params
from _media_common import run_tool


def main(**params):
    """Run the image_stats tool."""
    return run_tool(image_stats, params)


if "__mcp_params__" in globals():
    __mcp_result__ = main(**globals()["__mcp_params__"])

if __name__ == "__main__":
    emit(main(**read_params()))
