"""Regression tests for the marketplace skill release pipeline.

Guards the two properties the publishing flow depends on: archives pack
reproducibly, and rendered entries satisfy the marketplace catalog rules.
Run with ``python tests/test_marketplace_skill_release.py``.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts" / "release"))

import pack_skill_release as packer
import render_marketplace_entry as renderer

REPO_ROOT = Path(__file__).resolve().parents[1]


def build_fixture(root: Path) -> Path:
    """Create a minimal skill directory with a nested file."""
    skill = root / "sample-skill"
    (skill / "scripts").mkdir(parents=True)
    (skill / "SKILL.md").write_text(
        "---\n"
        "name: sample-skill\n"
        "description: A sample skill.\n"
        'compatibility: "dcc-mcp-core 0.19.91+, Python 3.7+"\n'
        "metadata:\n"
        "  dcc-mcp:\n"
        '    version: "1.2.3"\n'
        "---\n\nBody.\n",
        encoding="utf-8",
    )
    (skill / "tools.yaml").write_text("tools: []\n", encoding="utf-8")
    (skill / "scripts" / "run.py").write_text("print('hi')\n", encoding="utf-8")
    (skill / "__pycache__").mkdir()
    (skill / "__pycache__" / "stale.pyc").write_bytes(b"\x00")
    (skill / "notes.txt.swp").write_text("scratch", encoding="utf-8")
    return skill


def test_collect_skips_vcs_and_cache() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        names = [item.as_posix() for item in packer.collect_files(skill)]
        assert names == ["SKILL.md", "scripts/run.py", "tools.yaml"], names
        assert names == sorted(names), "collect_files must be deterministic"


def test_archive_is_reproducible() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        first = packer.build_zip_bytes(skill, "skill/sample-skill")
        second = packer.build_zip_bytes(skill, "skill/sample-skill")
        assert first == second, "archive bytes differ between identical runs"
        assert packer.sha256_hex(first) == packer.sha256_hex(second)


def test_pack_writes_manifest_with_digest() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        skill = build_fixture(root)
        out = root / "out"
        result = packer.pack_skill(skill, out, "9.9.9")

        assert result["skill"] == "sample-skill"
        assert result["name"] == "sample-skill"
        assert result["asset"] == "sample-skill-1.2.3.zip"
        assert len(result["sha256"]) == 64, result["sha256"]
        assert result["skill_root"] == "skill/sample-skill"
        assert (out / "sample-skill-1.2.3.zip").is_file()


def test_version_comes_from_skill_not_release_version() -> None:
    """The skill's own version wins; the release version names assets only."""
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        assert packer.read_skill_version(skill) == "1.2.3"
        result = packer.pack_skill(skill, Path(raw) / "out", "9.9.9")
        assert result["version"] == "1.2.3", result["version"]
        assert result["release_version"] == "9.9.9"
        # The archive name follows the skill version, not the release tag.
        assert result["asset"] == "sample-skill-1.2.3.zip"


def test_min_core_version_parsed_from_compatibility() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        assert packer.read_min_core_version(skill) == "0.19.91"
        assert packer.pack_skill(skill, Path(raw) / "out", "9.9.9")["min_core_version"] == "0.19.91"

        (skill / "SKILL.md").write_text("---\nname: sample-skill\n---\n", encoding="utf-8")
        assert packer.read_min_core_version(skill) is None


def test_min_core_version_is_normalized_to_semver() -> None:
    """The schema types minCoreVersion as semver, so '0.17' must become 0.17.0."""
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        (skill / "SKILL.md").write_text(
            '---\nname: sample-skill\ncompatibility: "dcc-mcp-core 0.17+, Python 3.7+"\n---\n',
            encoding="utf-8",
        )
        assert packer.read_min_core_version(skill) == "0.17.0"


def test_read_skill_version_strips_release_please_marker() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        (skill / "SKILL.md").write_text(
            '---\nname: sample-skill\nmetadata:\n  dcc-mcp:\n    version: "0.20.42"  # x-release-please-version\n---\n',
            encoding="utf-8",
        )
        assert packer.read_skill_version(skill) == "0.20.42"


def test_frontmatter_stops_before_markdown_body() -> None:
    """Body examples and later horizontal rules cannot supply missing fields."""
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        (skill / "SKILL.md").write_text(
            "---\nname: frontmatter-name\n---\n\nBody.\n\n---\n"
            "name: body-name\n"
            'compatibility: "dcc-mcp-core 0.17+"\n'
            'metadata:\n  dcc-mcp:\n    version: "9.9.9"\n---\n',
            encoding="utf-8",
        )
        assert packer.read_frontmatter(skill) == {"name": "frontmatter-name"}
        assert packer.read_skill_name(skill) == "frontmatter-name"
        assert packer.read_skill_version(skill) is None
        assert packer.read_min_core_version(skill) is None


def test_frontmatter_requires_opening_and_closing_delimiters() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        metadata = (
            'name: misplaced-name\ncompatibility: "dcc-mcp-core 0.17+"\nmetadata:\n  dcc-mcp:\n    version: "9.9.9"\n'
        )
        for content in ("Body.\n---\n" + metadata + "---\n", "---\n" + metadata, metadata, ""):
            (skill / "SKILL.md").write_text(content, encoding="utf-8")
            assert packer.read_frontmatter(skill) == {}, content
            assert packer.read_skill_name(skill) == "sample-skill", content
            assert packer.read_skill_version(skill) is None, content
            assert packer.read_min_core_version(skill) is None, content


def test_skill_version_rejects_other_mapping_scopes() -> None:
    """Only metadata.dcc-mcp.version can supply the archive version."""
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        skill = build_fixture(root)
        for metadata in (
            'metadata:\n  dcc-mcp:\n    layer: domain\n  other:\n    version: "9.9.9"\n',
            'metadata:\n  dcc-mcp:\n    layer: domain\n  version: "9.9.9"\n',
            'metadata:\n  dcc-mcp:\n    other:\n      version: "9.9.9"\n',
            'metadata:\n  other:\n    dcc-mcp:\n      version: "9.9.9"\n',
            'metadata:\n  dcc-mcp:\n    layer: domain\nother:\n  version: "9.9.9"\n',
        ):
            (skill / "SKILL.md").write_text("---\n" + metadata + "---\n", encoding="utf-8")
            assert packer.read_skill_version(skill) is None, metadata
            try:
                packer.pack_skill(skill, root / "out", "1.0.0")
            except SystemExit as exc:
                assert "has no metadata.dcc-mcp.version" in str(exc)
            else:
                raise AssertionError("packing must reject a missing skill version")
            assert not (root / "out").exists()


def test_skill_version_accepts_direct_mapping_after_nested_siblings() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        for indent in ("  ", "    "):
            (skill / "SKILL.md").write_text(
                "---\nmetadata:\n"
                f"{indent}other:\n{indent * 2}version: '9.9.9'\n"
                f"{indent}dcc-mcp:\n"
                f"{indent * 2}other:\n{indent * 3}version: '8.8.8'\n"
                "\n# A comment does not leave the mapping.\n"
                f"{indent * 2}version: '1.2.3' # x-release-please-version\n---\n",
                encoding="utf-8",
            )
            assert packer.read_skill_version(skill) == "1.2.3", indent


def test_min_core_version_requires_exact_package_and_numeric_version() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        for compatibility in (
            "dcc-mcp-core",
            "dcc-mcp-core   ",
            "dcc-mcp-core +",
            "dcc-mcp-core latest",
            "dcc-mcp-core 0.invalid.17+",
            "dcc-mcp-core 0.17.0.1+",
            "dcc-mcp-core-extra 0.17+",
            "dcc-mcp-core0.17+",
        ):
            (skill / "SKILL.md").write_text(
                f'---\ncompatibility: "{compatibility}, Python 3.7+"\n---\n', encoding="utf-8"
            )
            assert packer.read_min_core_version(skill) is None, compatibility


def test_bare_core_compatibility_uses_renderer_fallback() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        skill = build_fixture(root)
        skill_md = skill / "SKILL.md"
        skill_md.write_text(
            skill_md.read_text(encoding="utf-8").replace("dcc-mcp-core 0.19.91+", "dcc-mcp-core"),
            encoding="utf-8",
        )
        out = root / "out"
        manifest = out / "manifest.json"
        assert (
            packer.main(["--skill", str(skill), "--out", str(out), "--manifest", str(manifest), "--version", "9.9.9"])
            == 0
        )
        packed = json.loads(manifest.read_text(encoding="utf-8"))
        assert packed["min_core_version_fallback"] == ["sample-skill"]
        assert packed["skills"][0]["min_core_version"] is None

        entries = out / "entries.json"
        args = ["--manifest", str(manifest), "--base-url", "https://example.test/v9.9.9", "--out", str(entries)]
        assert renderer.main(args) == 1
        assert renderer.main([*args, "--min-core-version", "9.9.9"]) == 0
        assert json.loads(entries.read_text(encoding="utf-8"))[0]["minCoreVersion"] == "9.9.9"


def test_zip_metadata_is_host_independent() -> None:
    """Pinning create_system is what makes a digest verifiable cross-OS."""
    import zipfile

    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        payload = packer.build_zip_bytes(skill, "skill/sample-skill")
        import io

        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            for info in archive.infolist():
                assert info.create_system == 3, info.filename
                assert info.date_time == (1980, 1, 1, 0, 0, 0)
                assert info.external_attr == 0o644 << 16
                assert info.compress_type == zipfile.ZIP_DEFLATED


def test_read_skill_name_prefers_frontmatter() -> None:
    with tempfile.TemporaryDirectory() as raw:
        skill = build_fixture(Path(raw))
        assert packer.read_skill_name(skill) == "sample-skill"
        (skill / "SKILL.md").unlink()
        assert packer.read_skill_name(skill) == "sample-skill"


def test_source_validation_accepts_zip() -> None:
    source = {
        "type": "zip",
        "url": "https://example.test/a.zip",
        "sha256": "a" * 64,
        "skillRoots": ["skill/sample-skill"],
    }
    assert renderer.validate_source(source) == []


def test_source_validation_rejects_bad_shapes() -> None:
    good = {
        "type": "zip",
        "url": "https://example.test/a.zip",
        "sha256": "a" * 64,
        "skillRoots": ["skill/sample-skill"],
    }

    short = dict(good, sha256="abc123")
    assert renderer.validate_source(short)

    missing = dict(good)
    missing.pop("sha256")
    assert renderer.validate_source(missing)

    absolute = dict(good, skillRoots=["/etc/passwd"])
    assert renderer.validate_source(absolute)

    traversal = dict(good, skillRoots=["skill/../../escape"])
    assert renderer.validate_source(traversal)

    empty_roots = dict(good, skillRoots=[])
    assert renderer.validate_source(empty_roots)

    git_no_ref = {"type": "git", "url": "https://example.test/repo.git"}
    assert renderer.validate_source(git_no_ref)


def test_entry_requires_mandatory_fields() -> None:
    entry = renderer.build_entry(
        {
            "skill": "sample-skill",
            "name": "sample-skill",
            "version": "1.2.3",
            "asset": "sample-skill-1.2.3.zip",
            "sha256": "a" * 64,
            "skill_root": "skill/sample-skill",
        },
        base_url="https://example.test/download/v1.2.3",
        min_core_version="0.20.0",
        dcc=["python"],
        category="Skills",
        maintainer="dcc-mcp",
        tags=["skills"],
    )
    assert renderer.validate_entry(entry) == []
    assert entry["source"]["type"] == "zip"
    assert entry["source"]["skillRoots"] == ["skill/sample-skill"]

    broken = dict(entry)
    broken.pop("minCoreVersion")
    assert renderer.validate_entry(broken)


def test_entry_uses_per_skill_min_core_version() -> None:
    """A declared floor must beat the CLI fallback."""
    skill = {
        "skill": "sample-skill",
        "name": "sample-skill",
        "version": "1.2.3",
        "asset": "sample-skill-1.2.3.zip",
        "sha256": "a" * 64,
        "skill_root": "skill/sample-skill",
        "min_core_version": "0.19.91",
    }
    entry = renderer.build_entry(
        skill,
        base_url="https://example.test/download/v9.9.9",
        min_core_version="0.19.91",
        dcc=["python"],
        category="Skills",
        maintainer="dcc-mcp",
        tags=["skills"],
    )
    assert entry["minCoreVersion"] == "0.19.91"
    # The entry version follows the skill, and the asset name matches it.
    assert entry["version"] == "1.2.3"
    assert entry["source"]["url"].endswith("/sample-skill-1.2.3.zip")


def main() -> int:
    tests = [value for name, value in sorted(globals().items()) if name.startswith("test_")]
    failures = []
    for test in tests:
        try:
            test()
            print(f"PASS {test.__name__}")
        except AssertionError as exc:
            failures.append((test.__name__, str(exc)))
            print(f"FAIL {test.__name__}: {exc}")
    if failures:
        print(f"\n{len(failures)} test(s) failed")
        return 1
    print(f"\nall {len(tests)} pipeline tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
