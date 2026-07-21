"""Package OpenClaw/ClawHub skill directories into versioned zip archives."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from zipfile import ZIP_DEFLATED
from zipfile import ZipFile
from zipfile import ZipInfo

IGNORED_NAMES = {".DS_Store", "Thumbs.db"}
IGNORED_DIRECTORY_NAMES = {"__pycache__", ".mypy_cache", ".pytest_cache", ".ruff_cache"}
IGNORED_SUFFIXES = {".pyc", ".pyo"}
STABLE_SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
ZIP_TIMESTAMP = (1980, 1, 1, 0, 0, 0)
ZIP_FILE_MODE = 0o100644


def workspace_version_from_text(raw: str) -> str:
    """Extract workspace.package.version with a Python 3.7-safe TOML subset."""
    in_workspace_package = False
    for line in raw.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_workspace_package = stripped == "[workspace.package]"
            continue
        if in_workspace_package:
            match = re.fullmatch(r"version\s*=\s*['\"]([^'\"]+)['\"]\s*(?:#.*)?", stripped)
            if match is not None:
                return match.group(1)
    raise ValueError("missing [workspace.package] version")


def parse_args() -> argparse.Namespace:
    """Parse CLI for source path, output dir, and --all."""
    parser = argparse.ArgumentParser(
        description=(
            "Package one skill directory, or every immediate child with SKILL.md "
            "under a root, into versioned zip archives."
        )
    )
    parser.add_argument(
        "source",
        help="Skill directory or root directory when --all is set",
    )
    parser.add_argument("output_dir", help="Directory for zip outputs")
    parser.add_argument(
        "--all",
        action="store_true",
        help="Package every immediate child directory that contains SKILL.md",
    )
    parser.add_argument(
        "--manifest",
        action="store_true",
        help="Treat source as a ClawHub manifest and use each entry's independent version",
    )
    parser.add_argument(
        "--version",
        help="Override version in output filename (default: workspace Cargo.toml version)",
    )
    return parser.parse_args()


def workspace_version(repo_root: Path) -> str:
    """Read workspace version from the root Cargo.toml."""
    try:
        import tomllib
    except ModuleNotFoundError:  # pragma: no cover - exercised on Python < 3.11
        cargo_toml = repo_root / "Cargo.toml"
        try:
            return workspace_version_from_text(cargo_toml.read_text(encoding="utf-8"))
        except ValueError as error:
            raise ValueError(f"{error}: {cargo_toml}") from error

    cargo_toml = repo_root / "Cargo.toml"
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    return str(data["workspace"]["package"]["version"])


def iter_skill_files(skill_dir: Path):
    """Yield packable files under a skill directory."""
    if skill_dir.is_symlink():
        raise ValueError(f"refusing to package symbolic-link Skill directory: {skill_dir}")
    resolved_root = skill_dir.resolve()
    for path in sorted(skill_dir.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"refusing to package symbolic link: {path}")
        if not path.is_file():
            continue
        resolved = path.resolve()
        try:
            resolved.relative_to(resolved_root)
        except ValueError as error:
            raise ValueError(f"Skill file escapes package root: {path}") from error
        relative_parts = path.relative_to(skill_dir).parts
        if any(part in IGNORED_DIRECTORY_NAMES for part in relative_parts[:-1]):
            continue
        if path.name in IGNORED_NAMES:
            continue
        if path.suffix.lower() in IGNORED_SUFFIXES:
            continue
        yield path


def resolve_skill_dirs(source: Path, package_all: bool) -> list[Path]:
    """Resolve one or many skill directories from CLI source path."""
    if source.is_symlink():
        raise ValueError(f"refusing symbolic-link Skill source: {source}")
    if package_all:
        if not source.is_dir():
            raise ValueError(f"skills root does not exist: {source}")
        resolved_source = source.resolve()
        skill_dirs = []
        for path in sorted(source.iterdir()):
            if path.is_symlink() and (path / "SKILL.md").is_file():
                raise ValueError(f"refusing symbolic-link Skill directory: {path}")
            if path.is_dir() and (path / "SKILL.md").is_file():
                try:
                    path.resolve().relative_to(resolved_source)
                except ValueError as error:
                    raise ValueError(f"Skill directory escapes skills root: {path}") from error
                skill_dirs.append(path)
        if not skill_dirs:
            raise ValueError(f"no skill directories with SKILL.md under: {source}")
        return skill_dirs

    if not source.is_dir():
        raise ValueError(f"skill directory does not exist: {source}")
    if not (source / "SKILL.md").is_file():
        raise ValueError(f"missing SKILL.md in skill directory: {source}")
    return [source]


def resolve_manifest_skills(manifest: Path, repo_root: Path) -> list[tuple[Path, str]]:
    """Resolve Skill directories and immutable versions from a ClawHub manifest."""
    if not manifest.is_file():
        raise ValueError(f"ClawHub manifest does not exist: {manifest}")
    entries = json.loads(manifest.read_text(encoding="utf-8"))
    if not isinstance(entries, list) or not entries:
        raise ValueError(f"ClawHub manifest must be a non-empty JSON array: {manifest}")
    resolved: list[tuple[Path, str]] = []
    for entry in entries:
        if not isinstance(entry, dict) or not entry.get("path") or not entry.get("version"):
            raise ValueError(f"ClawHub manifest entry needs path and version: {entry!r}")
        unresolved_skill_dir = repo_root / str(entry["path"])
        if unresolved_skill_dir.is_symlink():
            raise ValueError(f"refusing symbolic-link Skill directory: {unresolved_skill_dir}")
        skill_dir = unresolved_skill_dir.resolve()
        try:
            skill_dir.relative_to(repo_root)
        except ValueError as error:
            raise ValueError(f"Skill path escapes repository root: {skill_dir}") from error
        if not skill_dir.is_dir() or not (skill_dir / "SKILL.md").is_file():
            raise ValueError(f"invalid Skill directory in ClawHub manifest: {skill_dir}")
        version = str(entry["version"]).strip()
        if STABLE_SEMVER_RE.fullmatch(version) is None:
            raise ValueError(f"invalid ClawHub manifest version: {entry['version']!r}")
        resolved.append((skill_dir, version))
    return resolved


def package_skill(skill_dir: Path, output_dir: Path, version: str) -> Path:
    """Create one reproducible Skill archive and return its path."""
    if STABLE_SEMVER_RE.fullmatch(version) is None:
        raise ValueError(f"invalid stable Skill archive version: {version!r}")
    if skill_dir.is_symlink():
        raise ValueError(f"refusing symbolic-link Skill directory: {skill_dir}")
    resolved_skill_dir = skill_dir.resolve()
    resolved_output_dir = output_dir.resolve()
    try:
        resolved_output_dir.relative_to(resolved_skill_dir)
    except ValueError:
        pass
    else:
        raise ValueError(f"output directory cannot be inside Skill directory: {output_dir}")
    archive_path = output_dir / f"{skill_dir.name}-{version}.zip"
    if archive_path.is_symlink():
        raise ValueError(f"refusing to overwrite symbolic-link archive: {archive_path}")
    with ZipFile(archive_path, "w", compression=ZIP_DEFLATED) as archive:
        for path in iter_skill_files(skill_dir):
            relative_path = path.relative_to(skill_dir.parent)
            info = ZipInfo(relative_path.as_posix(), date_time=ZIP_TIMESTAMP)
            info.create_system = 3
            info.compress_type = ZIP_DEFLATED
            info.external_attr = ZIP_FILE_MODE << 16
            archive.writestr(info, path.read_bytes())
    return archive_path


def main() -> int:
    """Package skill directories into dist/skills archives."""
    args = parse_args()
    repo_root = Path(__file__).resolve().parent.parent
    source = Path(args.source).absolute()
    output_dir = Path(args.output_dir).resolve()

    try:
        if args.manifest:
            if args.all or args.version:
                raise ValueError("--manifest cannot be combined with --all or --version")
            skill_releases = resolve_manifest_skills(source, repo_root)
        else:
            skill_dirs = resolve_skill_dirs(source, args.all)
            version = args.version or workspace_version(repo_root)
            skill_releases = [(skill_dir, version) for skill_dir in skill_dirs]
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1
    output_dir.mkdir(parents=True, exist_ok=True)

    try:
        for archive_path in (package_skill(skill_dir, output_dir, version) for skill_dir, version in skill_releases):
            print(archive_path)
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
