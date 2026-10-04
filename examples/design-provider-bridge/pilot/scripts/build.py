"""Build an offline editable frontend and schema-derived Pencil candidate."""

from __future__ import annotations

import argparse
import hashlib
import html
from pathlib import Path
import sys

from pencil_candidate import create_candidate
from specification import ROOT
from specification import encode_json
from specification import load_spec
from specification import token_name


def digest(value):
    """Return a stable SHA-256 digest for UTF-8 artifact text."""
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def escape(value):
    """Escape a value for HTML text or a quoted attribute."""
    return html.escape(str(value), quote=True)


def component(spec, name, body, **attributes):
    """Render one instance of a declared component with escaped attributes."""
    definition = spec["components"][name]
    attributes = dict(attributes, **{"class": definition["class"], "data-component": name})
    if definition["tag"] == "button":
        attributes["type"] = "button"
    attrs = " ".join(f'{key}="{escape(value)}"' for key, value in attributes.items())
    return "<{tag} {attrs}>{body}</{tag}>".format(tag=definition["tag"], attrs=attrs, body=body)


def render_page(spec):
    """Render the offline page and its complete editable specification."""
    page = spec["page"]
    asset = page["assets"][0]
    links = "".join(
        component(spec, "nav-link", escape(label), href=target, **attrs)
        for label, target, attrs in [
            ("Collection", "#collection", {"aria-current": "page"}),
            ("Page studio", "#page-studio", {}),
        ]
    )
    filters = "".join(
        component(
            spec, "filter-button", escape(value), **{"data-filter": value, "aria-pressed": str(index == 0).lower()}
        )
        for index, value in enumerate(page["filters"])
    )
    cards = []
    for item in page["assets"]:
        artwork = '<span class="art-letter">Aa</span>' if item["visual"] == "type" else ""
        body = '<span class="artwork {}" aria-hidden="true">{}</span>'.format(item["visual"], artwork)
        body += (
            '<span class="asset-copy"><span class="asset-title">{}</span>'
            '<span class="asset-meta">{}<span>{} items</span></span></span>'
        ).format(escape(item["name"]), component(spec, "category-tag", escape(item["category"])), item["items"])
        cards.append(
            component(
                spec,
                item["component"],
                body,
                **{
                    "data-asset-id": item["id"],
                    "aria-pressed": str(item == asset).lower(),
                    "aria-label": "Select " + item["name"],
                },
            )
        )
    fields = "".join(
        component(
            spec,
            "value-row",
            f'<span class="label">{label}</span><span id="selected-{key}">{escape(asset[key])}</span>',
        )
        for label, key in [("Category", "category"), ("Format", "format"), ("Items", "items")]
    )
    payload = encode_json(spec).replace("<", "\\u003c").replace("&", "\\u0026")
    template = "\n".join(
        [
            "<!doctype html>",
            '<html lang="en"><head><meta charset="utf-8">'
            '<meta name="viewport" content="width=device-width, initial-scale=1">',
            '<meta name="description" content="An editable collection page built from a shared component library.">',
            '<title>{brand} / Studio collection</title><link rel="stylesheet" href="styles.css">'
            '<script src="app.js" defer></script></head>',
            '<body><div class="shell"><aside class="sidebar"><a class="brand" href="#collection">'
            '<span class="brand-mark" aria-hidden="true">f</span>{brand}</a>',
            '<nav class="nav" aria-label="Workspace">{links}</nav>'
            '<p class="sidebar-note">Make room for your<br>next good idea.</p></aside>',
            '<main><header class="topbar"><div class="breadcrumb">Workspace / '
            "<strong>{collection}</strong></div>{export_button}</header>",
            '<div class="content"><section class="intro"><p class="eyebrow">Your creative library</p>'
            '<h1 id="page-title">{title}</h1><p class="subtitle">{subtitle}</p></section>',
            '<div class="workspace"><section id="collection" aria-label="Asset collection">'
            '<div class="collection-toolbar"><div class="filters" aria-label="Asset category">{filters}</div>',
            '<label class="search"><span class="sr-only">Search assets</span>'
            '<input id="asset-search" type="search" placeholder="Search assets"></label></div>',
            '<div class="asset-grid">{cards}</div><p class="empty-state" id="empty-state" hidden>'
            "No assets match. Try a different search or category.</p>",
            '<p class="result-count" id="result-count" role="status">{count} of {count} assets</p></section>',
            '<aside class="inspector" id="inspector" aria-label="Selected asset">'
            '<h2 id="selected-name">{selected}</h2><p id="selected-description">{description}</p>{fields}',
            '<section class="edit-panel" id="page-studio"><h3>Make it yours</h3>'
            '<label for="title-input">Page title</label>'
            '<input id="title-input" type="text" maxlength="300" value="{title}">',
            '<label for="accent-input">Library accent</label><div class="color-control">'
            '<input id="accent-input" type="color" value="{accent}">'
            '<output id="accent-value" for="accent-input">{accent}</output></div>',
            '<button id="reset-edits" class="reset-button" type="button">Reset page edits</button>'
            '<p class="status" id="edit-status" role="status" aria-live="polite"></p></section></aside></div>',
            '<footer class="collection-footer"><span>Built for your studio. Ready to make your own.</span>'
            "<span>{brand} collection</span></footer></div></main></div>",
            "<noscript><p>This page is readable without JavaScript. "
            "Enable JavaScript to filter, edit, and export.</p></noscript>",
            '<script type="application/json" id="pilot-document">{payload}</script></body></html>',
            "",
        ]
    )
    return template.format(
        brand=escape(spec["project"]["name"]),
        links=links,
        collection=escape(page["collection_name"]),
        export_button=component(spec, "action-button", "Export page", id="export-document"),
        title=escape(page["title"]),
        subtitle=escape(page["subtitle"]),
        filters=filters,
        cards="".join(cards),
        count=len(cards),
        selected=escape(asset["name"]),
        description=escape(asset["description"]),
        fields=fields,
        accent=escape(spec["tokens"]["color.accent"]["value"]),
        payload=payload,
    )


def render_styles(spec):
    """Render token declarations and component styles before editable layout CSS."""
    lines = [":root {"]
    for name, token in spec["tokens"].items():
        value = str(token["value"]) + ("px" if token["type"] == "dimension" else "")
        lines.append("  --{}: {};".format(name.replace(".", "-"), value))
    lines.append("}")
    for definition in spec["components"].values():
        props = " ".join(
            "{}: var(--{});".format(key, token_name(ref).replace(".", "-"))
            for key, ref in definition["token_refs"].items()
        )
        lines.append(".{} {{ {} }}".format(definition["class"], props))
    return "\n".join(lines) + "\n" + (ROOT / "templates" / "styles.css").read_text(encoding="utf-8")


def render_outputs(spec):
    """Return deterministic local artifacts and their public-safe hash manifest."""
    document = encode_json(spec)
    outputs = {
        "frontend/index.html": render_page(spec),
        "frontend/styles.css": render_styles(spec),
        "frontend/app.js": (ROOT / "templates" / "app.js").read_text(encoding="utf-8"),
        "frontend/design-spec.json": document,
        "native/fieldkit.pen": encode_json(create_candidate(spec)),
    }
    manifest = {
        "schema_version": 1,
        "project_id": spec["project"]["id"],
        "source_spec_sha256": digest(document),
        "native_acceptance": "unverified",
        "artifacts": [
            {
                "path": name,
                "sha256": digest(value),
                "source_kind": "schema_derived_native_unverified" if name.endswith(".pen") else "local_frontend",
            }
            for name, value in outputs.items()
        ],
    }
    outputs["artifacts.json"] = encode_json(manifest)
    return outputs


def main():
    """Build or check artifacts without starting a provider host."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spec", type=Path, default=ROOT / "design-spec.json")
    parser.add_argument("--check", action="store_true", help="Check deterministic outputs without writing")
    args = parser.parse_args()
    try:
        outputs = render_outputs(load_spec(args.spec))
        if args.check:
            different = [
                name
                for name, value in outputs.items()
                if not (ROOT / name).is_file() or (ROOT / name).read_bytes() != value.encode("utf-8")
            ]
            if different:
                raise ValueError("generated artifacts differ: " + ", ".join(different))
        else:
            for name, value in outputs.items():
                target = ROOT / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(value.encode("utf-8"))
        print(
            encode_json(
                {
                    "scope": "local_generation",
                    "status": "passed",
                    "artifact_count": len(outputs),
                    "native_acceptance": "unverified",
                }
            ).strip()
        )
        return 0
    except (ValueError, OSError, KeyError, TypeError) as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
