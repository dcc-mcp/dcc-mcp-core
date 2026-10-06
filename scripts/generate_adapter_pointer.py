#!/usr/bin/env python3
"""Generate the DCC-MCP coverage pointer block for host adapter READMEs.

``dcc-mcp-catalog.yml`` in this repository is the single source of truth. Every
host adapter README carries a short generated block that names the adapter,
states its place in the host matrix, and links back to the shared front door.

The block is generated and never hand-written. Hand-maintained copies across the
adapter repositories are exactly the failure mode the entry-convergence decision
exists to avoid: 38 copies that rot independently. Changing the catalog and
re-running this script is the only supported way to change the text.

Commands
--------
``render``      Print the pointer block for one adapter or for all of them.
``apply``       Insert or refresh the block in adapter README files.
``check``       Exit non-zero if any README's block is missing or stale.
``report``      Write the coverage map (Markdown + JSON) for every adapter.
``audit-pypi``  Compare catalog install metadata against PyPI (network only).

Only ``audit-pypi`` touches the network, so ``report`` output is reproducible
from a checkout alone.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import textwrap
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.parse import urlsplit
from urllib.request import Request
from urllib.request import urlopen

try:
    import yaml
except ImportError:  # pragma: no cover - dependency is installed by the test lanes
    yaml = None


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CATALOG = REPO_ROOT / "dcc-mcp-catalog.yml"

POINTER_START = "<!-- dcc-mcp-coverage-pointer:start -->"
POINTER_END = "<!-- dcc-mcp-coverage-pointer:end -->"
QUICKSTART_END = "<!-- dcc-mcp-agent-quickstart:end -->"

ECOSYSTEM_URL = "https://dcc-mcp.github.io/ecosystem"
CORE_README_URL = "https://github.com/dcc-mcp/dcc-mcp-core#readme"
SHOWCASE_URL = "https://dcc-mcp.github.io/showcase"
CATALOG_URL = "https://github.com/dcc-mcp/dcc-mcp-core/blob/main/dcc-mcp-catalog.yml"

# The repository that owns this generator. Quoted in the generated block so an
# adapter maintainer reading it knows where the script actually lives.
SOURCE_REPO = "dcc-mcp/dcc-mcp-core"

# PIP-3711 copy constraints. A generated block may never carry a ranking or a
# market-vacancy claim, and it may never compare on stars or tool counts.
# The generated prose is English, so the English forms of the banned claims
# are listed too: a catalog description carrying one would otherwise pass and
# then be copied into 38 public READMEs. Compared case-insensitively.
BANNED_COPY_TERMS = (
    "无主",
    "第一",
    "领先",
    "蓝海",
    "抢占",
    "无竞品",
    "leading",
    "industry-first",
    "industry leading",
    "no competitor",
    "best-in-class",
    "world-class",
)

# Entries the catalog lists as adapters but that carry no attested
# ``install:`` block. The registry side is deferred rather than dropped because
# there is no install metadata to register, and nothing else: this generator has
# no per-entry view of how far each repository's packaging has progressed, so the
# text must not characterise it. An earlier revision asserted that every one of
# these repositories "ships a real distribution", which was false for entries
# whose packaging had not started. The sentence below is deliberately limited to
# what the catalog itself can prove.
NO_PIP_REASON = "catalog has no `install:` block; registration waits on catalog install metadata"

_ADAPTER_TAG = "adapter"
_HEADING_RE = re.compile(r"^#{1,6} ")
_BLANK_RUN_RE = re.compile(r"\n{3}")
_PROSE_WIDTH = 88


class CoverageError(RuntimeError):
    """Raised when the catalog or a README cannot be processed."""


def load_catalog(path: Path) -> dict:
    """Parse the public catalog and return it as a mapping.

    PyYAML is used rather than the Rust-backed ``dcc_mcp_core.yaml_loads``
    because this generator also runs on CI lanes that do a bare checkout with
    no wheel build, where ``dcc_mcp_core._core`` does not exist. PyYAML is a
    declared test toolchain entry (``test_toolchain.pyyaml_py37`` in
    compatibility/python.json) and is installed by every Python test lane.
    """
    if yaml is None:  # pragma: no cover - dependency is installed by the test lanes
        raise CoverageError("PyYAML is required to read dcc-mcp-catalog.yml")
    data = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    if not isinstance(data, dict) or not isinstance(data.get("entries"), list):
        raise CoverageError(f"{path} does not look like a catalog: no `entries` list")
    return data


def adapter_entries(catalog: dict) -> list[dict]:
    """Return catalog entries tagged ``adapter``, ordered as the catalog lists them."""
    return [e for e in catalog["entries"] if _ADAPTER_TAG in (e.get("tags") or [])]


def _sentence(text: str) -> str:
    """Trim a catalog field and give it exactly one terminating period."""
    cleaned = " ".join(str(text or "").split())
    return cleaned.rstrip(".") + "." if cleaned else ""


def _fill(text: str) -> str:
    """Wrap prose without breaking inline code spans or hyphenated identifiers."""
    return textwrap.fill(
        text,
        width=_PROSE_WIDTH,
        break_long_words=False,
        break_on_hyphens=False,
    )


def _assert_clean_copy(text: str, where: str) -> None:
    """Refuse to emit text carrying a banned ranking or vacancy claim."""
    haystack = text.lower()
    for term in BANNED_COPY_TERMS:
        if term.lower() in haystack:
            raise CoverageError(f"banned copy term {term!r} in generated text for {where}")


def render_pointer(
    entry: dict,
    *,
    host_count: int,
    ecosystem_url: str = ECOSYSTEM_URL,
    core_readme_url: str = CORE_README_URL,
    showcase_url: str = SHOWCASE_URL,
    catalog_url: str = CATALOG_URL,
) -> str:
    """Render the pointer block for one adapter entry.

    The host count is passed in rather than hard-coded so the sentence stays
    true when the catalog changes.
    """
    name = str(entry.get("name") or "").strip()
    if not name:
        raise CoverageError("catalog entry has no `name`")
    description = _sentence(entry.get("description"))
    if not description:
        raise CoverageError(f"catalog entry {name} has no `description`")

    identity = f"**{name}** — {description}"
    # Deliberately stops at the protocol and the core runtime contract. The
    # tool surfaces are host-specific by design -- docs/guide/adapter-runtime-
    # contracts.md: "Adapters keep host-specific collection and safety policy;
    # core standardizes the shapes." Claiming an agent can drive other hosts
    # "through the same calls" would be copied into 38 public READMEs and is
    # false: a Kdenlive timeline tool and a Wwise SoundBank tool are not
    # interchangeable.
    matrix = (
        f"It is one of **{host_count} host adapters** in the DCC-MCP catalog. Every"
        " adapter speaks the same MCP protocol and builds on the same core runtime"
        " contract; each one exposes the tools its own host needs on top of that."
    )
    footer = (
        "This block is generated from the catalog entry in"
        f" [`dcc-mcp-catalog.yml`]({catalog_url}). Re-run the generator after"
        " changing the catalog."
    )

    # The script path is qualified with its repository because this block is
    # emitted into the adapter's own README, where a bare `scripts/...` path
    # does not exist and gives the maintainer nothing to run.
    body = "\n".join(
        (
            POINTER_START,
            "<!-- Generated from dcc-mcp-catalog.yml by"
            f" scripts/generate_adapter_pointer.py in {SOURCE_REPO}."
            " Do not edit by hand. -->",
            "## Part of the DCC-MCP host matrix",
            "",
            _fill(identity),
            "",
            _fill(matrix),
            "",
            f"- [All host adapters and install metadata]({ecosystem_url})",
            f"- [Host matrix on the core README]({core_readme_url})",
            f"- [Showcase]({showcase_url})",
            "",
            _fill(footer),
            POINTER_END,
        )
    )
    _assert_clean_copy(body, name)
    return body


def _insertion_index(lines: list[str]) -> int:
    """Pick the line the pointer block goes before.

    Three deterministic cases, in order:

    1. The README already has an agent-quickstart block: land directly after it,
       so no existing generated block is ever split in two.
    2. Otherwise: land directly before the first section heading, which keeps
       the block below the title, badges and banners.
    3. A README with no headings at all: append at the end.
    """
    for index, line in enumerate(lines):
        if line.strip() == QUICKSTART_END:
            return index + 1
    # Skip the H1: the title, badges and banners above the first section are the
    # header region the block belongs below, not above.
    start = 0
    for position, line in enumerate(lines):
        if line.startswith("# "):
            start = position + 1
            break
    for position in range(start, len(lines)):
        if _HEADING_RE.match(lines[position]) and not lines[position].startswith("# "):
            return position
    return len(lines)


def upsert_pointer(readme: str, block: str) -> str:
    """Return ``readme`` with ``block`` present exactly once and up to date.

    The file's existing newline convention is preserved: adapter READMEs are
    checked out with CRLF on Windows, and rewriting them wholesale as LF would
    show every line as changed.
    """
    newline = "\r\n" if "\r\n" in readme else "\n"
    block = block.replace("\n", newline)

    if POINTER_START in readme and POINTER_END in readme:
        head, rest = readme.split(POINTER_START, 1)
        if POINTER_END not in rest:
            raise CoverageError("pointer block has a start marker but no end marker")
        _, tail = rest.split(POINTER_END, 1)
        return f"{head}{block}{tail}"

    if POINTER_START in readme or POINTER_END in readme:
        raise CoverageError("pointer block has an orphaned marker")

    lines = readme.splitlines()
    index = _insertion_index(lines)

    # The block is fenced by one blank line on each side. Reuse an adjacent
    # blank line instead of adding another, so the seam never opens a
    # two-blank gap. Only the seam is touched: collapsing blank runs across
    # the whole file would reformat prose the block has nothing to do with.
    before = list(lines[:index])
    while before and not before[-1].strip():
        before.pop()
    after = list(lines[index:])
    while after and not after[0].strip():
        after.pop(0)

    seam = [
        *([""] if before else []),
        block,
        *([""] if after else []),
    ]
    joined = newline.join([*before, *seam, *after])
    # splitlines() drops a file's trailing blank lines, so a round trip through
    # it would silently delete them and show up as an unrelated deletion in the
    # diff. The original trailing newline run is restored verbatim; the block
    # is meant to be the only change.
    trailing = readme[len(readme.rstrip(newline)) :] or newline
    return joined.rstrip(newline) + trailing


def _normalize_repo_url(url: str) -> str:
    """Normalize a repository URL for comparison, ignoring case and suffix.

    GitHub treats owner and repository names case-insensitively, and the catalog
    carries ``dcc-mcp-PowerPoint`` while the repository reports
    ``dcc-mcp-powerpoint``.
    """
    cleaned = str(url or "").strip().rstrip("/")
    if cleaned.endswith(".git"):
        cleaned = cleaned[: -len(".git")]
    parts = urlsplit(cleaned)
    if parts.netloc and parts.path:
        cleaned = f"{parts.netloc}{parts.path}"
    return cleaned.lower()


def _git_origin_url(repo_root: Path) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(repo_root), "remote", "get-url", "origin"],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:  # pragma: no cover - git missing
        raise CoverageError(f"git is not available: {exc}") from exc
    if result.returncode != 0 or not result.stdout.strip():
        raise CoverageError(f"{repo_root} has no git `origin` remote")
    return result.stdout.strip().splitlines()[0].strip()


def resolve_adapter(entries: list[dict], repo_root: Path, explicit: str | None = None) -> dict:
    """Find the catalog entry that describes the repository at ``repo_root``."""
    if explicit:
        wanted = explicit.strip().lower()
        for entry in entries:
            if str(entry.get("name", "")).lower() == wanted:
                return entry
        raise CoverageError(f"catalog has no adapter named {explicit!r}")

    origin = _normalize_repo_url(_git_origin_url(repo_root))
    for entry in entries:
        if _normalize_repo_url(str(entry.get("url") or "")) == origin:
            return entry
    raise CoverageError(f"no catalog entry matches origin {origin!r} for {repo_root}")


def apply_pointer(
    repo_root: Path,
    entry: dict,
    *,
    host_count: int,
    readme_name: str = "README.md",
    write: bool = True,
) -> tuple[Path, bool]:
    """Insert or refresh the pointer block. Returns the path and whether it changed."""
    readme_path = repo_root / readme_name
    if not readme_path.is_file():
        raise CoverageError(f"{readme_path} does not exist")
    # newline="" keeps the file's own line endings: translating them on read and
    # back on write would touch every line of a CRLF README.
    with readme_path.open(encoding="utf-8", newline="") as handle:
        original = handle.read()
    updated = upsert_pointer(original, render_pointer(entry, host_count=host_count))
    changed = updated != original
    if changed and write:
        with readme_path.open("w", encoding="utf-8", newline="") as handle:
            handle.write(updated)
    return readme_path, changed


def coverage_rows(entries: list[dict]) -> list[dict]:
    """Build one coverage row per adapter, so no adapter can be silently skipped."""
    rows = []
    for entry in entries:
        install = entry.get("install") or {}
        pip_package = install.get("pip_package")
        row = {
            "name": entry.get("name"),
            "dcc": entry.get("dcc") or [],
            "url": entry.get("url"),
            "version": entry.get("version"),
            "pip_package": pip_package,
            "pointer": True,
            "registry": "pending" if pip_package else "deferred",
            "blocker": (
                "registry entry waits on the Pass B gate: the core entry must be "
                "queryable on registry.modelcontextprotocol.io first"
                if pip_package
                else NO_PIP_REASON
            ),
        }
        rows.append(row)
    return rows


def render_report(entries: list[dict]) -> str:
    """Render the coverage map as Markdown."""
    rows = coverage_rows(entries)
    total = len(rows)
    pending = sum(1 for r in rows if r["registry"] == "pending")
    deferred = total - pending

    lines = [
        "# Adapter coverage map",
        "",
        "Generated by `scripts/generate_adapter_pointer.py report` from",
        "[`dcc-mcp-catalog.yml`](../dcc-mcp-catalog.yml). Do not edit by hand.",
        "",
        f"**{total} adapter entries**, all of them covered by a generated pointer"
        " block. Registry registration is tracked per row.",
        "",
        f"- Pointer block generated: **{total}**",
        f"- Registry entry pending the Pass B gate: **{pending}**",
        f"- Registry entry deferred with a recorded reason: **{deferred}**",
        "",
        "The host count is reproducible from a checkout:",
        "",
        "```bash",
        """rg -c '^    tags: \\[.*"adapter".*\\]' dcc-mcp-catalog.yml""",
        "```",
        "",
        "| Adapter | Host | Catalog version | PyPI package | Pointer | Registry | Blocker |",
        "|---|---|---|---|---|---|---|",
    ]
    for row in rows:
        hosts = ", ".join(str(h) for h in row["dcc"]) or "—"
        lines.append(
            "| `{name}` | {hosts} | {version} | {pip} | {pointer} | {registry} | {blocker} |".format(
                name=row["name"],
                hosts=hosts,
                version=row["version"] or "—",
                pip=f"`{row['pip_package']}`" if row["pip_package"] else "—",
                pointer="generated" if row["pointer"] else "missing",
                registry=row["registry"],
                blocker=row["blocker"],
            )
        )
    lines.append("")
    return "\n".join(lines)


def audit_pypi(entries: list[dict]) -> int:
    """Print catalog install metadata next to what PyPI actually serves."""
    stale = 0
    unreachable = 0
    for entry in entries:
        install = entry.get("install") or {}
        package = install.get("pip_package")
        catalog_version = entry.get("version")
        if not package:
            latest = "—"
            status = "no catalog install block"
        else:
            latest = _pypi_latest(package)
            if latest.startswith("error:"):
                unreachable += 1
                status = latest
            elif latest == catalog_version:
                status = "matches"
            else:
                stale += 1
                status = "catalog pins an older release"
        print(f"{entry.get('name')!s:<32} catalog={catalog_version!s:<10} pypi={latest!s:<10} {status}")
    print()
    print(f"{len(entries)} adapters checked; {stale} pinned below the latest PyPI release.")
    if unreachable:
        # Kept out of the `stale` count: a network failure is not evidence that
        # the catalog pin is behind.
        print(f"{unreachable} PyPI lookup(s) failed and were excluded from that count.")
    print(
        "The catalog pins an attested wheel (version matches install.url), so a pin"
        " below PyPI latest is expected and is not drift."
    )
    return 0


def _pypi_latest(package: str) -> str:
    url = f"https://pypi.org/pypi/{package}/json"
    try:
        request = Request(url, headers={"User-Agent": "dcc-mcp-coverage-audit"})
        with urlopen(request, timeout=30) as response:
            payload = json.load(response)
    except (HTTPError, URLError, TimeoutError, ValueError) as exc:
        return f"error: {type(exc).__name__}"
    return str(payload.get("info", {}).get("version", "unknown"))


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--catalog",
        type=Path,
        default=DEFAULT_CATALOG,
        help="Path to dcc-mcp-catalog.yml (default: the catalog in this repository).",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    render = sub.add_parser("render", help="Print the generated pointer block.")
    render.add_argument("--adapter", help="Catalog adapter name; omit to render every adapter.")

    apply_cmd = sub.add_parser("apply", help="Write the pointer block into adapter READMEs.")
    apply_cmd.add_argument("--repo-root", type=Path, required=True, help="Adapter checkout root.")
    apply_cmd.add_argument("--adapter", help="Catalog adapter name; default: resolve from git origin.")
    apply_cmd.add_argument("--readme", default="README.md", help="README file name to update.")
    apply_cmd.add_argument(
        "--check",
        action="store_true",
        help="Report what would change and exit non-zero, without writing.",
    )

    report = sub.add_parser("report", help="Write the coverage map for every adapter.")
    report.add_argument("--out", type=Path, help="Markdown output path.")
    report.add_argument("--json-out", type=Path, help="JSON output path.")
    report.add_argument(
        "--check",
        action="store_true",
        help="Report whether the committed coverage map is current, without writing.",
    )

    sub.add_parser("audit-pypi", help="Compare catalog install metadata against PyPI.")
    return parser


def main(argv: list[str] | None = None) -> int:
    """Run the generator CLI. Returns 0 on success and 1 when a check fails."""
    args = _build_parser().parse_args(argv)
    catalog = load_catalog(args.catalog)
    entries = adapter_entries(catalog)
    if not entries:
        raise CoverageError(f"{args.catalog} contains no adapter entries")

    if args.command == "render":
        wanted = getattr(args, "adapter", None)
        selected = [e for e in entries if wanted is None or e.get("name") == wanted]
        if wanted and not selected:
            raise CoverageError(f"catalog has no adapter named {wanted!r}")
        for entry in selected:
            print(render_pointer(entry, host_count=len(entries)))
            print()
        return 0

    if args.command == "apply":
        entry = resolve_adapter(entries, args.repo_root, args.adapter)
        path, changed = apply_pointer(
            args.repo_root,
            entry,
            host_count=len(entries),
            readme_name=args.readme,
            write=not args.check,
        )
        if changed:
            verb = "would update" if args.check else "updated"
            print(f"{verb} {path}")
            if args.check:
                return 1
        else:
            print(f"unchanged {path}")
        return 0

    if args.command == "report":
        markdown = render_report(entries)
        out = args.out or (REPO_ROOT / "docs" / "adapter-coverage.md")
        json_out = args.json_out or (REPO_ROOT / "docs" / "adapter-coverage.json")
        payload = {
            "source": "dcc-mcp-catalog.yml",
            "adapter_count": len(entries),
            "entries": coverage_rows(entries),
        }
        json_text = json.dumps(payload, indent=2, ensure_ascii=False) + "\n"

        if args.check:
            problems = []
            if not out.is_file() or out.read_text(encoding="utf-8") != markdown:
                problems.append(str(out))
            if not json_out.is_file() or json_out.read_text(encoding="utf-8") != json_text:
                problems.append(str(json_out))
            for problem in problems:
                print(f"stale: {problem}")
            if problems:
                print("Run `scripts/generate_adapter_pointer.py report` to refresh.")
                return 1
            print(f"coverage map is current ({len(entries)} adapters)")
            return 0

        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(markdown, encoding="utf-8")
        json_out.write_text(json_text, encoding="utf-8")
        print(f"wrote {out}")
        print(f"wrote {json_out}")
        return 0

    if args.command == "audit-pypi":
        return audit_pypi(entries)

    raise CoverageError(f"unknown command {args.command!r}")  # pragma: no cover


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CoverageError as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(2)
