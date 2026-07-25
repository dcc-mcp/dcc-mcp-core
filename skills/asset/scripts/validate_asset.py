"""validate_asset — Validate asset file integrity, format, and metadata.

Performs layered validation:
  1. File existence and size sanity checks
  2. Format detection from extension
  3. Optional referenced-file resolution (textures, dependencies)
  4. Structured issue reporting with severity levels

Supports common DCC formats: FBX, USD, ABC, OBJ, GLTF, Maya ASCII/Binary,
Blender, and image textures (PNG, JPG, EXR, TGA, TIF).
"""
from __future__ import annotations

import os
from typing import Any


# Format → expected extension(s) mapping
FORMAT_EXTENSIONS: dict[str, list[str]] = {
    "fbx": [".fbx"],
    "usd": [".usd", ".usda", ".usdc", ".usdz"],
    "abc": [".abc"],
    "obj": [".obj"],
    "gltf": [".gltf", ".glb"],
    "ma": [".ma"],
    "mb": [".mb"],
    "blend": [".blend"],
    "png": [".png"],
    "jpg": [".jpg", ".jpeg"],
    "exr": [".exr"],
    "tga": [".tga"],
    "tif": [".tif", ".tiff"],
}

# File size sanity thresholds (bytes)
MIN_REASONABLE_SIZE = 100      # Smaller than this is suspicious
MAX_REASONABLE_SIZE = 2**34   # 16 GiB — larger is suspicious


def _detect_format(file_path: str) -> str | None:
    """Detect format from file extension."""
    ext = os.path.splitext(file_path)[1].lower()
    for fmt, exts in FORMAT_EXTENSIONS.items():
        if ext in exts:
            return fmt
    if ext:
        # Unknown extension — return it as-is for caller awareness
        return ext.lstrip(".")
    return None


def _scan_texture_refs(file_path: str, fmt: str) -> list[dict[str, Any]]:
    """Scan for referenced texture files.

    For text-based formats (Maya ASCII, USD ASCII, OBJ), parses the file
    and checks referenced texture paths.  For binary formats, returns empty.
    """
    referenced: list[dict[str, Any]] = []
    text_formats = {"ma", "usda", "obj"}

    if fmt not in text_formats:
        return referenced

    base_dir = os.path.dirname(file_path) or "."
    texture_exts = {".png", ".jpg", ".jpeg", ".exr", ".tga", ".tif", ".tiff", ".bmp", ".dds"}

    try:
        with open(file_path, "r", encoding="utf-8", errors="ignore") as fh:
            content = fh.read(65536)  # Read first 64 KiB — texture refs are near the top
    except Exception:
        return referenced

    # Simple path-fragment scanning — look for tokens that look like file paths
    # with texture extensions near the file path string
    for line in content.split("\n"):
        for tex_ext in texture_exts:
            if tex_ext in line.lower():
                # Extract a plausible file path
                for token in line.split():
                    clean = token.strip('";\',')
                    if clean.lower().endswith(tex_ext) and len(clean) > 4:
                        ref_path = clean
                        if not os.path.isabs(ref_path):
                            ref_path = os.path.normpath(os.path.join(base_dir, ref_path))
                        referenced.append({
                            "path": ref_path,
                            "exists": os.path.isfile(ref_path),
                            "type": "texture",
                        })
                        break

    return referenced


def validate_asset(
    file_path: str,
    format: str | None = None,
    check_textures: bool = False,
) -> dict[str, Any]:
    """Validate an asset file.

    Checks file existence, size sanity, format integrity, and optionally
    resolves referenced texture files to detect broken links.

    Args:
        file_path: Asset file path to validate.
        format: Expected format. Auto-detected from extension when omitted.
        check_textures: Also validate referenced texture files (default False).

    Returns:
        Validation report with issues array and optional referenced_files.
    """
    issues: list[dict[str, Any]] = []

    # --- File existence ---
    if not os.path.isfile(file_path):
        return {
            "success": True,
            "valid": False,
            "file_path": file_path,
            "format": format or "unknown",
            "file_exists": False,
            "file_size": 0,
            "issues": [
                {
                    "severity": "error",
                    "message": "File not found: {}".format(file_path),
                    "check": "file_exists",
                }
            ],
            "referenced_files": [],
        }

    # --- File size ---
    file_size = os.path.getsize(file_path)
    if file_size == 0:
        issues.append({
            "severity": "error",
            "message": "File is empty (0 bytes). Likely a failed or interrupted export.",
            "check": "file_size",
        })
    elif file_size < MIN_REASONABLE_SIZE:
        issues.append({
            "severity": "warning",
            "message": "File is very small ({} bytes). May be corrupted or truncated.".format(file_size),
            "check": "file_size",
        })
    elif file_size > MAX_REASONABLE_SIZE:
        issues.append({
            "severity": "warning",
            "message": "File is unusually large ({:.1f} GiB). Verify before import.".format(
                file_size / (1024 ** 3)
            ),
            "check": "file_size",
        })

    # --- Format detection ---
    detected = _detect_format(file_path)
    if format and detected and format != detected:
        issues.append({
            "severity": "error",
            "message": "Format mismatch: expected '{}' but file extension indicates '{}'.".format(
                format, detected
            ),
            "check": "format_match",
        })
    elif not detected:
        issues.append({
            "severity": "warning",
            "message": "Unknown file format — extension not recognized. Supported: {}.".format(
                ", ".join(sorted(FORMAT_EXTENSIONS.keys()))
            ),
            "check": "format_match",
        })

    resolved_format = format or detected or "unknown"

    # --- Texture references ---
    referenced: list[dict[str, Any]] = []
    if check_textures:
        referenced = _scan_texture_refs(file_path, resolved_format)
        missing = [r for r in referenced if not r["exists"]]
        for m in missing:
            issues.append({
                "severity": "warning",
                "message": "Referenced texture not found: {}".format(m["path"]),
                "check": "texture_refs",
            })

    valid = not any(i["severity"] == "error" for i in issues)

    return {
        "success": True,
        "valid": valid,
        "file_path": file_path,
        "format": resolved_format,
        "file_exists": True,
        "file_size": file_size,
        "issues": issues,
        "referenced_files": referenced,
    }
