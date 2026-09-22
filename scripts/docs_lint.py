#!/usr/bin/env python3
"""docs_lint.py -- mechanical documentation contract checker.

Design goal: a daily documentation patrol must be *mechanical*. Rules that
require taste ("this paragraph is unclear") produce noise and get ignored;
rules that are checkable produce a diff a reviewer can accept or reject.

Four rule families, mapped to the reported symptoms:

  A. structure  -- heading level jumps, duplicate sibling headings, multiple
     H1 in a body file, broken TOC anchors, unbalanced code fences, relative
     links pointing at missing files, missing trailing newline.
  B. emoji      -- emoji in headings (error), emoji density in body (warning).
     Pictographs and dingbats are counted separately: a section marker is not
     the same offence as a decorative rocket on every sentence.
  C. drift      -- documented CLI flags, repository paths, and identifiers
     that no longer exist. Opt-in via ``--check-symbols`` because it has to
     read the whole tree to build its corpus.
  D. content    -- agent-facing entry points that must teach the iteration
     playbook and name ``workflows_resume``. The coverage set lives in a
     manifest (``scripts/docs_lint_playbook_coverage.txt``), so adding an
     entry point is a one-line change and not a rule change.

Two design decisions that are easy to get wrong and expensive to re-learn:

  * Drift resolution is *filesystem first*. An early version matched
    backticked paths against the concatenated contents of the repo, which
    cannot work: ``scripts/install-cli.sh`` exists on disk but the string
    "scripts/install-cli.sh" appears in no source file. Paths are resolved
    against a real path index; only flags and identifiers go to the corpus.
  * Markdown is excluded from the drift corpus. If docs were part of the
    corpus, a document would validate its own claims -- which is precisely
    the failure mode this check exists to catch (a README advertising a tool
    count the code does not have). "Mentioned only in docs" is the signal.

Exit codes: 0 = clean, 1 = at least one finding at ``--fail-on`` severity,
2 = usage or IO failure.

Stdlib only, Python 3.7+ (organisation red line: keep 3.7 compatible).

Usage:
    python scripts/docs_lint.py .
    python scripts/docs_lint.py . --check-symbols
    python scripts/docs_lint.py . --playbook-only
    python scripts/docs_lint.py README.md docs/ --json
"""

import argparse
import contextlib
import json
import os
from pathlib import Path
import re
import sys

# Windows consoles default to a legacy codepage and choke on emoji output.
with contextlib.suppress(Exception):  # pragma: no cover - stream may not support it
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# --------------------------------------------------------------------------- #
# Emoji classification
# --------------------------------------------------------------------------- #

# True pictographs / emoji proper.
_PICTOGRAPH = (
    "\U0001f300-\U0001f5ff"
    "\U0001f600-\U0001f64f"
    "\U0001f680-\U0001f6ff"
    "\U0001f700-\U0001f77f"
    "\U0001f780-\U0001f7ff"
    "\U0001f800-\U0001f8ff"
    "\U0001f900-\U0001f9ff"
    "\U0001fa00-\U0001faff"
)
# Dingbats, arrows, misc symbols -- softer, often legitimate section markers.
_SYMBOL = "☀-➿⬀-⯿"

RE_EMOJI = re.compile("[" + _PICTOGRAPH + "]")
RE_SYMBOL = re.compile("[" + _SYMBOL + "]")
RE_KEYCAP = re.compile("[0-9#*]️")

# --------------------------------------------------------------------------- #
# Markdown parsing
# --------------------------------------------------------------------------- #

RE_ATX = re.compile(r"^(#{1,6})(\s+)(.*?)\s*$")

# A fenced code block opens on a run of three or more backticks or tildes (at
# most three spaces of indentation) and closes on a run of the *same* character
# that is at least as long as the opener and carries nothing but whitespace
# after it. Capturing the marker -- rather than asking "is this a fence line?"
# -- is what makes mixed and nested markers behave: a ``` run inside a ~~~~
# block is content, and a ~~ run cannot close a ``` block.
RE_FENCE = re.compile(r"^ {0,3}(?P<marker>`{3,}|~{3,})(?P<rest>.*)$")
RE_LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
RE_ANCHOR = re.compile(r"\[[^\]]*\]\(#([^)\s]+)\)")
RE_BACKTICK = re.compile(r"`([^`\n]+)`")
RE_URI_SCHEME = re.compile(r"^[A-Za-z][A-Za-z0-9+.\-]*:")

# A backticked span is code; emoji inside code samples are content, not style.
RE_CODE_SPAN = re.compile(r"`[^`\n]*`")


def fence_step(line, open_marker):
    """Advance the fenced-code state by one Markdown ``line``.

    Return ``(open_marker, is_fence_line)``: the marker of the fence that is
    open *after* ``line`` (``None`` outside a fence) and whether ``line`` was a
    fence delimiter. Both delimiters are reported, so callers skip the closing
    one too -- a closing delimiter is no more body text than the opener is.
    """
    match = RE_FENCE.match(line)
    if match is None:
        return open_marker, False
    marker = match.group("marker")
    rest = match.group("rest")
    if open_marker is None:
        # A backtick fence's info string may not contain a backtick
        # (CommonMark); such a line is prose, not an opener. Tildes may.
        if marker[0] == "`" and "`" in rest:
            return None, False
        return marker, True
    if marker[0] != open_marker[0] or len(marker) < len(open_marker):
        return open_marker, False
    if rest.strip():
        return open_marker, False
    return None, True


DEFAULT_EXCLUDE_DIRS = (
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "target",
    "dist",
    "build",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".vscode",
    "site-packages",
    ".tox",
    ".next",
    ".nuxt",
)

MD_SUFFIXES = (".md", ".markdown", ".mdx")

# Text-like extensions whose *contents* form the drift corpus. Markdown is
# deliberately absent -- see the module docstring.
CORPUS_SUFFIXES = frozenset(
    {
        ".py",
        ".rs",
        ".ts",
        ".js",
        ".tsx",
        ".jsx",
        ".toml",
        ".yaml",
        ".yml",
        ".json",
        ".sh",
        ".bash",
        ".c",
        ".cpp",
        ".h",
        ".hpp",
        ".go",
        ".java",
        ".cs",
        ".txt",
        ".cfg",
        ".ini",
    }
)

# Changelogs describe features that were added *and* removed; a drift finding
# there is history, not staleness.
DRIFT_EXEMPT_NAMES = frozenset({"changelog.md", "changelog"})

# Agent-facing files that must teach the iteration playbook. The list itself is
# data, not code: it lives beside the linter so widening coverage never means
# editing a rule.
PLAYBOOK_MANIFEST_DEFAULT = "scripts/docs_lint_playbook_coverage.txt"
PLAYBOOK_MARKER_DEFAULT = "iteration playbook"
PLAYBOOK_RESUME_SYMBOL = "workflows_resume"

RE_PLAYBOOK_SEPARATOR = re.compile(r"[\s\-_]+")

# --------------------------------------------------------------------------- #
# Drift token shapes
# --------------------------------------------------------------------------- #

RE_CLI_FLAG = re.compile(r"^(--?[A-Za-z][\w-]{2,})$")
RE_CODE_PATH = re.compile(r"^([\w.\-]+(?:/[\w.\-]+)+)$")
RE_SNAKE_CALL = re.compile(r"^([a-z][a-z0-9_]{4,})(?:\(\))?$")


def _slug(text):
    """Return a GitHub-flavoured heading slug, good enough for anchor matching."""
    value = text.strip().lower()
    value = re.sub(r"`([^`]*)`", r"\1", value)
    value = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", value)
    value = re.sub(r"[^\w\-一-鿿 ]", "", value)
    return value.replace(" ", "-")


# --------------------------------------------------------------------------- #
# Repository index
# --------------------------------------------------------------------------- #


class RepoIndex:
    """Path index + optional text corpus used to resolve documented symbols.

    The index answers "does this path exist?" from the filesystem, and
    "does this identifier exist anywhere?" from the concatenated contents of
    non-markdown text files.
    """

    def __init__(self, roots, exclude_dirs):
        """Walk ``roots`` once, collecting relative paths and directory names."""
        self.roots = [Path(root) for root in roots]
        self.exclude_dirs = set(exclude_dirs)
        self.files = set()
        self.dirs = set()
        self.toplevel = set()
        self.stems = set()
        self._corpus = None
        for root in self.roots:
            self._index(root)

    def _relative(self, path, root):
        """Return a posix relative path, rooted at the walk root when possible."""
        try:
            return path.resolve().relative_to(Path(root).resolve()).as_posix()
        except ValueError:
            return path.as_posix()

    def _index(self, root):
        path = Path(root)
        if path.is_file():
            self.files.add(path.as_posix())
            self.stems.add(path.stem)
            self.toplevel.add(path.name)
            return
        for dirpath, dirnames, filenames in os.walk(str(path)):
            dirnames[:] = sorted(d for d in dirnames if d not in self.exclude_dirs)
            for name in dirnames:
                self.dirs.add(self._relative(Path(dirpath) / name, path))
            for name in filenames:
                full = Path(dirpath) / name
                rel = self._relative(full, path)
                self.files.add(rel)
                self.stems.add(full.stem)
                self.toplevel.add(rel.split("/")[0])
        for known in self.dirs:
            self.toplevel.add(known.split("/")[0])

    # -- resolution ---------------------------------------------------------- #

    def has_path(self, token):
        """Return True when ``token`` names a file or directory in the repo."""
        token = token.strip("/")
        if not token:
            return False
        if token in self.files or token in self.dirs:
            return True
        if any(root.joinpath(token).exists() for root in self.roots):
            return True
        suffix = "/" + token
        return any(p.endswith(suffix) for p in self.files) or any(d.endswith(suffix) for d in self.dirs)

    def has_suffix_path(self, token):
        """Return True when ``token`` matches the tail of some repo path."""
        token = token.strip("/")
        if not token:
            return False
        suffix = "/" + token
        return any(p == token or p.endswith(suffix) for p in self.files) or any(
            d == token or d.endswith(suffix) for d in self.dirs
        )

    def is_path_candidate(self, token):
        """Return True when a slash-containing token is meant as a repo path.

        ``a/b`` is ambiguous: it may be a path, a cargo feature, or a
        slash-separated enumeration ("save/load/resume"). Only treat it as a
        path when the last segment carries a file extension, or when the first
        segment is a real top-level entry -- otherwise the rule fires on every
        ``dcc-mcp-workflow/job-persist-sqlite`` style feature name.
        """
        tail = token.rpartition("/")[2]
        if "." in tail:
            return True
        return token.partition("/")[0] in self.toplevel

    def resolve_link(self, target, base_dir):
        """Return True when a Markdown link target resolves on disk.

        Handles the three shapes a site generator produces: ordinary relative
        paths, site-root-absolute paths (``/guide/skills``), and extensionless
        clean URLs (``../zh/guide/skills``).
        """
        if not target:
            return True
        if target.startswith(("http://", "https://", "mailto:", "#", "<")):
            return True
        # Any other explicit scheme (mention://, gh://, ...) is not a file path.
        if RE_URI_SCHEME.match(target):
            return True
        target = target.split("#", 1)[0]
        if not target:
            return True

        candidates = [target.lstrip("/")] if target.startswith("/") else [(Path(base_dir) / target).as_posix()]

        expanded = []
        for candidate in candidates:
            normalized = candidate.replace("\\", "/")
            expanded.extend(
                [
                    normalized,
                    normalized + ".md",
                    normalized.rstrip("/") + "/index.md",
                ]
            )

        for candidate in expanded:
            if Path(candidate).exists():
                return True
            if self.has_suffix_path(candidate):
                return True
        return False

    # -- corpus -------------------------------------------------------------- #

    def _iter_corpus_files(self):
        for root in self.roots:
            path = Path(root)
            if path.is_file():
                yield path
                continue
            for dirpath, dirnames, filenames in os.walk(str(path)):
                dirnames[:] = sorted(d for d in dirnames if d not in self.exclude_dirs)
                for name in sorted(filenames):
                    if Path(name).suffix.lower() in CORPUS_SUFFIXES:
                        yield Path(dirpath) / name

    def corpus(self):
        """Return the concatenated contents of non-markdown text files."""
        if self._corpus is None:
            chunks = []
            for candidate in self._iter_corpus_files():
                try:
                    chunks.append(candidate.read_text(encoding="utf-8", errors="ignore"))
                except OSError:
                    continue
            self._corpus = "\n".join(chunks)
        return self._corpus

    def has_identifier(self, token):
        """Return True when a bare identifier exists in the corpus or as a file stem."""
        if token in self.stems:
            return True
        return token in self.corpus()

    def has_flag(self, token):
        """Return True when a documented CLI flag exists in the corpus.

        Rust and Python CLI layers declare ``--ws-port`` as ``ws_port``, so the
        literal hyphenated flag appears in no source file. Compare every
        spelling before declaring the documentation stale.
        """
        stripped = token.lstrip("-")
        for variant in (token, token.replace("-", "_"), stripped, stripped.replace("-", "_")):
            if variant in self.corpus():
                return True
        return False


# --------------------------------------------------------------------------- #
# Rule family A -- structure
# --------------------------------------------------------------------------- #


def check_structure(text):
    """Return structure findings: headings, fences, anchors, whitespace."""
    findings = []
    lines = text.split("\n")

    open_marker = None
    headings = []  # (line_no, level, title)
    h1_count = 0
    # Duplicate detection is scoped by ancestor path, not by whole file: a
    # changelog repeats "Added" under every release heading, and that is
    # correct. Only siblings colliding is a defect.
    stack = []  # open ancestor headings as (level, slug)
    seen_keys = {}

    for i, line in enumerate(lines, start=1):
        open_marker, is_fence_line = fence_step(line, open_marker)
        if open_marker is not None or is_fence_line:
            continue

        match = RE_ATX.match(line.rstrip())
        if not match:
            continue
        level = len(match.group(1))
        title = match.group(3)
        if level == 1:
            h1_count += 1
        slug = _slug(title)

        while stack and stack[-1][0] >= level:
            stack.pop()
        key = (level, slug, tuple(s for _, s in stack))
        if key in seen_keys:
            findings.append(
                {
                    "rule": "structure/duplicate-heading",
                    "severity": "warning",
                    "line": i,
                    "message": f"duplicate sibling heading, first seen at line {seen_keys[key]}: {title!r}",
                }
            )
        else:
            seen_keys[key] = i
        stack.append((level, slug))
        headings.append((i, level, title))

        if len(headings) >= 2:
            prev_line, prev_level, _ = headings[-2]
            if level > prev_level + 1:
                findings.append(
                    {
                        "rule": "structure/heading-level-jump",
                        "severity": "error",
                        "line": i,
                        "message": f"heading jumps from H{prev_level} (line {prev_line}) to H{level}",
                    }
                )

    if open_marker is not None:
        findings.append(
            {
                "rule": "structure/unclosed-code-fence",
                "severity": "error",
                "line": len(lines),
                "message": "code fence opened but never closed",
            }
        )

    if h1_count > 1:
        findings.append(
            {
                "rule": "structure/multiple-h1",
                "severity": "warning",
                "line": headings[0][0] if headings else 1,
                "message": f"{h1_count} level-1 headings in one document; a doc body "
                f"should start at H2 unless the H1 is the document title",
            }
        )

    findings.extend(_check_anchors(lines, headings))

    if text and not text.endswith("\n"):
        findings.append(
            {
                "rule": "structure/missing-trailing-newline",
                "severity": "warning",
                "line": len(lines),
                "message": "file does not end with a newline",
            }
        )
    if "\t" in text:
        findings.append(
            {
                "rule": "structure/tab-indentation",
                "severity": "warning",
                "line": 1,
                "message": "tab characters in a Markdown source; prefer spaces",
            }
        )
    return findings


def _check_anchors(lines, headings):
    """Return broken-TOC-anchor findings.

    Re-walk the file with a local fence marker: the main loop leaves its marker
    set on an unclosed fence, and reusing it silently skips every anchor after
    that.
    """
    findings = []
    known = set(_slug(title) for _, _, title in headings)
    open_marker = None
    for i, line in enumerate(lines, start=1):
        open_marker, is_fence_line = fence_step(line, open_marker)
        if open_marker is not None or is_fence_line:
            continue
        for anchor in RE_ANCHOR.findall(line):
            if known and anchor.lower() not in known:
                findings.append(
                    {
                        "rule": "structure/broken-toc-anchor",
                        "severity": "error",
                        "line": i,
                        "message": f"table-of-contents anchor #{anchor} matches no heading",
                    }
                )
    return findings


def check_links(text, base_dir, index):
    """Return findings for relative link targets that do not resolve."""
    findings = []
    for i, line in enumerate(text.split("\n"), start=1):
        for target in RE_LINK.findall(line):
            if not index.resolve_link(target, base_dir):
                findings.append(
                    {
                        "rule": "drift/broken-relative-link",
                        "severity": "error",
                        "line": i,
                        "message": f"link target does not exist: {target}",
                    }
                )
    return findings


# --------------------------------------------------------------------------- #
# Rule family B -- emoji
# --------------------------------------------------------------------------- #


def check_emoji(text, max_density):
    """Return emoji findings: headings must be plain, bodies must stay sparse."""
    findings = []
    body_lines = 0
    emoji_lines = 0
    pictographs = 0
    symbols = 0

    open_marker = None
    for i, line in enumerate(text.split("\n"), start=1):
        open_marker, is_fence_line = fence_step(line, open_marker)
        if open_marker is not None or is_fence_line:
            continue

        is_heading = bool(RE_ATX.match(line.rstrip()))
        content = RE_CODE_SPAN.sub("", line)
        n_pic = len(RE_EMOJI.findall(content)) + len(RE_KEYCAP.findall(content))
        n_sym = len(RE_SYMBOL.findall(content))

        if n_pic or n_sym:
            pictographs += n_pic
            symbols += n_sym
            if is_heading and n_pic:
                findings.append(
                    {
                        "rule": "emoji/emoji-in-heading",
                        "severity": "error",
                        "line": i,
                        "message": f"heading contains {n_pic} emoji; headings are the "
                        f"navigation surface, keep them plain text",
                    }
                )

        if content.strip():
            body_lines += 1
            if n_pic:
                emoji_lines += 1

    density = (float(emoji_lines) / body_lines) if body_lines else 0.0
    if body_lines >= 20 and density > max_density:
        findings.append(
            {
                "rule": "emoji/emoji-density",
                "severity": "warning",
                "line": 1,
                "message": f"emoji on {density * 100:.1f}% of {body_lines} body lines "
                f"(threshold {max_density * 100:.1f}%); "
                f"{pictographs} pictographs, {symbols} symbols",
            }
        )
    return findings


# --------------------------------------------------------------------------- #
# Rule family C -- drift (opt-in)
# --------------------------------------------------------------------------- #


def check_symbols(text, index):
    """Return drift findings for documented flags, paths, and identifiers."""
    findings = []
    open_marker = None
    for i, line in enumerate(text.split("\n"), start=1):
        open_marker, is_fence_line = fence_step(line, open_marker)
        if open_marker is not None or is_fence_line:
            continue
        for token in RE_BACKTICK.findall(line):
            token = token.strip()
            if not token or " " in token:
                continue
            message = _drift_message(token, index)
            if message:
                findings.append(
                    {
                        "rule": "drift/documented-symbol-missing",
                        "severity": "warning",
                        "line": i,
                        "message": message,
                    }
                )
    return findings


def _drift_message(token, index):
    """Return a drift message for ``token``, or None when it resolves."""
    if RE_CLI_FLAG.match(token):
        if index.has_flag(token):
            return None
    elif RE_CODE_PATH.match(token):
        if index.has_path(token):
            return None
        if not index.is_path_candidate(token):
            return None
    elif RE_SNAKE_CALL.match(token):
        needle = token.rstrip("()")
        if index.has_identifier(needle):
            return None
    else:
        return None
    return f"documented `{token}` is not found anywhere in the repository (stale doc or removed feature)"


# --------------------------------------------------------------------------- #
# Rule family D -- content
# --------------------------------------------------------------------------- #

# The playbook -- reuse a materialized script, iterate on params only, recover
# with `workflows_resume` -- is a contract, not a style preference: an
# agent-facing entry point that omits it regenerates code on every turn. Files
# are named in a manifest rather than hardcoded here because the set of
# agent-facing entry points grows with the product, and a rule that has to be
# edited to gain coverage is a rule nobody extends.


def load_playbook_manifest(path):
    """Return ``(entries, error)`` for a playbook coverage manifest.

    ``entries`` preserves file order with blank lines and ``#`` comments
    dropped, so the manifest stays a readable diff and adding an entry point
    is a one-line change. ``error`` is ``None`` when the file was readable.
    """
    try:
        text = Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        return [], str(exc)
    entries = []
    for line in text.split("\n"):
        entry = line.strip()
        if entry and not entry.startswith("#"):
            entries.append(entry)
    return entries, None


def playbook_matcher(marker):
    r"""Return a compiled regex matching ``marker`` across case and spacing.

    "Iteration Playbook", "iteration-playbook", and a line-wrapped
    "iteration\nplaybook" are one marker. An author should not have to
    memorise the canonical spelling to satisfy a mechanical check.
    """
    words = [word for word in RE_PLAYBOOK_SEPARATOR.split(marker.strip()) if word]
    if not words:
        return re.compile(r"(?!)")
    return re.compile(r"[\s\-_]+".join(re.escape(word) for word in words), re.IGNORECASE)


def resume_matcher(symbol):
    r"""Return a compiled regex matching the resume symbol as a whole word.

    The symbol is matched on word boundaries where ``_`` counts as a word
    character: snake_case is one identifier, so ``workflows_resume_old`` and
    ``not_workflows_resume`` are *not* the resume tool and must not satisfy the
    rule -- a false negative here is exactly the failure the gate exists to
    catch. The characters real docs wrap the symbol in (backticks, quotes,
    dots, parens, slashes) are not word characters, so ``` `workflows_resume`
    ```, ``"workflows_resume"`` and ``workflows_resume.`` still count.

    The boundary class is deliberately ASCII-only and not ``\w``: in Python 3
    ``\w`` is Unicode-aware and CJK counts as word characters, so a bilingual
    sentence such as ``请调用workflows_resume来恢复`` -- which names the tool
    exactly as intended -- would be reported as missing it. Rejecting an
    exotic ``\u03b1workflows_resume`` costs less than rejecting correct Chinese.
    """
    symbol = symbol.strip()
    if not symbol:
        return re.compile(r"(?!)")
    return re.compile(r"(?<![A-Za-z0-9_])" + re.escape(symbol) + r"(?![A-Za-z0-9_])")


def check_playbook_coverage(root, entries, marker=PLAYBOOK_MARKER_DEFAULT, resume_symbol=PLAYBOOK_RESUME_SYMBOL):
    """Return ``{path: [finding, ...]}`` for manifest entries that fall short.

    Each listed file has to do two things: point at the iteration playbook and
    name the resume tool. Teaching reuse without `workflows_resume` is the
    specific gap this rule exists to catch -- an interrupted run then gets
    restarted from scratch instead of resumed.
    """
    marker_re = playbook_matcher(marker)
    resume_re = resume_matcher(resume_symbol)
    report = {}
    for entry in entries:
        path = Path(root) / entry
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:
            report[path.as_posix()] = [
                {
                    "rule": "content/playbook-coverage-missing-file",
                    "severity": "error",
                    "line": 0,
                    "message": f"playbook manifest entry does not exist: {entry} ({exc})",
                }
            ]
            continue

        findings = []
        if not marker_re.search(text):
            findings.append(
                {
                    "rule": "content/playbook-coverage-missing-marker",
                    "severity": "error",
                    "line": 0,
                    "message": f"agent-facing file does not mention the {marker!r} "
                    f"(see docs/guide/agents-reference.md)",
                }
            )
        if not resume_re.search(text):
            findings.append(
                {
                    "rule": "content/playbook-coverage-missing-resume",
                    "severity": "error",
                    "line": 0,
                    "message": f"agent-facing file never names `{resume_symbol}`; "
                    f"an interrupted run then restarts from scratch",
                }
            )
        if findings:
            report[path.as_posix()] = findings
    return report


# --------------------------------------------------------------------------- #
# Driver
# --------------------------------------------------------------------------- #


def _iter_files(targets, exclude_dirs):
    for target in targets:
        path = Path(target)
        if path.is_file():
            yield path
            continue
        for dirpath, dirnames, filenames in os.walk(str(path)):
            dirnames[:] = sorted(d for d in dirnames if d not in exclude_dirs)
            for name in sorted(filenames):
                if name.lower().endswith(MD_SUFFIXES):
                    yield Path(dirpath) / name


def lint_file(path, args, index):
    """Return every finding for one Markdown file."""
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        return [{"rule": "io/read-error", "severity": "error", "line": 0, "message": str(exc)}]

    findings = check_structure(text)
    findings.extend(check_emoji(text, args.max_emoji_density))
    findings.extend(check_links(text, path.parent, index))
    if args.check_symbols and path.name.lower() not in DRIFT_EXEMPT_NAMES:
        findings.extend(check_symbols(text, index))
    return findings


def _is_excluded(path, patterns):
    posix = path.as_posix()
    return any(pattern in posix for pattern in patterns)


def main(argv=None):
    """Run the linter and return the process exit code."""
    parser = argparse.ArgumentParser(description="Mechanical documentation contract checker.")
    parser.add_argument("targets", nargs="+", help="Markdown files or repository roots")
    parser.add_argument("--json", action="store_true", help="emit JSON instead of a text report")
    parser.add_argument(
        "--check-symbols", action="store_true", help="also verify documented flags/paths/identifiers exist in the repo"
    )
    parser.add_argument(
        "--max-emoji-density",
        type=float,
        default=0.10,
        help="warn when emoji appear on more than this fraction of body lines",
    )
    parser.add_argument(
        "--exclude-dir", default=",".join(DEFAULT_EXCLUDE_DIRS), help="comma-separated directory names to skip"
    )
    parser.add_argument("--exclude-path", action="append", default=[], help="substring of a path to skip; repeatable")
    parser.add_argument(
        "--fail-on", choices=("error", "warning"), default="error", help="minimum severity that sets exit code 1"
    )
    parser.add_argument(
        "--playbook-manifest",
        default=None,
        help=f"agent-facing coverage manifest to check (default: {PLAYBOOK_MANIFEST_DEFAULT} when it exists)",
    )
    parser.add_argument(
        "--playbook-only",
        action="store_true",
        help="run only the playbook coverage pass; skip the per-file structure, emoji, and link rules",
    )
    args = parser.parse_args(argv)

    exclude_dirs = set(d for d in args.exclude_dir.split(",") if d)
    targets = [t for t in args.targets if Path(t).exists()]
    missing = [t for t in args.targets if not Path(t).exists()]
    if not targets:
        sys.stderr.write("no existing targets: {}\n".format(", ".join(missing)))
        return 2

    # Per-file rules need the repository index; the coverage pass reads only
    # the manifest entries, so a scoped run skips the walk entirely.
    index = None if args.playbook_only else RepoIndex(targets, exclude_dirs)

    report = {}

    # Manifest paths are resolved against the lint root, not the shell's
    # working directory: linting a subdirectory must not silently swap in a
    # coverage set that belongs to another tree.
    base = Path(targets[0]) if Path(targets[0]).is_dir() else Path()
    explicit_manifest = args.playbook_manifest is not None
    manifest_path = args.playbook_manifest if explicit_manifest else base / PLAYBOOK_MANIFEST_DEFAULT
    entries, manifest_error = load_playbook_manifest(manifest_path)
    if manifest_error:
        # A manifest named on the command line is a promise: when it cannot be
        # read the run fails instead of silently dropping the gate. The default
        # path is opportunistic -- linting a subdirectory from another working
        # directory must not start failing because the repo manifest is out of
        # reach.
        if explicit_manifest:
            report.setdefault(Path(manifest_path).as_posix(), []).append(
                {
                    "rule": "content/playbook-manifest-unreadable",
                    "severity": "error",
                    "line": 0,
                    "message": f"cannot read playbook manifest: {manifest_error}",
                }
            )
        entries = []

    if entries:
        report.update(check_playbook_coverage(base, entries))

    if not args.playbook_only:
        for path in _iter_files(targets, exclude_dirs):
            if _is_excluded(path, args.exclude_path):
                continue
            findings = lint_file(path, args, index)
            # Merge, never assign: a file listed in the manifest can also carry
            # structure/link findings, and assigning here would silently drop
            # the coverage findings recorded above -- exactly the defect this
            # rule exists to report.
            if findings:
                report.setdefault(path.as_posix(), []).extend(findings)

    if args.json:
        payload = {
            "targets": targets,
            "missing_targets": missing,
            "files_with_findings": len(report),
            "findings": [
                dict(f, file=p)
                for p, fl in sorted(report.items())
                for f in sorted(fl, key=lambda x: (x["line"], x["rule"]))
            ],
        }
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    else:
        counts = {"error": 0, "warning": 0}
        for path in sorted(report):
            print(f"\n{path}")
            for f in sorted(report[path], key=lambda x: (x["line"], x["rule"])):
                counts[f["severity"]] += 1
                print("  {:<8} {:<38} L{:<5} {}".format(f["severity"].upper(), f["rule"], f["line"], f["message"]))
        print(f"\n{len(report)} file(s), {counts['error']} error(s), {counts['warning']} warning(s)")
        for path in missing:
            print(f"skipped (not found): {path}")

    if counts_from(report, "error"):
        return 1
    if args.fail_on == "warning" and counts_from(report, "warning"):
        return 1
    return 0


def counts_from(report, severity):
    """Return the number of findings at ``severity`` across the report."""
    return sum(1 for fl in report.values() for f in fl if f["severity"] == severity)


if __name__ == "__main__":
    sys.exit(main())
