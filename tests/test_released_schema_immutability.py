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

Two further paths cover the registry itself, since an entry nobody maintains is a gate
that guards nothing:

3. Every ``*-vN.schema.json`` shipped by a stable release tag must appear in the
   registry, whose newest pin must match its latest released bytes. Release discovery
   is independent of the registry, so omitting a new tag cannot hide a released schema.
4. Every registered tag -- not only the newest -- must declare the ``$id`` its registry key
   claims, because clients resolve a released revision by ``$id`` alone.

Digests come from git object bytes (``git cat-file blob <rev>:<path>``) rather than from a
raw working-tree read, so ``core.autocrlf`` differences on Windows checkouts cannot produce
a false failure. The gate therefore evaluates the committed tree, which is what CI merges
and what a release publishes.

Adding an entry: see ``compatibility/README.md``.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

import pytest

ROOT = Path(__file__).resolve().parents[1]
REGISTRY_PATH = ROOT / "compatibility" / "schema-pins.json"

_HEX_DIGITS = frozenset("0123456789abcdef")
_RELEASE_TAG_RE = re.compile(r"v[0-9]+\.[0-9]+\.[0-9]+$")
_VERSIONED_SCHEMA_RE = re.compile(r"(?:^|.*/)[^/]+-v[0-9]+\.schema\.json$")


def _git(*args):
    """Run a git command against the repository root and return the completed process."""
    return subprocess.run(
        ["git", "-C", str(ROOT)] + [str(arg) for arg in args],
        capture_output=True,
        env=dict(os.environ, GIT_NO_LAZY_FETCH="1", GIT_TERMINAL_PROMPT="0"),
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


def _released_tags():
    """Discover stable Core releases from git refs, never from the pin registry.

    CI fetches all tags at checkout time. Tests use only local objects and fail on an
    incomplete checkout; they never fetch from a remote or query a release API. Only
    exact ``vMAJOR.MINOR.PATCH`` tags are Core stable releases, not RCs or branch names.
    """
    _require_tags([])
    listing = _git_bytes("for-each-ref", "--format=%(refname:strip=2)", "refs/tags/")
    if listing is None:
        pytest.fail("cannot enumerate released schema tags from refs/tags")
    tags = _sorted_tags(tag for tag in listing.decode("utf-8").splitlines() if _RELEASE_TAG_RE.fullmatch(tag))
    if not tags:
        pytest.fail("no stable release tags are available; check out with fetch-depth: 0 and all tags")
    _require_tags(tags)
    return tags


def _latest_released_schemas():
    """Map every released schema path to the newest stable tag that shipped it.

    Walk all released trees, so removing a file in a later release does not erase its
    contract. A new schema on HEAD is unrestricted until a release actually ships it.
    """
    latest = {}
    for tag in _released_tags():
        revision = "refs/tags/" + tag
        shipped = _versioned_schemas_at(revision)
        if shipped is None:
            pytest.fail(f"cannot list the committed tree of {tag}; fetch all release objects before testing")
        latest.update((path, tag) for path in shipped)
    return latest


def _versioned_schemas_at(revision):
    """Return every ``*-vN.schema.json`` path committed at ``revision``."""
    listing = _git_bytes("ls-tree", "-r", "--name-only", revision)
    if listing is None:
        return None
    return sorted(line for line in listing.decode("utf-8", "replace").splitlines() if _VERSIONED_SCHEMA_RE.match(line))


def _schema_id_at(revision, path):
    """Return the ``$id`` declared by the committed schema, or an error string."""
    raw = _blob_bytes(revision, path)
    if raw is None:
        return None, f"cannot read {path} from {revision}"
    try:
        document = json.loads(raw.decode("utf-8"))
    except ValueError as error:
        return None, f"{path} is not valid JSON: {error}"
    return document.get("$id"), None


def _require_tags(tags):
    """Fail closed on shallow history, missing objects, or non-release revisions.

    Even a shallow checkout containing every registered tag could omit a newer release
    that the registry forgot. Require full history and all tags at checkout time rather
    than silently skipping coverage or adding network access to the test runtime.
    """
    if _git("rev-parse", "--git-dir").returncode != 0:
        pytest.fail(f"released schema pins require a git checkout: {ROOT}")
    shallow = _git("rev-parse", "--is-shallow-repository")
    if shallow.returncode != 0 or shallow.stdout.strip() != b"false":
        pytest.fail("released schema pins require complete history and tags; check out with fetch-depth: 0")

    invalid = [tag for tag in tags if not _RELEASE_TAG_RE.fullmatch(tag)]
    if invalid:
        pytest.fail(f"schema pins must name stable release tags, not arbitrary revisions: {', '.join(invalid)}")
    missing = [
        tag for tag in tags if _git("rev-parse", "--verify", "--quiet", f"refs/tags/{tag}^{{commit}}").returncode != 0
    ]
    if missing:
        pytest.fail(
            f"released schema pin tags are missing; fetch all release tags before testing: {', '.join(missing)}"
        )


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
    """Every registry key must be the ``$id`` of the schema it pins, at every registered tag.

    Checking only the newest tag would let an older release carry a different ``$id`` for
    the same path: clients resolve a released revision by ``$id`` alone, so a mismatch on
    any registered tag is a broken contract.
    """
    pins = _load_registry()
    _require_tags(_registered_tags(pins))
    problems = []
    for schema_id, entry in sorted(pins.items()):
        path = entry["path"]
        for tag in _sorted_tags(entry["released"]):
            actual_id, error = _schema_id_at("refs/tags/" + tag, path)
            if error is not None:
                problems.append(f"{schema_id} @ {tag}: {error}")
                continue
            if actual_id != schema_id:
                problems.append(f"{schema_id} @ {tag}: registry key does not match $id {actual_id}")
    _fail("released schema pin registry keys are out of sync with the schemas:", problems)


def test_registry_covers_every_released_schema():
    """Every released schema is pinned to its latest independently discovered bytes.

    The registry does not define which releases exist. Walk stable git release refs,
    including tags absent from every pin, and retain paths from older releases too.
    Repeated unchanged releases need no duplicate pin, but the newest registered digest
    must represent the latest released content. Historical pre-gate pins remain intact.
    """
    pins = _load_registry()
    _require_tags(_registered_tags(pins))
    registered = {entry.get("path"): (schema_id, entry) for schema_id, entry in pins.items()}
    problems = []
    for path, tag in sorted(_latest_released_schemas().items()):
        if path not in registered:
            problems.append(f"{path} shipped in {tag} is not registered in {REGISTRY_PATH.name}")
            continue
        schema_id, entry = registered[path]
        actual_id, error = _schema_id_at("refs/tags/" + tag, path)
        if error is not None:
            problems.append(error)
            continue
        if actual_id != schema_id:
            problems.append(f"{path} @ {tag}: registry key does not match released $id {actual_id}")
        latest_pin = _sorted_tags(entry["released"])[-1]
        expected = entry["released"][latest_pin]
        actual = _digest(_blob_bytes("refs/tags/" + tag, path))
        if expected != actual:
            problems.append(
                f"{path} @ {tag}: latest released bytes are not pinned "
                f"(newest pin {latest_pin} has {_short(expected)}, release has {_short(actual)})"
            )
    _fail("released schemas are missing from the pin registry:", problems)


def test_released_revisions_match_registered_pins():
    """A released revision is immutable: ``git show <tag>:<path>`` must match the pin."""
    pins = _load_registry()
    _require_tags(_registered_tags(pins))
    problems = []
    for schema_id, entry in sorted(pins.items()):
        path = entry["path"]
        for tag in _sorted_tags(entry["released"]):
            expected = entry["released"][tag]
            raw = _blob_bytes("refs/tags/" + tag, path)
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
