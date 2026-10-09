#!/usr/bin/env python3
"""Fail when a workflow expression drifts from the golden test that pins it.

Why this gate exists
--------------------
A GitHub Actions expression (``${{ ... }}``) lives in two places at once: the
workflow that evaluates it, and the golden test that asserts its exact text.
Nothing forced the two to move together, so a workflow change that should have
updated the test did not, and the mismatch only surfaced after the full test
matrix had already run. Three independent pull requests hit this same shape in
one day, each costing a full ``NACK -> fix -> new head -> rerun matrix`` loop.

This check resolves both sides and compares them, so the mismatch is reported
in seconds, before any matrix job starts:

    .github/workflows/release.yml:700 -> jobs.build-wheels.with.reuse-release-assets
        workflow: ${{ ... && inputs.backfill_assets != true }}
    tests/test_release_workflow.py:16 -> REUSE_RELEASE_ASSETS_EXPRESSION
        test:     ${{ ... }}

Design: declarative bindings, not test-code inference
-----------------------------------------------------
Each binding names a workflow YAML path and the test literal that pins it. The
checker resolves the path against the parsed workflow and compares the two
strings.

The binding table is *not* inferred by parsing the tests. Reverse-engineering
arbitrary assertion code cannot reliably report the test-side line number and
would silently skip any assertion shape it fails to recognise -- a green stub
is worse than no gate. A binding is also deliberately **not** a rewrite of the
test to read the workflow: a golden test earns its keep by being an
independently written expectation. Reading the value back from the workflow
would make the assertion tautological and delete the signal. Declaring the
binding next to the literal keeps both sides explicit and reviewable.

Usage
-----
    python scripts/ci/check_workflow_expression_sync.py
    python scripts/ci/check_workflow_expression_sync.py --format github
    python scripts/ci/check_workflow_expression_sync.py --root /path/to/repo

Exit codes: ``0`` when every binding agrees, ``1`` when at least one drifts,
``2`` when the check itself could not run (unreadable file, missing key).
"""

from __future__ import annotations

import argparse
import ast
from pathlib import Path
import re
import sys
from typing import NamedTuple
from typing import Sequence

try:
    import yaml
except ImportError:  # pragma: no cover - the CI job installs PyYAML first
    yaml = None  # type: ignore[assignment]

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPT_DIR.parent.parent

EXPRESSION_PREFIX = "${{"
# A test_name of the form "<key>@with" reads a literal out of a dict compared
# against ``something["with"]``.
DICT_KEY_SELECTOR = re.compile(r"^(?P<key>[^@]+)@(?P<attr>.+)$")


class Binding(NamedTuple):
    """One workflow expression pinned by one golden-test literal.

    ``workflow`` is the repository-relative workflow path, ``path`` the dotted
    YAML path to resolve inside it.

    The expected value is **read from the test file**, not restated here.
    ``test_file`` / ``test_name`` identify it; for module-level constants
    ``test_name`` is the assigned name (``REUSE_RELEASE_ASSETS_EXPRESSION``),
    and for a literal inside an assertion it is ``key@<dict key>``
    (``token@with``). Restating the literal in this table would make the
    checker a third copy of the same fact, and an edit to the test alone could
    never turn the gate red -- the exact drift this check exists to catch.

    When ``uses`` is set, ``path`` is resolved inside the job's step that
    invokes that action, found by name rather than by list position -- step
    order changes as the workflow grows, and a positional binding would drift
    into reporting failures about the wrong step. ``step_name`` selects by the
    step's ``name:`` instead, for steps that run a shell command and invoke no
    action.
    """

    workflow: str
    path: str
    test_file: str
    test_name: str
    uses: str = ""
    step_name: str = ""


# Resolve a dotted path with optional ``[n]`` list indices, e.g.
# "jobs.build-wheels.with.reuse-release-assets" or "jobs.publish.needs[0]".
_PATH_SEGMENT = re.compile(r"^([^\[\]]+)((\[\d+\])*)$")


# The pinned pairs. Each entry binds one workflow YAML path to the golden-test
# literal that asserts it. The literal itself lives in the test file; only its
# location is recorded here.
BINDINGS: tuple[Binding, ...] = (
    # --- release.yml: the backfill / asset-overwrite contract ---------------
    Binding(
        workflow=".github/workflows/release.yml",
        path="jobs.build-wheels.with.reuse-release-assets",
        test_file="tests/test_release_workflow.py",
        test_name="REUSE_RELEASE_ASSETS_EXPRESSION",
    ),
    Binding(
        workflow=".github/workflows/release.yml",
        path="jobs.build-wheels.with.overwrite-release-assets",
        test_file="tests/test_release_workflow.py",
        test_name="OVERWRITE_FILES_EXPRESSION",
    ),
    Binding(
        workflow=".github/workflows/release.yml",
        path="jobs.build-wheels.secrets.RELEASE_TOKEN",
        test_file="tests/test_release_workflow.py",
        test_name="RELEASE_TOKEN@secrets",
    ),
    # The upload token is asserted once per softprops step, and the golden
    # test loops over every job that has one, so the binding spans all jobs
    # with ``jobs.*``. A single-job binding here would leave three of the
    # four upload steps unchecked.
    Binding(
        workflow=".github/workflows/release.yml",
        path="jobs.*.steps.with.token",
        test_file="tests/test_release_workflow.py",
        test_name="GITHUB_RELEASE_TOKEN",
        uses="softprops/action-gh-release@v3",
    ),
    # --- build-wheels.yml: the reusable-workflow upload contract ------------
    # Resolved by action name, not step index: the publish job gains and
    # reorders steps, and a positional binding would silently point at the
    # wrong step. These three come from the ``upload["with"] == {...}``
    # assertion, so they are keyed by their dict key.
    Binding(
        workflow=".github/workflows/build-wheels.yml",
        path="jobs.publish-release.steps.with.token",
        test_file="tests/test_build_wheels_workflow.py",
        test_name="token@with",
        uses="softprops/action-gh-release@v3",
    ),
    Binding(
        workflow=".github/workflows/build-wheels.yml",
        path="jobs.publish-release.steps.with.tag_name",
        test_file="tests/test_build_wheels_workflow.py",
        test_name="tag_name@with",
        uses="softprops/action-gh-release@v3",
    ),
    Binding(
        workflow=".github/workflows/build-wheels.yml",
        path="jobs.publish-release.steps.with.overwrite_files",
        test_file="tests/test_build_wheels_workflow.py",
        test_name="overwrite_files@with",
        uses="softprops/action-gh-release@v3",
    ),
    # --- release-please-lock-sync.yml: resolved head SHA --------------------
    # Both steps run a shell command and invoke no action, so they are
    # selected by ``name:``.
    Binding(
        workflow=".github/workflows/release-please-lock-sync.yml",
        path="jobs.sync-cargo-metadata.steps.env.PR_HEAD_SHA",
        test_file="tests/test_generated_lock_workflow_execution.py",
        test_name="RESOLVED_HEAD_EXPRESSION",
        step_name="Revalidate pull request identity and generated diff",
    ),
    Binding(
        workflow=".github/workflows/release-please-lock-sync.yml",
        path="jobs.sync-cargo-metadata.steps.env.EXPECTED_HEAD_SHA",
        test_file="tests/test_generated_lock_workflow_execution.py",
        test_name="RESOLVED_LEASE_EXPRESSION",
        step_name="Push fixed generated lock commit",
    ),
)


class Mismatch(NamedTuple):
    """One binding whose two sides disagree."""

    binding: Binding
    workflow_line: int | None
    test_line: int
    actual: str | None
    expected: str | None
    reason: str


class ResolutionError(NamedTuple):
    """A binding that could not be evaluated at all."""

    binding: Binding
    reason: str


def _split_path(path: str) -> list[object]:
    """Split ``"a.b[0].c"`` into ``["a", "b", 0, "c"]``."""
    keys: list[object] = []
    for raw in path.split("."):
        match = _PATH_SEGMENT.match(raw)
        if match is None:
            raise ValueError(f"invalid path segment: {raw!r}")
        keys.append(match.group(1))
        for index in re.findall(r"\[(\d+)\]", match.group(2)):
            keys.append(int(index))
    return keys


def resolve(node: object, path: str) -> tuple[bool, str | None, str]:
    """Resolve ``path`` inside ``node``.

    Returns ``(ok, value, reason)``. ``ok`` is False when the path does not
    exist; ``reason`` then explains why.
    """
    try:
        keys = _split_path(path)
    except ValueError as exc:
        return False, None, str(exc)
    current = node
    walked = ""
    for key in keys:
        if isinstance(key, int):
            if not isinstance(current, list) or key >= len(current):
                return False, None, "{} is not a list index at {!r}".format(key, walked or "<root>")
            current = current[key]
        elif key == "steps" and isinstance(current, dict) and isinstance(current.get("steps"), list):
            # A ``steps`` segment with an explicit ``uses`` selector picks the
            # step invoking that action; see Binding.uses.
            current = current["steps"]
        else:
            if not isinstance(current, dict) or key not in current:
                return False, None, "{!r} has no key {!r}".format(walked or "<root>", key)
            current = current[key]
        walked = f"{walked}.{key}" if walked else str(key)
    if not isinstance(current, str):
        return False, None, f"{path!r} resolved to {type(current).__name__}, not a string"
    return True, current, ""


def resolve_binding(workflow: object, binding: Binding) -> tuple[bool, str | None, str]:
    """Resolve ``binding.path``, honouring the ``uses`` step selector.

    When ``binding.uses`` is set, the ``steps`` list is first narrowed to the
    step that invokes that action. ``uses`` values may carry a trailing
    ``# <version>`` comment, so the comparison strips it.
    """
    node = workflow
    if binding.uses or binding.step_name:
        keys = _split_path(binding.path)
        # Walk to the list that holds the steps, then select by action.
        # A ``*`` job segment means "every job", because a golden test often
        # asserts one constant against the same action in several jobs at
        # once (all four release upload steps share one token expression).
        prefix = [key for key in keys[: keys.index("steps")]] if "steps" in keys else []
        tail = keys[keys.index("steps") + 1 :]

        containers: list[tuple[object, str]] = []

        def descend(current_obj: object, remaining: list, label: str) -> tuple[bool, str]:
            if not remaining:
                containers.append((current_obj, label))
                return True, ""
            key = remaining[0]
            if key == "*":
                if not isinstance(current_obj, dict):
                    return False, f"{label or '<root>'!r} is not a mapping, cannot expand '*'"
                for name, value in current_obj.items():
                    ok, err = descend(value, remaining[1:], f"{label}.{name}" if label else str(name))
                    if not ok:
                        return False, err
                return True, ""
            if not isinstance(current_obj, dict) or key not in current_obj:
                return False, f"{label or '<root>'!r} has no key {key!r}"
            return descend(current_obj[key], remaining[1:], f"{label}.{key}" if label else str(key))

        ok, err = descend(workflow, prefix, "")
        if not ok:
            return False, None, err

        resolved: list[str] = []
        seen_any = False
        for container, label in containers:
            steps = container.get("steps") if isinstance(container, dict) else None
            if not isinstance(steps, list):
                # A wildcard expands to every job, and most jobs legitimately
                # have no such step; the golden test filters the same way.
                # Only an explicit (non-wildcard) path is a hard error here.
                if "*" in prefix:
                    continue
                return False, None, f"no 'steps' list under {label!r}"
            if binding.uses:
                matches = [
                    step
                    for step in steps
                    if isinstance(step, dict) and str(step.get("uses") or "").split("#", 1)[0].strip() == binding.uses
                ]
                description = f"step using {binding.uses!r}"
            else:
                matches = [step for step in steps if isinstance(step, dict) and step.get("name") == binding.step_name]
                description = f"step named {binding.step_name!r}"
            if not matches:
                if "*" in prefix:
                    continue
                return False, None, f"no {description} in {label!r}"
            if len(matches) != 1:
                return False, None, f"expected exactly 1 {description} in {label!r}, found {len(matches)}"
            seen_any = True
            current: object = matches[0]
            walked = f"{label}.steps[{binding.uses or binding.step_name}]"
            for key in tail:
                if isinstance(key, int):
                    if not isinstance(current, list) or key >= len(current):
                        return False, None, f"{key} is not a list index at {walked!r}"
                    current = current[key]
                else:
                    if not isinstance(current, dict) or key not in current:
                        return False, None, f"{walked!r} has no key {key!r}"
                    current = current[key]
                walked = f"{walked}.{key}"
            if not isinstance(current, str):
                return False, None, f"{binding.path!r} resolved to {type(current).__name__}, not a string"
            resolved.append(current)

        distinct = sorted(set(resolved))
        if not seen_any:
            return False, None, f"no matching step found under {binding.path!r}"
        if len(distinct) != 1:
            return (
                False,
                None,
                (f"{binding.path!r} resolves to {len(distinct)} different values across jobs: {distinct}"),
            )
        return True, distinct[0], ""
    return resolve(node, binding.path)


def _workflow_line(root: Path, binding: Binding) -> int | None:
    """Find the 1-based line where the workflow sets the bound path.

    Only the final key is matched, which is enough to point a reader at the
    right line without pretending to be a full YAML source mapper.
    """
    workflow_path = root / binding.workflow
    if not workflow_path.is_file():
        return None
    leaf = _split_path(binding.path)[-1]
    needle = f"{leaf}:"
    try:
        lines = workflow_path.read_text(encoding="utf-8").splitlines()
    except OSError:
        return None
    for number, line in enumerate(lines, start=1):
        if line.strip().startswith(needle):
            return number
    return None


# Node class names that carry a plain string literal.
#
# Python 3.7 parses a string literal as ``ast.Str``; 3.8 folded ``Str``,
# ``Num`` and friends into ``ast.Constant``. Matching on the class *name*
# rather than importing ``ast.Str`` avoids the DeprecationWarning that
# touching that alias emits on 3.12+, and still works unchanged on 3.14
# where the alias was removed. Checking only ``ast.Constant`` makes every
# literal unreadable on 3.7, which is the failure this gate first shipped
# with.
_STRING_NODE_NAMES = frozenset({"Constant", "Str"})


def _string_value(node: ast.AST) -> str | None:
    """Return the value of a string-literal node, whatever type carries it.

    ``ast.Constant`` holds any constant, so the payload is re-checked for
    ``str``; a bare ``ast.Str`` only ever holds a string.
    """
    name = type(node).__name__
    if name not in _STRING_NODE_NAMES:
        return None
    if name == "Constant":
        candidate = getattr(node, "value", None)
        return candidate if isinstance(candidate, str) else None
    # A bare ast.Str stores the text on .s. Read it only for that type, so
    # the deprecated attribute is never touched on modern interpreters.
    candidate = getattr(node, "s", None)
    return candidate if isinstance(candidate, str) else None


def _const_str(node: ast.AST) -> str | None:
    """Return the string value of a string / string-concatenation node."""
    direct = _string_value(node)
    if direct is not None:
        return direct
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
        left = _const_str(node.left)
        right = _const_str(node.right)
        if left is not None and right is not None:
            return left + right
    return None


def _module_constants(tree: ast.Module) -> dict[str, tuple[str, int]]:
    """Map module-level string constant name -> (value, line number).

    ``ast.literal_eval`` cannot follow a reference to another constant, so
    names are resolved iteratively: ``REUSE_RELEASE_ASSETS_EXPRESSION`` is
    built from ``ASSETS_BACKFILL_GUARD``, which is itself a plain literal.
    """
    raw: dict[str, tuple[ast.AST, int]] = {}
    for node in tree.body:
        if not isinstance(node, ast.Assign) or len(node.targets) != 1:
            continue
        target = node.targets[0]
        if not isinstance(target, ast.Name):
            continue
        raw[target.id] = (node.value, node.lineno)

    def evaluate(node: ast.AST, seen: frozenset) -> str | None:
        """Fold string concatenation, following references to other constants.

        ``seen`` carries the names currently being expanded, so a genuinely
        self-referential constant stops instead of recursing forever. The
        depth is bounded by the number of names, which is what a
        concatenation chain can actually traverse.
        """
        direct = _const_str(node)
        if direct is not None:
            return direct
        if isinstance(node, ast.Name):
            if node.id in seen:
                return None
            referenced = raw.get(node.id)
            if referenced is None:
                return None
            return evaluate(referenced[0], seen | {node.id})
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            left = evaluate(node.left, seen)
            right = evaluate(node.right, seen)
            if left is not None and right is not None:
                return left + right
            return None
        if isinstance(node, ast.JoinedStr):
            parts = [evaluate(value, seen) for value in node.values]
            if all(part is not None for part in parts):
                return "".join(part for part in parts if part is not None)
            return None
        if isinstance(node, ast.FormattedValue):
            return evaluate(node.value, seen)
        return None

    resolved: dict[str, tuple[str, int]] = {}
    for name, (node, lineno) in raw.items():
        value = evaluate(node, frozenset({name}))
        if value is not None:
            resolved[name] = (value, lineno)
    return resolved


def _dict_literals(tree: ast.Module, attr: str) -> dict[str, tuple[str, int]]:
    """Find string keys of dicts compared against ``<expr>[attr]``.

    Locates ``assert something["with"] == {...}`` and returns each string
    value keyed by its dict key, so the checker reads the literal from the
    assertion instead of restating it.
    """
    found: dict[str, tuple[str, int]] = {}

    def subscript_attr(node: ast.AST) -> str | None:
        """Return the literal index of ``<expr>[<literal>]``, else None.

        The slice is an ``ast.Constant`` on Python 3.9+ and an ``ast.Index``
        wrapping a string node on 3.7/3.8 -- and on 3.7 that inner node is an
        ``ast.Str``, not an ``ast.Constant``. Unwrap only while the current
        node is neither kind of string node, otherwise the loop would drill
        into ``.value`` and hand back the subscripted string itself instead
        of the node holding it.
        """
        if not isinstance(node, ast.Subscript):
            return None
        index: object = node.slice
        while isinstance(index, ast.AST) and type(index).__name__ not in _STRING_NODE_NAMES:
            inner = getattr(index, "value", None)
            if inner is None:
                return None
            index = inner
        if isinstance(index, ast.AST):
            return _string_value(index)
        return None

    for node in ast.walk(tree):
        if not isinstance(node, ast.Compare) or not node.comparators:
            continue
        if subscript_attr(node.left) != attr:
            continue
        for comparator in node.comparators:
            if not isinstance(comparator, ast.Dict):
                continue
            for key, val in zip(comparator.keys, comparator.values):
                name = _string_value(key)
                if not isinstance(name, str):
                    continue
                text = _const_str(val)
                if text is not None and name not in found:
                    found[name] = (text, comparator.lineno)
    return found


def read_test_literal(root: Path, binding: Binding) -> tuple[bool, str | None, int, str]:
    """Read the expected literal out of the golden test.

    Returns ``(ok, value, line, reason)``. The value is parsed from the test's
    own source, so editing the test moves the gate.
    """
    test_path = root / binding.test_file
    if not test_path.is_file():
        return False, None, 0, f"missing test file {binding.test_file}"
    try:
        source = test_path.read_text(encoding="utf-8")
        tree = ast.parse(source, filename=str(test_path))
    except (OSError, SyntaxError) as exc:
        return False, None, 0, f"cannot parse {binding.test_file}: {exc}"

    selector = DICT_KEY_SELECTOR.match(binding.test_name)
    if selector is not None:
        literals = _dict_literals(tree, selector.group("attr"))
        key = selector.group("key")
        if key not in literals:
            return (
                False,
                None,
                0,
                "{} has no {!r} key in its {!r} assertion".format(
                    binding.test_file,
                    key,
                    selector.group("attr"),
                ),
            )
        value, line = literals[key]
        return True, value, line, ""

    constants = _module_constants(tree)
    if binding.test_name not in constants:
        return False, None, 0, f"{binding.test_file} defines no constant {binding.test_name!r}"
    value, line = constants[binding.test_name]
    return True, value, line, ""


# Workflow test files whose module-level string constants are scanned by
# ``unbound_pinned_constants``. Only files that pin expressions verbatim as a
# module-level constant belong here; a test that asserts a fragment with
# ``in run`` cannot be bound by path and is listed in the docstring instead.
GOLDEN_TEST_FILES: tuple[str, ...] = (
    "tests/test_release_workflow.py",
    "tests/test_build_wheels_workflow.py",
    "tests/test_generated_lock_workflow_execution.py",
)

# Pinned expressions that are deliberately NOT bound, and why. A reader (or
# the completeness guard) must be able to see the gap rather than infer it.
#
# These are asserted as substrings inside a multi-line ``run:`` shell script
# (for example ``assert '--version "${{ needs.release-please.outputs.version }}"'
# in run``), so they have no single YAML path to resolve. Covering them would
# mean parsing shell text, which is a different and much weaker check.
KNOWN_UNBOUND_PINNED = (
    "tests/test_release_workflow.py: ${{ needs.release-please.outputs.version }} "
    "and the ${{ matrix.* }} bundle arguments, asserted as substrings of a run: script",
)


def unbound_pinned_constants(root: Path, bindings: tuple = BINDINGS) -> list[tuple[str, str]]:
    """Report pinned-but-unbound module-level expression constants.

    ``test_every_binding_pins_an_expression`` checks that every binding pins
    an expression; this is the reverse direction -- every expression the
    golden tests pin has a binding. Without it, adding a new golden constant
    while forgetting the binding silently narrows the gate's coverage and no
    test goes red.

    Returns ``(test_file, constant_name)`` pairs for constants that declare a
    workflow expression but appear in no binding.
    """
    bound_by_file: dict[str, set] = {}
    for binding in bindings:
        # A ``key@attr`` selector reads a dict literal, not a module constant.
        if DICT_KEY_SELECTOR.match(binding.test_name):
            continue
        bound_by_file.setdefault(binding.test_file, set()).add(binding.test_name)

    unbound: list[tuple[str, str]] = []
    for relative in GOLDEN_TEST_FILES:
        test_path = root / relative
        if not test_path.is_file():
            continue
        try:
            tree = ast.parse(test_path.read_text(encoding="utf-8"), filename=str(test_path))
        except (OSError, SyntaxError):
            continue
        for name, (_value, _line) in _module_constants(tree).items():
            # A test's own fixtures are not workflow expectations.
            if name.startswith("CLEAN_") or name.startswith("DRIFT"):
                continue
            if "${{" not in _value:
                continue
            if name not in bound_by_file.get(relative, set()):
                unbound.append((relative, name))
    return sorted(unbound)


def _load_workflows(root: Path) -> dict[str, object]:
    """Parse each bound workflow once, keyed by repository-relative path."""
    workflows: dict[str, object] = {}
    for binding in BINDINGS:
        if binding.workflow in workflows:
            continue
        workflow_path = root / binding.workflow
        if not workflow_path.is_file():
            workflows[binding.workflow] = None
            continue
        try:
            workflows[binding.workflow] = yaml.safe_load(workflow_path.read_text(encoding="utf-8"))
        except (OSError, yaml.YAMLError) as exc:
            workflows[binding.workflow] = None
            workflows[binding.workflow + "::__error__"] = str(exc)
    return workflows


def check(root: Path) -> tuple[list[Mismatch], list[ResolutionError]]:
    """Evaluate every binding under ``root``.

    Returns the lists of drifted bindings and of bindings that could not be
    evaluated. Both are empty only when every binding agrees.
    """
    if yaml is None:
        raise RuntimeError("PyYAML is required; install it with `python -m pip install pyyaml`")

    workflows = _load_workflows(root)
    mismatches: list[Mismatch] = []
    errors: list[ResolutionError] = []

    for binding in BINDINGS:
        test_ok, expected, test_line, test_reason = read_test_literal(root, binding)
        if not test_ok:
            errors.append(ResolutionError(binding, test_reason))
            continue

        workflow = workflows.get(binding.workflow)
        error = workflows.get(binding.workflow + "::__error__")
        if error is not None:
            errors.append(ResolutionError(binding, f"cannot parse {binding.workflow}: {error}"))
            continue
        if workflow is None:
            errors.append(ResolutionError(binding, f"missing workflow {binding.workflow}"))
            continue

        ok, actual, reason = resolve_binding(workflow, binding)
        if not ok:
            errors.append(ResolutionError(binding, reason))
            continue

        if actual == expected:
            continue

        mismatches.append(
            Mismatch(
                binding=binding,
                workflow_line=_workflow_line(root, binding),
                test_line=test_line,
                actual=actual,
                expected=expected,
                reason="workflow value does not match the literal the test asserts",
            )
        )

    return mismatches, errors


def format_report(root: Path, mismatches: Sequence[Mismatch], errors: Sequence[ResolutionError]) -> str:
    """Render a human-readable report naming both sides of every mismatch."""
    lines: list[str] = []
    if not mismatches and not errors:
        return f"workflow expression sync: {len(BINDINGS)} binding(s) agree"

    lines.append(f"workflow expression drift: {len(mismatches) + len(errors)} binding(s) disagree")
    lines.append("")
    lines.append(
        "A GitHub Actions expression and the golden-test literal that pins it must be changed in the same commit."
    )
    lines.append("")

    for mismatch in mismatches:
        binding = mismatch.binding
        location = "{}:{}".format(binding.workflow, mismatch.workflow_line or "?")
        lines.append(f"{location} -> {binding.path}")
        lines.append(f"    workflow: {mismatch.actual}")
        lines.append(f"{binding.test_file}:{mismatch.test_line} -> {binding.test_name}")
        lines.append(f"    test:     {mismatch.expected}")
        lines.append(f"    {mismatch.reason}")
        lines.append("")

    for error in errors:
        binding = error.binding
        lines.append(f"{binding.workflow} -> {binding.path}")
        lines.append(f"{binding.test_file} -> {binding.test_name}")
        lines.append(f"    unresolvable: {error.reason}")
        lines.append("")

    lines.append("Fix: update the workflow and its golden test together, then rerun this check.")
    return "\n".join(lines)


def _escape_annotation(text: str) -> str:
    r"""Escape a value for use inside a GitHub Actions workflow command.

    ``%``, ``\r`` and ``\n`` are the data characters GitHub re-interprets
    inside a command, and a bare ``,`` would split the attribute list, which
    is what silently dropped every attribute in the previous revision.
    """
    return str(text).replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A").replace(",", "%2C")


def format_github_annotations(root: Path, mismatches: Sequence[Mismatch], errors: Sequence[ResolutionError]) -> str:
    """Render GitHub Actions error annotations for each mismatch.

    The syntax is ``::error file=<f>,line=<n>,title=<t>::<message>``:
    attributes belong **before** the ``::`` separator and are comma-separated.
    Putting them after the message makes GitHub parse the whole first segment
    as a single unknown attribute key, which drops ``file``, ``line`` and
    ``title`` and leaves the annotation unattached to any file.
    """
    lines: list[str] = []
    for mismatch in mismatches:
        binding = mismatch.binding
        message = (
            f"{binding.path} expected={mismatch.expected} actual={mismatch.actual} "
            f"(test: {binding.test_file}:{mismatch.test_line} -> {binding.test_name})"
        )
        lines.append(
            f"::error file={_escape_annotation(binding.workflow)},"
            f"line={mismatch.workflow_line or 1},"
            f"title=workflow expression drift::{_escape_annotation(message)}"
        )
    for error in errors:
        message = f"{error.binding.path} -> {error.binding.test_name}: {error.reason}"
        lines.append(
            f"::error file={_escape_annotation(error.binding.workflow)},"
            f"title=workflow expression drift::{_escape_annotation(message)}"
        )
    return "\n".join(lines)


def main(argv: Sequence[str] | None = None) -> int:
    """Run the sync check and return the process exit code."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", default=str(DEFAULT_ROOT), help="repository root (default: %(default)s)")
    parser.add_argument(
        "--format",
        choices=("text", "github"),
        default="text",
        help="report format: plain text or GitHub Actions annotations (default: text)",
    )
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    try:
        mismatches, errors = check(root)
    except RuntimeError as exc:
        print(f"workflow expression sync: {exc}", file=sys.stderr)
        return 2

    # The plain-text report is always printed, including in annotation mode.
    # The acceptance criterion is that a failure names both sides, and that
    # must not depend on the CI runner rendering the annotation correctly --
    # a command with unparsed attributes is dropped whole, which would leave
    # the log with a bare "does not match" and no location.
    print(format_report(root, mismatches, errors))

    if args.format == "github":
        annotations = format_github_annotations(root, mismatches, errors)
        if annotations:
            print(annotations)

    return 1 if (mismatches or errors) else 0


if __name__ == "__main__":
    sys.exit(main())
