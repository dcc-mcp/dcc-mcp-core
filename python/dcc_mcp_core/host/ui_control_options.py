"""Owner-selected, integrity-pinned UI Control runtime configuration."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import re

PIXEL_ACTIONS = frozenset(
    {"click", "double_click", "right_click", "toggle", "keypress", "keyboard_shortcut", "type", "type_chars"}
)


def _ordinary_absolute_path(value: str) -> Path:
    """Validate path spelling without assuming an artifact already exists."""
    if not isinstance(value, str):
        raise ValueError("path must be an ordinary absolute local path")
    path = Path(value)
    spelling = value.replace("\\", "/")
    if (
        len(value) > 4096
        or value != value.strip()
        or any(ord(character) < 32 or ord(character) == 127 for character in value)
        or any(part in {".", ".."} or part.endswith((".", " ")) for part in spelling.split("/") if part)
        or spelling.startswith("//")
        or any(":" in part for part in path.parts[1:])
        or not path.is_absolute()
    ):
        raise ValueError("path must be an ordinary absolute local path")
    return path


@dataclass(frozen=True)
class UiControlRecordingOptions:
    """Operator-owned video destination and bounded native cleanup wait.

    The root must already be an ordinary local directory. The runtime chooses
    an immutable task child; tool callers cannot redirect it. This grants no
    input and never starts recording automatically.
    """

    output_root: str
    cleanup_timeout_seconds: float = 65.0

    def __post_init__(self) -> None:
        path = _ordinary_absolute_path(self.output_root)
        if not path.is_dir() or path.is_symlink():
            raise ValueError("recording output_root must be an absolute ordinary directory")
        for ancestor in (path, *path.parents):
            if ancestor.is_symlink() or getattr(ancestor.stat(), "st_file_attributes", 0) & 0x400:
                raise ValueError("recording output_root must not contain reparse or symlink ancestors")
        if type(self.cleanup_timeout_seconds) not in (int, float) or not 60 < self.cleanup_timeout_seconds <= 65:
            raise ValueError("recording cleanup_timeout_seconds must be greater than 60 and at most 65")


@dataclass(frozen=True)
class UiControlRuntimeOptions:
    """Select one owned public-MCP runtime without changing shared Host defaults.

    Construct this in trusted adapter bootstrap code, never from tool arguments.
    The binary is hashed before launch and its MCP identity/version is checked
    before a task is opened. Actions are a ceiling, not an approval token; the
    runtime owns task authorization and native input policy.

    Args:
        binary: Absolute path to the operator-selected executable.
        sha256: Exact SHA-256 of that executable.
        runtime_version: Exact expected MCP runtime version.
        allowed_actions: Bounded physical-input action names; empty grants no physical input.
        window_operations: Explicit window mutations: activate, restore_activate,
            or observation-bound minimize. Empty grants no window mutations.
        ttl_minutes: Lifetime of the runtime-owned task (1 through 60 minutes).
        timeout_seconds: Bounded response wait, including initialization.
        transport: Only the explicit owned public-MCP pixels transport is supported.
        observation_mode: Only pixels_only is supported by this adapter.
        recording: Optional independent owner video ceiling; disabled by default.

    """

    binary: str
    sha256: str
    runtime_version: str
    allowed_actions: tuple[str, ...] = ()
    ttl_minutes: int = 15
    timeout_seconds: float = 45.0
    transport: str = "owned_mcp_pixels"
    observation_mode: str = "pixels_only"
    window_operations: tuple[str, ...] = ()
    recording: UiControlRecordingOptions | None = None

    def __post_init__(self) -> None:
        if self.recording is not None and not isinstance(self.recording, UiControlRecordingOptions):
            raise TypeError("recording must be UiControlRecordingOptions or None")
        if not isinstance(self.binary, str) or not Path(self.binary).is_absolute():
            raise ValueError("UI Control binary must be an absolute path")
        if not isinstance(self.sha256, str) or not re.fullmatch(r"[0-9a-f]{64}", self.sha256):
            raise ValueError("UI Control sha256 must be 64 lowercase hexadecimal characters")
        if not isinstance(self.runtime_version, str) or not self.runtime_version.strip():
            raise ValueError("UI Control runtime_version must be explicit")
        if self.transport != "owned_mcp_pixels" or self.observation_mode != "pixels_only":
            raise ValueError("Only owned_mcp_pixels with pixels_only is supported")
        if not isinstance(self.allowed_actions, tuple) or any(
            item not in PIXEL_ACTIONS for item in self.allowed_actions
        ):
            raise ValueError("allowed_actions must be a tuple of supported physical-input actions")
        if len(set(self.allowed_actions)) != len(self.allowed_actions):
            raise ValueError("allowed_actions must not contain duplicates")
        if (
            not isinstance(self.window_operations, tuple)
            or any(item not in {"activate", "restore_activate", "minimize"} for item in self.window_operations)
            or len(set(self.window_operations)) != len(self.window_operations)
        ):
            raise ValueError("window_operations must be a distinct tuple of activate, restore_activate, minimize")
        if type(self.ttl_minutes) is not int or not 1 <= self.ttl_minutes <= 60:
            raise ValueError("ttl_minutes must be an integer from 1 through 60")
        if type(self.timeout_seconds) not in (int, float) or not 0 < self.timeout_seconds <= 120:
            raise ValueError("timeout_seconds must be positive and at most 120")
