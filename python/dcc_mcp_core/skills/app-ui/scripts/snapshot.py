"""app_ui__snapshot entry point."""

from __future__ import annotations

from _entrypoint import emit
from _entrypoint import snapshot_tool

if __name__ == "__main__":
    emit(snapshot_tool())
