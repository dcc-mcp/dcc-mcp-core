"""Author the explicit, client-compatible typed tool schemas once."""

import json
from pathlib import Path


def object_schema(properties, required=()):
    return {"type": "object", "additionalProperties": False, "properties": properties, "required": list(required)}


node = {
    key: {"type": "number"}
    for key in (
        "x",
        "y",
        "width",
        "height",
        "rx",
        "ry",
        "cx",
        "cy",
        "r",
        "x1",
        "y1",
        "x2",
        "y2",
        "stroke_width",
        "font_size",
        "letter_spacing",
        "opacity",
        "font_weight",
    )
}
node.update(
    {
        key: {"type": "string", "maxLength": 4096}
        for key in (
            "id",
            "parent",
            "label",
            "text",
            "transform",
            "fill",
            "stroke",
            "fill_rule",
            "stroke_linecap",
            "stroke_linejoin",
            "font_family",
            "text_anchor",
            "gradient_units",
            "gradient_transform",
        )
    }
)
node["type"] = {
    "type": "string",
    "enum": ["group", "layer", "path", "rect", "circle", "ellipse", "text", "linearGradient", "linear_gradient"],
}
node["d"] = {"type": "string", "maxLength": 100000}
node["stops"] = {
    "type": "array",
    "minItems": 2,
    "maxItems": 64,
    "items": object_schema(
        {
            "offset": {"type": "number", "minimum": 0, "maximum": 1},
            "color": {"type": "string"},
            "opacity": {"type": "number", "minimum": 0, "maximum": 1},
        },
        ("offset", "color"),
    ),
}
plan = object_schema(
    {
        "canvas": object_schema(
            {
                "width": {"type": "number", "minimum": 1, "maximum": 32768},
                "height": {"type": "number", "minimum": 1, "maximum": 32768},
                "view_box": {"type": "string", "description": "Four finite numbers: min-x min-y width height"},
            },
            ("width", "height"),
        ),
        "nodes": {"type": "array", "minItems": 1, "maxItems": 5000, "items": object_schema(node, ("type", "id"))},
    },
    ("canvas", "nodes"),
)
schemas = {
    "capabilities": object_schema({}),
    "document_build": object_schema({"output_file": {"type": "string"}, "plan": plan}, ("output_file", "plan")),
    "document_export": object_schema(
        {
            "source_file": {"type": "string"},
            "output_file": {"type": "string"},
            "format": {"type": "string", "enum": ["svg", "png", "pdf"], "default": "png"},
            "width": {"type": "number", "minimum": 1, "maximum": 32768},
            "height": {"type": "number", "minimum": 1, "maximum": 32768},
            "background": {"type": "string", "default": "#ffffff"},
            "background_opacity": {"type": "number", "minimum": 0, "maximum": 1, "default": 0},
            "plain_svg": {"type": "boolean", "default": False},
            "text_to_path": {"type": "boolean", "default": False},
        },
        ("source_file", "output_file"),
    ),
    "document_inspect": object_schema({"source_file": {"type": "string"}}, ("source_file",)),
    "document_open": object_schema({"source_file": {"type": "string"}}, ("source_file",)),
}
descriptions = {
    "capabilities": "Query the configured Inkscape version, native actions, isolated profile, and limitations.",
    "document_build": "Create editable SVG with an Inkscape-hosted inkex effect from a typed vector plan.",
    "document_export": "Export PNG, SVG, or PDF with Inkscape; optionally convert text to paths.",
    "document_inspect": "Reopen an SVG with Inkscape, query native geometry, and count vector objects.",
    "document_open": "Open a separate Inkscape GUI process for visual acceptance and return its PID.",
}
output = {
    "type": "object",
    "required": ["success", "message"],
    "properties": {
        "success": {"type": "boolean"},
        "message": {"type": "string"},
        "context": {"type": "object"},
        "error": {"type": "string"},
    },
}
tools = []
for name, schema in schemas.items():
    read_only = name in {"capabilities", "document_inspect"}
    tools.append(
        {
            "name": name,
            "description": descriptions[name],
            "source_file": "scripts/" + name + ".py",
            "execution": "sync",
            "affinity": "any",
            "enforce_thread_affinity": True,
            "timeout_hint_secs": 150,
            "input_schema": schema,
            "output_schema": output,
            "annotations": {
                "read_only_hint": read_only,
                "destructive_hint": not read_only,
                "idempotent_hint": read_only,
                "open_world_hint": False,
            },
        }
    )

if __name__ == "__main__":
    Path(__file__).with_name("skills").joinpath("inkscape-vector", "tools.yaml").write_text(
        json.dumps({"tools": tools}, indent=2) + "\n", encoding="utf-8"
    )
