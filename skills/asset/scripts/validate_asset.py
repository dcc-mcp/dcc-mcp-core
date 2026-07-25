"""Validate asset file integrity.

Performs a series of checks on an asset file:
- File exists and is readable
- Non-zero size within expected bounds
- Format is recognised (by extension or explicit hint)
- Optionally checks referenced texture paths for existence
- Reports warnings for unusual file sizes or missing references

This is a read-only, filesystem-level check. It does not open or parse
the asset file content — only validates file-level properties.
"""

from __future__ import annotations

import os as _os
import sys as _sys
from pathlib import Path as _Path

from dcc_mcp_core.skill import skill_entry, skill_error, skill_success, skill_warning


# ---------------------------------------------------------------------------
# Format constants
# ---------------------------------------------------------------------------

_RECOGNISED_EXTENSIONS = {
    ".obj", ".fbx", ".gltf", ".glb", ".usd", ".usda", ".usdc", ".usdz",
    ".abc", ".blend", ".ma", ".mb", ".hip", ".hipnc", ".max", ".c4d",
    ".uasset", ".png", ".jpg", ".jpeg", ".tga", ".exr", ".tif", ".tiff",
    ".hdr", ".bmp", ".psd", ".ai", ".svg",
}

# Known texture reference patterns in text-based formats
_TEXTURE_REF_PATTERNS = {
    ".obj": ["map_Kd", "map_Ka", "map_Ks", "map_Bump", "map_d", "disp"],
    ".usd": ["@", ".png", ".jpg", ".exr", ".tga"],
    ".usda": ["@", ".png", ".jpg", ".exr", ".tga"],
}

# Reasonable size thresholds (bytes)
_MIN_FILE_SIZE = 1
_DEFAULT_MAX_FILE_SIZE = 2 * 1024 * 1024 * 1024  # 2 GB
_WARN_SIZE_LOW = 1024  # 1 KB — suspiciously small
_WARN_SIZE_HIGH = 500 * 1024 * 1024  # 500 MB — suspiciously large


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _check_file_existence(file_path):
    """Check that the file exists and is readable. Returns (ok, error_message)."""
    path = _Path(file_path)
    if not path.exists():
        return False, "File does not exist: %s" % file_path
    if not path.is_file():
        return False, "Path is not a file: %s" % file_path
    try:
        with open(str(path), "rb") as f:
            f.read(1)
    except (OSError, IOError) as exc:
        return False, "File is not readable: %s" % str(exc)
    return True, None


def _check_file_size(file_path, max_size_bytes=None):
    """Check file size. Returns (ok, actual_size, warnings, error_message)."""
    try:
        size = _Path(file_path).stat().st_size
    except OSError as exc:
        return False, None, [], "Cannot stat file: %s" % str(exc)

    warnings = []

    if size < _MIN_FILE_SIZE:
        return False, size, [], "File is empty (0 bytes)"

    if max_size_bytes is not None and size > max_size_bytes:
        return False, size, [], (
            "File size %d bytes exceeds maximum %d bytes" % (size, max_size_bytes)
        )

    if size < _WARN_SIZE_LOW:
        warnings.append(
            "File size (%d bytes) is suspiciously small for an asset file" % size
        )

    if size > _WARN_SIZE_HIGH:
        warnings.append(
            "File size (%.1f MB) is unusually large" % (size / (1024.0 * 1024.0))
        )

    return True, size, warnings, None


def _check_format(file_path, expected_format=None):
    """Check that the file extension is recognised. Returns (ok, format_str, message)."""
    ext = _Path(file_path).suffix.lower()
    if not ext:
        return False, "unknown", "File has no extension"

    if expected_format:
        # Normalize: strip leading dot and lowercase
        expected = expected_format.lower().lstrip(".")
        actual = ext.lstrip(".")
        if actual != expected:
            return False, actual, (
                "Format mismatch: expected '%s', got '%s' (extension '%s')"
                % (expected, actual, ext)
            )

    if ext not in _RECOGNISED_EXTENSIONS:
        return True, ext, "Unrecognised extension '%s' — may still be valid" % ext

    return True, ext, "Recognised format: '%s'" % ext.lstrip(".")


def _find_texture_refs(file_path, fmt):
    """Scan a text-based file for texture path references."""

    def _resolve_texture_path(base_dir, ref):
        """Try to resolve a texture reference to an actual file."""
        # Try absolute path
        if _os.path.isabs(ref):
            if _Path(ref).exists():
                return ref
            return None

        # Try relative to asset file directory
        candidate = _Path(base_dir) / ref
        if candidate.exists():
            return str(candidate)

        # Try common texture subdirectories
        for sub in ["textures", "tex", "maps", "sourceimages"]:
            candidate = _Path(base_dir) / sub / _Path(ref).name
            if candidate.exists():
                return str(candidate)

        return None

    if not fmt or fmt not in _TEXTURE_REF_PATTERNS:
        return [], []

    base_dir = _Path(file_path).parent
    found = []
    missing = []

    try:
        with open(file_path, "r", encoding="utf-8", errors="replace") as f:
            content = f.read(32768)  # Read first 32 KB only
    except (OSError, IOError):
        return [], []

    lines = content.split("\n")

    if fmt == ".obj":
        for line in lines:
            line = line.strip()
            if any(line.startswith(p) for p in _TEXTURE_REF_PATTERNS[".obj"]):
                parts = line.split()
                if len(parts) >= 2:
                    ref = parts[-1]
                    resolved = _resolve_texture_path(base_dir, ref)
                    if resolved:
                        found.append(resolved)
                    else:
                        missing.append(ref)

    elif fmt in (".usd", ".usda"):
        for line in lines:
            line = line.strip()
            if "@" in line:
                # Extract paths between @ delimiters
                import re
                matches = re.findall(r"@([^@]+)@", line)
                for ref in matches:
                    ref = ref.strip()
                    if ref:
                        resolved = _resolve_texture_path(base_dir, ref)
                        if resolved:
                            found.append(resolved)
                        else:
                            missing.append(ref)

    return found, missing


# ---------------------------------------------------------------------------
# Tool entry point
# ---------------------------------------------------------------------------


@skill_entry
def main(file_path, format=None, check_textures=False,
         max_size_bytes=None, **kwargs):
    """Validate asset file integrity.

    Args:
        file_path: Path to the asset file to validate.
        format: Expected format. Inferred from extension if omitted.
        check_textures: Whether to scan for and check referenced textures.
        max_size_bytes: Maximum allowed file size. Defaults to 2 GB.

    Returns:
        Skill result dict with validation report in context.

    """
    file_path = file_path.strip()
    if not file_path:
        return skill_error("Empty file_path", "file_path must not be empty")

    # Phase 1: Existence
    ok, err = _check_file_existence(file_path)
    if not ok:
        return skill_error(
            "File existence check failed",
            err,
            file_path=file_path,
        )

    # Phase 2: Size
    ok, actual_size, size_warnings, err = _check_file_size(file_path, max_size_bytes)
    if not ok:
        return skill_error(
            "File size check failed",
            err,
            file_path=file_path,
            file_size=actual_size,
        )

    # Phase 3: Format
    ext = _Path(file_path).suffix.lower()
    ok, detected_fmt, fmt_msg = _check_format(file_path, format)
    if not ok:
        return skill_error(
            "Format check failed",
            fmt_msg,
            file_path=file_path,
            expected_format=format,
            detected_extension=ext,
        )

    # Phase 4: Texture references (optional)
    all_warnings = list(size_warnings)
    texture_refs_found = []
    texture_refs_missing = []

    if check_textures:
        found, missing = _find_texture_refs(file_path, ext)
        texture_refs_found = found
        texture_refs_missing = missing
        if missing:
            all_warnings.append(
                "%d referenced texture(s) not found" % len(missing)
            )

    # Build result
    checks = {
        "exists": True,
        "size_ok": True,
        "format_ok": True,
        "extensions_recognised": ext in _RECOGNISED_EXTENSIONS,
    }
    if texture_refs_found or texture_refs_missing:
        checks["textures_resolved"] = len(texture_refs_found)
        checks["textures_missing"] = len(texture_refs_missing)

    valid = len(all_warnings) == 0

    context = {
        "file_path": file_path,
        "file_size": actual_size,
        "file_size_mb": round(actual_size / (1024.0 * 1024.0), 2) if actual_size else None,
        "format": detected_fmt.lstrip("."),
        "checks": checks,
        "texture_refs_found": texture_refs_found[:20],
        "texture_refs_missing": texture_refs_missing[:20],
    }

    if all_warnings:
        return skill_warning(
            "Validation completed with %d warning(s) for '%s'" % (
                len(all_warnings), _Path(file_path).name,
            ),
            warning="; ".join(all_warnings),
            prompt="Review warnings and fix issues before import.",
            **context
        )

    return skill_success(
        "Validation passed for '%s' (%s, %.1f MB)" % (
            _Path(file_path).name,
            detected_fmt.lstrip("."),
            actual_size / (1024.0 * 1024.0),
        ),
        prompt="File is valid. Proceed with import or further processing.",
        **context
    )


if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
