"""Check saved local artifacts. Native host and browser interaction are unverified."""

from __future__ import annotations

import argparse
import collections
import copy
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
from pathlib import PurePosixPath
import re
import sys
import tempfile

from build import render_outputs
from specification import PROVIDERS
from specification import ROOT
from specification import STAGES
from specification import encode_json
from specification import load_spec
from specification import require
from specification import validate_spec


class PageInventory(HTMLParser):
    """Collect component instances and embedded data from a saved page."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.components = collections.Counter()
        self.asset_ids = []
        self.ids = []
        self.embedded = []
        self.inside_document = False
        self.external_resources = []

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if "data-component" in attrs:
            self.components[attrs["data-component"]] += 1
        if "data-asset-id" in attrs:
            self.asset_ids.append(attrs["data-asset-id"])
        if "id" in attrs:
            self.ids.append(attrs["id"])
        if tag == "script" and attrs.get("id") == "pilot-document":
            self.inside_document = True
        for attribute in ["src", "href"]:
            value = attrs.get(attribute, "")
            if value.startswith(("https:", "http:", "//")):
                self.external_resources.append(value)

    def handle_endtag(self, tag):
        if tag == "script":
            self.inside_document = False

    def handle_data(self, value):
        if self.inside_document:
            self.embedded.append(value)


def validate_manifest(manifest):
    """Validate bounded artifact paths and their actual saved hashes."""
    require(manifest.get("native_acceptance") == "unverified", "manifest must not claim native acceptance")
    require(isinstance(manifest.get("artifacts"), list), "manifest artifacts must be an array")
    paths = set()
    for entry in manifest["artifacts"]:
        name = entry["path"]
        relative = PurePosixPath(name)
        require(
            not relative.is_absolute()
            and ".." not in relative.parts
            and "\\" not in name
            and ":" not in name
            and relative.parts[0] in {"frontend", "native"},
            "unsafe artifact path",
        )
        require(name not in paths, "duplicate artifact path")
        paths.add(name)
        target = ROOT / name
        require(target.is_file() and ROOT in target.resolve().parents, "artifact missing or outside pilot")
        actual = hashlib.sha256(target.read_bytes()).hexdigest()
        require(actual == entry["sha256"], "artifact digest mismatch: " + name)
    require(
        paths
        == {
            "frontend/index.html",
            "frontend/styles.css",
            "frontend/app.js",
            "frontend/design-spec.json",
            "native/fieldkit.pen",
        },
        "unexpected artifact set",
    )


def validate_pencil_subset(candidate):
    """Validate the documented candidate subset without a native editor engine."""
    require(candidate.get("version") == "2.20", "unsupported .pen candidate version")
    nodes = {}
    refs = []
    variables = candidate["variables"]

    def visit(node):
        identifier = node.get("id")
        require(
            isinstance(identifier, str) and identifier and "/" not in identifier and identifier not in nodes,
            "invalid or duplicate Pencil node id",
        )
        nodes[identifier] = node
        require(node.get("type") in {"frame", "rectangle", "text", "ref"}, "outside documented candidate subset")
        if node["type"] == "ref":
            refs.append(node)
        if node["type"] == "frame":
            require(node.get("layout") == "none", "candidate frames must preserve explicit positioning")
        for value in node.values():
            if isinstance(value, str) and value.startswith("$"):
                require(value[1:] in variables, "unresolved Pencil variable")
        for child in node.get("children", []):
            visit(child)

    for root in candidate["children"]:
        visit(root)
    for reference in refs:
        target = nodes.get(reference["ref"])
        require(target is not None and target.get("reusable") is True, "unresolved or non-reusable Pencil ref")
        for override in reference.get("descendants", {}):
            require(override.split("/")[-1] in nodes, "unresolved Pencil descendant override")
    require(
        nodes["collection-page"]["metadata"]["acceptance"] == "native-unverified",
        "Pencil fixture must preserve its unverified acceptance marker",
    )
    return {
        "node_count": len(nodes),
        "instance_count": len(refs),
        "reusable_component_count": sum(node.get("reusable") is True for node in nodes.values()),
    }


def expect_invalid(callback, label):
    """Require a negative validation case to raise a validation error."""
    try:
        callback()
    except ValueError:
        return
    raise ValueError("negative validation failed: " + label)


def check_local_round_trip(spec):
    """Write, reopen, and rebuild an edited local specification."""
    edited = copy.deepcopy(spec)
    edited["page"]["title"] = "A collection for tomorrow."
    edited["tokens"]["color.accent"]["value"] = "#245C78"
    with tempfile.TemporaryDirectory(prefix=".local-check-", dir=str(ROOT)) as directory:
        saved = Path(directory) / "fieldkit-edited.json"
        saved.write_bytes(encode_json(edited).encode("utf-8"))
        reopened = load_spec(saved)
        first = render_outputs(reopened)
        second = render_outputs(reopened)
        require(first == second, "rebuild must be deterministic")
        page = PageInventory()
        page.feed(first["frontend/index.html"])
        require(json.loads("".join(page.embedded)) == edited, "edited document did not round-trip into frontend")
        require("--color-accent: #245C78;" in first["frontend/styles.css"], "edited token did not reach CSS")
    broken = copy.deepcopy(spec)
    broken["components"]["asset-card"]["token_refs"]["color"] = "{color.missing}"
    expect_invalid(lambda: validate_spec(broken), "unresolved token")
    broken = copy.deepcopy(spec)
    broken["page"]["assets"][1]["id"] = broken["page"]["assets"][0]["id"]
    expect_invalid(lambda: validate_spec(broken), "duplicate asset id")
    broken = copy.deepcopy(spec)
    broken["tokens"]["color.accent"] = {"type": "dimension", "value": 8}
    expect_invalid(lambda: validate_spec(broken), "token namespace/type mismatch")
    return True


def verify():
    """Verify saved local artifacts without claiming native or browser acceptance."""
    spec = load_spec(ROOT / "frontend" / "design-spec.json")
    manifest = json.loads((ROOT / "artifacts.json").read_text(encoding="utf-8"))
    validate_manifest(manifest)
    broken = copy.deepcopy(manifest)
    broken["artifacts"][0]["path"] = "../outside-pilot.html"
    expect_invalid(lambda: validate_manifest(broken), "artifact path traversal")
    require(
        manifest["source_spec_sha256"]
        == hashlib.sha256((ROOT / "frontend" / "design-spec.json").read_bytes()).hexdigest(),
        "specification digest mismatch",
    )
    page = PageInventory()
    page.feed((ROOT / "frontend" / "index.html").read_text(encoding="utf-8"))
    require(json.loads("".join(page.embedded)) == spec, "embedded specification drift")
    require(not page.external_resources, "frontend must work without network resources")
    require(len(page.ids) == len(set(page.ids)), "duplicate frontend element ids")
    require(
        page.asset_ids == [asset["id"] for asset in spec["page"]["assets"]], "asset instances differ from specification"
    )
    require(set(page.components) == set(spec["components"]), "frontend component coverage mismatch")
    require(
        page.components["asset-card"] > 1 and page.components["category-tag"] > 1, "library components must be reused"
    )
    css = (ROOT / "frontend" / "styles.css").read_text(encoding="utf-8")
    declared = set(re.findall(r"(--[a-z0-9-]+)\s*:", css))
    used = set(re.findall(r"var\((--[a-z0-9-]+)\)", css))
    require(used <= declared, "unresolved CSS token reference")
    candidate = json.loads((ROOT / "native" / "fieldkit.pen").read_text(encoding="utf-8"))
    native = validate_pencil_subset(candidate)
    broken = copy.deepcopy(candidate)
    broken["children"][-1]["children"][-1]["ref"] = "missing-component"
    expect_invalid(lambda: validate_pencil_subset(broken), "unresolved Pencil instance")
    evidence = json.loads((ROOT / "evidence-template.json").read_text(encoding="utf-8"))
    require(set(evidence["providers"]) == set(PROVIDERS), "native evidence provider scope mismatch")
    for provider in PROVIDERS:
        stages = evidence["providers"][provider]["stages"]
        require(set(stages) == set(STAGES), "native stage scope mismatch")
        require(
            all(entry["status"] == "pending" and entry["evidence"] == [] for entry in stages.values()),
            "template cannot contain fabricated host evidence",
        )
    check_local_round_trip(spec)
    expected = render_outputs(spec)
    require(
        all((ROOT / name).read_bytes() == value.encode("utf-8") for name, value in expected.items()),
        "generated output does not reproduce from saved document",
    )
    return {
        "schema_version": 1,
        "scope": "local_artifact",
        "status": "passed",
        "checks": [
            "saved_artifact_sha256",
            "embedded_specification",
            "offline_resources",
            "unique_element_ids",
            "component_reuse",
            "css_token_resolution",
            "local_json_save_reopen",
            "edited_spec_rebuild",
            "deterministic_generation",
            "reject_invalid_references",
            "native_evidence_remains_pending",
        ],
        "component_instances": dict(page.components),
        "css_token_count": len(declared),
        "pencil_candidate": dict(
            native, validation_scope="documented_subset_static_check", native_acceptance="unverified"
        ),
        "browser_interactions": "unverified",
        "native_provider_sessions": "not_run",
        "source_spec_sha256": manifest["source_spec_sha256"],
    }


def main():
    """Print local verification results and optionally save the sanitized report."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--record", action="store_true", help="Save the public-safe local verification report")
    args = parser.parse_args()
    try:
        report = verify()
        if args.record:
            (ROOT / "local-verification.json").write_bytes(encode_json(report).encode("utf-8"))
        print(encode_json(report).strip())
        return 0
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
