"""Exercise the released-schema gate against tiny, offline Git repositories."""

from __future__ import annotations

# Import built-in modules
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess

# Import third-party modules
import pytest

GATE_PATH = Path(__file__).with_name("test_released_schema_immutability.py")


def _git(root, *args):
    return subprocess.check_output(
        ["git", "-C", str(root), *args], stderr=subprocess.PIPE, universal_newlines=True
    ).strip()


def _commit(root, tag=None):
    _git(root, "add", ".")
    _git(root, "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "Schema gate fixture")
    if tag:
        _git(root, "tag", tag)


def _schema(root, name, schema_type="string"):
    path = f"schemas/{name}-v1.schema.json"
    schema_id = f"https://example.invalid/{name}-v1.schema.json"
    target = root / path
    target.parent.mkdir(exist_ok=True)
    raw = (json.dumps({"$id": schema_id, "type": schema_type}, sort_keys=True) + "\n").encode("utf-8")
    target.write_bytes(raw)
    return schema_id, {"path": path, "released": {"v1.0.0": hashlib.sha256(raw).hexdigest()}}


def _pins(root, pins):
    target = root / "compatibility/schema-pins.json"
    target.parent.mkdir(exist_ok=True)
    target.write_text(json.dumps({"schema_version": 1, "pins": pins}, indent=2) + "\n", encoding="utf-8")


@pytest.fixture
def release_repo(tmp_path, monkeypatch):
    """Provide a real tagged release and load the production gate against it."""
    root = tmp_path / "upstream"
    root.mkdir()
    _git(root, "init", "-q")
    _git(root, "config", "user.name", "Schema fixture")
    _git(root, "config", "user.email", "schema-fixture@example.invalid")
    _git(root, "config", "core.autocrlf", "false")
    # No fixture may rely on an available remote or credentials.
    _git(root, "remote", "add", "origin", str(tmp_path / "unavailable-remote"))
    schema_id, entry = _schema(root, "alpha")
    pins = {schema_id: entry}
    _pins(root, pins)
    _commit(root, "v1.0.0")
    spec = importlib.util.spec_from_file_location("released_schema_gate_fixture", GATE_PATH)
    gate = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gate)
    monkeypatch.setattr(gate, "ROOT", root)
    monkeypatch.setattr(gate, "REGISTRY_PATH", root / "compatibility/schema-pins.json")
    return root, pins, gate


def _assert_gate_passes(gate):
    gate.test_registry_entries_are_well_formed()
    gate.test_registry_keys_match_released_schema_ids()
    gate.test_registry_covers_every_released_schema()
    gate.test_released_revisions_match_registered_pins()
    gate.test_released_schemas_are_not_rewritten_on_head()


def test_registered_release_passes_offline(release_repo):
    _, _, gate = release_repo
    _assert_gate_passes(gate)


@pytest.mark.parametrize("rewrite", [False, True])
def test_new_release_omitted_from_registry_cannot_hide_new_schema(release_repo, rewrite):
    root, _, gate = release_repo
    _schema(root, "beta")
    _commit(root, "v1.0.1")
    if rewrite:
        _schema(root, "beta", "integer")
        _commit(root)
    with pytest.raises(pytest.fail.Exception, match=r"beta-v1.schema.json shipped in v1.0.1 is not registered"):
        gate.test_registry_covers_every_released_schema()


def test_new_schema_pin_uses_actual_release_bytes(release_repo):
    root, pins, gate = release_repo
    schema_id, entry = _schema(root, "beta")
    entry["released"] = {"v1.0.1": entry["released"]["v1.0.0"]}
    _commit(root, "v1.0.1")
    pins[schema_id] = entry
    _pins(root, pins)
    _commit(root)
    _assert_gate_passes(gate)
    _schema(root, "beta", "integer")
    _commit(root)
    with pytest.raises(pytest.fail.Exception, match=r"beta-v1.schema.json changed after v1.0.1"):
        gate.test_released_schemas_are_not_rewritten_on_head()


def test_unreleased_schema_is_not_required_to_have_a_pin(release_repo):
    root, _, gate = release_repo
    _schema(root, "beta")
    _commit(root)
    _assert_gate_passes(gate)


def test_unchanged_new_release_needs_no_redundant_pin(release_repo):
    root, _, gate = release_repo
    _commit(root, "v1.0.1")
    _assert_gate_passes(gate)


def test_older_release_schema_cannot_disappear_from_coverage(release_repo):
    root, _, gate = release_repo
    _, entry = _schema(root, "beta")
    _commit(root, "v1.0.1")
    (root / entry["path"]).unlink()
    _commit(root, "v1.0.2")
    with pytest.raises(pytest.fail.Exception, match=r"beta-v1.schema.json shipped in v1.0.1 is not registered"):
        gate.test_registry_covers_every_released_schema()


def test_newer_released_bytes_must_be_represented_by_latest_pin(release_repo):
    root, _, gate = release_repo
    _schema(root, "alpha", "integer")
    _commit(root, "v1.0.1")
    with pytest.raises(pytest.fail.Exception, match="latest released bytes are not pinned"):
        gate.test_registry_covers_every_released_schema()


def test_recorded_historical_rewrite_remains_supported(release_repo):
    root, pins, gate = release_repo
    schema_id, updated = _schema(root, "alpha", "integer")
    pins[schema_id]["released"]["v1.0.10"] = updated["released"]["v1.0.0"]
    _pins(root, pins)
    _commit(root, "v1.0.10")
    _assert_gate_passes(gate)
    # Reintroducing older bytes and removing the latest individual pin cannot
    # redefine the release history, even though other registered tags remain.
    del pins[schema_id]["released"]["v1.0.10"]
    _schema(root, "alpha")
    _pins(root, pins)
    _commit(root)
    with pytest.raises(pytest.fail.Exception, match="latest released bytes are not pinned"):
        gate.test_registry_covers_every_released_schema()


def test_release_order_is_numeric_and_branch_names_do_not_shadow_tags(release_repo):
    root, _, gate = release_repo
    _commit(root, "v1.0.9")
    _commit(root, "v1.0.10")
    _schema(root, "alpha", "integer")
    _commit(root)
    _git(root, "branch", "v1.0.10")
    assert gate._released_tags() == ["v1.0.0", "v1.0.9", "v1.0.10"]
    gate.test_registry_covers_every_released_schema()


def test_prerelease_and_unrelated_tags_are_not_stable_core_releases(release_repo):
    root, _, gate = release_repo
    _schema(root, "beta")
    _commit(root, "v1.0.1-rc.1")
    _git(root, "tag", "adapter-v2.0.0")
    assert gate._released_tags() == ["v1.0.0"]
    _assert_gate_passes(gate)


def test_annotated_stable_release_tags_are_discovered(release_repo):
    root, _, gate = release_repo
    _schema(root, "beta")
    _commit(root)
    _git(root, "-c", "tag.gpgsign=false", "tag", "-a", "v1.0.1", "-m", "Published release")
    with pytest.raises(pytest.fail.Exception, match=r"beta-v1.schema.json shipped in v1.0.1"):
        gate.test_registry_covers_every_released_schema()


def test_missing_release_tag_fails_even_when_a_branch_has_that_name(release_repo):
    root, _, gate = release_repo
    _git(root, "tag", "-d", "v1.0.0")
    _git(root, "branch", "v1.0.0")
    with pytest.raises(pytest.fail.Exception, match="released schema pin tags are missing"):
        gate.test_registry_covers_every_released_schema()


def test_no_release_refs_cannot_be_reported_as_complete(release_repo):
    root, _, gate = release_repo
    _git(root, "tag", "-d", "v1.0.0")
    with pytest.raises(pytest.fail.Exception, match="no stable release tags are available"):
        gate._released_tags()


def test_head_is_not_a_release_tag(release_repo):
    root, pins, gate = release_repo
    entry = next(iter(pins.values()))
    entry["released"] = {"HEAD": entry["released"]["v1.0.0"]}
    _pins(root, pins)
    _commit(root)
    with pytest.raises(pytest.fail.Exception, match="must name stable release tags"):
        gate.test_registry_covers_every_released_schema()


@pytest.mark.parametrize("include_pinned_tag", [False, True])
def test_shallow_checkout_fails_even_if_registered_tags_are_present(
    release_repo, tmp_path, monkeypatch, include_pinned_tag
):
    root, _, gate = release_repo
    if not include_pinned_tag:
        _commit(root)
    clone = tmp_path / "shallow"
    _git(tmp_path, "clone", "-q", "--depth=1", root.as_uri(), str(clone))
    assert _git(clone, "rev-parse", "--is-shallow-repository") == "true"
    assert bool(_git(clone, "tag", "--list")) == include_pinned_tag
    monkeypatch.setattr(gate, "ROOT", clone)
    monkeypatch.setattr(gate, "REGISTRY_PATH", clone / "compatibility/schema-pins.json")
    with pytest.raises(pytest.fail.Exception, match="require complete history and tags"):
        gate.test_registry_covers_every_released_schema()


def test_unreadable_release_tree_fails_closed(release_repo, monkeypatch):
    _, _, gate = release_repo
    original = gate._git_bytes

    def unreadable_tree(*args):
        return None if args[0] == "ls-tree" else original(*args)

    monkeypatch.setattr(gate, "_git_bytes", unreadable_tree)
    with pytest.raises(pytest.fail.Exception, match=r"cannot list the committed tree of v1.0.0"):
        gate.test_registry_covers_every_released_schema()


def test_changed_id_in_unregistered_release_is_rejected(release_repo):
    root, pins, gate = release_repo
    path = root / next(iter(pins.values()))["path"]
    document = json.loads(path.read_text(encoding="utf-8"))
    document["$id"] = "https://example.invalid/different-v1.schema.json"
    path.write_text(json.dumps(document) + "\n", encoding="utf-8")
    _commit(root, "v1.0.1")
    with pytest.raises(pytest.fail.Exception, match="registry key does not match released \\$id"):
        gate.test_registry_covers_every_released_schema()
