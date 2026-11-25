#!/usr/bin/env python3
"""Generate a reproducible third-party license text bundle for release archives."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Dict
from typing import List
from typing import Tuple

SPDX_ID_PATTERN = re.compile(r"[A-Za-z0-9][A-Za-z0-9.+-]*")
SPDX_OPERATORS = {"AND", "OR", "WITH"}
LICENSE_FILE_PREFIXES = ("license", "copying", "notice")


def _source_revision(root: Path) -> str:
    value = os.environ.get("DCC_MCP_SOURCE_REVISION", "").strip()
    if value:
        return value
    try:
        completed = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return "unknown"
    return completed.stdout.strip() or "unknown"


def _spdx_ids(expression: str) -> List[str]:
    return [token for token in SPDX_ID_PATTERN.findall(expression) if token not in SPDX_OPERATORS]


def _package_license_files(package_root: Path) -> List[Tuple[str, bytes]]:
    files: List[Tuple[str, bytes]] = []
    for path in sorted(package_root.iterdir()):
        if not path.is_file() or not path.name.lower().startswith(LICENSE_FILE_PREFIXES):
            continue
        try:
            content = path.read_bytes()
            content.decode("utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        files.append((path.name, content))
    return files


def _canonical_license_files(root: Path, expression: str) -> List[Tuple[str, bytes]]:
    files: List[Tuple[str, bytes]] = []
    for identifier in _spdx_ids(expression):
        path = root / "licenses" / "spdx" / f"{identifier}.txt"
        if not path.is_file():
            raise ValueError(f"missing vendored SPDX text for {identifier!r} in {expression!r}")
        files.append((f"SPDX-{identifier}.txt", path.read_bytes()))
    return files


def _package_texts(root: Path, package: Dict) -> Tuple[str, List[Tuple[str, bytes]]]:
    expression = str(package.get("license") or "").strip()
    if not expression:
        raise ValueError(f"package {package.get('name')} has no license expression")
    manifest_path = package.get("manifest_path")
    files = _package_license_files(Path(manifest_path).parent) if manifest_path else []
    if files:
        return "package", files
    return "SPDX", _canonical_license_files(root, expression)


def _rust_packages(root: Path) -> List[Dict]:
    try:
        completed = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = getattr(exc, "stderr", None) or str(exc)
        raise ValueError(f"cargo metadata failed: {detail.strip()}") from exc
    metadata = json.loads(completed.stdout)
    return sorted(
        [package for package in metadata["packages"] if package.get("source")],
        key=lambda package: (package["name"], package["version"]),
    )


def generate(root: Path) -> str:
    """Generate the deterministic license bundle text."""
    packages = _rust_packages(root)
    package_rows: List[Tuple[Dict, str, List[str], str]] = []
    text_by_digest: Dict[str, Tuple[str, bytes]] = {}

    for package in packages:
        source_kind, files = _package_texts(root, package)
        references: List[str] = []
        for filename, content in files:
            digest = hashlib.sha256(content).hexdigest()
            text_by_digest.setdefault(digest, (filename, content))
            references.append(digest)
        package_rows.append((package, source_kind, references, str(package["license"])))

    lines = [
        "Third-party license texts",
        "",
        "This file is generated from Cargo.lock and the vendored SPDX license corpus.",
        f"Source revision: {_source_revision(root)}",
        "SPDX corpus: license-list-data v3.27.0",
        f"Packages: {len(package_rows)}",
        "",
        "Package index",
        "=============",
    ]
    for package, source_kind, references, expression in package_rows:
        source = f"https://crates.io/crates/{package['name']}/{package['version']}"
        lines.extend(
            [
                f"- {package['name']} {package['version']}",
                f"  License: {expression}",
                f"  Source: {source}",
                f"  Text source: {source_kind}",
                f"  Text digests: {', '.join(references)}",
            ]
        )

    lines.extend(["", "License texts", "=============="])
    for digest in sorted(text_by_digest):
        filename, content = text_by_digest[digest]
        lines.extend([f"[{digest}] {filename}", content.decode("utf-8").rstrip("\r\n"), ""])
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    """Generate the license bundle and write it to the requested path."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("THIRD_PARTY_LICENSES.txt"))
    parser.add_argument("root", nargs="?", type=Path, default=Path.cwd())
    args = parser.parse_args()
    try:
        content = generate(args.root.resolve())
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open("w", encoding="utf-8", newline="\n") as stream:
            stream.write(content)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    print(f"generated {args.output} from Cargo.lock")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
