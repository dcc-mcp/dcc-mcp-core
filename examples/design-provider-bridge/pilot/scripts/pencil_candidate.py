"""Generate the documented .pen 2.20 subset; this is not native engine evidence."""

from __future__ import annotations


def label(identifier, content, x, y, size=15, color="$color.ink"):
    """Create a text node from documented Pencil properties."""
    return {
        "id": identifier,
        "type": "text",
        "x": x,
        "y": y,
        "content": content,
        "fontFamily": "Arial",
        "fontSize": size,
        "fill": color,
    }


def frame(identifier, width, height, children, **properties):
    """Create a frame that preserves explicit child positioning."""
    result = {
        "id": identifier,
        "type": "frame",
        "layout": "none",
        "width": width,
        "height": height,
        "children": children,
    }
    result.update(properties)
    return result


def create_candidate(spec):
    """Create an unverified native candidate with reusable components and refs."""
    button = frame(
        "button-component",
        168,
        42,
        [label("button-label", "Export collection", 16, 12, color="$color.accent-ink")],
        reusable=True,
        x=-420,
        y=0,
        fill="$color.accent",
        cornerRadius="$radius.control",
    )
    tag = frame(
        "tag-component",
        100,
        28,
        [label("tag-label", "Interface", 10, 6, 12, "$color.accent")],
        reusable=True,
        x=-420,
        y=70,
        fill="$color.accent-soft",
        cornerRadius="$radius.control",
    )
    card = frame(
        "asset-card-component",
        300,
        236,
        [
            {
                "id": "card-artwork",
                "type": "rectangle",
                "x": 12,
                "y": 12,
                "width": 276,
                "height": 128,
                "fill": "$color.accent-soft",
                "cornerRadius": "$radius.control",
            },
            label("card-title", "Tidal interface kit", 16, 158, 17),
            {"id": "card-tag", "type": "ref", "ref": "tag-component", "x": 16, "y": 193},
            label("card-count", "12 items", 216, 201, 12, "$color.muted"),
        ],
        reusable=True,
        x=-420,
        y=125,
        fill="$color.surface",
        stroke="$color.line",
        strokeWidth=1,
        cornerRadius="$radius.card",
    )
    page_children = [
        label("page-brand", spec["project"]["name"], 40, 28, 25),
        label("page-heading", spec["page"]["title"], 40, 108, 40),
        label("page-description", spec["page"]["subtitle"], 40, 170, 16, "$color.muted"),
        {"id": "export-button", "type": "ref", "ref": "button-component", "x": 976, "y": 26},
        {
            "id": "browse-button",
            "type": "ref",
            "ref": "button-component",
            "x": 40,
            "y": 218,
            "descendants": {"button-label": {"content": "All assets"}},
        },
    ]
    for index, asset in enumerate(spec["page"]["assets"]):
        page_children.append(
            {
                "id": "asset-" + asset["id"],
                "type": "ref",
                "ref": "asset-card-component",
                "x": 40 + (index % 3) * 328,
                "y": 300 + (index // 3) * 260,
                "descendants": {
                    "card-title": {"content": asset["name"]},
                    "card-tag/tag-label": {"content": asset["category"]},
                    "card-count": {"content": "{} items".format(asset["items"])},
                },
            }
        )
    page = frame(
        "collection-page",
        1200,
        340 + ((len(spec["page"]["assets"]) + 2) // 3) * 260,
        page_children,
        x=0,
        y=0,
        fill="$color.canvas",
        metadata={
            "type": "design-provider-pilot",
            "acceptance": "native-unverified",
            "source": "../frontend/design-spec.json",
            "coverage": "three reusable components; collection layout",
        },
    )
    variables = {
        name: {"type": "color" if token["type"] == "color" else "number", "value": token["value"]}
        for name, token in spec["tokens"].items()
    }
    return {"version": "2.20", "variables": variables, "children": [button, tag, card, page]}
