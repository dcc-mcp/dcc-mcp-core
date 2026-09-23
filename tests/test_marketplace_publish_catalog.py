"""P1 coverage for marketplace-publish-extension catalog I/O and path resolution.

Complements ``test_marketplace_publish_integrity.py`` (which pins the git ref and
zip SHA-256 guards of ``_build_catalog_entry``) by covering the parts of
``publish.py`` that read, merge, and persist the catalog: ``_parse_skill_md``,
the metadata-derived fields of ``_build_catalog_entry``, ``_upsert_entry``,
``_load_marketplace_json`` / ``_save_marketplace_json``, and
``_resolve_marketplace_path``.
"""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess

import pytest

from conftest import REPO_ROOT

_PUBLISH_SCRIPT = REPO_ROOT / "skills" / "marketplace-publish-extension" / "scripts" / "publish.py"
_CREATE_SCRIPT = REPO_ROOT / "skills" / "marketplace-create-extension" / "scripts" / "create_extension.py"


def _load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_PUBLISH = _load(_PUBLISH_SCRIPT, "marketplace_publish_catalog")
_CREATE = _load(_CREATE_SCRIPT, "marketplace_create_extension_for_publish")

_GIT_REF = "a" * 40

_SKILL_MD = """---
name: maya-pipeline-tools
description: >-
  Publish Maya pipeline tools.
license: MIT-0
metadata:
  dcc-mcp:
    dcc: maya
    version: "1.4.0"
    maintainer: loonghao
    tags: ["marketplace", "maya"]
---

# Maya Pipeline Tools
"""


def _write_skill_md(directory: Path, text: str = _SKILL_MD) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    skill_md = directory / "SKILL.md"
    skill_md.write_text(text, encoding="utf-8")
    return skill_md


def _build_entry(skill_md: dict, **overrides) -> dict:
    options = {
        "skill_md": skill_md,
        "install_url": "https://github.com/dcc-mcp/example.git",
        "install_type": "git",
        "install_ref": _GIT_REF,
        "sha256": None,
        "version": None,
        "maintainer": None,
        "icon": None,
        "tags": [],
        "min_core_version": None,
        "extension_url": None,
    }
    options.update(overrides)
    return _PUBLISH._build_catalog_entry(**options)


# ── _parse_skill_md ───────────────────────────────────────────────────────────


def test_parse_skill_md_reads_frontmatter_and_ignores_body(tmp_path: Path) -> None:
    path = _write_skill_md(tmp_path / "ext")

    parsed = _PUBLISH._parse_skill_md(path)

    assert parsed["name"] == "maya-pipeline-tools"
    assert parsed["description"] == "Publish Maya pipeline tools."
    assert parsed["license"] == "MIT-0"
    assert parsed["metadata"]["dcc-mcp"] == {
        "dcc": "maya",
        "version": "1.4.0",
        "maintainer": "loonghao",
        "tags": ["marketplace", "maya"],
    }
    assert "Maya Pipeline Tools" not in parsed.values()


def test_parse_skill_md_round_trips_the_generated_scaffold(tmp_path: Path) -> None:
    """The create-extension template must parse back with the values it was given."""
    skill_dir = Path(
        _CREATE.create_extension(
            "maya-pipeline-tools",
            str(tmp_path),
            description="Publish Maya pipeline tools.",
            dcc_targets=["maya"],
            author="loonghao",
            action_name="run_export",
        )
    )

    parsed = _PUBLISH._parse_skill_md(skill_dir / "SKILL.md")

    assert parsed["name"] == "maya-pipeline-tools"
    assert parsed["description"] == "Publish Maya pipeline tools."
    assert parsed["license"] == "MIT-0"
    dcc_mcp = parsed["metadata"]["dcc-mcp"]
    assert dcc_mcp["dcc"] == "maya"
    assert dcc_mcp["version"] == "0.1.0"
    assert dcc_mcp["tools"] == "tools.yaml"
    assert dcc_mcp["maintainer"] == "loonghao"
    assert dcc_mcp["tags"] == ["marketplace", "extension", "maya"]


def test_parse_skill_md_accepts_crlf_document(tmp_path: Path) -> None:
    path = _write_skill_md(tmp_path / "ext", "---\r\nname: crlf-skill\r\ndcc: maya\r\n---\r\n\r\n# Body\r\n")

    assert _PUBLISH._parse_skill_md(path) == {"name": "crlf-skill", "dcc": "maya"}


def test_parse_skill_md_missing_file_raises_file_not_found(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError, match=r"SKILL\.md not found"):
        _PUBLISH._parse_skill_md(tmp_path / "missing" / "SKILL.md")


def test_parse_skill_md_without_opening_delimiter_raises(tmp_path: Path) -> None:
    path = _write_skill_md(tmp_path / "ext", "# Maya Pipeline Tools\n\nNo frontmatter here.\n")

    with pytest.raises(ValueError, match="no YAML frontmatter"):
        _PUBLISH._parse_skill_md(path)


def test_parse_skill_md_unterminated_frontmatter_raises(tmp_path: Path) -> None:
    path = _write_skill_md(tmp_path / "ext", "---\nname: unterminated\n")

    with pytest.raises(ValueError, match="unclosed YAML frontmatter"):
        _PUBLISH._parse_skill_md(path)


def test_parse_skill_md_bom_prefixed_file_is_rejected(tmp_path: Path) -> None:
    """Characterization: a UTF-8 BOM hides the opening `---` and the file is rejected."""
    path = _write_skill_md(tmp_path / "ext")
    path.write_text("\ufeff" + path.read_text(encoding="utf-8"), encoding="utf-8")

    with pytest.raises(ValueError, match="no YAML frontmatter"):
        _PUBLISH._parse_skill_md(path)


def test_parse_skill_md_unterminated_frontmatter_is_silently_truncated_at_inline_dashes(tmp_path: Path) -> None:
    """Characterization: the fallback scan matches `---` mid-line and truncates the value."""
    path = _write_skill_md(tmp_path / "ext", "---\nname: truncated\ndescription: alpha---beta\n")

    assert _PUBLISH._parse_skill_md(path) == {"name": "truncated", "description": "alpha"}


# ── _build_catalog_entry: metadata-derived fields ─────────────────────────────


@pytest.mark.parametrize(
    ("dcc_raw", "expected"),
    [
        pytest.param("maya", ["maya"], id="scalar"),
        pytest.param("maya, blender", ["maya", "blender"], id="comma-separated-string"),
        pytest.param(["maya", "blender"], ["maya", "blender"], id="list"),
        pytest.param(None, ["python"], id="missing-defaults-to-python"),
    ],
)
def test_build_catalog_entry_derives_dcc_targets(dcc_raw, expected: list) -> None:
    skill_md = {"name": "skill", "description": "d", "metadata": {"dcc-mcp": {"dcc": dcc_raw}}}

    entry = _build_entry(skill_md)

    assert entry["dcc"] == expected


@pytest.mark.parametrize(
    ("meta_tags", "cli_tags", "expected"),
    [
        pytest.param("maya, rigging", [], ["maya", "rigging"], id="comma-separated-metadata-tags"),
        pytest.param(["maya"], ["rigging"], ["maya", "rigging"], id="metadata-then-cli-tags"),
        pytest.param(["maya"], ["maya", "rigging"], ["maya", "rigging"], id="duplicates-deduplicated"),
        pytest.param("", ["rigging"], ["rigging"], id="empty-metadata-tags"),
        pytest.param("", [], None, id="no-tags-omits-the-key"),
    ],
)
def test_build_catalog_entry_merges_tags(meta_tags, cli_tags: list, expected) -> None:
    skill_md = {"name": "skill", "description": "d", "metadata": {"dcc-mcp": {"tags": meta_tags}}}

    entry = _build_entry(skill_md, tags=cli_tags)

    assert entry.get("tags") == expected


def test_build_catalog_entry_cli_arguments_override_metadata() -> None:
    skill_md = {
        "name": "skill",
        "description": "from frontmatter",
        "metadata": {"dcc-mcp": {"version": "1.0.0", "maintainer": "from-metadata"}},
    }

    entry = _build_entry(skill_md, version="2.0.0", maintainer="from-cli")

    assert entry["version"] == "2.0.0"
    assert entry["maintainer"] == "from-cli"


def test_build_catalog_entry_falls_back_to_metadata() -> None:
    skill_md = {
        "name": "skill",
        "description": "from frontmatter",
        "metadata": {"dcc-mcp": {"version": "1.0.0", "maintainer": "from-metadata"}},
    }

    entry = _build_entry(skill_md)

    assert entry["version"] == "1.0.0"
    assert entry["maintainer"] == "from-metadata"
    assert entry["description"] == "from frontmatter"


def test_build_catalog_entry_omits_empty_optional_fields() -> None:
    entry = _build_entry({"name": "skill", "description": "d"})

    assert set(entry) == {"name", "description", "dcc", "install"}
    assert entry["install"] == {"type": "git", "url": "https://github.com/dcc-mcp/example.git", "ref": _GIT_REF}


def test_build_catalog_entry_includes_optional_fields_when_set() -> None:
    entry = _build_entry(
        {"name": "skill", "description": "d"},
        icon="icons/skill.svg",
        min_core_version="0.20.0",
        extension_url="https://example.invalid/skill",
    )

    assert entry["icon"] == "icons/skill.svg"
    assert entry["min_core_version"] == "0.20.0"
    assert entry["url"] == "https://example.invalid/skill"


def test_build_catalog_entry_requires_a_name() -> None:
    with pytest.raises(ValueError, match="missing required 'name' field"):
        _build_entry({"description": "d"})


# ── _upsert_entry ─────────────────────────────────────────────────────────────


def test_upsert_entry_appends_new_entry() -> None:
    catalog = {"version": "1", "entries": [{"name": "other"}]}

    catalog, was_updated = _PUBLISH._upsert_entry(catalog, {"name": "new", "dcc": ["maya"]})

    assert was_updated is False
    assert [entry["name"] for entry in catalog["entries"]] == ["other", "new"]


def test_upsert_entry_replaces_existing_entry_in_place() -> None:
    catalog = {"version": "1", "entries": [{"name": "first"}, {"name": "target", "version": "1.0.0"}]}

    catalog, was_updated = _PUBLISH._upsert_entry(catalog, {"name": "target", "version": "2.0.0"})

    assert was_updated is True
    assert catalog["entries"] == [{"name": "first"}, {"name": "target", "version": "2.0.0"}]


def test_upsert_entry_creates_the_entries_key_when_missing() -> None:
    catalog, was_updated = _PUBLISH._upsert_entry({}, {"name": "new"})

    assert was_updated is False
    assert catalog == {"entries": [{"name": "new"}]}


# ── marketplace.json I/O ──────────────────────────────────────────────────────


def test_load_marketplace_json_returns_default_template_for_missing_file(tmp_path: Path) -> None:
    assert _PUBLISH._load_marketplace_json(tmp_path / "marketplace.json") == {"version": "1", "entries": []}


def test_save_and_load_marketplace_json_round_trip(tmp_path: Path) -> None:
    catalog = {
        "version": "1",
        "entries": [{"name": "maya-pipeline-tools", "description": "Maya \u5de5\u5177\u94fe", "dcc": ["maya"]}],
    }
    path = tmp_path / "nested" / "dir" / "marketplace.json"

    _PUBLISH._save_marketplace_json(path, catalog)

    assert _PUBLISH._load_marketplace_json(path) == catalog
    raw = path.read_text(encoding="utf-8")
    assert raw.endswith("\n")
    assert "\n  " in raw  # indent=2
    assert "Maya \u5de5\u5177\u94fe" in raw  # ensure_ascii=False


# ── _resolve_marketplace_path ─────────────────────────────────────────────────


def test_resolve_marketplace_path_appends_catalog_file_to_a_directory(tmp_path: Path) -> None:
    assert _PUBLISH._resolve_marketplace_path(str(tmp_path)) == tmp_path.resolve() / "marketplace.json"


def test_resolve_marketplace_path_keeps_a_json_file_path(tmp_path: Path) -> None:
    path = tmp_path / "custom-catalog.json"

    assert _PUBLISH._resolve_marketplace_path(str(path)) == path.resolve()


def test_resolve_marketplace_path_treats_a_suffixless_path_as_a_directory(tmp_path: Path) -> None:
    path = tmp_path / "catalog-dir"

    assert _PUBLISH._resolve_marketplace_path(str(path)) == path.resolve() / "marketplace.json"


def test_resolve_marketplace_path_resolves_relative_paths_against_cwd(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.chdir(tmp_path)

    assert _PUBLISH._resolve_marketplace_path("marketplace.json") == (tmp_path / "marketplace.json").resolve()


@pytest.mark.parametrize(
    "source",
    [
        pytest.param("dcc-mcp/marketplace", id="canonical-slug"),
        pytest.param("dcc-mcp/other-marketplace", id="other-slug"),
        pytest.param("  dcc-mcp/marketplace  ", id="surrounding-whitespace"),
        pytest.param("marketplace/catalog", id="characterization-two-segment-relative-path"),
    ],
)
def test_resolve_marketplace_path_rejects_github_slugs(source: str) -> None:
    with pytest.raises(ValueError, match="looks like a GitHub slug"):
        _PUBLISH._resolve_marketplace_path(source)


@pytest.mark.parametrize(
    "source",
    [
        pytest.param("https://example.invalid/marketplace.json", id="https"),
        pytest.param("http://example.invalid/marketplace.json", id="http"),
    ],
)
def test_resolve_marketplace_path_rejects_urls(source: str) -> None:
    with pytest.raises(ValueError, match="is a URL"):
        _PUBLISH._resolve_marketplace_path(source)


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        pytest.param("dcc-mcp/marketplace", True, id="slug"),
        pytest.param("owner/repo", True, id="minimal-slug"),
        pytest.param("not-a-slug", False, id="single-segment"),
        pytest.param("a/b/c", False, id="three-segments"),
        pytest.param("https://example.invalid/a/b", False, id="url-with-two-segments"),
        pytest.param("C:\\tmp\\catalog", False, id="windows-path"),
    ],
)
def test_looks_like_github_slug(value: str, expected: bool) -> None:
    assert _PUBLISH._looks_like_github_slug(value) is expected


# ── _is_git_repo ──────────────────────────────────────────────────────────────


def test_is_git_repo_is_false_outside_a_working_tree(tmp_path: Path) -> None:
    catalog = tmp_path / "marketplace.json"
    catalog.write_text("{}", encoding="utf-8")

    assert _PUBLISH._is_git_repo(catalog) is False


@pytest.mark.skipif(shutil.which("git") is None, reason="git is not available")
def test_is_git_repo_is_true_inside_a_working_tree(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    catalog = tmp_path / "marketplace.json"
    catalog.write_text("{}", encoding="utf-8")

    assert _PUBLISH._is_git_repo(catalog) is True


# ── End-to-end: parse → build → upsert → persist ──────────────────────────────


def test_publish_pipeline_persists_a_catalog_entry(tmp_path: Path) -> None:
    skill_md = _PUBLISH._parse_skill_md(_write_skill_md(tmp_path / "ext"))
    catalog_path = tmp_path / "marketplace.json"

    entry = _build_entry(skill_md, install_url="https://github.com/dcc-mcp/maya-pipeline-tools.git")
    catalog, was_updated = _PUBLISH._upsert_entry(_PUBLISH._load_marketplace_json(catalog_path), entry)
    _PUBLISH._save_marketplace_json(catalog_path, catalog)

    assert was_updated is False
    persisted = json.loads(catalog_path.read_text(encoding="utf-8"))
    assert persisted["entries"] == [
        {
            "name": "maya-pipeline-tools",
            "description": "Publish Maya pipeline tools.",
            "dcc": ["maya"],
            "install": {
                "type": "git",
                "url": "https://github.com/dcc-mcp/maya-pipeline-tools.git",
                "ref": _GIT_REF,
            },
            "version": "1.4.0",
            "maintainer": "loonghao",
            "tags": ["marketplace", "maya"],
        }
    ]
