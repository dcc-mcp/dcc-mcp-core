"""Package OpenClaw/ClawHub skill directories into versioned zip archives."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys
from zipfile import ZIP_DEFLATED
from zipfile import ZipFile

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised on Python < 3.11
    import tomli as tomllib

IGNORED_NAMES = {".DS_Store", "Thumbs.db"}
IGNORED_DIRECTORY_NAMES = {"__pycache__", ".mypy_cache", ".pytest_cache", ".ruff_cache"}
IGNORED_SUFFIXES = {".pyc", ".pyo"}


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
        "--version",
        help="Override version in output filename (default: workspace Cargo.toml version)",
    )
    return parser.parse_args()


def workspace_version(repo_root: Path) -> str:
    """Read workspace version from the root Cargo.toml."""
    cargo_toml = repo_root / "Cargo.toml"
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    return str(data["workspace"]["package"]["version"])


def iter_skill_files(skill_dir: Path):
    """Yield packable files under a skill directory."""
    for path in sorted(skill_dir.rglob("*")):
        if not path.is_file():
            continue
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
    if package_all:
        if not source.is_dir():
            raise ValueError(f"skills root does not exist: {source}")
        skill_dirs = [path for path in sorted(source.iterdir()) if path.is_dir() and (path / "SKILL.md").is_file()]
        if not skill_dirs:
            raise ValueError(f"no skill directories with SKILL.md under: {source}")
        return skill_dirs

    if not source.is_dir():
        raise ValueError(f"skill directory does not exist: {source}")
    if not (source / "SKILL.md").is_file():
        raise ValueError(f"missing SKILL.md in skill directory: {source}")
    return [source]


def package_skill(skill_dir: Path, output_dir: Path, version: str) -> Path:
    """Zip one skill tree and return the archive path."""
    archive_path = output_dir / f"{skill_dir.name}-{version}.zip"
    with ZipFile(archive_path, "w", compression=ZIP_DEFLATED) as archive:
        for path in iter_skill_files(skill_dir):
            relative_path = path.relative_to(skill_dir.parent)
            archive.write(path, arcname=relative_path.as_posix())
    return archive_path


def main() -> int:
    """Package skill directories into dist/skills archives."""
    args = parse_args()
    repo_root = Path(__file__).resolve().parent.parent
    source = Path(args.source).resolve()
    output_dir = Path(args.output_dir).resolve()

    try:
        skill_dirs = resolve_skill_dirs(source, args.all)
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1

    version = args.version or workspace_version(repo_root)
    output_dir.mkdir(parents=True, exist_ok=True)

    for archive_path in (package_skill(skill_dir, output_dir, version) for skill_dir in skill_dirs):
        print(archive_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
