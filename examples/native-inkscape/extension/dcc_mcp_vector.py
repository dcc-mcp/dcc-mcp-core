"""Native Inkscape effect, invoked exclusively by an Inkscape document action.

This extension returns the edited document through inkex's normal stdout
protocol. It never writes an SVG file; Inkscape owns committing and exporting.
"""

import json
import os
from pathlib import Path

import inkex
from plan import GEOMETRY
from plan import STYLE
from plan import validate_plan


def parent_executable(pid):
    """Read the parent process image for local pipeline provenance."""
    if os.name != "nt":
        return os.readlink(f"/proc/{pid}/exe")
    import ctypes
    from ctypes import wintypes

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.LPWSTR,
        ctypes.POINTER(wintypes.DWORD),
    ]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel.OpenProcess(0x1000, False, pid)
    if not handle:
        raise inkex.AbortExtension("Cannot verify the Inkscape parent process")
    try:
        buffer = ctypes.create_unicode_buffer(32768)
        size = wintypes.DWORD(len(buffer))
        if not kernel.QueryFullProcessImageNameW(handle, 0, buffer, ctypes.byref(size)):
            raise inkex.AbortExtension("Cannot read the Inkscape parent executable")
        return buffer.value
    finally:
        kernel.CloseHandle(handle)


class TypedVectorPlan(inkex.EffectExtension):
    """Apply a bounded vector plan to the document supplied by Inkscape."""

    def effect(self):
        if os.environ.get("SELF_CALL") != "true":
            raise inkex.AbortExtension("This effect must be invoked by the Inkscape host")
        request = json.loads(Path(os.environ["DCC_MCP_INKSCAPE_REQUEST"]).read_text(encoding="utf-8"))
        plan = validate_plan(request["plan"])
        self.svg.set("width", str(plan["width"]) + "px")
        self.svg.set("height", str(plan["height"]) + "px")
        self.svg.set("viewBox", " ".join(str(n) for n in plan.get("view_box", [0, 0, plan["width"], plan["height"]])))
        elements = {}
        classes = {
            "group": inkex.Group,
            "layer": inkex.Layer,
            "path": inkex.PathElement,
            "rect": inkex.Rectangle,
            "circle": inkex.Circle,
            "ellipse": inkex.Ellipse,
            "text": inkex.TextElement,
            "linear_gradient": inkex.LinearGradient,
        }
        for node in plan["nodes"]:
            element = classes[node["type"]]()
            element.set("id", node["id"])
            if "label" in node:
                element.set(inkex.addNS("label", "inkscape"), node["label"])
            for key in GEOMETRY:
                if key in node:
                    element.set(key, str(node[key]))
            for key in {"d", "transform"}:
                if key in node:
                    element.set(key, node[key])
            for key in STYLE:
                if key in node:
                    element.style[key.replace("_", "-")] = str(node[key])
            if node["type"] == "text":
                element.text = node["text"]
            if node["type"] == "linear_gradient":
                element.set("gradientUnits", node.get("gradient_units", "userSpaceOnUse"))
                if "gradient_transform" in node:
                    element.set("gradientTransform", node["gradient_transform"])
                for item in node["stops"]:
                    stop = inkex.Stop()
                    stop.set("offset", str(item["offset"]))
                    stop.style["stop-color"] = item["color"]
                    stop.style["stop-opacity"] = str(item.get("opacity", 1))
                    element.append(stop)
                self.svg.defs.append(element)
            else:
                elements.get(node.get("parent"), self.svg).append(element)
            elements[node["id"]] = element
        parent_probe_error = None
        try:
            parent_image = parent_executable(os.getppid())
        except (inkex.AbortExtension, OSError) as exc:
            parent_image = None
            parent_probe_error = str(exc)
        report = {
            "nonce": request["nonce"],
            "extension_pid": os.getpid(),
            "parent_pid": os.getppid(),
            "self_call": os.environ.get("SELF_CALL"),
            "parent_executable": parent_image,
            "parent_probe_error": parent_probe_error,
            "document_path": os.environ.get("DOCUMENT_PATH"),
            "object_count": len(elements),
            "producer": "Inkscape native inkex effect",
        }
        if os.name == "nt":
            from windows_process import query_process

            lineage = []
            process_id = os.getpid()
            for _ in range(3):
                try:
                    information = query_process(process_id)
                except OSError as exc:
                    lineage.append({"requested_pid": process_id, "error": str(exc)})
                    break
                lineage.append(information)
                process_id = information["parent_pid"]
            report["windows_process_lineage"] = lineage
        Path(request["evidence_path"]).write_text(json.dumps(report, indent=2), encoding="utf-8")


if __name__ == "__main__":
    TypedVectorPlan().run()
