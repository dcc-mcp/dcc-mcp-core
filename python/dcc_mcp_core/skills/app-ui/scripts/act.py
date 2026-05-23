"""app_ui__act entry point for the mock backend."""

from __future__ import annotations

from _backend import act_tool
from _backend import emit

if __name__ == "__main__":
    emit(act_tool())
