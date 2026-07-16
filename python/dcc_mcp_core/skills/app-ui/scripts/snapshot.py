"""app_ui__snapshot entry point."""

from __future__ import annotations

if __package__:
    from ._entrypoint import emit
    from ._entrypoint import snapshot_tool
else:
    from _entrypoint import emit
    from _entrypoint import snapshot_tool


def main(**kwargs):
    """Run the tool in an in-process DCC executor."""
    return snapshot_tool(kwargs)


if __name__ == "__main__":
    emit(snapshot_tool())
