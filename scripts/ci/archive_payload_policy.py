"""Shared fail-closed policy for Python distribution archive members."""

from __future__ import annotations

import re

_METADATA_SUFFIXES = (".data", ".dist-info", ".egg-info")


def normalize_archive_member(name: str) -> str:
    """Return a platform-independent relative archive path or reject it."""
    if not isinstance(name, str) or not name or "\x00" in name:
        raise ValueError(f"unsafe archive member {name!r}")
    portable = name.replace("\\", "/")
    if portable.startswith("/") or re.match(r"^[A-Za-z]:", portable):
        raise ValueError(f"unsafe archive member {name!r}")
    parts = []
    for part in portable.split("/"):
        if part in ("", "."):
            continue
        if part == "..":
            raise ValueError(f"unsafe archive member {name!r}")
        parts.append(part)
    if not parts:
        raise ValueError(f"unsafe archive member {name!r}")
    return "/".join(parts)


def _project_name(value: str) -> str:
    return re.sub(r"[-_.]+", "-", value).lower()


def _is_typing_extensions_component(component: str) -> bool:
    folded = component.casefold()
    if folded.endswith(".py") and _project_name(folded[:-3]) == "typing-extensions":
        return True
    if _project_name(folded) == "typing-extensions":
        return True
    for suffix in _METADATA_SUFFIXES:
        if not folded.endswith(suffix):
            continue
        distribution = _project_name(folded[: -len(suffix)])
        if distribution == "typing-extensions" or distribution.startswith("typing-extensions-"):
            return True
    return False


def archive_member_errors(names) -> list[str]:
    """Reject unsafe paths and any normalized typing_extensions payload."""
    errors = []
    for name in names:
        try:
            normalized = normalize_archive_member(name)
        except ValueError as exc:
            errors.append(str(exc))
            continue
        if any(_is_typing_extensions_component(part) for part in normalized.split("/")):
            errors.append(f"archive contains forbidden typing_extensions payload {name!r}")
    return sorted(set(errors))
