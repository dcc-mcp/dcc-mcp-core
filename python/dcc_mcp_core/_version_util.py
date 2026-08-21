"""Shared version parsing helpers for Python runtime decisions."""

from __future__ import annotations

import re


def parse_semver(value: str) -> tuple[int, int, int] | None:
    """Return the numeric SemVer core, or ``None`` for an invalid version."""
    text = str(value).strip().lstrip("vV")
    text = re.split(r"[-+]", text, maxsplit=1)[0]
    parts = text.split(".")
    if not parts or any(not part.isdigit() for part in parts[:3]):
        return None
    padded = [int(part) for part in parts[:3]]
    while len(padded) < 3:
        padded.append(0)
    return tuple(padded[:3])
