"""Validate the bounded vector object protocol without generating SVG."""

import json
import math
import re

KINDS = {"group", "layer", "path", "rect", "circle", "ellipse", "text", "linear_gradient"}
ID = re.compile(r"[A-Za-z_][A-Za-z0-9_.-]{0,95}\Z")
PAINT = re.compile(
    r"(?:#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})|none|currentColor|[A-Za-z]+|url\(#[A-Za-z_][A-Za-z0-9_.-]{0,95}\))\Z"
)
GEOMETRY = {"x", "y", "width", "height", "rx", "ry", "cx", "cy", "r", "x1", "y1", "x2", "y2"}
STYLE = {
    "fill",
    "stroke",
    "stroke_width",
    "fill_rule",
    "stroke_linecap",
    "stroke_linejoin",
    "opacity",
    "font_family",
    "font_size",
    "font_weight",
    "letter_spacing",
    "text_anchor",
}
FIELDS = (
    GEOMETRY
    | STYLE
    | {"type", "id", "parent", "label", "d", "text", "transform", "stops", "gradient_units", "gradient_transform"}
)
NUMERIC = r"[-+]?(?:\d*\.\d+|\d+\.?\d*)(?:[eE][-+]?\d+)?"


def validate_transform(value):
    """Support only SVG transform functions with finite numeric arguments."""
    remaining = value.strip()
    while remaining:
        match = re.match(r"(matrix|translate|scale|rotate|skewX|skewY)\s*\(([^()]*)\)\s*", remaining)
        if not match or re.sub(NUMERIC, "", match[2]).strip(" ,\t"):
            raise ValueError("Unsupported transform syntax")
        values = re.findall(NUMERIC, match[2])
        allowed = {"matrix": {6}, "translate": {1, 2}, "scale": {1, 2}, "rotate": {1, 3}, "skewX": {1}, "skewY": {1}}
        if len(values) not in allowed[match[1]]:
            raise ValueError("Invalid transform argument count")
        for item in values:
            number(float(item), "transform coordinate")
        remaining = remaining[match.end() :].lstrip(" ,")


def number(value, name, minimum=None, maximum=1000000):
    """Accept finite numeric values and reject bools, NaN, and infinity."""
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f"{name} must be a finite number")
    if abs(value) > maximum or (minimum is not None and value < minimum):
        raise ValueError(f"{name} is outside its supported range")
    return value


def validate_plan(plan):
    """Validate a complete plan before any Inkscape process is launched."""
    if not isinstance(plan, dict) or set(plan) - {"width", "height", "view_box", "canvas", "nodes"}:
        raise ValueError("Plan supports width, height, view_box, and nodes only")
    if len(json.dumps(plan, ensure_ascii=False).encode("utf-8")) > 2000000:
        raise ValueError("Vector plan exceeds the 2 MB request limit")
    plan = json.loads(json.dumps(plan))
    if "canvas" in plan:
        canvas = plan.pop("canvas")
        if (
            not isinstance(canvas, dict)
            or set(canvas) - {"width", "height", "view_box"}
            or any(key in plan for key in {"width", "height", "view_box"})
        ):
            raise ValueError("canvas supports width, height, and view_box only")
        plan.update(canvas)
    number(plan.get("width"), "width", 1, 32768)
    number(plan.get("height"), "height", 1, 32768)
    view = plan.get("view_box", [0, 0, plan["width"], plan["height"]])
    if isinstance(view, str) and not re.sub(NUMERIC, "", view).strip(" ,\t"):
        view = [float(item) for item in re.findall(NUMERIC, view)]
    if not isinstance(view, list) or len(view) != 4:
        raise ValueError("view_box must contain four numbers")
    for index, value in enumerate(view):
        number(value, "view_box", 0.001 if index > 1 else None)
    plan["view_box"] = view
    nodes = plan.get("nodes")
    if not isinstance(nodes, list) or not 1 <= len(nodes) <= 5000:
        raise ValueError("nodes must contain 1 to 5000 vector objects")
    ids = {}
    for node in nodes:
        if not isinstance(node, dict) or set(node) - FIELDS:
            raise ValueError("Unknown object field; arbitrary SVG/XML is not accepted")
        kind, identity = node.get("type"), node.get("id")
        if kind == "linearGradient":
            kind = node["type"] = "linear_gradient"
        if (
            not isinstance(kind, str)
            or kind not in KINDS
            or not isinstance(identity, str)
            or not ID.fullmatch(identity)
        ):
            raise ValueError("Every object needs a supported type and safe id")
        if identity in ids:
            raise ValueError("Duplicate object id: " + identity)
        parent = node.get("parent")
        if parent is not None and (not isinstance(parent, str) or ids.get(parent) not in {"group", "layer"}):
            raise ValueError("Parent must be an earlier group or layer")
        if (
            "font_weight" in node
            and isinstance(node["font_weight"], (int, float))
            and not isinstance(node["font_weight"], bool)
        ):
            number(node["font_weight"], "font_weight", 100, 900)
            if node["font_weight"] % 100:
                raise ValueError("font_weight must be 100..900 in steps of 100")
            node["font_weight"] = str(int(node["font_weight"]))
        for key in GEOMETRY | {"stroke_width", "font_size", "letter_spacing", "opacity"}:
            if key in node:
                number(
                    node[key],
                    key,
                    0 if key in {"width", "height", "r", "rx", "ry", "stroke_width", "font_size", "opacity"} else None,
                )
        if "opacity" in node and node["opacity"] > 1:
            raise ValueError("opacity must be within 0..1")
        for key in {"fill", "stroke"}:
            if key in node and (not isinstance(node[key], str) or not PAINT.fullmatch(node[key])):
                raise ValueError("Paint must be a color, currentColor, none, or local gradient reference")
        for key in {"label", "text", "font_family", "font_weight", "transform", "gradient_transform", "d"}:
            if key in node and (not isinstance(node[key], str) or len(node[key]) > (100000 if key == "d" else 4096)):
                raise ValueError(key + " must be a bounded string")
        if kind == "path" and (
            not node.get("d") or not re.fullmatch(r"[MmZzLlHhVvCcSsQqTtAa0-9eE.,+\-\s]+", node["d"])
        ):
            raise ValueError("Path data must contain SVG path commands and numeric coordinates")
        if kind == "path":
            for item in re.findall(NUMERIC, node["d"]):
                number(float(item), "path coordinate")
        for key in {"transform", "gradient_transform"}:
            if key in node:
                validate_transform(node[key])
        if "font_family" in node and not re.fullmatch(r"[A-Za-z0-9 ,_\-'\"]{1,256}", node["font_family"]):
            raise ValueError("font_family must be a safe font family list")
        if "font_weight" in node and node["font_weight"] not in {
            "normal",
            "bold",
            "100",
            "200",
            "300",
            "400",
            "500",
            "600",
            "700",
            "800",
            "900",
        }:
            raise ValueError("font_weight must be normal, bold, or 100..900")
        if kind == "text" and "text" not in node:
            raise ValueError("Text objects require text")
        for key, allowed in (
            ("fill_rule", {"nonzero", "evenodd"}),
            ("stroke_linecap", {"butt", "round", "square"}),
            ("stroke_linejoin", {"miter", "round", "bevel"}),
            ("text_anchor", {"start", "middle", "end"}),
            ("gradient_units", {"userSpaceOnUse", "objectBoundingBox"}),
        ):
            if key in node and (not isinstance(node[key], str) or node[key] not in allowed):
                raise ValueError("Unsupported " + key)
        if kind == "linear_gradient":
            stops = node.get("stops")
            if not isinstance(stops, list) or not 2 <= len(stops) <= 64 or parent is not None:
                raise ValueError("Gradients require 2..64 stops and no parent")
            previous = -1
            for stop in stops:
                if not isinstance(stop, dict) or set(stop) - {"offset", "color", "opacity"}:
                    raise ValueError("Unsupported gradient stop")
                offset = number(stop.get("offset"), "stop offset", 0, 1)
                if (
                    offset < previous
                    or not isinstance(stop.get("color"), str)
                    or not PAINT.fullmatch(stop["color"])
                    or stop["color"].startswith("url")
                ):
                    raise ValueError("Gradient stops need ordered offsets and literal colors")
                number(stop.get("opacity", 1), "stop opacity", 0, 1)
                previous = offset
        ids[identity] = kind
    for node in nodes:
        for key in {"fill", "stroke"}:
            value = node.get(key, "")
            if value.startswith("url(#") and ids.get(value[5:-1]) != "linear_gradient":
                raise ValueError("Unknown gradient reference")
    return plan
