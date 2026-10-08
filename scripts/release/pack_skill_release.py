#!/usr/bin/env python
"""Pack skill bundles into reproducible zip archives with a sha256 digest.

Marketplace install sources must be immutable, so a published entry pins a
GitHub Release asset together with its SHA-256. Recomputing that digest by
hand is error-prone, and an unreproducible archive makes the digest
unverifiable, so packing is centralized here and used by both CI and local
publish runs.

Archives are byte-reproducible: entries are sorted, directories and VCS
metadata are skipped, and mtimes are pinned to a fixed epoch. Two runs over
the same tree therefore produce the same digest.

Usage:
    python scripts/release/pack_skill_release.py --skill skills/asset-source
    python scripts/release/pack_skill_release.py --all --out dist/marketplace
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import zipfile

# Fixed timestamp keeps the archive byte-identical across runs. The zip
# format's lower bound is 1980-01-01, hence the offset below.
_ZIP_EPOCH = (1980, 1, 1, 0, 0, 0)

_SKIP_DIRS = frozenset(
    {
        ".git",
        ".hg",
        ".svn",
        "__pycache__",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".tox",
        ".venv",
        "node_modules",
    }
)

_SKIP_SUFFIXES = (".pyc", ".pyo", ".swp")

_SHA256_PATTERN_LENGTH = 64


def repo_root() -> Path:
    """Return the repository root, defined as the parent of this script's dir."""
    return Path(__file__).resolve().parents[2]


def collect_files(skill_dir: Path) -> list[Path]:
    """Return the relative paths to package, sorted and VCS-free."""
    collected: list[Path] = []
    for root, dirnames, filenames in os.walk(str(skill_dir)):
        dirnames[:] = sorted(name for name in dirnames if name not in _SKIP_DIRS)
        for filename in sorted(filenames):
            if filename.endswith(_SKIP_SUFFIXES):
                continue
            absolute = Path(root) / filename
            collected.append(absolute.relative_to(skill_dir))
    # Sort on the POSIX form so ordering does not depend on the host path
    # separator or on case-fold differences in path comparison.
    return sorted(collected, key=lambda item: item.as_posix())


def build_zip_bytes(skill_dir: Path, prefix: str) -> bytes:
    """Build a reproducible zip of ``skill_dir`` rooted at ``prefix``."""
    import io

    buffer = io.BytesIO()
    files = collect_files(skill_dir)
    if not files:
        raise SystemExit(f"pack: no files found under {skill_dir}")

    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as archive:
        for relative in files:
            info = zipfile.ZipInfo(f"{prefix}/{relative.as_posix()}", date_time=_ZIP_EPOCH)
            info.external_attr = 0o644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            payload = (skill_dir / relative).read_bytes()
            archive.writestr(info, payload)
    return buffer.getvalue()


def sha256_hex(payload: bytes) -> str:
    """Return the lowercase hex SHA-256 digest of ``payload``."""
    return hashlib.sha256(payload).hexdigest()


def display_path(path: Path, root: Path) -> str:
    """Render ``path`` relative to the repo when possible, else absolute."""
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return str(path)


def read_skill_name(skill_dir: Path) -> str:
    """Read the skill name from SKILL.md frontmatter, falling back to the dir."""
    skill_md = skill_dir / "SKILL.md"
    if not skill_md.is_file():
        return skill_dir.name
    in_frontmatter = False
    for line in skill_md.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped == "---":
            in_frontmatter = not in_frontmatter
            continue
        if in_frontmatter and stripped.startswith("name:"):
            return stripped.split(":", 1)[1].strip().strip('"').strip("'")
    return skill_dir.name


def pack_skill(skill_dir: Path, out_dir: Path, version: str) -> dict[str, object]:
    """Pack one skill directory and return its release metadata."""
    if not skill_dir.is_dir():
        raise SystemExit(f"pack: not a directory: {skill_dir}")

    name = read_skill_name(skill_dir)
    # The archive root matches the install layout expected at skillRoots.
    prefix = f"skill/{skill_dir.name}"
    payload = build_zip_bytes(skill_dir, prefix)

    out_dir.mkdir(parents=True, exist_ok=True)
    archive_name = f"{skill_dir.name}-{version}.zip"
    archive_path = out_dir / archive_name
    archive_path.write_bytes(payload)

    digest = sha256_hex(payload)
    return {
        "skill": skill_dir.name,
        "name": name,
        "version": version,
        "asset": archive_name,
        "path": str(archive_path),
        "sha256": digest,
        "sha256_prefixed": f"sha256:{digest}",
        "bytes": len(payload),
        "skill_root": prefix,
        "files": len(collect_files(skill_dir)),
    }


def discover_skills(root: Path) -> list[Path]:
    """Return every skill directory under the repo-level ``skills/`` tree."""
    skills_root = root / "skills"
    if not skills_root.is_dir():
        return []
    return sorted(path for path in skills_root.iterdir() if path.is_dir())


def main(argv: list[str] | None = None) -> int:
    """Pack the selected skills, write the digest manifest, and report."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--skill", action="append", default=[], help="skill directory to pack")
    parser.add_argument("--all", action="store_true", help="pack every skill under skills/")
    parser.add_argument("--out", default="dist/marketplace", help="output directory")
    parser.add_argument("--version", required=True, help="version stamped on the archive name")
    parser.add_argument(
        "--manifest",
        default="dist/marketplace/manifest.json",
        help="where to write the digest manifest",
    )
    parser.add_argument(
        "--exclude",
        action="append",
        default=[],
        help="skill directory name to skip (repeatable)",
    )
    args = parser.parse_args(argv)

    root = repo_root()
    excluded = set(args.exclude)

    targets: list[Path] = []
    for value in args.skill:
        candidate = Path(value)
        if not candidate.is_absolute():
            candidate = root / value
        targets.append(candidate)
    if args.all:
        targets.extend(discover_skills(root))

    seen = set()
    unique_targets = []
    for target in targets:
        if target.name in excluded:
            continue
        if target.name in seen:
            continue
        seen.add(target.name)
        unique_targets.append(target)

    if not unique_targets:
        sys.stderr.write("pack: no skills selected\n")
        return 2

    out_dir = root / args.out
    results = [pack_skill(target, out_dir, args.version) for target in unique_targets]

    manifest_path = root / args.manifest
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(
        json.dumps({"version": args.version, "skills": results}, indent=2) + "\n",
        encoding="utf-8",
    )

    for result in results:
        print(
            "packed {} -> {} ({} bytes, sha256 {})".format(
                result["skill"], result["asset"], result["bytes"], result["sha256"]
            )
        )
    print(f"manifest: {display_path(manifest_path, root)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
