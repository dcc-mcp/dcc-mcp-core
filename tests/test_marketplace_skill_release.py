"""Regression tests for the marketplace skill release pipeline.

Guards the two properties the publishing flow depends on: archives pack
reproducibly, and rendered entries satisfy the marketplace catalog rules.
Run with ``python tests/test_marketplace_skill_release.py``.
"""

from __future__ import annotations

import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts" / "release"))

import pack_skill_release as packer  # noqa: E402
import render_marketplace_entry as renderer  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[1]


def build_fixture(root: Path) -> Path:
    """Create a minimal skill directory with a nested file."""
    skill = root / "sample-skill"
    (skill / "scripts").mkdir(parents=True)
    (skill / "SKILL.md").write_text(
        "---\nname: sample-skill\ndescription: A sample skill.\n---\n\nBody.\n",
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
        result = packer.pack_skill(skill, out, "1.2.3")

        assert result["skill"] == "sample-skill"
        assert result["name"] == "sample-skill"
        assert result["asset"] == "sample-skill-1.2.3.zip"
        assert len(result["sha256"]) == 64, result["sha256"]
        assert result["skill_root"] == "skill/sample-skill"
        assert (out / "sample-skill-1.2.3.zip").is_file()


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


def main() -> int:
    tests = [value for name, value in sorted(globals().items()) if name.startswith("test_")]
    failures = []
    for test in tests:
        try:
            test()
            print("PASS {}".format(test.__name__))
        except AssertionError as exc:
            failures.append((test.__name__, str(exc)))
            print("FAIL {}: {}".format(test.__name__, exc))
    if failures:
        print("\n{} test(s) failed".format(len(failures)))
        return 1
    print("\nall {} pipeline tests passed".format(len(tests)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
