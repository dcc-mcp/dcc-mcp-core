from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

_SCRIPT = Path(__file__).resolve().parents[1] / "skills" / "marketplace-publish-extension" / "scripts" / "publish.py"
_SPEC = importlib.util.spec_from_file_location("marketplace_publish_integrity", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
_PUBLISH = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_PUBLISH)


def _build_entry(**overrides):
    options = {
        "skill_md": {"name": "test-skill", "description": "Test skill"},
        "install_url": "https://github.com/dcc-mcp/example.git",
        "install_type": "git",
        "install_ref": "a" * 40,
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


def test_git_publish_requires_full_commit_object_id():
    with pytest.raises(ValueError, match="40-character commit"):
        _build_entry(install_ref="main")

    entry = _build_entry(install_ref="A" * 40)
    assert entry["install"]["ref"] == "A" * 40


def test_zip_publish_requires_and_preserves_sha256():
    with pytest.raises(ValueError, match="64 hexadecimal SHA-256"):
        _build_entry(
            install_type="zip",
            install_ref=None,
            sha256=None,
            install_url="https://example.invalid/package.zip",
        )

    digest = "sha256:" + "b" * 64
    entry = _build_entry(
        install_type="zip",
        install_ref=None,
        sha256=digest,
        install_url="https://example.invalid/package.zip",
    )
    assert entry["install"]["sha256"] == digest
