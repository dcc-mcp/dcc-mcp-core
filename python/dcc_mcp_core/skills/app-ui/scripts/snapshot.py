"""app_ui__snapshot entry point for the mock backend."""

from __future__ import annotations

from _backend import emit
from _backend import snapshot_tool

if __name__ == "__main__":
    emit(snapshot_tool())
