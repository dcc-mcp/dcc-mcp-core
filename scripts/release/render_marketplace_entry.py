#!/usr/bin/env python
"""Render marketplace catalog entries from a pack manifest and validate them.

Turns the digest manifest produced by ``pack_skill_release.py`` into catalog
entry fragments whose ``source`` is a pinned zip plus SHA-256, then checks
every fragment against the ``$defs/source`` and ``$defs/skillEntry`` rules of
the marketplace v1 schema before anything is handed to a registry.

Usage:
    python scripts/release/render_marketplace_entry.py \
        --manifest dist/marketplace/manifest.json \
        --base-url https://github.com/dcc-mcp/dcc-mcp-core/releases/download/v0.20.42 \
        --min-core-version 0.20.42
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

HEX64 = re.compile(r"^[a-fA-F0-9]{64}$")
SKILL_ROOT = re.compile(r"^(?!/)(?!.*(?:^|/)\.\.(?:/|$))[A-Za-z0-9._/-]+$")

REQUIRED_ENTRY_FIELDS = (
    "name",
    "description",
    "version",
    "dcc",
    "tags",
    "category",
    "maintainer",
    "minCoreVersion",
    "source",
    "policy",
)


def repo_root() -> Path:
    """Return the repository root, defined as the parent of this script's dir."""
    return Path(__file__).resolve().parents[2]


def display_path(path: Path, root: Path) -> str:
    """Render ``path`` relative to the repo when possible, else absolute."""
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return str(path)


def validate_source(source: Dict[str, Any]) -> List[str]:
    """Validate one ``source`` object against the schema's ``$defs/source``."""
    errors: List[str] = []
    if source.get("type") not in ("git", "zip"):
        errors.append("source.type must be 'git' or 'zip'")
    if not isinstance(source.get("url"), str) or not source.get("url"):
        errors.append("source.url is required")

    sha = source.get("sha256")
    if source.get("type") == "zip":
        if not isinstance(sha, str):
            errors.append("zip sources require source.sha256")
        else:
            candidate = sha[7:] if sha.startswith("sha256:") else sha
            if not HEX64.match(candidate):
                errors.append("source.sha256 must be 64 hex digits (optionally sha256: prefixed)")

    if source.get("type") == "git" and not source.get("ref"):
        errors.append("git sources require source.ref")

    roots = source.get("skillRoots")
    if roots is not None:
        if not isinstance(roots, list) or not roots:
            errors.append("source.skillRoots must be a non-empty array when present")
        else:
            for root in roots:
                if not isinstance(root, str) or not SKILL_ROOT.match(root):
                    errors.append("invalid skillRoots entry: {!r}".format(root))
    return errors


def validate_entry(entry: Dict[str, Any]) -> List[str]:
    """Validate one catalog entry against the required-field list and source."""
    errors: List[str] = []
    for field in REQUIRED_ENTRY_FIELDS:
        if field not in entry:
            errors.append("missing required field: {}".format(field))

    dcc = entry.get("dcc")
    if not isinstance(dcc, list) or not dcc:
        errors.append("dcc must be a non-empty array")

    if not isinstance(entry.get("tags"), list) or not entry["tags"]:
        errors.append("tags must be a non-empty array")

    source = entry.get("source")
    if isinstance(source, dict):
        errors.extend(validate_source(source))
    else:
        errors.append("source must be an object")
    return errors


def build_entry(
    skill: Dict[str, Any],
    base_url: str,
    min_core_version: str,
    dcc: List[str],
    category: str,
    maintainer: str,
    tags: List[str],
    description: Optional[str] = None,
) -> Dict[str, Any]:
    """Build a catalog entry fragment for one packed skill."""
    url = "{}/{}".format(base_url.rstrip("/"), skill["asset"])
    return {
        "name": skill["name"],
        "description": description or skill["name"],
        "version": skill["version"],
        "dcc": dcc,
        "tags": tags,
        "category": category,
        "maintainer": maintainer,
        "minCoreVersion": min_core_version,
        "license": "MIT-0",
        "source": {
            "type": "zip",
            "url": url,
            "sha256": skill["sha256"],
            "skillRoots": [skill["skill_root"]],
        },
        "policy": {"installation": "available"},
    }


def load_defaults(root: Path) -> Dict[str, Any]:
    """Load per-skill overrides from ``scripts/release/marketplace_entry_map.json``."""
    path = root / "scripts" / "release" / "marketplace_entry_map.json"
    if not path.is_file():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--manifest", required=True, help="pack manifest JSON")
    parser.add_argument("--base-url", required=True, help="GitHub Release asset base URL")
    parser.add_argument("--min-core-version", required=True)
    parser.add_argument("--out", default="dist/marketplace/entries.json")
    parser.add_argument("--category", default="Skills")
    parser.add_argument("--maintainer", default="dcc-mcp")
    parser.add_argument("--dcc", default="python", help="comma-separated dcc list")
    args = parser.parse_args(argv)

    root = repo_root()
    manifest_path = root / args.manifest
    if not manifest_path.is_file():
        sys.stderr.write("render: manifest not found: {}\n".format(manifest_path))
        return 2

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    defaults = load_defaults(root)
    dcc_list = [item.strip() for item in args.dcc.split(",") if item.strip()]

    entries: List[Dict[str, Any]] = []
    failures: List[Tuple[str, List[str]]] = []

    for skill in manifest.get("skills", []):
        override = defaults.get(skill["skill"], {})
        entry = build_entry(
            skill,
            base_url=args.base_url,
            min_core_version=args.min_core_version,
            dcc=override.get("dcc", dcc_list),
            category=override.get("category", args.category),
            maintainer=override.get("maintainer", args.maintainer),
            tags=override.get("tags", ["skills"]),
            description=override.get("description"),
        )
        errors = validate_entry(entry)
        if errors:
            failures.append((skill["skill"], errors))
        entries.append(entry)

    out_path = root / args.out
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(entries, indent=2) + "\n", encoding="utf-8")

    for name, errors in failures:
        print("INVALID {}:".format(name))
        for error in errors:
            print("  - {}".format(error))

    if failures:
        print("\n{} entry(s) failed validation".format(len(failures)))
        return 1

    for entry in entries:
        print(
            "OK {} {} -> {} (sha256 {})".format(
                entry["name"],
                entry["version"],
                entry["source"]["url"].rsplit("/", 1)[-1],
                entry["source"]["sha256"][:16] + "...",
            )
        )
    print("entries: {}".format(display_path(out_path, root)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
