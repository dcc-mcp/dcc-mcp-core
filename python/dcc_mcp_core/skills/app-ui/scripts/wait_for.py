"""app_ui__wait_for entry point for the mock backend."""

from __future__ import annotations

from _backend import emit
from _backend import wait_for_tool

if __name__ == "__main__":
    emit(wait_for_tool())
