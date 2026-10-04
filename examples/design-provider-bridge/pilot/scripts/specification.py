"""Validate the bounded, provider-neutral Fieldkit fixture using only stdlib."""

from __future__ import annotations

import json
import math
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
PROVIDERS = ["figma", "penpot", "pencil", "sketch", "framer"]
STAGES = ["read", "write", "save_reopen", "export"]
COMPONENT_TAGS = {
    "asset-card": "button",
    "action-button": "button",
    "filter-button": "button",
    "category-tag": "span",
    "nav-link": "a",
    "value-row": "div",
}
VISUALS = {"tidal", "terrain", "orbit", "mono", "forms", "type"}
IDENTIFIER = re.compile(r"^[a-z][a-z0-9-]*$")
TOKEN_NAME = re.compile(r"^[a-z][a-z0-9.-]*$")
COLOR = re.compile(r"^#[0-9A-Fa-f]{6}$")


def require(condition, message):
    """Raise a validation error when a required condition is false."""
    if not condition:
        raise ValueError(message)


def text(value, label, maximum=300):
    """Validate bounded, non-empty content text."""
    require(
        isinstance(value, str) and 0 < len(value.strip()) <= maximum,
        f"{label} must be non-empty text of at most {maximum} characters",
    )


def token_name(reference):
    """Extract a token name from a braced reference."""
    require(
        isinstance(reference, str) and reference.startswith("{") and reference.endswith("}"),
        "token references must use {token.name}",
    )
    return reference[1:-1]


def validate_spec(spec):
    """Validate the shared fixture structure and its reusable references."""
    require(isinstance(spec, dict), "specification must be an object")
    require(spec.get("schema_version") == 1, "unsupported specification schema")
    for key in ["project", "tokens", "components", "page", "acceptance"]:
        require(isinstance(spec.get(key), dict), f"{key} must be an object")
    text(spec["project"].get("name"), "project name")
    require(IDENTIFIER.fullmatch(spec["project"].get("id", "")), "invalid project id")
    require(spec["acceptance"].get("providers") == PROVIDERS, "provider scope must remain shared")
    require(spec["acceptance"].get("native") == STAGES, "unexpected native stage names")
    tokens = spec["tokens"]
    require(tokens, "tokens cannot be empty")
    for name, token in tokens.items():
        require(TOKEN_NAME.fullmatch(name), "invalid token name")
        require(isinstance(token, dict), "token must be an object")
        expected = "color" if name.startswith("color.") else "dimension"
        require(token.get("type") == expected, "token type does not match namespace: " + name)
        value = token.get("value")
        if token.get("type") == "color":
            require(isinstance(value, str) and COLOR.fullmatch(value), "invalid color token: " + name)
        elif token.get("type") == "dimension":
            require(
                isinstance(value, (int, float))
                and not isinstance(value, bool)
                and math.isfinite(value)
                and 0 <= value <= 1000,
                "invalid dimension token: " + name,
            )
        else:
            raise ValueError("unsupported token type: " + name)
    required_tokens = {
        "color.canvas",
        "color.surface",
        "color.ink",
        "color.muted",
        "color.line",
        "color.accent",
        "color.accent-soft",
        "color.accent-ink",
        "space.unit",
        "space.card",
        "radius.control",
        "radius.card",
        "type.body",
        "type.heading",
    }
    require(required_tokens <= set(tokens), "missing required frontend tokens")
    components = spec["components"]
    require(set(components) == set(COMPONENT_TAGS), "unexpected component library")
    for name, definition in components.items():
        require(isinstance(definition, dict), "component must be an object")
        require(definition.get("tag") == COMPONENT_TAGS[name], "component tag mismatch: " + name)
        require(IDENTIFIER.fullmatch(definition.get("class", "")), "invalid component class")
        require(isinstance(definition.get("token_refs"), dict), "token_refs must be an object")
        for property_name, reference in definition["token_refs"].items():
            require(
                property_name in {"background", "color", "border-color", "border-radius"}, "unsupported component style"
            )
            require(token_name(reference) in tokens, "unresolved token reference: " + reference)
    page = spec["page"]
    for key in ["title", "subtitle", "collection_name"]:
        text(page.get(key), "page " + key)
    require(IDENTIFIER.fullmatch(page.get("id", "")), "invalid page id")
    filters = page.get("filters")
    require(
        isinstance(filters, list) and len(filters) > 1 and filters[0] == "All assets",
        "filters must start with All assets",
    )
    for value in filters:
        text(value, "filter", 40)
    require(len(set(filters)) == len(filters), "duplicate filters")
    assets = page.get("assets")
    require(isinstance(assets, list) and 2 <= len(assets) <= 24, "use 2 to 24 shared assets")
    ids = set()
    for asset in assets:
        require(isinstance(asset, dict), "asset must be an object")
        asset_id = asset.get("id", "")
        require(IDENTIFIER.fullmatch(asset_id) and asset_id not in ids, "invalid or duplicate asset id")
        ids.add(asset_id)
        require(asset.get("component") == "asset-card", "asset must reuse asset-card")
        require(asset.get("category") in filters[1:], "unknown asset category")
        require(asset.get("visual") in VISUALS, "unknown artwork treatment")
        for key in ["name", "description", "format"]:
            text(asset.get(key), "asset " + key)
        require(
            type(asset.get("items")) is int and 0 < asset["items"] <= 1000,
            "asset item count must be a positive integer",
        )
    return spec


def load_spec(path):
    """Read and validate a saved UTF-8 specification."""
    return validate_spec(json.loads(Path(path).read_text(encoding="utf-8")))


def encode_json(value):
    """Encode deterministic JSON with a trailing newline and finite numbers."""
    return json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n"
