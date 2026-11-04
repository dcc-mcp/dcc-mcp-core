"""Exercise release projection validation without importing the Core runtime.

Temporary repositories include real production manifests, marker sources, seeds,
independent versions, and resolved dependencies. Git commits below are fixture
objects only; verification must derive its expected bytes from immutable blobs.
"""

from __future__ import annotations

# Import built-in modules
import copy
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys

# Import third-party modules
import pytest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/ci/core_version_contract.py"
CURRENT_VERSION = json.loads((ROOT / ".release-please-manifest.json").read_text(encoding="utf-8"))["."]
_RELEASE_PARTS = CURRENT_VERSION.split(".")
NEXT_VERSION = ".".join([*_RELEASE_PARTS[:2], str(int(_RELEASE_PARTS[2].split("-")[0].split("+")[0]) + 1)])
TOML_PROJECTIONS = {
    "Cargo.toml": "workspace.package",
    "pyproject.toml": "project",
    "pkg/dcc-mcp-server-bin/pyproject.toml": "project",
    "pkg/dcc-mcp-core-semantic/pyproject.toml": "project",
    "pkg/dcc-mcp-cli-bin/pyproject.toml": "project",
}
JSON_PROJECTIONS = {
    "admin-ui/package.json": (("version",),),
    "admin-ui/package-lock.json": (("version",), ("packages", "", "version")),
    "server.json": (("version",), ("packages", 0, "version")),
}
GENERIC_PROJECTIONS = {
    "pkg/dcc-mcp-server-bin/python/dcc_mcp_server/__init__.py": 1,
    "pkg/dcc-mcp-cli-bin/python/dcc_mcp_cli/__init__.py": 1,
    "python/dcc_mcp_core/_core.pyi": 1,
    ".agents/skills/dcc-mcp-core/SKILL.md": 1,
    "docs/.vitepress/config.mts": 2,
    "docs/guide/what-is-dcc-mcp-core.md": 1,
    "docs/zh/guide/what-is-dcc-mcp-core.md": 1,
    "llms.txt": 1,
    "skills/marketplace-create-extension/SKILL.md": 1,
    "skills/marketplace-publish-extension/SKILL.md": 1,
}
SEEDS = "benchmarks/skills/seeds.json"
MANIFEST = ".release-please-manifest.json"
CONFIG = "release-please-config.json"
OUTPUTS = set(TOML_PROJECTIONS) | set(JSON_PROJECTIONS) | set(GENERIC_PROJECTIONS) | {SEEDS}
INDEPENDENT_FILES = {
    ".gitattributes",
    "crates/dcc-mcp-attestation/tests/fixtures/core-release.json",
    "uv.lock",
    "skills/asset-source/SKILL.md",
    "skills/mesh-ops/SKILL.md",
    "python/dcc_mcp_core/skills/workflow/SKILL.md",
    "crates/dcc-mcp-models/src/skill_metadata/mod.rs",
    "crates/dcc-mcp-models/src/skill_metadata/serde_impl.rs",
    "crates/dcc-mcp-naming/src/lib.rs",
    "compatibility/core-bounds.json",
    "compatibility/python.json",
    "compatibility/schema-pins.json",
    "src/bin/stub_gen.rs",
}

WORKSPACE_MANIFESTS = {path.relative_to(ROOT).as_posix() for path in (ROOT / "crates").glob("*/Cargo.toml")}


def _git(root, *args):
    return subprocess.check_output(["git", *args], cwd=str(root), stderr=subprocess.PIPE, timeout=30).decode().strip()


def _read(root, path):
    return (root / path).read_text(encoding="utf-8")


def _write(root, path, text):
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")


def _edit_json(root, path, edit):
    data = json.loads(_read(root, path))
    edit(data)
    _write(root, path, json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def _tracked(root):
    return _git(root, "ls-files", "-z").strip("\0").split("\0")


def _snapshot(root):
    return {
        path.relative_to(root).as_posix(): path.read_bytes()
        for path in root.rglob("*")
        if ".git" not in path.relative_to(root).parts and path.is_file()
    }


def _bump(root):
    _edit_json(root, MANIFEST, lambda data: data.update({".": NEXT_VERSION}))


def _commit(root, message):
    _git(root, "add", "--all")
    _git(root, "-c", "commit.gpgsign=false", "commit", "-qm", message)
    return _git(root, "rev-parse", "HEAD")


def _slot(data, keys):
    for key in keys[:-1]:
        data = data[key]
    return data, keys[-1]


def _seed(data, path="skills/marketplace-create-extension"):
    return next(skill for skill in data["skills"] if skill["skill_path"] == path)


def _toml_match(text, section):
    body = re.search(r"(?m)^\[" + re.escape(section) + r"\][^\n]*\n([^[]*)", text)
    assert body is not None
    version = re.search(r"(?m)^version\s*=\s*[\"']([^\"']+)[\"']", body.group(1))
    assert version is not None
    start = body.start(1) + version.start(1)
    end = body.start(1) + version.end(1)
    return text[start:end], text[:start] + "<release>" + text[end:]


@pytest.fixture(scope="module")
def contract():
    spec = importlib.util.spec_from_file_location("core_version_contract_under_test", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture
def repository(tmp_path):
    root = tmp_path / "repository"
    root.mkdir()
    for path in (
        OUTPUTS | INDEPENDENT_FILES | WORKSPACE_MANIFESTS | {MANIFEST, CONFIG, "scripts/ci/core_version_contract.py"}
    ):
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(str(ROOT / path), str(root / path))
    _git(root, "init", "-q")
    _git(root, "config", "user.name", "Release Contract Fixture")
    _git(root, "config", "user.email", "release-contract@example.invalid")
    _git(root, "config", "core.hooksPath", str(root / ".git" / "disabled-hooks"))
    _git(root, "config", "core.autocrlf", "false")
    _git(root, "add", "--all")
    return root


def test_explicit_projection_inventory_is_complete(contract):
    assert contract.OUTPUTS == OUTPUTS
    assert contract.TOML_FIELDS == TOML_PROJECTIONS
    assert contract.JSON_FIELDS == JSON_PROJECTIONS
    assert contract.GENERIC_FILES == GENERIC_PROJECTIONS


def test_next_bump_changes_only_typed_generic_and_seed_versions(contract, repository):
    _bump(repository)
    before = _snapshot(repository)
    rendered = contract.render(lambda path: _read(repository, path), _tracked(repository))
    assert set(rendered) == OUTPUTS
    assert _snapshot(repository) == before, "render must never mutate input files"
    assert set(contract.run(repository, "sync")) == OUTPUTS
    after = _snapshot(repository)
    assert {path for path in before if before[path] != after[path]} == OUTPUTS

    for path, section in TOML_PROJECTIONS.items():
        old_version, old_rest = _toml_match(before[path].decode(), section)
        version, rest = _toml_match(after[path].decode(), section)
        assert version == NEXT_VERSION
        assert old_version != version
        assert rest == old_rest
    for path, fields in JSON_PROJECTIONS.items():
        expected = json.loads(before[path])
        for field in fields:
            cursor, key = _slot(expected, field)
            cursor[key] = NEXT_VERSION
        assert json.loads(after[path]) == expected
    for path, expected_count in GENERIC_PROJECTIONS.items():
        old_lines = before[path].decode().splitlines(keepends=True)
        new_lines = after[path].decode().splitlines(keepends=True)
        assert len(old_lines) == len(new_lines)
        count = 0
        for old_line, new_line in zip(old_lines, new_lines):
            if contract.MARKER not in old_line:
                assert new_line == old_line
                continue
            count += 1
            prefix = old_line.split(contract.MARKER, 1)[0]
            version = re.search(r"\d+\.\d+\.\d+", prefix)
            assert version is not None
            assert new_line == old_line[: version.start()] + NEXT_VERSION + old_line[version.end() :]
        assert count == expected_count

    expected_seeds = json.loads(before[SEEDS])
    for path in ("skills/marketplace-create-extension", "skills/marketplace-publish-extension"):
        skill = _seed(expected_seeds, path)
        skill["version"] = NEXT_VERSION
        skill["metadata"]["dcc-mcp.version"] = NEXT_VERSION
    assert json.loads(after[SEEDS]) == expected_seeds
    for path in INDEPENDENT_FILES | WORKSPACE_MANIFESTS | {MANIFEST, CONFIG}:
        assert after[path] == before[path], path


def test_sync_is_idempotent_and_check_is_read_only(contract, repository):
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match="projection drift"):
        contract.run(repository, "check")
    assert _snapshot(repository) == before
    contract.run(repository, "sync")
    synced = _snapshot(repository)
    assert contract.run(repository, "sync") == []
    assert contract.run(repository, "check") == []
    assert contract.render(lambda path: _read(repository, path), _tracked(repository)) == {
        path: contents.decode() for path, contents in synced.items() if path in OUTPUTS
    }
    assert _snapshot(repository) == synced


@pytest.mark.parametrize("path", sorted(OUTPUTS | {MANIFEST, CONFIG}))
def test_sync_missing_file_fails_before_writing_any_projection(contract, repository, path):
    _bump(repository)
    (repository / path).unlink()
    before = _snapshot(repository)
    with pytest.raises((contract.ContractError, OSError)):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("path", sorted(TOML_PROJECTIONS))
def test_sync_missing_toml_version_is_atomic(contract, repository, path):
    _bump(repository)
    text = _read(repository, path)
    section = TOML_PROJECTIONS[path]
    text = text.replace("[" + section + "]", "[missing_version_table]", 1)
    _write(repository, path, text)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match="expected one"):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize(
    ("path", "field"),
    [(path, field) for path, fields in JSON_PROJECTIONS.items() for field in fields],
)
def test_sync_missing_json_version_is_atomic(contract, repository, path, field):
    _bump(repository)

    def remove(data):
        cursor, key = _slot(data, field)
        del cursor[key]

    _edit_json(repository, path, remove)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match="missing JSON path"):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("path", sorted(GENERIC_PROJECTIONS))
def test_removed_marker_fails_atomically(contract, repository, path):
    _bump(repository)
    _write(repository, path, _read(repository, path).replace(contract.MARKER, "removed-marker", 1))
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match=r"release markers|nested skill version marker|__version__"):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("prefix", ["no version", "0.20.43 and 1.2.3"])
def test_marker_must_label_exactly_one_version(contract, repository, prefix):
    path = "llms.txt"
    text = _read(repository, path)
    lines = text.splitlines(keepends=True)
    lines = [prefix + " # " + contract.MARKER + "\n" if contract.MARKER in line else line for line in lines]
    _write(repository, path, "".join(lines))
    with pytest.raises(contract.ContractError, match="exactly one version"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("mutation", ["path", "type", "jsonpath", "duplicate", "missing", "package"])
def test_registration_drift_fails_closed(contract, repository, mutation):
    def mutate(data):
        package = data["packages"]["."]
        entries = package["extra-files"]
        if mutation in ("path", "type", "jsonpath"):
            entries[0][mutation] = {"path": "unknown.toml", "type": "generic", "jsonpath": "$.version"}[mutation]
        elif mutation == "duplicate":
            entries.append(copy.deepcopy(entries[0]))
        elif mutation == "missing":
            entries.pop()
        else:
            package["package-name"] = "unrelated-package"

    _edit_json(repository, CONFIG, mutate)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match=r"registration|root package"):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("path", ["new_version.py", ".hidden/release.txt", "docs/.private/version.md"])
@pytest.mark.parametrize("comment", ["#", "//", "<!--"])
def test_new_tracked_markers_are_found_outside_declared_inventory(contract, repository, path, comment):
    _write(repository, path, 'version = "0.20.43" ' + comment + " " + contract.MARKER + "\n")
    _git(repository, "add", "--", path)
    assert path not in OUTPUTS
    with pytest.raises(contract.ContractError, match="unregistered release marker"):
        contract.run(repository, "check")


def test_tracked_binary_content_does_not_hide_later_unknown_marker(contract, repository):
    (repository / "a_binary.bin").write_bytes(b"\x80\xff\x00")
    _write(repository, "z_hidden.py", 'version = "0.20.43" # ' + contract.MARKER + "\n")
    _git(repository, "add", "--all")
    with pytest.raises(contract.ContractError, match=r"z_hidden\.py: unregistered release marker"):
        contract.run(repository, "check")


def test_new_package_manifest_requires_explicit_classification(contract, repository):
    path = "pkg/new-package/pyproject.toml"
    _write(repository, path, '[project]\nname = "new-package"\nversion = "1.0.0"\n')
    _git(repository, "add", "--", path)
    with pytest.raises(contract.ContractError, match="unclassified package version"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("path", [MANIFEST, CONFIG, *JSON_PROJECTIONS, SEEDS])
def test_duplicate_json_keys_are_rejected_at_every_contract_input(contract, repository, path):
    text = _read(repository, path)
    _write(repository, path, text.replace("{", '{"duplicate": 1, "duplicate": 2,', 1))
    with pytest.raises(contract.ContractError, match="duplicate JSON key"):
        contract.run(repository, "sync")


def test_nested_duplicate_json_keys_are_rejected(contract, repository):
    path = "server.json"
    text = _read(repository, path).replace('"identifier":', '"identifier": "other", "identifier":', 1)
    _write(repository, path, text)
    with pytest.raises(contract.ContractError, match="duplicate JSON key"):
        contract.run(repository, "sync")


@pytest.mark.parametrize(
    "mutation", ["schema", "roots", "duplicate-roots", "skills", "missing", "duplicate", "name", "version", "metadata"]
)
def test_seed_schema_and_projected_skill_identity_are_strict(contract, repository, mutation):
    def mutate(data):
        if mutation == "schema":
            data["schema"] = "skills-corpus-v999"
        elif mutation == "roots":
            data["roots"] = data["roots"][:-1]
        elif mutation == "duplicate-roots":
            data["roots"].append(data["roots"][0])
        elif mutation == "skills":
            data["skills"] = {}
        elif mutation == "missing":
            data["skills"].remove(_seed(data))
        elif mutation == "duplicate":
            data["skills"].append(copy.deepcopy(_seed(data)))
        elif mutation == "name":
            _seed(data)["name"] = "wrong-skill"
        elif mutation == "version":
            del _seed(data)["version"]
        else:
            del _seed(data)["metadata"]["dcc-mcp.version"]

    _bump(repository)
    _edit_json(repository, SEEDS, mutate)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match=r"seed|JSON path"):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("path", ["server.json", "admin-ui/package.json", "admin-ui/package-lock.json"])
def test_json_package_identity_cannot_be_repurposed(contract, repository, path):
    _edit_json(repository, path, lambda data: data.update(name="unrelated-package"))
    with pytest.raises(contract.ContractError, match="identity"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("mutation", ["extra", "identifier"])
def test_server_package_set_is_explicit(contract, repository, mutation):
    def mutate(data):
        if mutation == "extra":
            data["packages"].append(copy.deepcopy(data["packages"][0]))
        else:
            data["packages"][0]["identifier"] = "unrelated-package"

    _edit_json(repository, "server.json", mutate)
    with pytest.raises(contract.ContractError, match="package set"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("version", [None, 2044, "0.20", "release-0.20.44", "0.20.44\n", "0.20.44.1"])
def test_manifest_requires_an_exact_three_part_version(contract, repository, version):
    _edit_json(repository, MANIFEST, lambda data: data.update({".": version}))
    with pytest.raises(contract.ContractError, match="three-part release version"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("path", [MANIFEST, "Cargo.toml", SEEDS, "llms.txt"])
def test_symlink_files_are_rejected_before_writing(contract, repository, tmp_path, path):
    target = tmp_path / "outside.txt"
    target.write_bytes((repository / path).read_bytes())
    original = target.read_bytes()
    (repository / path).unlink()
    try:
        (repository / path).symlink_to(target)
    except OSError as error:
        pytest.skip("Fixture filesystem cannot create symlinks: " + str(error))
    with pytest.raises(contract.ContractError, match="symlink"):
        contract.run(repository, "sync")
    assert target.read_bytes() == original


def test_symlink_parent_directories_are_rejected(contract, repository, tmp_path):
    original = repository / "admin-ui"
    outside = tmp_path / "outside-admin"
    original.rename(outside)
    try:
        original.symlink_to(outside, target_is_directory=True)
    except OSError as error:
        pytest.skip("Fixture filesystem cannot create symlinks: " + str(error))
    before = {path.name: path.read_bytes() for path in outside.iterdir()}
    with pytest.raises(contract.ContractError, match="symlink"):
        contract.run(repository, "sync")
    assert {path.name: path.read_bytes() for path in outside.iterdir()} == before


@pytest.fixture
def release_head(repository):
    _bump(repository)
    return repository, _commit(repository, "Fixture release manifest bump")


def test_verify_diff_accepts_exact_projection_from_original_head(contract, release_head):
    root, _parent = release_head
    contract.run(root, "sync")
    before = _snapshot(root)
    assert contract.run(root, "verify-diff") == []
    assert _snapshot(root) == before


@pytest.mark.parametrize("path", ["Cargo.toml", "llms.txt", "admin-ui/package.json", SEEDS])
def test_verify_diff_rejects_nonversion_mutation_inside_allowed_paths(contract, release_head, path):
    root, _parent = release_head
    contract.run(root, "sync")
    if path in ("Cargo.toml", "llms.txt"):
        _write(root, path, _read(root, path) + "\n# unauthorized nonversion mutation\n")
    elif path == SEEDS:
        _edit_json(root, path, lambda data: _seed(data).update(description="Unauthorized description"))
    else:
        _edit_json(root, path, lambda data: data.update(description="Unauthorized description"))
    # A second sync makes the live checkout self-consistent, but verification
    # must still derive all nonversion bytes from the original Git objects.
    contract.run(root, "sync")
    assert contract.run(root, "check") == []
    with pytest.raises(contract.ContractError, match="projection drift"):
        contract.run(root, "verify-diff")


def test_verify_diff_ignores_index_as_a_source_of_expected_content(contract, release_head):
    root, _parent = release_head
    contract.run(root, "sync")
    _write(root, "llms.txt", _read(root, "llms.txt") + "\nUnapproved index content.\n")
    _git(root, "add", "--all")
    with pytest.raises(contract.ContractError, match="projection drift"):
        contract.run(root, "verify-diff")


def test_verify_commit_reads_original_parent_and_committed_head_not_worktree(contract, release_head):
    root, parent = release_head
    contract.run(root, "sync")
    _commit(root, "Fixture generated version projections")
    assert contract.run(root, "verify-commit", parent) == []
    _write(root, "llms.txt", "Working tree must not define committed evidence.\n")
    _edit_json(root, MANIFEST, lambda data: data.update({".": "9.9.9"}))
    _git(root, "add", "--all")
    assert contract.run(root, "verify-commit", parent) == []


@pytest.mark.parametrize("path", ["pyproject.toml", "llms.txt", "server.json", SEEDS])
def test_verify_commit_rejects_nonversion_changes_in_allowed_paths(contract, release_head, path):
    root, parent = release_head
    contract.run(root, "sync")
    if path in ("pyproject.toml", "llms.txt"):
        _write(root, path, _read(root, path) + "\n# unexpected payload\n")
    elif path == SEEDS:
        _edit_json(root, path, lambda data: _seed(data).update(description="Unexpected committed payload"))
    else:
        _edit_json(root, path, lambda data: data.update(description="Unexpected committed payload"))
    contract.run(root, "sync")
    _commit(root, "Fixture malicious projection commit")
    assert contract.run(root, "check") == []
    with pytest.raises(contract.ContractError, match="projection drift"):
        contract.run(root, "verify-commit", parent)


@pytest.mark.parametrize("expected_parent", [None, "not-the-parent", "0" * 40])
def test_verify_commit_requires_exact_original_parent(contract, release_head, expected_parent):
    root, _parent = release_head
    contract.run(root, "sync")
    _commit(root, "Fixture projections")
    with pytest.raises(contract.ContractError, match="exact original head"):
        contract.run(root, "verify-commit", expected_parent)


def test_verify_commit_rejects_a_root_commit(contract, release_head):
    root, parent = release_head
    with pytest.raises(contract.ContractError, match="exact original head"):
        contract.run(root, "verify-commit", parent)


def test_dependency_free_cli_reports_validation_errors_without_tracebacks(repository):
    _edit_json(repository, MANIFEST, lambda data: data.update({".": "broken"}))
    result = subprocess.run(
        [sys.executable, "-I", str(SCRIPT), "check", "--root", str(repository)],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert "Core version contract:" in result.stderr
    assert "three-part release version" in result.stderr
    assert "Traceback" not in result.stderr


@pytest.mark.parametrize("path", ["Cargo.toml", "crates/dcc-mcp-models/Cargo.toml"])
@pytest.mark.parametrize("mutation", ["version", "inheritance", "missing", "duplicate"])
def test_workspace_member_versions_must_inherit_the_release(contract, repository, mutation, path):
    text = _read(repository, path)
    assert "version.workspace = true" in text
    replacement = {
        "version": 'version = "0.20.43"',
        "inheritance": "version.workspace = false",
        "missing": "# missing version inheritance",
        "duplicate": 'version.workspace = true\nversion = "0.20.43"',
    }[mutation]
    _write(repository, path, text.replace("version.workspace = true", replacement, 1))
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match=r"must inherit version\.workspace"):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


def test_workspace_hack_keeps_its_independent_version(contract, repository):
    path = "crates/workspace-hack/Cargo.toml"
    text = _read(repository, path)
    _write(repository, path, re.sub(r'(?m)^version = "[^"]+"', 'version = "8.7.6"', text, count=1))
    before = _read(repository, path)
    assert 'version = "8.7.6"' in before
    _bump(repository)
    contract.run(repository, "sync")
    assert _read(repository, path) == before


@pytest.mark.parametrize("location", ["top-level", "wrong-metadata", "compatibility", "body"])
def test_skill_marker_cannot_move_to_an_independent_version(contract, repository, location):
    path = "skills/marketplace-create-extension/SKILL.md"
    text = _read(repository, path)
    marked_line = next(line for line in text.splitlines() if contract.MARKER in line)
    text = text.replace(marked_line, marked_line.replace("# " + contract.MARKER, ""), 1)
    marker = " # " + contract.MARKER
    if location == "top-level":
        text = text.replace("\nname:", '\nversion: "1.0.0"' + marker + "\nname:", 1)
    elif location == "wrong-metadata":
        text = text.replace("  openclaw:\n", '  openclaw:\n    version: "1.0.0"' + marker + "\n", 1)
    elif location == "compatibility":
        text = re.sub(r"(?m)^compatibility:.*$", 'compatibility: "Core 0.19.0"' + marker, text, count=1)
    else:
        text += '\nExample version: "1.0.0"' + marker + "\n"
    _write(repository, path, text)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError, match=r"metadata\.dcc-mcp\.version|nested skill version marker"):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("mutation", ["missing", "duplicate-field", "duplicate-table"])
def test_toml_version_field_and_table_are_unambiguous(contract, repository, mutation):
    path = "pyproject.toml"
    text = _read(repository, path)
    if mutation == "missing":
        text = re.sub(r"(?m)^version =", "removed_version =", text, count=1)
    elif mutation == "duplicate-field":
        text = text.replace("[project]", '[project]\nversion = "1.2.3"', 1)
    else:
        text += '\n[project]\nversion = "1.2.3"\n'
    _write(repository, path, text)
    with pytest.raises(contract.ContractError, match="expected one"):
        contract.run(repository, "sync")


def test_extra_marker_in_known_file_is_rejected(contract, repository):
    path = "llms.txt"
    _write(repository, path, _read(repository, path) + '\nversion = "1.2.3" # ' + contract.MARKER + "\n")
    with pytest.raises(contract.ContractError, match="expected 1 release markers, found 2"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("path", [MANIFEST, CONFIG, *JSON_PROJECTIONS, SEEDS])
def test_json_contract_inputs_must_have_object_roots(contract, repository, path):
    _write(repository, path, "[]\n")
    with pytest.raises(contract.ContractError, match="expected a JSON object"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("field", ["path", "type", "jsonpath"])
def test_registration_fields_require_scalar_strings(contract, repository, field):
    def mutate(data):
        data["packages"]["."]["extra-files"][0][field] = ["unexpected-list"]

    _edit_json(repository, CONFIG, mutate)
    with pytest.raises(contract.ContractError, match="registration values must be strings"):
        contract.run(repository, "sync")


def test_seed_entries_must_be_objects(contract, repository):
    _edit_json(repository, SEEDS, lambda data: data["skills"].append(None))
    with pytest.raises(contract.ContractError, match="unexpected seed schema"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("mutation", ["registry", "non-object", "non-array"])
def test_server_packages_have_strict_types_and_registry(contract, repository, mutation):
    def mutate(data):
        if mutation == "registry":
            data["packages"][0]["registryType"] = "npm"
        elif mutation == "non-object":
            data["packages"] = [None]
        else:
            data["packages"] = {"identifier": "dcc-mcp-server"}

    _edit_json(repository, "server.json", mutate)
    with pytest.raises(contract.ContractError, match="package set"):
        contract.run(repository, "sync")


def test_lock_root_package_identity_is_independently_checked(contract, repository):
    def mutate(data):
        data["packages"][""]["name"] = "unrelated-package"

    _edit_json(repository, "admin-ui/package-lock.json", mutate)
    with pytest.raises(contract.ContractError, match="lock root identity"):
        contract.run(repository, "sync")


def test_dependency_free_cli_sync_then_check_succeeds(repository):
    _bump(repository)
    for command in ("sync", "check"):
        result = subprocess.run(
            [sys.executable, "-I", str(SCRIPT), command, "--root", str(repository)],
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stderr
        assert "Core version contract: " + command in result.stdout


def _workflow_core_invocations():
    text = _read(ROOT, ".github/workflows/release-please-lock-sync.yml")
    invocations = []
    for line in text.replace("\\\n", "").splitlines():
        if "core_version_contract.py" not in line or " | " not in line:
            continue
        pipeline = shlex.split(line.strip())
        assert pipeline.count("|") == 1
        separator = pipeline.index("|")
        source, argv = pipeline[:separator], pipeline[separator + 1 :]
        scrubbed = False
        if source[:3] == ["env", "-u", "PUSH_TOKEN"]:
            source = source[3:]
            assert argv[:3] == ["env", "-u", "PUSH_TOKEN"]
            argv = argv[3:]
            scrubbed = True
        assert source == [
            "git",
            "-C",
            "../trusted-lock-validator",
            "show",
            "$TRUSTED_VALIDATOR_REF:scripts/ci/core_version_contract.py",
        ]
        assert argv[:3] == ["python", "-I", "-"]
        invocations.append((argv[3], argv, scrubbed))
    assert [command for command, _, _ in invocations] == ["sync", "verify-diff", "verify-commit"]
    return invocations


def test_privileged_release_allowlist_matches_semantic_projection_outputs(contract):
    spec = importlib.util.spec_from_file_location(
        "generated_lock_sync_core_contract", ROOT / "scripts/ci/generated_lock_sync.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    # The outer validator enforces the file boundary; the Core validator
    # additionally rejects nonversion changes inside every allowed file.
    assert module.RELEASE_OUTPUTS == contract.OUTPUTS == OUTPUTS
    assert module.RELEASE_OUTPUTS.isdisjoint({MANIFEST, CONFIG, "scripts/ci/core_version_contract.py"})


def test_privileged_core_commands_use_one_pinned_object_before_write_credentials():
    text = _read(ROOT, ".github/workflows/release-please-lock-sync.yml")
    invocations = _workflow_core_invocations()
    pin = re.search(r"(?m)^      TRUSTED_VALIDATOR_REF: ([0-9a-f]{40})$", text)
    assert pin is not None
    assert "          ref: " + pin.group(1) in text
    assert 'cat-file -e "$TRUSTED_VALIDATOR_REF:scripts/ci/core_version_contract.py"' in text
    assert "  pull_request_target:" in text
    assert [command for command, _, _ in invocations] == ["sync", "verify-diff", "verify-commit"]
    assert invocations[-1][2], "Pre-push verification must scrub PUSH_TOKEN on both pipeline processes"
    assert invocations[-1][1][-2:] == ["--expected-parent", "$EXPECTED_HEAD_SHA"]
    credential_start = text.index('credential_file="$(mktemp)"')
    for line in text.splitlines():
        if "core_version_contract.py" in line and " | " in line:
            assert text.index(line) < credential_start
    push_step = text[text.index("      - name: Push fixed generated lock commit") :]
    assert push_step.index("secrets.PERSONAL_ACCESS_TOKEN") > push_step.index('credential_file="$(mktemp)"')
    assert "persist-credentials: true" not in text


def test_version_consistency_is_triggered_for_new_paths_on_every_pull_request():
    text = _read(ROOT, ".github/workflows/version-consistency.yml")
    trigger = re.search(r"(?ms)^on:\n(.*?)(?=^permissions:)", text)
    assert trigger is not None
    assert "  pull_request:" in trigger.group(1)
    assert re.search(r"(?m)^\s+paths(?:-ignore)?:", trigger.group(1)) is None
    assert "python -I scripts/ci/core_version_contract.py check" in text


@pytest.mark.parametrize("invocation", _workflow_core_invocations(), ids=lambda invocation: invocation[0])
@pytest.mark.parametrize("vector", ["cwd", "pythonpath"])
def test_all_privileged_core_commands_reject_candidate_import_shadow(
    contract, release_head, tmp_path, invocation, vector
):
    candidate, parent = release_head
    command, argv, _scrubbed = invocation
    if command != "sync":
        contract.run(candidate, "sync")
    if command == "verify-commit":
        _commit(candidate, "Fixture isolated projection verification")
    shadow_root = candidate if vector == "cwd" else tmp_path / "pythonpath"
    shadow_root.mkdir(exist_ok=True)
    _write(shadow_root, "argparse.py", 'raise SystemExit("SYNTHETIC_CORE_IMPORT_SHADOW")\n')
    _write(candidate, "scripts/ci/core_version_contract.py", 'raise SystemExit("CANDIDATE_SCRIPT_EXECUTED")\n')
    env = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "WINDIR") if key in os.environ}
    if vector == "pythonpath":
        env["PYTHONPATH"] = str(shadow_root)
    result = subprocess.run(
        [sys.executable, *(parent if argument == "$EXPECTED_HEAD_SHA" else argument for argument in argv[1:])],
        cwd=str(candidate),
        env=env,
        input=SCRIPT.read_text(encoding="utf-8"),
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    assert "Core version contract: " + command in result.stdout
    assert result.stderr == ""


@pytest.mark.parametrize("vector", ["cwd", "pythonpath"])
def test_import_shadow_control_fails_when_isolation_is_removed(tmp_path, vector):
    candidate = tmp_path / "candidate"
    candidate.mkdir()
    shadow_root = candidate if vector == "cwd" else tmp_path / "pythonpath"
    shadow_root.mkdir(exist_ok=True)
    _write(shadow_root, "argparse.py", 'raise SystemExit("SYNTHETIC_CORE_IMPORT_SHADOW")\n')
    env = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "WINDIR") if key in os.environ}
    if vector == "pythonpath":
        env["PYTHONPATH"] = str(shadow_root)
    _command, argv, _scrubbed = _workflow_core_invocations()[0]
    result = subprocess.run(
        [sys.executable, *(argument for argument in argv[1:] if argument != "-I"), "--help"],
        cwd=str(candidate),
        env=env,
        input=SCRIPT.read_text(encoding="utf-8"),
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 1
    assert result.stderr.strip() == "SYNTHETIC_CORE_IMPORT_SHADOW"


@pytest.mark.parametrize("path", ["pyproject.toml", "pkg/dcc-mcp-server-bin/pyproject.toml"])
@pytest.mark.parametrize("delimiter", ['"""', "'''"], ids=["basic-string", "literal-string"])
def test_toml_multiline_string_cannot_impersonate_a_version_field(contract, repository, path, delimiter):
    text = (
        '[project]\nname = "dcc-mcp-core"\n"version" = "'
        + CURRENT_VERSION
        + '"\ndescription = '
        + delimiter
        + '\nversion = "'
        + CURRENT_VERSION
        + '"\n'
        + delimiter
        + "\n"
    )
    _write(repository, path, text)
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("key", ["metadata:", "  dcc-mcp:"])
def test_skill_block_scalar_cannot_impersonate_nested_version_metadata(contract, repository, key):
    path = "skills/marketplace-create-extension/SKILL.md"
    text = _read(repository, path)
    assert key + "\n" in text
    _write(repository, path, text.replace(key + "\n", key + " |\n", 1))
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


def test_verify_diff_rejects_staged_executable_mode_change(contract, release_head):
    root, _parent = release_head
    contract.run(root, "sync")
    _git(root, "add", "--all")
    _git(root, "update-index", "--chmod=+x", "llms.txt")
    assert _git(root, "ls-files", "--stage", "llms.txt").startswith("100755 ")
    with pytest.raises(contract.ContractError, match=r"mode|type"):
        contract.run(root, "verify-diff")


@pytest.mark.skipif(os.name == "nt", reason="Windows lacks POSIX worktree executable mode bits")
def test_verify_diff_rejects_worktree_executable_mode_change(contract, release_head):
    root, _parent = release_head
    contract.run(root, "sync")
    path = root / "llms.txt"
    path.chmod(path.stat().st_mode | 0o111)
    with pytest.raises(contract.ContractError, match=r"mode|type"):
        contract.run(root, "verify-diff")


def test_verify_commit_rejects_committed_executable_mode_change(contract, release_head):
    root, parent = release_head
    contract.run(root, "sync")
    _git(root, "add", "--all")
    _git(root, "update-index", "--chmod=+x", "llms.txt")
    _git(root, "-c", "commit.gpgsign=false", "commit", "-qm", "Fixture unexpected projection mode")
    assert _git(root, "ls-tree", "HEAD", "llms.txt").startswith("100755 ")
    with pytest.raises(contract.ContractError, match=r"mode|type"):
        contract.run(root, "verify-commit", parent)


@pytest.mark.parametrize("key", ["metadata", "dcc-mcp", "version"])
def test_skill_version_ancestors_and_field_must_be_unique(contract, repository, key):
    path = "skills/marketplace-create-extension/SKILL.md"
    text = _read(repository, path)
    if key == "metadata":
        text = text.replace("\nmetadata:\n", "\nmetadata:\n  unrelated: true\nmetadata:\n", 1)
    elif key == "dcc-mcp":
        text = text.replace("\n  dcc-mcp:\n", "\n  dcc-mcp:\n    unrelated: true\n  dcc-mcp:\n", 1)
    else:
        text = text.replace("\n  dcc-mcp:\n", '\n  dcc-mcp:\n    version: "1.0.0"\n', 1)
    _write(repository, path, text)
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize(
    "path",
    [
        "pkg/dcc-mcp-cli-bin/python/dcc_mcp_cli/__init__.py",
        "pkg/dcc-mcp-server-bin/python/dcc_mcp_server/__init__.py",
        "python/dcc_mcp_core/_core.pyi",
    ],
)
def test_python_marker_cannot_move_from_runtime_version_to_example(contract, repository, path):
    text = _read(repository, path).replace(contract.MARKER, "removed-marker")
    text += '\n_DESCRIPTION_EXAMPLE = "' + CURRENT_VERSION + '"  # ' + contract.MARKER + "\n"
    _write(repository, path, text)
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("path", sorted(TOML_PROJECTIONS))
def test_extra_marker_inside_typed_manifest_must_be_classified(contract, repository, path):
    text = _read(repository, path)
    text += '\n[tool.unexpected]\nversion = "1.0.0" # ' + contract.MARKER + "\n"
    _write(repository, path, text)
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


@pytest.mark.parametrize("packages", [None, [], {"": None}, {"": []}])
def test_lock_package_containers_fail_closed_without_type_errors(contract, repository, packages):
    _edit_json(repository, "admin-ui/package-lock.json", lambda data: data.update(packages=packages))
    with pytest.raises(contract.ContractError, match="lock root identity"):
        contract.run(repository, "sync")


@pytest.mark.parametrize("key", ["metadata", "dcc-mcp", "version"])
def test_quoted_yaml_version_path_keys_cannot_duplicate_marked_metadata(contract, repository, key):
    path = "skills/marketplace-create-extension/SKILL.md"
    text = _read(repository, path)
    if key == "metadata":
        injected = '\n"metadata":\n  "dcc-mcp":\n    version: "1.2.3"\n'
        text = text.replace("\n---\n", injected + "---\n", 1)
    elif key == "dcc-mcp":
        text = text.replace("\n  openclaw:", '\n  "dcc-mcp":\n    version: "1.2.3"\n  openclaw:', 1)
    else:
        text = text.replace("\n  dcc-mcp:\n", '\n  dcc-mcp:\n    "version": "1.2.3"\n', 1)
    _write(repository, path, text)
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


def test_clean_autocrlf_checkout_preserves_projection_and_literal_fixture_bytes(contract, repository):
    contract.run(repository, "sync")
    _commit(repository, "Fixture canonical release projections")
    literal_fixture = "crates/dcc-mcp-attestation/tests/fixtures/core-release.json"
    literal_bytes = (repository / literal_fixture).read_bytes()
    assert b"\r" in literal_bytes
    paths = sorted(OUTPUTS)
    canonical = {path: (repository / path).read_bytes() for path in paths}
    assert all(b"\r\n" not in value for value in canonical.values())
    _git(repository, "config", "core.autocrlf", "true")
    for path in [*paths, literal_fixture]:
        (repository / path).unlink()
    _git(repository, "checkout", "HEAD", "--", *paths, literal_fixture)
    assert {path: (repository / path).read_bytes() for path in paths} == canonical
    assert (repository / literal_fixture).read_bytes() == literal_bytes
    assert _git(repository, "check-attr", "text", "--", literal_fixture).endswith(": text: unset")
    assert contract.run(repository, "check") == []
    assert contract.run(repository, "sync") == []
    assert contract.run(repository, "verify-diff") == []


@pytest.mark.parametrize("sibling", ["custom_data", '"custom-data"'])
def test_skill_unknown_sibling_cannot_reuse_a_previous_metadata_ancestor(contract, repository, sibling):
    path = "skills/marketplace-create-extension/SKILL.md"
    text = _read(repository, path)
    line = next(line for line in text.splitlines() if contract.MARKER in line)
    replacement = (
        '    "version": "'
        + CURRENT_VERSION
        + '"\n    '
        + sibling
        + ':\n      version: "'
        + CURRENT_VERSION
        + '" # '
        + contract.MARKER
    )
    _write(repository, path, text.replace(line, replacement, 1))
    _bump(repository)
    before = _snapshot(repository)
    with pytest.raises(contract.ContractError):
        contract.run(repository, "sync")
    assert _snapshot(repository) == before


STAGING_LANES = (
    ".github/workflows/release-please-lock-sync.yml",
    ".github/actions/generated-lock-repair/action.yml",
)
IGNORED_PROJECTION = ".agents/skills/dcc-mcp-core/SKILL.md"


def _staging_commands(lane):
    """Execute the real staging arguments, without generation or remote writes."""
    return [shlex.split(line.strip()) for line in _read(ROOT, lane).splitlines() if line.strip().startswith("git add ")]


@pytest.mark.parametrize("lane", STAGING_LANES)
def test_projection_staging_uses_exact_tracked_output_allowlist(lane):
    locks, projections = _staging_commands(lane)
    assert locks == ["git", "add", "Cargo.lock", "uv.lock", "crates/workspace-hack/Cargo.toml"]
    assert projections[:4] == ["git", "add", "--update", "--"]
    assert set(projections[4:]) == OUTPUTS
    assert len(projections[4:]) == len(OUTPUTS)


@pytest.mark.parametrize("lane", STAGING_LANES)
@pytest.mark.parametrize("ignored_changes", [False, True], ids=["release-2774-three-files", "all-projections"])
@pytest.mark.parametrize("windows_text_io", [False, True], ids=["native-text-io", "windows-text-io"])
def test_projection_staging_handles_tracked_files_under_ignored_directories(
    contract, repository, lane, ignored_changes, windows_text_io, monkeypatch
):
    if windows_text_io:
        # Exercise Windows text-mode newline translation on every platform.
        # Projection fixtures must retain canonical bytes through this path.
        def write_windows_text(path, data, encoding=None, errors=None):
            with path.open("w", encoding=encoding, errors=errors, newline="\r\n") as stream:
                return stream.write(data)

        monkeypatch.setattr(Path, "write_text", write_windows_text)
    # Track the projections before applying the real ignore rules. Ordinary
    # `git add <tracked ignored path>` still returns 1 on this worktree shape.
    original = {path: (repository / path).read_bytes() for path in OUTPUTS}
    _git(repository, "add", "--all")
    _write(repository, ".gitignore", _read(ROOT, ".gitignore"))
    _write(repository, "Cargo.lock", "fixture lock\n")
    _bump(repository)
    contract.run(repository, "sync")
    stale = OUTPUTS if ignored_changes else {"admin-ui/package-lock.json", SEEDS, "server.json"}
    for path in stale:
        # A text round trip translates LF to CRLF on Windows, creating
        # unrelated byte drift before the staging commands can be exercised.
        (repository / path).write_bytes(original[path])
    parent = _commit(repository, "Fixture release with tracked ignored projections")
    assert IGNORED_PROJECTION in _tracked(repository)
    assert _git(repository, "check-ignore", "--no-index", IGNORED_PROJECTION) == IGNORED_PROJECTION
    assert set(contract.run(repository, "sync")) == stale
    _write(repository, "Cargo.lock", "regenerated fixture lock\n")
    ignored_sibling = ".agents/private-local.txt"
    _write(repository, ignored_sibling, "Must remain untracked.\n")
    assert contract.run(repository, "verify-diff") == []

    for command in _staging_commands(lane):
        _git(repository, *command[1:])

    assert set(_git(repository, "diff", "--cached", "--name-only").splitlines()) == stale | {"Cargo.lock"}
    assert ignored_sibling not in _tracked(repository)
    assert _git(repository, "diff", "--name-only") == ""
    _git(repository, "-c", "commit.gpgsign=false", "commit", "-qm", "Fixture staged projection repair")
    assert contract.run(repository, "verify-commit", parent) == []
    # The same staging commands stay idempotent on an already repaired tree.
    for command in _staging_commands(lane):
        _git(repository, *command[1:])
    assert _git(repository, "diff", "--cached", "--name-only") == ""


@pytest.mark.parametrize("lane", STAGING_LANES)
@pytest.mark.parametrize("untracked_projection", [False, True])
def test_projection_staging_never_admits_untracked_or_unlisted_files(repository, lane, untracked_projection):
    _commit(repository, "Fixture tracked projection boundary")
    _write(repository, ".gitignore", _read(ROOT, ".gitignore"))
    _git(repository, "add", ".gitignore")
    if untracked_projection:
        _git(repository, "rm", "--cached", "--", IGNORED_PROJECTION)
    _git(repository, "-c", "commit.gpgsign=false", "commit", "-qm", "Fixture ignored projection boundary")
    _write(repository, "untracked.txt", "Must remain untracked.\n")
    _write(repository, "compatibility/python.json", "Unrelated tracked change must not be staged.\n")
    _write(repository, "llms.txt", _read(repository, "llms.txt") + "Projection staging fixture.\n")
    command = _staging_commands(lane)[1]

    if untracked_projection:
        # A projection missing from the index is an invalid checkout. Do not
        # silently admit it with --force or continue to commit a partial repair.
        with pytest.raises(subprocess.CalledProcessError) as error:
            _git(repository, *command[1:])
        assert IGNORED_PROJECTION in error.value.stderr.decode()
        assert _git(repository, "diff", "--cached", "--name-only") == ""
        assert IGNORED_PROJECTION not in _tracked(repository)
    else:
        _git(repository, *command[1:])
        assert _git(repository, "diff", "--cached", "--name-only") == "llms.txt"
    assert "untracked.txt" not in _tracked(repository)
