"""Publish (register or update) an extension to a marketplace catalog.

Reads an extension directory containing SKILL.md, constructs a CatalogEntry,
and upserts it into the target marketplace.json. Optionally commits and pushes
when the catalog source is a local git repository.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
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

    # utf-8-sig strips a leading BOM, which would otherwise hide the opening ---.
    content = path.read_text(encoding="utf-8-sig")
    # Frontmatter is delimited by --- lines. The first line must be ---.
    if not content.startswith("---"):
        raise ValueError(f"SKILL.md at {path} has no YAML frontmatter (missing opening ---)")

    # Find the closing ---. It must be a line of its own; a `---` that appears
    # mid-line (e.g. `description: alpha---beta`) is content, not a delimiter.
    rest = content[3:]  # skip opening ---
    end_idx = _find_frontmatter_end(rest)
    if end_idx == -1:
        raise ValueError(f"SKILL.md at {path} has unclosed YAML frontmatter")

    frontmatter_text = rest[:end_idx].strip()

    # Use a minimal YAML parser — we could import yaml, but that adds a
    # dependency. For the dcc-mcp frontmatter subset, a line-oriented
    # parser that handles simple scalars, flow and block lists, folded
    # scalars, and nested blocks under metadata is sufficient.
    return _parse_simple_yaml(frontmatter_text)


def _find_frontmatter_end(rest: str) -> int:
    """Index of the closing frontmatter delimiter, or -1 when there is none.

    A delimiter is a line whose stripped content is exactly ``---`` (a run of
    three or more dashes is also accepted, as YAML treats it as a document
    end marker). Scanning line by line keeps dashes that appear *inside* a
    value -- ``description: alpha---beta`` -- from being mistaken for the
    closing fence.
    """
    offset = 0
    for line in rest.splitlines(keepends=True):
        if line.strip().strip("\r\n").startswith("---"):
            candidate = line.strip()
            if candidate == "---" or set(candidate) == {"-"}:
                return offset
        offset += len(line)
    return -1


def _parse_simple_yaml(text: str) -> dict[str, Any]:
    """Minimal YAML subset parser for dcc-mcp SKILL.md frontmatter.

    Handles:
    - plain scalars (key: value)
    - flow-sequence lists (key: [a, b, c])
    - block sequences (- item)
    - nested blocks (2-space indent, tabs supported) for metadata.dcc-mcp.*
    - >- folded block scalars (description: >-)
    """
    result: dict[str, Any] = {}
    # Stack of (indent, key path) for the blocks currently open. Every key --
    # with or without a value -- is resolved against it, so a sibling that
    # follows a nested block lands next to that block instead of inside it.
    block_stack: list[tuple[int, list[str]]] = []
    fold_key: str | None = None
    fold_lines: list[str] = []
    _fold_indent = 0
    _fold_parent: list[str] = []
    seq_indent = -1
    seq_path: list[str] = []

    def close_fold() -> None:
        nonlocal fold_key, fold_lines
        if fold_key is None:
            return
        # Write to the block captured when the fold started. Recomputing the path
        # from the terminating line's indent would lift the key into an ancestor
        # block and drop the host block entirely.
        _set_nested(result, [*_fold_parent, fold_key], _join_folded(fold_lines))
        fold_key = None
        fold_lines = []

    def path_for_indent(indent: int) -> list[str]:
        """Key path a key at ``indent`` belongs to, popping closed blocks."""
        while block_stack and indent <= block_stack[-1][0]:
            block_stack.pop()
        return list(block_stack[-1][1]) if block_stack else []

    for line in text.splitlines():
        stripped = line.strip()

        if not stripped:
            if fold_key is not None:
                fold_lines.append("")
            continue

        indent = _measure_indent(line)

        # A pending fold continues while lines stay more indented than its key.
        if fold_key is not None and indent > _fold_indent:
            fold_lines.append(stripped)
            continue

        # Finalise any pending fold before handling the line that ends it.
        close_fold()

        # Detect a block-sequence item: `- value`, or a bare `-` for an empty item.
        if stripped[0] == "-" and (len(stripped) == 1 or stripped[1] in " \t"):
            if seq_indent < 0:
                # No `key:` introduced this sequence; attach it to the enclosing
                # block so `- a` under `items:` still produces items: [a].
                owner_path = path_for_indent(indent)
                seq_key = owner_path[-1] if owner_path else None
                if seq_key is None:
                    continue
                seq_path = owner_path
                seq_indent = indent
            _get_nested(result, seq_path).append(_strip_quotes(stripped[1:].strip()))
            continue

        if seq_indent >= 0:
            seq_indent = -1
            seq_path = []

        # Detect fold start: key: >-
        if stripped.endswith(">-"):
            key = stripped[:-2].strip().rstrip(":")
            fold_key = key
            _fold_indent = indent  # continuation lines must be more indented than key
            _fold_parent = path_for_indent(indent)
            continue

        # Detect key: value, key: [list], or a parent key for a nested block
        if ":" in stripped:
            # Split on first colon
            colon_idx = stripped.index(":")
            key = stripped[:colon_idx].strip()
            value_str = stripped[colon_idx + 1 :].strip()

            parent_path = path_for_indent(indent)

            if not value_str:
                # Parent key for a nested block — push onto the stack. The block
                # itself is created lazily so a key with no children stays absent.
                block_stack.append((indent, [*parent_path, key]))
                continue

            # Check for flow-sequence: [a, b, c]
            if value_str.startswith("[") and value_str.endswith("]"):
                inner = value_str[1:-1]
                items = _parse_flow_sequence(inner) if inner.strip() else []
                _set_nested(result, [*parent_path, key], items)
                continue

            _set_nested(result, [*parent_path, key], _strip_quotes(value_str))

    # Finalise any pending fold at EOF
    if fold_key is not None:
        _set_nested(result, [*_fold_parent, fold_key], _join_folded(fold_lines))

    return result


def _measure_indent(line: str) -> int:
    """Measure a line's indent in columns, expanding tabs like YAML does."""
    indent = 0
    for ch in line:
        if ch == " ":
            indent += 1
        elif ch == "\t":
            indent += 2  # a tab is worth one indent level
        else:
            break
    return indent


def _join_folded(fold_lines: list[str]) -> str:
    """Join folded-scalar lines: a blank line folds into a single newline."""
    parts: list[str] = []
    for i, current in enumerate(fold_lines):
        if not current:
            parts.append("\n")
            continue
        if i and parts and parts[-1] != "\n" and fold_lines[i - 1]:
            parts.append(" ")
        parts.append(current)
    return "".join(parts)


def _strip_quotes(value: str) -> str:
    """Remove one layer of matching surrounding quotes."""
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ('"', "'"):
        return value[1:-1]
    return value


def _get_nested(d: dict[str, Any], path: list[str]) -> Any:
    """Read the list at a nested key path, creating intermediate dicts."""
    for key in path[:-1]:
        if key not in d:
            d[key] = {}
        d = d[key]
    if not isinstance(d.get(path[-1]), list):
        d[path[-1]] = []
    return d[path[-1]]


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
    if in_quotes:
        raise ValueError(f"unterminated quoted item in flow sequence: {inner!r}")
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
    sha256: str | None,
    version: str | None,
    maintainer: str | None,
    icon: str | None,
    tags: list[str],
    min_core_version: str | None,
    extension_url: str | None,
) -> dict[str, Any]:
    """Build a CatalogEntry dict from SKILL.md metadata and CLI inputs."""
    if install_type == "git" and not re.fullmatch(r"[0-9a-fA-F]{40}", install_ref or ""):
        raise ValueError("git installs require a full 40-character commit object ID")
    if install_type == "zip" and not re.fullmatch(r"(?:sha256:)?[0-9a-fA-F]{64}", sha256 or ""):
        raise ValueError("zip installs require exactly 64 hexadecimal SHA-256 digits")
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
    if sha256:
        entry["install"]["sha256"] = sha256

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
    parser.add_argument(
        "--install_ref",
        default=None,
        help="Full 40-character commit object ID for git-type installs",
    )
    parser.add_argument(
        "--sha256",
        default=None,
        help="Required 64-hex SHA-256 for zip-type installs",
    )
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
            sha256=args.sha256,
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
