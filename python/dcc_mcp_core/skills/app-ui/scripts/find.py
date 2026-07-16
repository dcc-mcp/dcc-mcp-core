"""app_ui__find entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import find_tool
else:
    from _entrypoint import emit
    from _entrypoint import find_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return find_tool(kwargs)


if __name__ == "__main__":
    emit(find_tool())
