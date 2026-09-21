"""Release-anchored immutability gate for published ``-vN`` schemas.

The four live digest pins for the Install SOP contract (``compatibility/python.json``,
``python/dcc_mcp_core/deployment/install_sop.py``,
``crates/dcc-mcp-models/src/schema_validation.rs`` and the cross-assert in
``tests/test_install_catalog_provenance_contract.py``) are all *derived from the current
file*. Editing a released ``-vN`` schema and updating those four pins in the same change
keeps every one of them green -- that is how the 0.20.30 in-place rewrite reached
``main``.

This module closes the loophole by anchoring schema bytes to released git revisions
instead of to the working tree:

1. Every registered ``(schema, tag)`` pair must still hash to the digest recorded in
   ``compatibility/schema-pins.json``, so a released revision cannot change silently.
2. The bytes committed at ``HEAD`` for a registered schema must equal the digest of its
   newest registered release, so editing a released ``-vN`` schema in place fails and the
   only way forward is a new ``-v(N+1)`` revision.

Digests come from git object bytes (``git cat-file blob <rev>:<path>``) rather than from a
raw working-tree read, so ``core.autocrlf`` differences on Windows checkouts cannot produce
a false failure. The gate therefore evaluates the committed tree, which is what CI merges
and what a release publishes.

Adding an entry: see ``compatibility/README.md``.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[1]
REGISTRY_PATH = ROOT / "compatibility" / "schema-pins.json"

_HEX_DIGITS = frozenset("0123456789abcdef")


def _git(*args):
    """Run a git command against the repository root and return the completed process."""
    return subprocess.run(
        ["git", "-C", str(ROOT)] + [str(arg) for arg in args],
        capture_output=True,
    )


def _blob_bytes(revision, path):
    """Return the committed bytes of ``path`` at ``revision``, or ``None`` if unreadable."""
    return _git_bytes("cat-file", "blob", f"{revision}:{path}")


def _git_bytes(*args):
    """Run git and return its stdout bytes, or ``None`` when the command fails."""
    process = _git(*args)
    if process.returncode != 0:
        return None
    return process.stdout


def _digest(data):
    return hashlib.sha256(data).hexdigest()


def _short(digest):
    return digest[:12]


def _version_key(tag):
    """Sort releases numerically so ``v0.20.9`` orders before ``v0.20.30``."""
    return [int(chunk) if chunk.isdigit() else -1 for chunk in tag.lstrip("v").split(".")]


def _sorted_tags(released):
    return sorted(released, key=_version_key)


def _load_registry():
    """Load the pin registry, failing the test when it is missing or unusable."""
    if not REGISTRY_PATH.is_file():
        pytest.fail(f"released schema pin registry is missing: {REGISTRY_PATH}")
    try:
        registry = json.loads(REGISTRY_PATH.read_text(encoding="utf-8"))
    except ValueError as error:
        pytest.fail(f"released schema pin registry is not valid JSON: {error}")
    pins = registry.get("pins")
    if not isinstance(pins, dict) or not pins:
        pytest.fail(f"released schema pin registry declares no pins: {REGISTRY_PATH}")
    return pins


def _registered_tags(pins):
    tags = set()
    for entry in pins.values():
        tags.update(entry.get("released", {}))
    return sorted(tags, key=_version_key)


def _require_tags(tags):
    """Fail loudly when release tags are unavailable; skip only on a shallow checkout.

    A gate that silently skips is no gate at all, so this fails on a full checkout that
    has lost a registered tag. It only skips on a confirmed shallow checkout -- the
    depth-1 ``python-test`` matrix never fetches tags, so the gate is enforced by the
    dedicated ``released-schema-immutability`` job, which checks out with
    ``fetch-depth: 0`` and asserts the tests actually ran.
    """
    if _git("rev-parse", "--git-dir").returncode != 0:
        pytest.fail(f"released schema pins require a git checkout: {ROOT}")

    missing = [tag for tag in tags if _git("rev-parse", "--verify", "--quiet", f"{tag}^{{commit}}").returncode != 0]
    if not missing:
        return

    shallow = _git("rev-parse", "--is-shallow-repository")
    if shallow.returncode == 0 and shallow.stdout.strip() == b"true":
        pytest.skip(
            "released schema pins need release tags; this is a shallow checkout "
            f"(check out with fetch-depth: 0). Missing: {', '.join(missing)}"
        )
    pytest.fail(f"released schema pin tags are missing from a full checkout: {', '.join(missing)}")


def _fail(heading, failures):
    if failures:
        pytest.fail(f"{heading}\n  - " + "\n  - ".join(failures))


def test_registry_entries_are_well_formed():
    """Guard the registry itself: usable paths, well formed digests, no empty pin maps."""
    problems = []
    for schema_id, entry in sorted(_load_registry().items()):
        if not isinstance(entry, dict):
            problems.append(f"{schema_id}: entry must be an object")
            continue
        if not entry.get("path"):
            problems.append(f"{schema_id}: missing 'path'")
        released = entry.get("released")
        if not isinstance(released, dict) or not released:
            problems.append(f"{schema_id}: missing or empty 'released' map")
            continue
        for tag in sorted(released):
            digest = released[tag]
            if not isinstance(digest, str) or len(digest) != 64 or not set(digest) <= _HEX_DIGITS:
                problems.append(f"{schema_id} @ {tag}: digest must be 64 lowercase hex chars")
    _fail("released schema pin registry is malformed:", problems)


def test_registry_keys_match_released_schema_ids():
    """Every registry key must be the ``$id`` of the schema it pins."""
    pins = _load_registry()
    _require_tags(_registered_tags(pins))
    problems = []
    for schema_id, entry in sorted(pins.items()):
        path = entry["path"]
        latest = _sorted_tags(entry["released"])[-1]
        raw = _blob_bytes(latest, path)
        if raw is None:
            problems.append(f"{schema_id}: cannot read {path} at {latest}")
            continue
        try:
            document = json.loads(raw.decode("utf-8"))
        except ValueError as error:
            problems.append(f"{schema_id}: {path} at {latest} is not valid JSON: {error}")
            continue
        actual_id = document.get("$id")
        if actual_id != schema_id:
            problems.append(f"{schema_id}: registry key does not match $id {actual_id} at {latest}")
    _fail("released schema pin registry keys are out of sync with the schemas:", problems)


def test_released_revisions_match_registered_pins():
    """A released revision is immutable: ``git show <tag>:<path>`` must match the pin."""
    pins = _load_registry()
    _require_tags(_registered_tags(pins))
    problems = []
    for schema_id, entry in sorted(pins.items()):
        path = entry["path"]
        for tag in _sorted_tags(entry["released"]):
            expected = entry["released"][tag]
            raw = _blob_bytes(tag, path)
            if raw is None:
                problems.append(f"{schema_id} @ {tag}: cannot read {path} from the tag")
                continue
            actual = _digest(raw)
            if actual != expected:
                problem = f"{schema_id} @ {tag}: released bytes changed"
                problems.append(f"{problem} (pinned {_short(expected)}, tag has {_short(actual)})")
    _fail("released schema revisions were rewritten in place:", problems)


def test_released_schemas_are_not_rewritten_on_head():
    """The current bytes of a released ``-vN`` schema must equal its latest release.

    This is the check the 0.20.30 incident slipped past: the four live pins move with the
    file, this one does not. It compares committed bytes at ``HEAD``, so it is immune to
    ``core.autocrlf`` differences between Linux, macOS and Windows checkouts.
    """
    pins = _load_registry()
    _require_tags(_registered_tags(pins))
    problems = []
    for schema_id, entry in sorted(pins.items()):
        path = entry["path"]
        latest = _sorted_tags(entry["released"])[-1]
        expected = entry["released"][latest]
        raw = _blob_bytes("HEAD", path)
        if raw is None:
            problems.append(f"{schema_id}: cannot read {path} at HEAD")
            continue
        actual = _digest(raw)
        if actual != expected:
            problems.append(
                f"{schema_id}: {path} changed after {latest} "
                f"(released {_short(expected)}, now {_short(actual)}); "
                "add a new -vN revision instead of rewriting a released one"
            )
    _fail("released schemas were rewritten in place on HEAD:", problems)
