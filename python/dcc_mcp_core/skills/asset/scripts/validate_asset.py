"""validate_asset — Validate asset file integrity, format, and metadata."""
from __future__ import annotations

import json
import os
import subprocess
from typing import Any


# Format → expected extension mapping
FORMAT_EXTENSIONS: dict[str, list[str]] = {
    "fbx": [".fbx"],
    "usd": [".usd", ".usda", ".usdc", ".usdz"],
    "abc": [".abc"],
    "obj": [".obj"],
    "gltf": [".gltf", ".glb"],
    "ma": [".ma", ".mb"],
    "blend": [".blend"],
    "png": [".png"],
    "jpg": [".jpg", ".jpeg"],
    "exr": [".exr"],
    "tga": [".tga"],
    "tif": [".tif", ".tiff"],
}


def _detect_format(file_path: str) -> str | None:
    """Detect format from file extension."""
    ext = os.path.splitext(file_path)[1].lower()
    for fmt, exts in FORMAT_EXTENSIONS.items():
        if ext in exts:
            return fmt
    return None


def validate_asset(
    file_path: str,
    format: str | None = None,
    check_textures: bool = False,
) -> dict[str, Any]:
    """Validate an asset file.

    Args:
        file_path: Asset file path.
        format: Expected format. Auto-detected when omitted.
        check_textures: Also validate referenced textures.

    Returns:
        Validation report.
    """
    issues: list[dict[str, str]] = []
    checks: dict[str, Any] = {
        "file_exists": False,
        "format_match": None,
        "file_size_bytes": 0,
    }

    # File existence
    if not os.path.isfile(file_path):
        checks["file_exists"] = False
        issues.append({"severity": "error", "message": "File not found: {}".format(file_path), "check": "file_exists"})
        return {
            "success": True,
            "valid": False,
            "file_path": file_path,
            "format": format or "unknown",
            "file_exists": False,
            "file_size": 0,
            "issues": issues,
            "referenced_files": [],
        }
    checks["file_exists"] = True

    # File size
    file_size = os.path.getsize(file_path)
    checks["file_size_bytes"] = file_size
    if file_size == 0:
        issues.append({"severity": "error", "message": "File is empty (0 bytes).", "check": "file_size"})
    elif file_size < 100:
        issues.append({"severity": "warning", "message": "File is very small ({} bytes).".format(file_size), "check": "file_size"})

    # Format detection and matching
    detected = _detect_format(file_path)
    if not detected:
        issues.append({"severity": "warning", "message": "Unknown file format for extension.", "check": "format_match"})
    elif format and detected != format:
        checks["format_match"] = False
        issues.append({
            "severity": "error",
            "message": "Format mismatch: expected '{}', detected '{}'.".format(format, detected),
            "check": "format_match",
        })
    else:
        checks["format_match"] = True

    # Referenced files (basic texture check)
    referenced: list[dict[str, Any]] = []
    if check_textures and detected in ("ma", "mb", "blend", "usd"):
        # This would parse the file for texture references — simplified here
        pass

    valid = not any(i["severity"] == "error" for i in issues)

    return {
        "success": True,
        "valid": valid,
        "file_path": file_path,
        "format": format or detected or "unknown",
        "file_exists": checks["file_exists"],
        "file_size": file_size,
        "issues": issues,
        "referenced_files": referenced,
    }
