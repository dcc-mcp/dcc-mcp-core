"""Regression tests for workspace dependency ownership."""

from __future__ import annotations

from pathlib import Path
import re

_ROOT = Path(__file__).resolve().parents[1]
_INTERNAL_NAME = re.compile(r'^name\s*=\s*"(dcc-mcp-[^"]+)"', re.MULTILINE)
_DEPENDENCY_LINE = re.compile(r"^(dcc-mcp-[a-z0-9-]+)\s*=\s*\{([^\n]+)\}", re.MULTILINE)
_SHARED_THIRD_PARTY = {
    "axum",
    "futures",
    "http",
    "rand",
    "sha2",
    "tempfile",
    "tower",
    "tower-http",
}
_DEFAULT_FEATURE_EXCEPTIONS = {
    ("dcc-mcp-cli", "dcc-mcp-sidecar"),
    ("dcc-mcp-gateway", "dcc-mcp-db"),
    ("dcc-mcp-gateway", "dcc-mcp-skills"),
    ("dcc-mcp-gateway-admin", "dcc-mcp-db"),
    ("dcc-mcp-server", "dcc-mcp-http"),
    ("dcc-mcp-server", "dcc-mcp-sidecar"),
}


def _workspace_dependency_names() -> set[str]:
    root_manifest = (_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    section = root_manifest.split("[workspace.dependencies]", 1)[1].split("[workspace.lints", 1)[0]
    return {match.group(1) for match in re.finditer(r"^(dcc-mcp-[a-z0-9-]+)\s*=", section, re.MULTILINE)}


def test_every_internal_crate_has_a_workspace_dependency() -> None:
    workspace_dependencies = _workspace_dependency_names()
    crate_names = set()
    for manifest in (_ROOT / "crates").glob("dcc-mcp-*/Cargo.toml"):
        match = _INTERNAL_NAME.search(manifest.read_text(encoding="utf-8"))
        assert match is not None, manifest
        crate_names.add(match.group(1))
    assert crate_names - workspace_dependencies == set()


def test_internal_paths_are_only_used_to_disable_workspace_defaults() -> None:
    exceptions = set()
    for manifest in (_ROOT / "crates").glob("dcc-mcp-*/Cargo.toml"):
        crate_name = _INTERNAL_NAME.search(manifest.read_text(encoding="utf-8")).group(1)
        text = manifest.read_text(encoding="utf-8")
        for match in _DEPENDENCY_LINE.finditer(text):
            dependency, declaration = match.groups()
            if 'path = "../dcc-mcp-' not in declaration:
                continue
            assert "default-features = false" in declaration
            exceptions.add((crate_name, dependency))
    assert exceptions == _DEFAULT_FEATURE_EXCEPTIONS


def test_shared_third_party_dependencies_are_inherited() -> None:
    direct = []
    for manifest in (_ROOT / "crates").glob("dcc-mcp-*/Cargo.toml"):
        text = manifest.read_text(encoding="utf-8")
        for name in _SHARED_THIRD_PARTY:
            match = re.search(rf"^{re.escape(name)}\s*=\s*(.+)$", text, re.MULTILINE)
            if match and "workspace = true" not in match.group(1):
                direct.append(f"{manifest.parent.name}:{name}")
    assert direct == []


def test_removed_manifest_noops_do_not_return() -> None:
    root_manifest = (_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    lib_section = root_manifest.split("[lib]", 1)[1].split("[dependencies]", 1)[0]
    assert "required-features" not in lib_section

    server_manifest = (_ROOT / "crates/dcc-mcp-server/Cargo.toml").read_text(encoding="utf-8")
    assert not re.search(r"^tunnel-agent\s*=", server_manifest, re.MULTILINE)
