"""Keep every packaging / docs version claim on one source of truth.

``dcc-mcp-core`` ships two independent version artifacts from one release:

* the wheel's ``.dist-info`` metadata, generated from ``pyproject.toml``
  (``project.version``) — this is what ``pip`` reports and what
  ``dcc_mcp_core.__version__`` resolves to;
* the compiled PyO3 extension, whose ``__version__`` comes from
  ``env!("CARGO_PKG_VERSION")`` (``src/lib.rs``) — this is the artifact that
  actually executes and the value ``_version_util.package_version()`` prefers.

release-please bumps ``.release-please-manifest.json``, ``pyproject.toml``,
``Cargo.toml`` and every extra-file in the same release commit, so a correct
build advertises one number from both sides.  The field report that opened
this work had a ``.dist-info`` saying ``0.19.3`` next to an extension saying
``0.19.2`` — a partial upgrade, i.e. exactly the divergence these tests make
impossible to ship on purpose.

These tests are file-based (not import-based) so they gate the repository
state in CI regardless of which wheel happens to be installed in the
interpreter running pytest.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from pathlib import Path
import re

# Import third-party modules
import pytest

ROOT = Path(__file__).resolve().parents[1]
PYPROJECT = ROOT / "pyproject.toml"
CARGO = ROOT / "Cargo.toml"
#: release-please keeps the released version here; it is the base for every
#: other claim in the repository.
RELEASE_MANIFEST = ROOT / ".release-please-manifest.json"
RELEASE_PLEASE_CONFIG = ROOT / "release-please-config.json"
#: The Python-visible declaration of the compiled extension's ``__version__``.
PYTHON_STUB = ROOT / "python" / "dcc_mcp_core" / "_core.pyi"
#: The Rust declaration the extension is actually compiled from.
RUST_LIB = ROOT / "src" / "lib.rs"

MARKER = "x-release-please-version"
VERSION_TOKEN = re.compile(r"\d+\.\d+(?:\.\d+)?(?:[A-Za-z0-9.+-]*)?")
STUB_VERSION_LITERAL = re.compile(r'^__version__\s*:\s*builtins\.str\s*=\s*"(?P<version>[^"]+)"', re.MULTILINE)


SECTION_HEADER = re.compile(r"^\[(?P<name>[^\]]+)\]\s*(?:#.*)?$")
VERSION_ASSIGNMENT = re.compile(r"^version\s*=\s*[\"'](?P<version>[^\"']+)[\"']", re.MULTILINE)


def _section_version(path: Path, section: str) -> str:
    """Return ``version`` declared inside ``[section]`` of a TOML file.

    Deliberately regex-based rather than :mod:`tomllib`: this module gates the
    repository state and the suite runs on Python 3.7 (Maya 2022), where
    ``tomllib`` does not exist and ``tomli`` is not a declared dependency.
    Both call sites resolve a single flat ``version`` key under a named table,
    which a section scan answers unambiguously.
    """
    text = path.read_text(encoding="utf-8")
    body: list[str] = []
    in_section = False
    for line in text.splitlines():
        header = SECTION_HEADER.match(line)
        if header is not None:
            in_section = header.group("name").strip() == section
            continue
        if in_section:
            body.append(line)
    match = VERSION_ASSIGNMENT.search("\n".join(body))
    assert match is not None, f"{path.name}: no version declared under [{section}]"
    return match.group("version")


def _pyproject_version() -> str:
    return _section_version(PYPROJECT, "project")


def _cargo_version() -> str:
    return _section_version(CARGO, "workspace.package")


def _stub_version() -> str:
    match = STUB_VERSION_LITERAL.search(PYTHON_STUB.read_text(encoding="utf-8"))
    assert match is not None, f"{PYTHON_STUB} must declare __version__"
    return match.group("version")


def _release_please_files() -> list[Path]:
    """Files release-please rewrites on every release (the managed set)."""
    config = json.loads(RELEASE_PLEASE_CONFIG.read_text(encoding="utf-8"))
    return sorted(
        ROOT / entry["path"] for entry in config["packages"]["."]["extra-files"] if (ROOT / entry["path"]).is_file()
    )


def _extra_file_entry(path: str) -> dict:
    config = json.loads(RELEASE_PLEASE_CONFIG.read_text(encoding="utf-8"))
    return next(entry for entry in config["packages"]["."]["extra-files"] if entry["path"] == path)


def test_pyproject_and_cargo_version_agree():
    """The two artifacts shipped from one release must quote one version."""
    assert _pyproject_version() == _cargo_version()


def test_release_manifest_agrees_with_pyproject():
    """``.release-please-manifest.json`` is the base of the version chain."""
    manifest = json.loads(RELEASE_MANIFEST.read_text(encoding="utf-8"))
    assert manifest["."] == _pyproject_version()


def test_python_stub_agrees_with_pyproject():
    """``_core.pyi`` declares what type-checkers and readers see as the version."""
    assert _stub_version() == _pyproject_version()


def test_python_stub_carries_the_release_please_marker():
    text = PYTHON_STUB.read_text(encoding="utf-8")
    assert MARKER in text, f"release-please must bump {PYTHON_STUB} alongside pyproject.toml"


def _extra_files_entries() -> list[dict]:
    config = json.loads(RELEASE_PLEASE_CONFIG.read_text(encoding="utf-8"))
    return config["packages"]["."]["extra-files"]


def _extra_file_entry(path: str) -> dict:
    return next(entry for entry in _extra_files_entries() if entry["path"] == path)


def test_release_please_manages_pyproject_cargo_and_the_stub():
    paths = {entry["path"].replace("\\", "/") for entry in _extra_files_entries()}

    assert "pyproject.toml" in paths
    assert "python/dcc_mcp_core/_core.pyi" in paths
    assert _extra_file_entry("pyproject.toml")["jsonpath"] == "$.project.version"
    assert _extra_file_entry("Cargo.toml")["jsonpath"] == "$.workspace.package.version"


def test_extension_version_is_compiled_from_the_cargo_manifest():
    """``_core.__version__`` must read Cargo's version, not a copied literal.

    A hand-maintained constant here would silently decouple the executing
    artifact from ``Cargo.toml``, which is the other half of the drift this
    module guards.
    """
    text = RUST_LIB.read_text(encoding="utf-8")
    assert 'm.add("__version__", env!("CARGO_PKG_VERSION"))' in text


@pytest.mark.parametrize("path", _release_please_files(), ids=lambda p: str(p.relative_to(ROOT)).replace("\\", "/"))
def test_every_released_version_claim_matches(path: Path):
    """Each marker line in a release-please-managed file quotes the release."""
    relative = str(path.relative_to(ROOT)).replace("\\", "/")
    expected = _pyproject_version()
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if MARKER not in line:
            continue
        match = VERSION_TOKEN.search(line)
        assert match is not None, f"{relative}:{number} carries the marker but no version"
        assert match.group(0) == expected, f"{relative}:{number} claims {match.group(0)}, expected {expected}"


def test_startup_log_version_comes_from_the_single_source():
    """The startup log line must not print a hand-copied version constant.

    ``server_base`` used to interpolate ``_PKG_VERSION`` (a ``getattr`` on the
    loaded extension) straight into its log line with no cross-check, which is
    how a stale extension advertised ``0.19.2`` next to ``0.19.3`` metadata.
    """
    from dcc_mcp_core import server_base

    source = Path(server_base.__file__).read_text(encoding="utf-8")
    assert "resolve_startup_version(" in source
    assert "version_check_enabled(" in source
