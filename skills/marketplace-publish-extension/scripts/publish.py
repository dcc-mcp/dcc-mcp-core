"""Publish (register or update) an extension to a marketplace catalog.

Reads an extension directory containing SKILL.md, constructs a CatalogEntry,
and upserts it into the target marketplace.json. Optionally commits and pushes
when the catalog source is a local git repository.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from typing import Any

# ── SKILL.md frontmatter parsing ──────────────────────────────────────────────


def _parse_skill_md(path: Path) -> dict[str, Any]:
    """Parse YAML frontmatter from a SKILL.md file.

    Returns a dict with the frontmatter contents, or raises an error if
    the file is missing or has no valid frontmatter.
    """
    if not path.is_file():
        raise FileNotFoundError(f"SKILL.md not found at {path}")

    content = path.read_text(encoding="utf-8")
    # Frontmatter is delimited by --- lines. The first line must be ---.
    if not content.startswith("---"):
        raise ValueError(f"SKILL.md at {path} has no YAML frontmatter (missing opening ---)")

    # Find the closing ---
    rest = content[3:]  # skip opening ---
    end_idx = rest.find("\n---")
    if end_idx == -1:
        # Try with just --- at the very start
        end_idx = rest.find("---")
    if end_idx == -1:
        raise ValueError(f"SKILL.md at {path} has unclosed YAML frontmatter")

    frontmatter_text = rest[:end_idx].strip()

    # Use a minimal YAML parser — we could import yaml, but that adds a
    # dependency. For the dcc-mcp frontmatter subset, a line-oriented
    # parser that handles simple scalars, lists with [], and nested
    # key: value under metadata is sufficient.
    return _parse_simple_yaml(frontmatter_text)


def _parse_simple_yaml(text: str) -> dict[str, Any]:
    """Minimal YAML subset parser for dcc-mcp SKILL.md frontmatter.

    Handles:
    - plain scalars (key: value)
    - flow-sequence lists (key: [a, b, c])
    - nested blocks (2-space indent) for metadata.dcc-mcp.*
    - >- folded block scalars (description: >-)
    """
    result: dict[str, Any] = {}
    current_path: list[str] = []
    current_indent = 0
    fold_key: str | None = None
    fold_lines: list[str] = []

    for line in text.splitlines():
        # Skip empty lines unless we're in a fold
        stripped = line.strip()
        if not stripped:
            if fold_key is not None:
                fold_lines.append("")
            continue

        # If we're in a folded scalar (>-) and the line is more indented
        indent = len(line) - len(line.lstrip())
        if fold_key is not None and indent > current_indent:
            fold_lines.append(stripped)
            continue

        # Finalise any pending fold
        if fold_key is not None:
            _set_nested(result, [*current_path, fold_key], " ".join(fold_lines))
            fold_key = None
            fold_lines = []

        # Detect fold start: key: >-
        if stripped.endswith(">-"):
            key = stripped[:-2].strip().rstrip(":")
            fold_key = key
            current_indent = indent  # continuation lines must be more indented than key
            # Do NOT mutate current_path here — fold_key is combined with
            # current_path at finalisation time.
            continue

        # Detect key: value or key: [list]
        if ":" in stripped:
            # Split on first colon
            colon_idx = stripped.index(":")
            key = stripped[:colon_idx].strip()
            value_str = stripped[colon_idx + 1 :].strip()

            if not value_str:
                # Parent key for a nested block — push onto path
                current_path = _path_from_indent(result, indent, key, current_path)
                continue

            # Check for flow-sequence: [a, b, c]
            if value_str.startswith("[") and value_str.endswith("]"):
                inner = value_str[1:-1]
                items = _parse_flow_sequence(inner) if inner.strip() else []
                _set_nested(result, [*current_path, key], items)
                continue

            # Remove surrounding quotes
            value = value_str
            if (value.startswith('"') and value.endswith('"')) or (value.startswith("'") and value.endswith("'")):
                value = value[1:-1]
            _set_nested(result, [*current_path, key], value)

    # Finalise any pending fold at EOF
    if fold_key is not None:
        _set_nested(result, [*current_path, fold_key], " ".join(fold_lines))

    return result


def _path_from_indent(root: dict[str, Any], indent: int, key: str, current_path: list[str]) -> list[str]:
    """Determine the nested key path based on indentation level."""
    # 0 indent = top-level; 2 indent = one level deep; etc.
    depth = indent // 2
    new_path = current_path[:depth] if depth < len(current_path) else current_path
    if depth == len(new_path):
        new_path.append(key)
    else:
        new_path = [*new_path[:depth], key]
    return new_path


def _set_nested(d: dict[str, Any], path: list[str], value: Any) -> None:
    """Set a value at a nested key path, creating intermediate dicts."""
    for key in path[:-1]:
        if key not in d:
            d[key] = {}
        d = d[key]
    d[path[-1]] = value


def _parse_flow_sequence(inner: str) -> list[str]:
    """Parse a YAML flow sequence like 'a, b, "c d"' into a list of strings."""
    items: list[str] = []
    current = ""
    in_quotes = False
    quote_char = ""
    for ch in inner:
        if in_quotes:
            if ch == quote_char:
                in_quotes = False
            else:
                current += ch
        elif ch in ('"', "'"):
            in_quotes = True
            quote_char = ch
        elif ch == ",":
            trimmed = current.strip()
            if trimmed:
                items.append(trimmed)
            current = ""
        else:
            current += ch
    trimmed = current.strip()
    if trimmed:
        items.append(trimmed)
    return items


# ── CatalogEntry building ─────────────────────────────────────────────────────


def _build_catalog_entry(
    skill_md: dict[str, Any],
    install_url: str,
    install_type: str,
    install_ref: str | None,
    version: str | None,
    maintainer: str | None,
    icon: str | None,
    tags: list[str],
    min_core_version: str | None,
    extension_url: str | None,
) -> dict[str, Any]:
    """Build a CatalogEntry dict from SKILL.md metadata and CLI inputs."""
    dcc_mcp_meta = skill_md.get("metadata", {}).get("dcc-mcp", {})

    # Name from SKILL.md frontmatter
    name = skill_md.get("name", "")
    if not name:
        raise ValueError("SKILL.md frontmatter is missing required 'name' field")

    # Description from SKILL.md
    description = skill_md.get("description", "")

    # DCC targets from metadata.dcc-mcp.dcc
    dcc_raw = dcc_mcp_meta.get("dcc", "python")
    if isinstance(dcc_raw, list):
        dcc_targets = dcc_raw
    elif isinstance(dcc_raw, str):
        dcc_targets = [t.strip() for t in dcc_raw.split(",") if t.strip()]
    else:
        dcc_targets = ["python"]

    # Version: CLI override > metadata > None
    entry_version = version or dcc_mcp_meta.get("version")

    # Tags: merge metadata tags + CLI tags
    meta_tags_raw = dcc_mcp_meta.get("tags", "")
    if isinstance(meta_tags_raw, list):
        meta_tags = meta_tags_raw
    elif isinstance(meta_tags_raw, str):
        meta_tags = [t.strip() for t in meta_tags_raw.split(",") if t.strip()]
    else:
        meta_tags = []
    merged_tags = list(dict.fromkeys(meta_tags + tags))  # dedupe, preserve order

    # Maintainer: CLI > metadata
    entry_maintainer = maintainer or dcc_mcp_meta.get("maintainer")

    entry: dict[str, Any] = {
        "name": name,
        "description": description,
        "dcc": dcc_targets,
        "install": {
            "type": install_type,
            "url": install_url,
        },
    }

    if install_ref:
        entry["install"]["ref"] = install_ref

    if entry_version:
        entry["version"] = entry_version

    if entry_maintainer:
        entry["maintainer"] = entry_maintainer

    if icon:
        entry["icon"] = icon

    if min_core_version:
        entry["min_core_version"] = min_core_version

    if extension_url:
        entry["url"] = extension_url

    if merged_tags:
        entry["tags"] = merged_tags

    return entry


# ── marketplace.json I/O ──────────────────────────────────────────────────────


def _load_marketplace_json(path: Path) -> dict[str, Any]:
    """Load a marketplace.json file or return a default template."""
    if path.is_file():
        text = path.read_text(encoding="utf-8")
        return json.loads(text)
    return {"version": "1", "entries": []}


def _save_marketplace_json(path: Path, catalog: dict[str, Any]) -> None:
    """Write a marketplace.json file with standardised formatting."""
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(catalog, indent=2, ensure_ascii=False) + "\n"
    path.write_text(text, encoding="utf-8")


def _upsert_entry(catalog: dict[str, Any], entry: dict[str, Any]) -> tuple[dict[str, Any], bool]:
    """Insert or update an entry in the catalog by name.

    Returns (catalog, was_updated) — was_updated is True when an
    existing entry was replaced, False when a new entry was appended.
    """
    entries: list[dict[str, Any]] = catalog.setdefault("entries", [])
    name = entry["name"]

    for i, existing in enumerate(entries):
        if existing.get("name") == name:
            entries[i] = entry
            return catalog, True

    entries.append(entry)
    return catalog, False


# ── marketplace source resolution ─────────────────────────────────────────────


def _resolve_marketplace_path(raw: str) -> Path:
    """Resolve a marketplace source string to a local file Path.

    Handles:
    - GitHub slug "owner/repo" → constructs raw.githubusercontent.com URL,
      but since we can't write to a URL, returns the local representation.
      For local operations users should pass a local path.
    - Full URL → not writable locally, returns a temp note path
    - Local path → resolves to the marketplace.json file
    """
    trimmed = raw.strip()

    # GitHub slug like "owner/repo" or "dcc-mcp/marketplace"
    if _looks_like_github_slug(trimmed):
        # For local write operations, the user should provide a local path.
        # We return a path relative to cwd as a sensible default.
        raise ValueError(
            f"'{trimmed}' looks like a GitHub slug. For publish operations "
            f"please provide a local path to the marketplace.json file or "
            f"the git repository directory containing it."
        )

    # URL
    if trimmed.startswith("http://") or trimmed.startswith("https://"):
        raise ValueError(
            f"'{trimmed}' is a URL. Cannot write marketplace.json to a remote "
            f"URL. Please provide a local path to the marketplace catalog file "
            f"or the git repository directory."
        )

    # Local path
    path = Path(trimmed).resolve()

    # If it's a directory, look for marketplace.json inside
    if path.is_dir():
        return path / "marketplace.json"

    # If it's a file path ending in .json, use as-is
    if path.suffix == ".json":
        return path

    # Otherwise, treat the parent as the directory
    return path / "marketplace.json"


def _looks_like_github_slug(value: str) -> bool:
    """Check if a string looks like 'owner/repo'."""
    if "/" not in value:
        return False
    parts = value.split("/")
    if len(parts) != 2:
        return False
    owner, repo = parts
    return bool(owner and repo and not value.startswith("http") and "\\" not in value and "." not in owner)


# ── Git helpers ───────────────────────────────────────────────────────────────


def _is_git_repo(path: Path) -> bool:
    """Check if a directory is inside a git working tree."""
    try:
        result = subprocess.run(
            ["git", "-C", str(path.parent), "rev-parse", "--git-dir"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        return result.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return False


def _git_commit_and_push(
    repo_dir: Path,
    marketplace_rel_path: str,
    entry_name: str,
    was_updated: bool,
) -> dict[str, Any]:
    """Stage marketplace.json, commit, and push to origin.

    Returns a dict with git operation results.
    """
    action = "Update" if was_updated else "Add"
    message = f"{action} marketplace entry: {entry_name}"

    try:
        # Stage the file
        subprocess.run(
            ["git", "-C", str(repo_dir), "add", marketplace_rel_path],
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )

        # Commit
        commit_result = subprocess.run(
            ["git", "-C", str(repo_dir), "commit", "-m", message],
            capture_output=True,
            text=True,
            timeout=30,
        )

        if commit_result.returncode != 0:
            # If nothing to commit, that's fine (file may not have changed)
            if "nothing to commit" in commit_result.stdout or "nothing to commit" in commit_result.stderr:
                return {
                    "committed": False,
                    "reason": "no changes to commit",
                }
            raise subprocess.CalledProcessError(
                commit_result.returncode,
                commit_result.args,
                commit_result.stdout,
                commit_result.stderr,
            )

        # Push
        push_result = subprocess.run(
            ["git", "-C", str(repo_dir), "push", "origin"],
            capture_output=True,
            text=True,
            timeout=60,
        )

        if push_result.returncode != 0:
            raise subprocess.CalledProcessError(
                push_result.returncode,
                push_result.args,
                push_result.stdout,
                push_result.stderr,
            )

        return {
            "committed": True,
            "message": message,
            "push_success": True,
            "stdout": push_result.stdout.strip(),
        }

    except subprocess.CalledProcessError as e:
        return {
            "committed": False,
            "error": f"git command failed: {e}",
            "stderr": e.stderr.strip() if e.stderr else "",
            "stdout": e.stdout.strip() if e.stdout else "",
        }


# ── Main ──────────────────────────────────────────────────────────────────────


def main() -> None:
    """Publish an extension to a marketplace catalog."""
    parser = argparse.ArgumentParser(description="Publish (register/update) an extension to a marketplace catalog.")
    parser.add_argument("--extension_dir", required=True, help="Path to the extension directory containing SKILL.md")
    parser.add_argument(
        "--marketplace_source",
        default="dcc-mcp/marketplace",
        help="Marketplace catalog source (local path, or directory)",
    )
    parser.add_argument("--install_url", required=True, help="Install source URL for the CatalogEntry")
    parser.add_argument("--install_type", default="git", choices=["git", "path", "zip"], help="Install source type")
    parser.add_argument("--install_ref", default=None, help="Git ref, branch, or tag for git-type installs")
    parser.add_argument("--version", default=None, help="Semantic version (overrides SKILL.md metadata if set)")
    parser.add_argument("--maintainer", default=None, help="Extension maintainer name")
    parser.add_argument("--icon", default=None, help="Icon path or URL for the CatalogEntry")
    parser.add_argument("--tags", nargs="*", default=[], help="Additional tags to add to the CatalogEntry")
    parser.add_argument("--min_core_version", default=None, help="Minimum dcc-mcp-core version required")
    parser.add_argument("--extension_url", default=None, help="Canonical URL for the extension (homepage, docs, etc.)")
    parser.add_argument(
        "--commit",
        action="store_true",
        default=False,
        help="Commit and push the updated marketplace.json (git repos only)",
    )
    args = parser.parse_args()

    try:
        # 1. Resolve and validate extension directory
        ext_dir = Path(args.extension_dir).resolve()
        if not ext_dir.is_dir():
            result = {
                "success": False,
                "message": f"Extension directory not found: {ext_dir}",
            }
            print(json.dumps(result))
            sys.exit(1)

        # 2. Parse SKILL.md
        skill_md_path = ext_dir / "SKILL.md"
        try:
            skill_md = _parse_skill_md(skill_md_path)
        except (FileNotFoundError, ValueError) as e:
            result = {
                "success": False,
                "message": str(e),
            }
            print(json.dumps(result))
            sys.exit(1)

        # 3. Build CatalogEntry
        entry = _build_catalog_entry(
            skill_md=skill_md,
            install_url=args.install_url,
            install_type=args.install_type,
            install_ref=args.install_ref,
            version=args.version,
            maintainer=args.maintainer,
            icon=args.icon,
            tags=args.tags,
            min_core_version=args.min_core_version,
            extension_url=args.extension_url,
        )

        # 4. Resolve marketplace.json path
        try:
            mp_path = _resolve_marketplace_path(args.marketplace_source)
        except ValueError as e:
            result = {
                "success": False,
                "message": str(e),
            }
            print(json.dumps(result))
            sys.exit(1)

        # 5. Load, upsert, save
        catalog = _load_marketplace_json(mp_path)
        catalog, was_updated = _upsert_entry(catalog, entry)
        _save_marketplace_json(mp_path, catalog)

        entry_name = entry["name"]

        # 6. Optional git commit+push
        git_result = None
        commit_error = None
        if args.commit:
            repo_dir = mp_path.parent
            if _is_git_repo(mp_path):
                # Determine relative path for git add
                try:
                    rel_path = str(mp_path.relative_to(repo_dir)).replace("\\", "/")
                except ValueError:
                    rel_path = mp_path.name

                git_result = _git_commit_and_push(repo_dir, rel_path, entry_name, was_updated)
                if git_result.get("error"):
                    commit_error = git_result["error"]
            else:
                commit_error = f"Marketplace source directory {repo_dir} is not a git repository. Skipping commit."

        # 7. Build result
        action = "updated" if was_updated else "created"
        message = f"Successfully {action} marketplace entry: {entry_name}"

        context: dict[str, Any] = {
            "entry": entry,
            "marketplace_path": str(mp_path),
            "action": action,
            "was_updated": was_updated,
            "total_entries": len(catalog["entries"]),
        }

        if git_result:
            context["git"] = git_result

        if commit_error:
            context["commit_error"] = commit_error

        result = {
            "success": True,
            "message": message,
            "context": context,
        }
        print(json.dumps(result, ensure_ascii=False))

    except Exception as e:
        result = {
            "success": False,
            "message": f"Unexpected error: {e}",
        }
        print(json.dumps(result))
        sys.exit(1)


if __name__ == "__main__":
    main()
