"""Tests for primary and compatibility CLI release bundles."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from zipfile import ZipFile

from conftest import REPO_ROOT


SCRIPT = REPO_ROOT / "scripts" / "release" / "build_cli_bundle.py"


def load_builder():
    spec = importlib.util.spec_from_file_location("build_cli_bundle", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_build_bundle_supports_dccmcp_name(tmp_path: Path) -> None:
    builder = load_builder()
    binary = tmp_path / "dccmcp-linux-x86_64"
    binary.write_bytes(b"dccmcp")

    bundle = builder.build_bundle(
        version="0.19.60",
        platform="linux-x86_64",
        cli_bin=binary,
        out_dir=tmp_path / "dist",
        name_prefix="dccmcp",
    )

    assert bundle.name == "dccmcp-0.19.60-linux-x86_64.zip"
    with ZipFile(bundle) as archive:
        assert archive.namelist() == ["dccmcp"]
        assert archive.read("dccmcp") == b"dccmcp"
