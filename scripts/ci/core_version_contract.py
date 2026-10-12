#!/usr/bin/env python3
"""Check and deterministically sync Core release projections without resolving dependencies.

The manifest is the release source. Only the explicit projections below follow it;
independent skill versions, compatibility floors, catalog/schema versions, historical
examples and resolved registry dependencies do not. This file is self-contained so
privileged automation can pipe its immutable Git object to ``python -I -``.
"""

from __future__ import annotations

import argparse
import ast
import io
import json
from pathlib import Path
import re
import stat
import subprocess
import sys
import tokenize

MARKER = "x-release-please-version"
VERSION = re.compile(r"(?<![\d.])\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?(?![\w.])")
MARKER_COMMENT = re.compile(r"(?:#|//|<!--)\s*" + MARKER + r"\b")
MANIFEST = ".release-please-manifest.json"
CONFIG = "release-please-config.json"
SEEDS = "benchmarks/skills/seeds.json"
# The updater registration and the semantic location are separate contracts.
# A config typo must fail even when the misplaced field happens to quote Core.
TOML_FIELDS = {
    "Cargo.toml": "workspace.package",
    "pyproject.toml": "project",
    "pkg/dcc-mcp-server-bin/pyproject.toml": "project",
    "pkg/dcc-mcp-core-semantic/pyproject.toml": "project",
    "pkg/dcc-mcp-cli-bin/pyproject.toml": "project",
}
JSON_FIELDS = {
    "admin-ui/package.json": (("version",),),
    "admin-ui/package-lock.json": (("version",), ("packages", "", "version")),
    "server.json": (("version",), ("packages", 0, "version")),
}
# Nested lock/package fields are derived here, rather than broad JSONPath wildcards
# that could rewrite dependency pins or rely on duplicate-path updater merging.
JSONPATHS = {
    "admin-ui/package.json": ("$.version",),
    "admin-ui/package-lock.json": ("$.version",),
    "server.json": ("$.version",),
}
# Counts also catch accidental removal of a release marker from a known file.
GENERIC_FILES = {
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
# This Rust template emits _core.pyi's marker; it is not itself a version claim.
MARKER_TEMPLATES = frozenset(("src/bin/stub_gen.rs",))
OUTPUTS = frozenset(TOML_FIELDS) | frozenset(JSON_FIELDS) | frozenset(GENERIC_FILES) | {SEEDS}


class ContractError(ValueError):
    """A declared release projection is missing, ambiguous or outside the contract."""


def git(root, *args):
    """Read repository data without executing hooks, filters or project code."""
    return subprocess.check_output(["git", *args], cwd=str(root), timeout=30)


def _json(text, path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ContractError(f"{path}: duplicate JSON key {key!r}")
            result[key] = value
        return result

    try:
        data = json.loads(text, object_pairs_hook=unique)
        if not isinstance(data, dict):
            raise ContractError(f"{path}: expected a JSON object")
        return data
    except ValueError as error:
        raise ContractError(f"{path}: {error}") from error


def _version(value, location):
    if not isinstance(value, str) or VERSION.fullmatch(value) is None:
        raise ContractError(f"{location}: expected a three-part release version")
    return value


def _json_slot(data, keys, path):
    cursor = data
    for key in keys[:-1]:
        if isinstance(key, int):
            if not isinstance(cursor, list) or len(cursor) <= key:
                raise ContractError(f"{path}: missing JSON path {keys!r}")
        elif not isinstance(cursor, dict) or key not in cursor:
            raise ContractError(f"{path}: missing JSON path {keys!r}")
        cursor = cursor[key]
    key = keys[-1]
    if not isinstance(cursor, dict) or key not in cursor:
        raise ContractError(f"{path}: missing JSON path {keys!r}")
    _version(cursor[key], f"{path} {keys!r}")
    return cursor, key


def _toml_code(text, path):
    """Mask strings/comments while retaining offsets, lines and real string spans.

    This is deliberately a lexer, not a permissive TOML parser. Release tables
    must use bare keys and simple version strings; unsupported forms fail closed.
    In particular, a version-looking line inside a multiline string is data.
    """
    code = list(text)
    strings = []
    index = 0
    while index < len(text):
        char = text[index]
        if char == "#":
            while index < len(text) and text[index] not in "\r\n":
                code[index] = " "
                index += 1
        elif char in "\"'":
            start = index
            quote = char
            width = 3 if text.startswith(quote * 3, index) else 1
            index += width
            while index < len(text):
                if quote == '"' and text[index] == "\\":
                    index += 2
                elif text.startswith(quote * width, index):
                    index += width
                    break
                elif width == 1 and text[index] in "\r\n":
                    raise ContractError(f"{path}: newline inside a single-line TOML string")
                else:
                    index += 1
            else:
                raise ContractError(f"{path}: unterminated TOML string")
            strings.append((start, index))
            for cursor in range(start, min(index, len(text))):
                if code[cursor] not in "\r\n":
                    code[cursor] = " "
        else:
            index += 1
    return "".join(code), strings


def _toml_table(text, section, path):
    code, strings = _toml_code(text, path)
    headers = list(re.finditer(r"^[ \t]*\[([^\r\n]*)\][ \t]*\r?$", code, re.MULTILINE))
    matches = [i for i, header in enumerate(headers) if header.group(1) == section]
    if len(matches) != 1:
        raise ContractError(f"{path}: expected one [{section}] table with bare keys")
    index = matches[0]
    start = headers[index].end()
    end = headers[index + 1].start() if index + 1 < len(headers) else len(text)
    # Quoted/escaped keys are valid TOML, but outside this deliberately narrow
    # updater syntax. Reject them rather than overlook a second version key.
    for match in re.finditer(r"(?m)^[ \t]*=", code[start:end]):
        original = text[start + match.start() : start + match.end() - 1].strip()
        if original.startswith(('"', "'")):
            raise ContractError(f"{path}: quoted keys are unsupported in [{section}]")
    return code, strings, start, end


def _toml_version(text, section, path, version):
    code, _, start, end = _toml_table(text, section, path)
    assignments = list(re.finditer(r"(?m)^[ \t]*version[ \t]*=[^\r\n]*", code[start:end]))
    if len(assignments) != 1:
        raise ContractError(f"{path}: expected one [{section}].version")
    assignment = assignments[0]
    line_start = start + assignment.start()
    line_end = start + assignment.end()
    line = text[line_start:line_end]
    match = re.fullmatch(r"[ \t]*version[ \t]*=[ \t]*([\"'])([^\"'\r\n]+)\1[ \t]*(?:#.*)?", line)
    if match is None:
        raise ContractError(f"{path}: version must be a simple quoted string")
    markers = list(MARKER_COMMENT.finditer(text))
    if len(markers) != 1 or not line_start <= markers[0].start() < line_end:
        raise ContractError(f"{path}: marker must label only [{section}].version")
    _version(match.group(2), path)
    return text[: line_start + match.start(2)] + version + text[line_start + match.end(2) :]


def _markers(text, path, version, expected_count):
    lines = text.splitlines(keepends=True)
    count = 0
    for index, line in enumerate(lines):
        marker = MARKER_COMMENT.search(line)
        if marker is None:
            continue
        count += 1
        tokens = list(VERSION.finditer(line[: marker.start()]))
        if len(tokens) != 1:
            raise ContractError(f"{path}:{index + 1}: marker must label exactly one version")
        token = tokens[0]
        lines[index] = line[: token.start()] + version + line[token.end() :]
    if count != expected_count:
        raise ContractError(f"{path}: expected {expected_count} release markers, found {count}")
    return "".join(lines)


def _registrations(read):
    config = _json(read(CONFIG), CONFIG)
    try:
        package = config["packages"]["."]
        entries = package["extra-files"]
        if package["package-name"] != "dcc-mcp-core":
            raise ContractError("release-please root package must be dcc-mcp-core")
        if not isinstance(entries, list) or not all(isinstance(entry, dict) for entry in entries):
            raise ContractError("release-please extra-files must be a list of objects")
        actual = [(entry["path"], entry["type"], entry.get("jsonpath")) for entry in entries]
        if not all(
            isinstance(path, str) and isinstance(kind, str) and (field is None or isinstance(field, str))
            for path, kind, field in actual
        ):
            raise ContractError("release-please registration values must be strings")
    except (KeyError, TypeError) as error:
        raise ContractError("release-please extra-files registration is missing or malformed") from error
    expected = [(path, "toml", "$." + section + ".version") for path, section in TOML_FIELDS.items()]
    expected += [(path, "json", field) for path, fields in JSONPATHS.items() for field in fields]
    expected += [(path, "generic", None) for path in GENERIC_FILES]
    if len(actual) != len(set(actual)) or set(actual) != set(expected):
        missing = set(expected) - set(actual)
        unknown = set(actual) - set(expected)
        raise ContractError(f"release-please registration drift: missing={missing!r}, unexpected={unknown!r}")


def _python_marker(text, path):
    """Bind the marker to the unique top-level Python version string assignment.

    Tokenize the full file, but only parse the declaration: generated stubs may
    contain newer typing syntax even when this checker is running on Python 3.7.
    """
    try:
        tokens = list(tokenize.generate_tokens(io.StringIO(text).readline))
    except tokenize.TokenError as error:
        raise ContractError(f"{path}: invalid Python tokens") from error
    declarations = []
    depth = 0
    for token in tokens:
        if token.type == tokenize.NAME and token.string == "__version__" and token.start[1] == 0 and depth == 0:
            declarations.append(token.start[0])
        if token.type == tokenize.OP:
            if token.string in "([{":
                depth += 1
            elif token.string in ")]}":
                depth -= 1
    if len(declarations) != 1:
        raise ContractError(f"{path}: expected one top-level __version__ declaration")
    line = text.splitlines()[declarations[0] - 1]
    try:
        statements = ast.parse(line).body
    except SyntaxError as error:
        raise ContractError(f"{path}: __version__ must be a single-line string assignment") from error
    if len(statements) != 1 or not isinstance(statements[0], (ast.Assign, ast.AnnAssign)):
        raise ContractError(f"{path}: __version__ must be a string assignment")
    statement = statements[0]
    targets = statement.targets if isinstance(statement, ast.Assign) else [statement.target]
    value = statement.value
    if isinstance(value, ast.Constant):
        literal = value.value
    elif sys.version_info < (3, 8) and isinstance(value, ast.Str):
        literal = value.s
    else:
        literal = None
    if (
        len(targets) != 1
        or not isinstance(targets[0], ast.Name)
        or targets[0].id != "__version__"
        or not isinstance(literal, str)
        or MARKER_COMMENT.search(line) is None
    ):
        raise ContractError(f"{path}: marker must label the __version__ string assignment")
    _version(literal, path)


def _skill_marker(text, path, expected=("metadata", "dcc-mcp", "version")):
    """Require the marked skill version to be the production loader's nested key."""
    lines = text.splitlines()
    if not lines or lines[0] != "---" or "---" not in lines[1:]:
        raise ContractError(f"{path}: missing skill frontmatter")
    stack = []
    found = 0
    seen = set()
    for line in lines[1 : lines[1:].index("---") + 1]:
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip(" "))
        while stack and stack[-1][0] >= indent:
            stack.pop()
        # Unknown/quoted sibling keys still end the preceding mapping. Never
        # retain an old ancestor just because a key uses unsupported syntax.
        match = re.match(r"^( *)([^:]+):(?:\s|$)", line)
        if match:
            key = match.group(2).strip()
            parent = tuple(entry[1] for entry in stack)
            if key.startswith(('"', "'", "?")) and parent in [expected[:i] for i in range(len(expected))]:
                raise ContractError(f"{path}: quoted/complex release-path keys are unsupported")
            mapping = not line[match.end() :].strip() or line[match.end() :].lstrip().startswith("#")
        else:
            key, mapping = "<scalar>", False
        stack.append((indent, key, mapping))
        keys = tuple(entry[1] for entry in stack)
        if keys in [expected[:index] for index in range(1, len(expected) + 1)]:
            if keys in seen:
                raise ContractError(f"{path}: duplicate skill version path")
            seen.add(keys)
        if MARKER_COMMENT.search(line):
            if not match or tuple(entry[1] for entry in stack) != expected or not all(entry[2] for entry in stack[:-1]):
                raise ContractError(f"{path}: marker must label {'.'.join(expected)}")
            value = line[match.end() :].split("#", 1)[0].strip()
            literal = re.fullmatch(r"([\"'])([^\"']+)\1", value)
            if literal is None:
                raise ContractError(f"{path}: skill version must be a simple quoted string")
            _version(literal.group(2), path)
            found += 1
    if found != 1:
        raise ContractError(f"{path}: expected one nested skill version marker")


def _workspace_inheritance(read):
    """Keep every Core crate on workspace inheritance; workspace-hack is independent."""
    text = read("Cargo.toml")
    code, strings, start, end = _toml_table(text, "workspace", "Cargo.toml")
    arrays = list(re.finditer(r"(?m)^[ \t]*members[ \t]*=[ \t]*\[([^\]]*)\]", code[start:end]))
    if len(arrays) != 1 or re.fullmatch(r"[\s,]*", arrays[0].group(1)) is None:
        raise ContractError("Cargo.toml: expected one flat workspace member inventory")
    array_start = start + arrays[0].start(1)
    array_end = start + arrays[0].end(1)
    members = []
    for first, last in strings:
        if array_start <= first < last <= array_end:
            match = re.fullmatch(r"([\"'])(crates/[A-Za-z0-9_-]+)\1", text[first:last])
            if match is None:
                raise ContractError("Cargo.toml: unsupported workspace member path")
            members.append(match.group(2))
    if not members or len(members) != len(set(members)):
        raise ContractError("Cargo.toml: missing/duplicate workspace members")
    for member in [".", *members]:
        path = "Cargo.toml" if member == "." else member + "/Cargo.toml"
        package, _, start, end = _toml_table(read(path), "package", path)
        if member == "crates/workspace-hack":
            continue
        assignments = re.findall(r"(?m)^[ \t]*version[^\r\n]*", package[start:end])
        if (
            len(assignments) != 1
            or re.fullmatch(r"[ \t]*version\.workspace[ \t]*=[ \t]*true[ \t]*", assignments[0]) is None
        ):
            raise ContractError(f"{path}: Core crates must inherit version.workspace = true")


def render(read, paths):
    """Return exact updated texts, validating every location before any write."""
    version = _version(_json(read(MANIFEST), MANIFEST).get("."), MANIFEST)
    _registrations(read)
    _workspace_inheritance(read)
    known = set(TOML_FIELDS) | set(GENERIC_FILES)
    for path in paths:
        if path in MARKER_TEMPLATES:
            continue
        try:
            text = read(path)
        except UnicodeDecodeError:
            continue
        if MARKER_COMMENT.search(text) and path not in known:
            raise ContractError(f"{path}: unregistered release marker")
        if path.startswith("pkg/") and path.endswith("/pyproject.toml") and path not in TOML_FIELDS:
            raise ContractError(f"{path}: unclassified package version")
    output = {}
    for path, section in TOML_FIELDS.items():
        output[path] = _toml_version(read(path), section, path, version)
    for path, count in GENERIC_FILES.items():
        if path.startswith("skills/"):
            _skill_marker(read(path), path)
        elif path.endswith("SKILL.md"):
            _skill_marker(read(path), path, ("version",))
        elif path.endswith((".py", ".pyi")):
            _python_marker(read(path), path)
        output[path] = _markers(read(path), path, version, count)
    for path, fields in JSON_FIELDS.items():
        data = _json(read(path), path)
        if path == "server.json":
            if data.get("name") != "io.github.dcc-mcp/dcc-mcp-core":
                raise ContractError("server.json: unexpected server identity")
            packages = data.get("packages")
            if (
                not isinstance(packages, list)
                or len(packages) != 1
                or not isinstance(packages[0], dict)
                or packages[0].get("identifier") != "dcc-mcp-server"
                or packages[0].get("registryType") != "pypi"
            ):
                raise ContractError("server.json: classify the changed package set before releasing")
        if path == "admin-ui/package-lock.json":
            packages = data.get("packages")
            root_package = packages.get("") if isinstance(packages, dict) else None
            if not isinstance(root_package, dict) or root_package.get("name") != "dcc-mcp-core-admin-ui":
                raise ContractError(f"{path}: unexpected lock root identity")
        if path.startswith("admin-ui/") and data.get("name") != "dcc-mcp-core-admin-ui":
            raise ContractError(f"{path}: unexpected package identity")
        for field in fields:
            cursor, key = _json_slot(data, field, path)
            cursor[key] = version
        output[path] = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    data = _json(read(SEEDS), SEEDS)
    if (
        data.get("schema") != "skills-corpus-v2"
        or data.get("roots") != ["skills", "examples/skills", "python/dcc_mcp_core/skills", "tests/fixtures/skills"]
        or not isinstance(data.get("skills"), list)
        or not all(isinstance(skill, dict) for skill in data["skills"])
    ):
        raise ContractError(f"{SEEDS}: unexpected seed schema")
    # Production SkillMetadata flattens metadata.dcc-mcp.version and also
    # projects it to the typed version field. Only these two copies change.
    for source in GENERIC_FILES:
        if not source.startswith("skills/"):
            continue
        skill_path = source.rsplit("/", 1)[0]
        skills = [skill for skill in data["skills"] if skill.get("skill_path") == skill_path]
        if len(skills) != 1 or skills[0].get("name") != skill_path.split("/")[-1]:
            raise ContractError(f"{SEEDS}: missing/duplicate seed {skill_path}")
        skill = skills[0]
        for field in (("version",), ("metadata", "dcc-mcp.version")):
            cursor, key = _json_slot(skill, field, SEEDS + " " + skill_path)
            cursor[key] = version
    output[SEEDS] = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    return output


def _reader(root, ref=None):
    def read(path):
        if ref is not None:
            return git(root, "show", ref + ":" + path).decode("utf-8")
        candidate = root / path
        for part in (candidate, *candidate.parents):
            if part == root:
                break
            info = part.lstat()
            if stat.S_ISLNK(info.st_mode) or bool(
                getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
            ):
                raise ContractError(f"{path}: release projection cannot use a symlink")
        return candidate.read_bytes().decode("utf-8")

    return read


def _verify_modes(root, ref, committed):
    """Reject any file type or executable-mode change in a version projection."""
    commands = [("diff", "--raw", "--no-ext-diff", "--no-renames", ref)]
    if committed:
        commands[0] += ("HEAD",)
    else:
        commands.append(("diff", "--raw", "--no-ext-diff", "--no-renames", "--cached", ref))
    for command in commands:
        raw = git(root, *command, "--", *sorted(OUTPUTS)).decode("utf-8")
        for line in raw.splitlines():
            modes = line.split()[:2]
            if len(modes) == 2 and modes[0].lstrip(":") != modes[1]:
                raise ContractError("release projection file mode/type changed: " + line)


def run(root, command, expected_parent=None):
    """Check, sync or verify version-only changes against immutable Git blobs."""
    ref = None
    if command == "verify-diff":
        ref = "HEAD"
    elif command == "verify-commit":
        parents = git(root, "rev-list", "--parents", "-n", "1", "HEAD").decode().split()
        if not expected_parent or len(parents) != 2 or parents[1] != expected_parent:
            raise ContractError("projection commit must have the exact original head as its only parent")
        ref = expected_parent
    paths = (
        (git(root, "ls-tree", "-rz", "--name-only", ref) if ref else git(root, "ls-files", "-z", "--cached"))
        .decode()
        .split("\0")
    )
    if ref is not None:
        _verify_modes(root, ref, command == "verify-commit")
    read = _reader(root, ref)
    expected = render(read, [path for path in paths if path])
    current = _reader(root, "HEAD" if command == "verify-commit" else None)
    changed = [path for path, text in expected.items() if current(path) != text]
    if command == "sync":
        for path in changed:
            (root / path).write_bytes(expected[path].encode("utf-8"))
    elif changed:
        raise ContractError("Core release projection drift: " + ", ".join(sorted(changed)))
    return changed


def main():
    """Provide one dependency-free entry point for developer and trusted CI use."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "sync", "verify-diff", "verify-commit"))
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--expected-parent")
    args = parser.parse_args()
    try:
        changed = run(args.root.resolve(), args.command, args.expected_parent)
    except (ContractError, OSError, subprocess.SubprocessError) as error:
        print(f"Core version contract: {error}", file=sys.stderr)
        return 1
    print(f"Core version contract: {args.command} ({len(changed)} changed files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
