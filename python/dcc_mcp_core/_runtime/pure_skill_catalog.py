"""Metadata-only skill catalog for the Python 3.7 lite runtime."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from typing import Iterable

from dcc_mcp_core._py37_fallback import _parse_skill_fields


class PurePythonSkillMetadata:
    """Import-light metadata shape matching the fields used by skill queries."""

    __slots__ = (
        "dcc",
        "depends",
        "description",
        "implicit_invocation",
        "layer",
        "loaded",
        "missing_dependencies",
        "name",
        "path",
        "prompts",
        "runtime",
        "scope",
        "search_hint",
        "stage",
        "status",
        "tags",
        "tool_count",
        "tool_names",
        "tools",
        "version",
    )

    def __init__(
        self,
        *,
        name: str,
        description: str,
        path: str,
        dcc: str = "python",
        version: str = "1.0.0",
        scope: str = "repo",
        search_hint: str = "",
        layer: str | None = None,
        stage: str | None = None,
        tags: list[str] | None = None,
        depends: list[str] | None = None,
        tools: list[str] | None = None,
        prompts: list[str] | None = None,
        implicit_invocation: bool = True,
    ) -> None:
        self.name = name
        self.description = description
        self.path = path
        self.dcc = dcc
        self.version = version
        self.scope = scope
        self.search_hint = search_hint or description
        self.layer = layer
        self.stage = stage
        self.tags = list(tags or [])
        self.depends = list(depends or [])
        self.tools = list(tools or [])
        self.prompts = list(prompts or [])
        self.tool_names: list[str] = []
        self.tool_count = 0
        self.loaded = False
        self.status = "discovered"
        self.missing_dependencies: list[str] = []
        self.implicit_invocation = implicit_invocation
        self.runtime = None


class PurePythonSkillCatalog:
    """Discover and query SKILL.md metadata without the native extension.

    The lite sidecar remains dispatch-only, so this catalog intentionally does
    not claim that a discovered skill can be loaded or executed. It provides
    the metadata discovery portion of the public factory contract and keeps
    unsupported activation explicit at the owning facade.
    """

    def __init__(self, dcc_name: str) -> None:
        self._dcc_name = str(dcc_name or "dcc")
        self._skills: dict[str, Any] = {}

    def discover(self, search_paths: Iterable[tuple[str, str]]) -> int:
        """Replace the catalog with valid, DCC-compatible skill metadata."""
        discovered: dict[str, Any] = {}
        for skill_dir, scope in _skill_directories(search_paths):
            try:
                metadata = _read_skill_metadata(skill_dir, scope=scope)
            except (OSError, ValueError):
                continue
            if metadata is None:
                continue
            name = str(getattr(metadata, "name", "") or "").strip()
            if name and name not in discovered:
                discovered[name] = metadata
        self._skills = discovered
        return len(discovered)

    def list_skills(self) -> list[Any]:
        return [self._skills[name] for name in sorted(self._skills)]

    def search_skills(
        self,
        *,
        query: str | None = None,
        tags: Iterable[str] | None = None,
        dcc: str | None = None,
        scope: str | None = None,
        limit: int | None = None,
    ) -> list[Any]:
        """Return deterministic metadata matches for the supported fields."""
        query_terms = [part.casefold() for part in str(query or "").split() if part]
        required_tags = {str(tag).casefold() for tag in (tags or ()) if str(tag).strip()}
        expected_dcc = str(dcc or "").casefold()
        expected_scope = str(scope or "").casefold()
        if limit is not None and int(limit) <= 0:
            return []
        matches: list[Any] = []
        for skill in self.list_skills():
            skill_tags = {str(tag).casefold() for tag in getattr(skill, "tags", [])}
            skill_dcc = str(getattr(skill, "dcc", "") or "").casefold()
            skill_scope = str(getattr(skill, "scope", "") or "").casefold()
            haystack = " ".join(
                [
                    str(getattr(skill, "name", "")),
                    str(getattr(skill, "description", "")),
                    str(getattr(skill, "search_hint", "")),
                    " ".join(str(tag) for tag in getattr(skill, "tags", [])),
                ]
            ).casefold()
            if query_terms and not all(term in haystack for term in query_terms):
                continue
            if required_tags and not required_tags.issubset(skill_tags):
                continue
            if expected_dcc and skill_dcc != expected_dcc:
                continue
            if expected_scope and skill_scope != expected_scope:
                continue
            matches.append(skill)
            if limit is not None and len(matches) >= int(limit):
                break
        return matches

    def get_skill(self, name: str) -> Any | None:
        return self._skills.get(str(name))


def _skill_directories(search_paths: Iterable[tuple[str, str]]) -> list[tuple[Path, str]]:
    """Return unique immediate skill directories in path priority order."""
    result: list[tuple[Path, str]] = []
    seen: set[str] = set()
    for raw_path, scope in search_paths:
        root = Path(str(raw_path)).expanduser()
        if not root.is_dir():
            continue
        candidates = [root] if (root / "SKILL.md").is_file() else _child_directories(root)
        for candidate in candidates:
            if not (candidate / "SKILL.md").is_file():
                continue
            try:
                key = str(candidate.resolve())
            except OSError:
                key = str(candidate.absolute())
            if key not in seen:
                seen.add(key)
                result.append((candidate, scope))
    return result


def _child_directories(root: Path) -> list[Path]:
    try:
        return sorted((child for child in root.iterdir() if child.is_dir()), key=lambda path: path.name)
    except OSError:
        return []


def _read_skill_metadata(skill_dir: Path, *, scope: str) -> PurePythonSkillMetadata | None:
    fields = _parse_skill_fields(skill_dir)
    if fields is None:
        return None
    return PurePythonSkillMetadata(
        name=fields["name"],
        description=fields["description"],
        path=fields["path"],
        dcc=fields["dcc"],
        version=fields["version"],
        scope=scope,
        search_hint=fields["search_hint"],
        layer=fields["layer"],
        stage=fields["stage"],
        tags=fields["tags"],
        depends=fields["depends"],
        tools=fields["tools"],
        prompts=fields["prompts"],
        implicit_invocation=fields["implicit_invocation"],
    )


__all__ = ["PurePythonSkillCatalog", "PurePythonSkillMetadata"]
