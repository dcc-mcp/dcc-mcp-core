#!/usr/bin/env python3
"""Compile an opt-in adapter release index into review-only candidate evidence.

Reuse the install-catalog publisher's bounded transport, archive verification,
runbook pinning and atomic output. Never install, import, sign, publish or change
approved catalog pins. Export requires explicit versions and no blockers.
"""

from __future__ import annotations

import argparse
import configparser
import copy
from email.parser import Parser
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import sys
import time
from urllib.parse import quote

try:
    from . import prepare_install_catalog as publisher
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import prepare_install_catalog as publisher

Error = publisher.CatalogPreparationError
ROOT = Path(__file__).resolve().parents[2]
_VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
# Match PyPA-normalized identity before checking syntax or duplicate claims.
# Adjacent direct-reference separators also belong to the Core dependency.
_CORE_NAME = re.compile(r"^dcc[-_.]+mcp[-_.]+core(?=[\s\[(@<>=!~;]|$)", re.IGNORECASE)
_MAX_SOURCES = 32


def _core_bounds():
    # Load the existing pure-stdlib policy without importing the native package
    # facade or any adapter. The repository, not installed site-packages, owns it.
    spec = importlib.util.spec_from_file_location(
        "_release_candidate_core_bounds", ROOT / "python/dcc_mcp_core/deployment/core_bounds.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _digest(document):
    data = json.dumps(document, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(data).hexdigest()


def _version(value):
    if not isinstance(value, str) or not _VERSION.fullmatch(value):
        raise Error("pilot requires an exact stable major.minor.patch version")
    return value


def _sources(index, catalog):
    if (
        not isinstance(index, dict)
        or set(index) != {"schema_version", "sources"}
        or type(index["schema_version"]) is not int
        or index["schema_version"] != 1
    ):
        raise Error("unsupported release index schema")
    sources = index["sources"]
    if not isinstance(sources, list) or not 1 <= len(sources) <= _MAX_SOURCES:
        raise Error("release index requires 1..32 sources")
    if (
        not isinstance(catalog, dict)
        or set(catalog) - {"version", "entries"}
        or catalog.get("version", "1") != "1"
        or not isinstance(catalog.get("entries"), list)
    ):
        raise Error("catalog entries are required")
    entries = {}
    for entry in catalog["entries"]:
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str) or entry["name"] in entries:
            raise Error("catalog entry identity is invalid or duplicated")
        entries[entry["name"]] = entry
    names = set()
    for source in sources:
        if not isinstance(source, dict) or set(source) - {"name", "repository", "pypi_project", "compatibility_path"}:
            raise Error("invalid release source fields")
        name = source.get("name")
        if not isinstance(name, str) or name in names or name not in entries:
            raise Error("release source must uniquely bind an existing catalog entry")
        names.add(name)
        entry = entries[name]
        if not isinstance(entry.get("url"), str):
            raise Error("approved repository URL must be a string")
        repo = publisher._official_repo(entry)
        install = entry.get("install", {})
        policy = entry.get("policy", {})
        if (
            not isinstance(install, dict)
            or not isinstance(install.get("pip_package"), str)
            or not isinstance(entry.get("description"), str)
            or not entry["description"]
            or not isinstance(policy, dict)
            or set(policy) - {"installation", "reason"}
            or (policy and policy.get("installation") not in ("available", "not_available"))
            or any(not isinstance(value, str) or not value for value in policy.values())
        ):
            raise Error("invalid approved installation metadata or policy")
        if (
            source.get("repository") != repo
            or source.get("pypi_project") != name
            or not publisher._NAME.fullmatch(name)
            or install.get("type") != "pip"
            or publisher._project_name(install.get("pip_package", "")) != publisher._project_name(name)
        ):
            raise Error("release source repository/package differs from the approved catalog binding")
        path = source.get("compatibility_path")
        if path is not None and (
            not isinstance(path, str)
            or not re.fullmatch(r"[A-Za-z0-9_-]+(?:/[A-Za-z0-9_.-]+)*\.json", path)
            or any(part in (".", "..") for part in path.split("/"))
        ):
            raise Error("compatibility evidence must be a relative JSON member of the verified wheel")
    return [(source, entries[source["name"]]) for source in sources]


def _core_declaration_syntax(declaration):
    suffix = _CORE_NAME.sub("", declaration, count=1).strip()
    if suffix.startswith("(") and suffix.endswith(")"):
        suffix = suffix[1:-1].strip()
    clause = r"(?:~=|==|!=|<=|>=|<|>)\s*[0-9]+\.[0-9]+\.[0-9]+"
    return re.fullmatch(clause + r"(?:\s*,\s*" + clause + r")*", suffix) is not None


def _wheel_evidence(data, source):
    with publisher._open_archive(data) as archive:
        names = archive.namelist()
        metadata_name = next(name for name in names if name.endswith(".dist-info/METADATA"))
        metadata = Parser().parsestr(archive.read(metadata_name).decode("utf-8"))
        entrypoints = {}
        entrypoints_name = metadata_name.rsplit("/", 1)[0] + "/entry_points.txt"
        if entrypoints_name in names:
            parser = configparser.ConfigParser(interpolation=None)
            parser.optionxform = str
            try:
                parser.read_string(archive.read(entrypoints_name).decode("utf-8"))
                if parser.defaults():
                    raise Error("wheel entry point metadata must not contain DEFAULT options")
                entrypoints = {section: dict(parser.items(section)) for section in parser.sections()}
            except (configparser.Error, UnicodeError) as error:
                raise Error("invalid wheel entry point metadata") from error
        compatibility = None
        path = source.get("compatibility_path")
        if path:
            if path not in names:
                raise Error("indexed compatibility document is missing from the verified wheel")
            raw = archive.read(path)
            if len(raw) > publisher.MAX_JSON_BYTES:
                raise Error("compatibility document exceeds the size limit")
            try:
                document = json.loads(raw.decode("utf-8"))
                json.dumps(document, allow_nan=False)
            except (UnicodeError, ValueError) as error:
                raise Error("invalid packaged compatibility JSON") from error
            if not isinstance(document, dict):
                raise Error("packaged compatibility document must be an object")
            compatibility = {"path": path, "sha256": hashlib.sha256(raw).hexdigest(), "document": document}
        requirements = metadata.get_all("Requires-Dist", [])
        core = [item for item in requirements if _CORE_NAME.match(item)]
        blockers = []
        bounds = None
        if len(core) != 1 or ";" in core[0] or "[" in core[0]:
            blockers.append("core_requirement_missing_ambiguous_or_conditional")
        elif not _core_declaration_syntax(core[0]):
            blockers.append("core_requirement_invalid_syntax")
        else:
            bounds = _core_bounds().evaluate(core[0])
            if not bounds["ok"]:
                blockers.extend("core_" + code for code in bounds["codes"])
            else:
                parsed = _core_bounds().parse_requirement(core[0])
                if parsed.lower[0] == parsed.upper[0] and (parsed.lower[1] or parsed.upper[1]):
                    blockers.append("core_empty_range")
        return {
            "requires_python": metadata["Requires-Python"],
            "requires_dist": requirements,
            "entry_points": entrypoints,
            "core_bounds": bounds,
            "packaged_compatibility": compatibility,
            "host_status": "not_checked",
            "native_extras_status": "not_checked",
        }, blockers


def _candidate(source, base, selected, transport):
    name = source["name"]
    project = quote(source["pypi_project"], safe="")
    discovery_url = f"https://pypi.org/pypi/{project}/json"
    if selected is None:
        latest = publisher._json(transport, discovery_url)
        version = _version(latest.get("info", {}).get("version"))
    else:
        version = _version(selected)
    metadata_url = f"https://pypi.org/pypi/{project}/{version}/json"
    metadata = publisher._json(transport, metadata_url)
    filename = name.replace("-", "_") + f"-{version}-py3-none-any.whl"
    artifacts = metadata.get("urls", [])
    if not isinstance(artifacts, list):
        raise Error("PyPI artifact list is invalid")
    matches = [item for item in artifacts if isinstance(item, dict) and item.get("filename") == filename]
    if len(matches) != 1:
        raise Error("release requires exactly one official universal wheel")
    artifact = matches[0]
    entry = copy.deepcopy(base)
    entry["version"] = version
    entry["install"]["url"] = artifact.get("url")
    entry["install"]["sha256"] = publisher._checksum(artifact.get("digests", {}).get("sha256"))
    # SOP declarations belong to each release's runbook. Do not carry a prior
    # release's assertion into a new release which may not declare it.
    entry["install"].pop("sop_version", None)
    entry["install"].pop("sop_schema_digest", None)
    _, data = publisher._verify_pip(transport, entry)
    commit = publisher._release_commit(transport, source["repository"], version)
    publisher._pin_instructions(transport, entry, commit)
    evidence, blockers = _wheel_evidence(data, source)
    policy = base.get("policy", {})
    if policy.get("installation") == "not_available":
        blockers.append("installation_policy_not_available")
    if policy.get("reason"):
        blockers.append("curated_policy_requires_review")
    if tuple(map(int, version.split("."))) < tuple(map(int, _version(base.get("version")).split("."))):
        blockers.append("candidate_older_than_approved")
    if version == base.get("version"):
        if publisher._checksum(base["install"].get("sha256")) != entry["install"]["sha256"]:
            blockers.append("artifact_changed_for_approved_version")
        previous = base["install"].get("instructions_url", "").split("/")
        if len(previous) > 5 and publisher._SHA.fullmatch(previous[5]) and previous[5].lower() != commit:
            blockers.append("release_commit_changed_for_approved_version")
    if evidence["core_bounds"] and evidence["core_bounds"]["ok"]:
        requirement = _core_bounds().parse_requirement(evidence["core_bounds"]["declaration"])
        entry["core_requirement"] = requirement.to_spec()
        entry["min_core_version"] = str(requirement.lower[0])
    changes = {
        key: {"before": before, "after": after}
        for key, before, after in (
            ("version", base.get("version"), version),
            ("artifact_url", base["install"].get("url"), entry["install"]["url"]),
            ("sha256", base["install"].get("sha256"), entry["install"]["sha256"]),
            ("instructions_url", base["install"].get("instructions_url"), entry["install"]["instructions_url"]),
            ("core_requirement", base.get("core_requirement"), entry.get("core_requirement")),
        )
        if before != after
    }
    return {
        "name": name,
        "version": version,
        "selection": "explicit_version" if selected is not None else "observed_latest",
        "status": "blocked" if blockers else "review_required",
        "blockers": blockers,
        "approved_entry_sha256": _digest(base),
        "repository": source["repository"],
        "source_commit": commit,
        "metadata_url": metadata_url,
        "artifact": {
            "url": entry["install"]["url"],
            "sha256": entry["install"]["sha256"],
            "size": len(data),
            "platform_tag": "py3-none-any",
        },
        "compatibility": evidence,
        "provenance": {
            "url": f"https://pypi.org/integrity/{project}/{version}/{filename}/provenance",
            "verification": "not_checked",
        },
        "changes": changes,
        "catalog_entry": None if blockers else entry,
    }


def prepare_candidates(index, catalog, selections, now, transport):
    """Return review evidence; a newer version never grants installation authority."""
    if isinstance(now, bool) or not isinstance(now, int) or now < 0:
        raise Error("observation time must be a nonnegative Unix timestamp")
    sources = _sources(index, catalog)
    names = {source["name"] for source, _ in sources}
    if not isinstance(selections, dict) or set(selections) - names:
        raise Error("explicit selections must belong to the release index")
    candidates = []
    for source, base in sources:
        try:
            candidates.append(_candidate(source, base, selections.get(source["name"]), transport))
        except (Error, KeyError, TypeError, AttributeError, UnicodeError) as error:
            raise Error("{}: {}".format(source["name"], error)) from error
    return {
        "schema_version": 1,
        "authority": "candidate_only",
        "observed_at": now,
        "index_sha256": _digest(index),
        "approved_catalog_sha256": _digest(catalog),
        "candidates": candidates,
    }


def export_catalog(report):
    """Export only explicitly selected, unblocked entries for operator review."""
    candidates = report["candidates"]
    if any(item["selection"] != "explicit_version" or item["blockers"] for item in candidates):
        raise Error("catalog export requires explicit versions and no promotion blockers")
    return {"version": "1", "entries": [copy.deepcopy(item["catalog_entry"]) for item in candidates]}


def main(argv=None):
    """Write candidate evidence atomically without changing any published catalog."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--index", type=Path, default=ROOT / "compatibility/adapter-release-index.json")
    parser.add_argument("--catalog", type=Path, default=ROOT / "dcc-mcp-catalog.yml")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--select", action="append", default=[], metavar="NAME==VERSION")
    parser.add_argument(
        "--catalog-output", type=Path, help="Explicit opt-in unsigned catalog; all entries must be unblocked"
    )
    parser.add_argument("--now", type=int, default=None)
    args = parser.parse_args(argv)
    try:
        selections = {}
        for value in args.select:
            parts = value.split("==")
            if len(parts) != 2 or parts[0] in selections:
                raise Error("each selection must uniquely specify NAME==VERSION")
            selections[parts[0]] = _version(parts[1])
        outputs = [args.output] + ([args.catalog_output] if args.catalog_output else [])
        protected = {args.index.resolve(), args.catalog.resolve()}
        if len({path.resolve() for path in outputs}) != len(outputs) or any(
            path.resolve() in protected for path in outputs
        ):
            raise Error("output paths must be distinct and must not overwrite input files")
        transport = publisher.HttpTransport(os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN"))
        report = prepare_candidates(
            publisher._load_document(args.index.read_text(encoding="utf-8")),
            publisher._load_document(args.catalog.read_text(encoding="utf-8")),
            selections,
            int(time.time()) if args.now is None else args.now,
            transport,
        )
        # Validate export eligibility before replacing either prior output.
        catalog = export_catalog(report) if args.catalog_output else None
        publisher.write_payload(report, args.output)
        if catalog is not None:
            publisher.write_payload(catalog, args.catalog_output)
    except (Error, OSError) as error:
        print(f"Release candidate preparation failed: {error}", file=sys.stderr)
        return 1
    blocked = sum(bool(item["blockers"]) for item in report["candidates"])
    print(
        f"Prepared {len(report['candidates'])} review-only candidates; {blocked} blocked. "
        "Nothing published or installed."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
