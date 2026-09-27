#!/usr/bin/env python3
"""Fail closed when the release workflow drifts from its approved snapshot.

`.github/workflows/release.yml` holds PyPI Trusted Publishing
(`id-token: write`), `contents: write` and `attestations: write`, so an
unreviewed edit to it is a supply-chain change rather than a CI tweak. This
check binds the workflow to a canonical digest: any edit that changes the
workflow's *meaning* fails CI until the snapshot under `scripts/ci/` is
refreshed in the same pull request.

The digest is taken over a canonical form of the YAML document, so comments,
blank lines, mapping key order, CRLF endings and quoting style do not change
it. Structural changes do, which is exactly the signal we want: adding a
permission, swapping a pinned action, or dropping a publish-path assertion all
move the digest, while reformatting the file does not.

The one exception is the body of a `run:` block, which is a shell script and
therefore content down to its whitespace: there, trailing spaces, blank lines
and comment lines all move the digest. Whitespace that cannot change YAML
meaning is still ignored everywhere outside those blocks.

Scope: this is a drift check, **not** an approval gate. The snapshot is an
ordinary tracked file refreshed by an ordinary pull request, deliberately so —
the previous trust-root approval path deadlocked on repositories with a single
maintainer, because no independent approver existed. What this restores is the
tamper evidence and the audit trail: a release-workflow change now shows up as
an explicit, reviewable snapshot diff and as a digest in the CI log instead of
landing silently between two release tags.

This file is the shared kit copy. It is byte-identical across repositories and
has no repository-specific content: every path is derived from its own
location, so porting it to a new repository is a copy plus one snapshot
initialisation command. See the sibling `README.md` in
`scripts/ci/release_workflow_integrity/` (dcc-mcp-core) for the porting steps.

Usage:
    python scripts/ci/check_release_workflow_digest.py
    python scripts/ci/check_release_workflow_digest.py --print-digest
    python scripts/ci/check_release_workflow_digest.py --update-snapshot
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import pathlib
import re
import stat
import sys
import typing

import yaml
from yaml.constructor import ConstructorError
from yaml.events import AliasEvent
from yaml.nodes import MappingNode

ROOT = pathlib.Path(__file__).resolve().parents[2]
RELEASE_WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"
APPROVED_SNAPSHOT = ROOT / "scripts" / "ci" / "approved_release_workflow.yml"
MAX_WORKFLOW_BYTES = 256 * 1024


class DriftError(ValueError):
    """The release workflow cannot be bound to the approved snapshot."""


class StrictWorkflowLoader(yaml.SafeLoader):
    """Safe YAML loader with YAML 1.2 booleans and unique string keys."""


StrictWorkflowLoader.yaml_implicit_resolvers = copy.deepcopy(yaml.SafeLoader.yaml_implicit_resolvers)
for first_character, resolvers in list(StrictWorkflowLoader.yaml_implicit_resolvers.items()):
    StrictWorkflowLoader.yaml_implicit_resolvers[first_character] = [
        resolver for resolver in resolvers if resolver[0] != "tag:yaml.org,2002:bool"
    ]
StrictWorkflowLoader.add_implicit_resolver(
    "tag:yaml.org,2002:bool",
    re.compile(r"^(?:true|false)$", re.IGNORECASE),
    list("tTfF"),
)


def _construct_unique_mapping(
    loader: StrictWorkflowLoader, node: MappingNode, deep: bool = False
) -> typing.Dict[str, typing.Any]:
    mapping: typing.Dict[str, typing.Any] = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if not isinstance(key, str):
            raise ConstructorError(None, None, "workflow mapping keys must be strings", key_node.start_mark)
        if key in mapping:
            raise ConstructorError(None, None, f"duplicate workflow key {key!r}", key_node.start_mark)
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping


StrictWorkflowLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG,
    _construct_unique_mapping,
)


def _read_regular_bytes(path: pathlib.Path) -> bytes:
    try:
        metadata = path.lstat()
    except OSError:
        raise DriftError(f"workflow input is unavailable: {path}") from None
    reparse_flag = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    file_attributes = getattr(metadata, "st_file_attributes", 0)
    if path.is_symlink() or file_attributes & reparse_flag or not stat.S_ISREG(metadata.st_mode):
        raise DriftError(f"workflow input must be a regular non-symlink file: {path}")
    if metadata.st_size > MAX_WORKFLOW_BYTES:
        raise DriftError(f"workflow input exceeds the trusted size limit: {path}")
    try:
        return path.read_bytes()
    except OSError:
        raise DriftError(f"workflow input is unreadable: {path}") from None


def _load_workflow(path: pathlib.Path) -> typing.Mapping[str, typing.Any]:
    try:
        source = _read_regular_bytes(path).decode("utf-8")
    except UnicodeError:
        raise DriftError(f"workflow input is not valid UTF-8: {path}") from None
    try:
        if any(isinstance(event, AliasEvent) for event in yaml.parse(source, Loader=StrictWorkflowLoader)):
            raise DriftError(f"workflow aliases are not allowed: {path}")
        documents = list(yaml.load_all(source, Loader=StrictWorkflowLoader))
    except yaml.YAMLError as exc:
        raise DriftError(f"workflow YAML is invalid: {path}: {exc.__class__.__name__}") from None
    if len(documents) != 1 or not isinstance(documents[0], dict):
        raise DriftError(f"workflow must contain exactly one mapping document: {path}")
    return documents[0]


def _normalize_run(value: typing.Any) -> str:
    """Normalize only line endings; preserve every other byte of a `run` block.

    A `run:` block scalar is a shell script, not a list of independent commands,
    so whitespace inside it is content: a space after a line-continuation
    backslash breaks the continuation and turns the next line into a new
    command, and a blank line inside a heredoc changes the file the script
    writes. Dropping trailing whitespace, blank lines or `#` lines would make
    both edits invisible to the digest, so none of them are normalized. Only
    CRLF and lone CR are folded, which is what keeps the digest identical
    across Windows and Linux checkouts of the same commit.
    """
    return str(value).replace("\r\n", "\n").replace("\r", "\n")


def _canonicalize(value: typing.Any, key: str = "") -> typing.Any:
    if isinstance(value, dict):
        return {child_key: _canonicalize(value[child_key], child_key) for child_key in sorted(value)}
    if isinstance(value, list):
        return [_canonicalize(item) for item in value]
    if key == "run":
        return _normalize_run(value)
    if value is None or isinstance(value, (str, bool, int, float)):
        return value
    raise DriftError(f"workflow contains unsupported value type {value.__class__.__name__}")


def release_workflow_digest(path: pathlib.Path) -> str:
    """Return the canonical sha256 digest of a workflow document."""
    canonical = _canonicalize(_load_workflow(path))
    encoded = json.dumps(canonical, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Check the release workflow against its approved snapshot.")
    parser.add_argument("--workflow", type=pathlib.Path, default=RELEASE_WORKFLOW, help="release workflow under test")
    parser.add_argument(
        "--snapshot", type=pathlib.Path, default=APPROVED_SNAPSHOT, help="approved snapshot to compare against"
    )
    parser.add_argument("--print-digest", action="store_true", help="print the canonical digest of --workflow and exit")
    parser.add_argument(
        "--update-snapshot",
        action="store_true",
        help="rewrite the approved snapshot from --workflow and exit (refresh after an intentional change)",
    )
    return parser


def main() -> int:
    """Compare the release workflow with its approved snapshot; return the exit code."""
    arguments = _parser().parse_args()
    try:
        if arguments.print_digest:
            print(release_workflow_digest(arguments.workflow))
            return 0
        if arguments.update_snapshot:
            payload = _read_regular_bytes(arguments.workflow)
            _load_workflow(arguments.workflow)
            arguments.snapshot.write_bytes(payload)
            print(f"updated {arguments.snapshot} from {arguments.workflow}")
            print(f"new digest: {release_workflow_digest(arguments.snapshot)}")
            return 0
        approved_digest = release_workflow_digest(arguments.snapshot)
        candidate_digest = release_workflow_digest(arguments.workflow)
    except DriftError as exc:
        print(f"release workflow integrity check failed: {exc}", file=sys.stderr)
        return 1
    if approved_digest != candidate_digest:
        print(
            "\n".join(
                [
                    f"release workflow integrity check failed: {arguments.workflow} drifted from {arguments.snapshot}",
                    f"  approved  : {approved_digest}",
                    f"  candidate : {candidate_digest}",
                    "",
                    "The release workflow holds PyPI Trusted Publishing credentials, so this change",
                    "needs a human review. If the change is intentional, refresh the snapshot in the",
                    "same pull request so the edit is visible and reviewable:",
                    "",
                    "    python scripts/ci/check_release_workflow_digest.py --update-snapshot",
                    "",
                ]
            ),
            file=sys.stderr,
        )
        return 1
    print(f"release workflow integrity ok: {candidate_digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
