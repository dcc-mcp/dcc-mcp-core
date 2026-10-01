"""Bounded Inkscape CLI bridge; only Inkscape writes vector output files."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import uuid
from xml.etree import ElementTree

from plan import number
from plan import validate_plan

ACTION = "org.dcc-mcp.typed-vector-plan"
HERE = Path(__file__).resolve().parent


def safe_action_value(value):
    """Prevent Inkscape action separators from entering a typed argument."""
    text = str(value)
    if any(char in text for char in ";\r\n\x00"):
        raise ValueError("Action arguments cannot contain separators or control characters")
    return text


def contained_path(root, value, suffix=None, existing=False):
    """Resolve paths against an operator-owned root, including symlinks."""
    path = Path(value)
    path = (path if path.is_absolute() else root / path).resolve()
    try:
        path.relative_to(root.resolve())
    except ValueError as exc:
        raise ValueError("File path is outside the configured workspace") from exc
    safe_action_value(path)
    if suffix is not None and path.suffix.lower() != suffix:
        raise ValueError("File suffix must be " + suffix)
    if existing and not path.is_file():
        raise ValueError("Input file does not exist")
    return path


def verify_evidence(evidence, nonce, host_pid, executable):
    """Do not accept a standalone extension run as native host execution."""
    if not isinstance(evidence, dict) or evidence.get("nonce") != nonce or evidence.get("self_call") != "true":
        raise RuntimeError("Native effect evidence has invalid invocation correlation")
    for field in ("extension_pid", "parent_pid"):
        if isinstance(evidence.get(field), bool) or not isinstance(evidence.get(field), int) or evidence[field] <= 0:
            raise RuntimeError("Native effect evidence has invalid process identity")
    if evidence.get("parent_pid") != host_pid or evidence.get("extension_pid") == host_pid:
        raise RuntimeError("The effect was not executed by this Inkscape process")
    parent_image = evidence.get("parent_executable")
    if not parent_image or Path(parent_image).resolve() != executable.resolve():
        raise RuntimeError("Native effect parent executable does not match Inkscape")


def vector_preflight(source):
    """Reject executable objects and external resources before native reopening."""
    if source.stat().st_size > 30000000:
        raise ValueError("SVG input exceeds the 30 MB document limit")
    raw = source.read_bytes()
    try:
        raw.decode("utf-8-sig")
    except UnicodeError as exc:
        raise ValueError("Only UTF-8 vector SVG documents are accepted") from exc
    if b"\x00" in raw:
        raise ValueError("NUL bytes and alternate XML encodings are not accepted")
    if b"<!doctype" in raw.lower() or b"<?xml-stylesheet" in raw.lower():
        raise ValueError("DTD and stylesheet processing instructions are not accepted")
    tree = ElementTree.parse(source)
    if tree.getroot().tag != "{http://www.w3.org/2000/svg}svg":
        raise ValueError("Input must be an SVG document")
    for element in tree.iter():
        if element.tag.rsplit("}", 1)[-1] in {"image", "script", "foreignObject", "style"}:
            raise ValueError("Only native vector SVG objects are accepted")
        for key, value in element.attrib.items():
            local = key.rsplit("}", 1)[-1]
            if (
                local.lower().startswith("on")
                or key == "{http://www.w3.org/XML/1998/namespace}base"
                or (local == "href" and not re.fullmatch(r"#[A-Za-z_][A-Za-z0-9_.-]{0,95}", value))
            ):
                raise ValueError("Executable attributes and external references are not accepted")
            if local in {
                "style",
                "fill",
                "stroke",
                "filter",
                "clip-path",
                "mask",
                "marker-start",
                "marker-mid",
                "marker-end",
            } and ("@" in value or "\\" in value):
                raise ValueError("CSS escapes and imports are not accepted")
            for url in re.findall(r"url\(([^)]*)\)", value, re.IGNORECASE):
                if not re.fullmatch(r"#[A-Za-z_][A-Za-z0-9_.-]{0,95}", url.strip().strip("'\"")):
                    raise ValueError("External CSS resources are not accepted")
    return tree


def verify_document(source, plan):
    """Prove that the native host committed the requested vector objects."""
    tree = vector_preflight(source)
    root = tree.getroot()
    parents = {child: parent for parent in tree.iter() for child in parent}
    identities = {}
    for element in tree.iter():
        identities.setdefault(element.get("id"), []).append(element)
    kinds = {"layer": "g", "group": "g", "linear_gradient": "linearGradient"}
    for node in plan["nodes"]:
        matches = identities.get(node["id"], [])
        if len(matches) != 1 or matches[0].tag != "{http://www.w3.org/2000/svg}" + kinds.get(
            node["type"], node["type"]
        ):
            raise RuntimeError("Inkscape did not commit the expected native object: " + node["id"])
        if (
            node["type"] == "layer"
            and matches[0].get("{http://www.inkscape.org/namespaces/inkscape}groupmode") != "layer"
        ):
            raise RuntimeError("Inkscape did not preserve the native layer semantics")
        parent = parents.get(matches[0])
        if node.get("parent") and (parent is None or parent.get("id") != node["parent"]):
            raise RuntimeError("Inkscape did not preserve the requested native grouping")
        if node["type"] == "linear_gradient" and parent.tag.rsplit("}", 1)[-1] != "defs":
            raise RuntimeError("Inkscape did not commit the native gradient definition")
    for key in ("width", "height"):
        dimension = root.get(key, "0")
        if float(dimension[:-2] if dimension.endswith("px") else dimension) != plan[key]:
            raise RuntimeError("Inkscape did not preserve the requested canvas")
    if [float(item) for item in root.get("viewBox", "").split()] != plan["view_box"]:
        raise RuntimeError("Inkscape did not preserve the requested viewBox")
    return tree


class InkscapeRuntime:
    """A standalone controller launching isolated native Inkscape operations."""

    def __init__(self, executable, workspace, state_dir=None, font_dirs=()):
        self.executable = Path(executable).resolve()
        if self.executable.suffix.lower() == ".com":
            self.executable = self.executable.with_suffix(".exe")
        if not self.executable.is_file() or self.executable.stem.lower() != "inkscape":
            raise ValueError("An existing Inkscape executable must be configured")
        self.workspace = Path(workspace).resolve()
        self.workspace.mkdir(parents=True, exist_ok=True)
        self.state = contained_path(self.workspace, state_dir or ".inkscape-mcp")
        self.profile = self.state / "profile"
        extensions = self.profile / "extensions"
        extensions.mkdir(parents=True, exist_ok=True)
        for filename in ("dcc_mcp_vector.inx", "dcc_mcp_vector.py"):
            shutil.copy2(HERE / "extension" / filename, extensions / filename)
        shutil.copy2(HERE / "plan.py", extensions / "plan.py")
        self.environment = dict(os.environ)
        self.environment["INKSCAPE_PROFILE_DIR"] = str(self.profile)
        self.environment.pop("DCC_MCP_INKSCAPE_REQUEST", None)
        self.font_config = None
        if font_dirs:
            default = self.executable.parent.parent / "etc" / "fonts" / "fonts.conf"
            if not default.is_file():
                default = self.executable.parent / "etc" / "fonts" / "fonts.conf"
            if not default.is_file():
                raise ValueError("Cannot locate Inkscape's default fontconfig; font setup is unavailable")
            config = ElementTree.Element("fontconfig")
            ElementTree.SubElement(config, "include").text = str(default)
            for directory in font_dirs:
                directory = Path(directory).resolve()
                if not directory.is_dir():
                    raise ValueError("Font directory does not exist")
                ElementTree.SubElement(config, "dir").text = str(directory)
            ElementTree.SubElement(config, "cachedir").text = str(self.state / "font-cache")
            self.font_config = self.state / "fonts.conf"
            ElementTree.ElementTree(config).write(self.font_config, encoding="utf-8", xml_declaration=True)
            self.environment["FONTCONFIG_FILE"] = str(self.font_config)

    def _run(self, arguments, environment=None, timeout=120):
        command = [str(self.executable), "--app-id-tag=dccmcp_" + uuid.uuid4().hex, *arguments]
        process = subprocess.Popen(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment or self.environment,
            shell=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired as exc:
            process.kill()
            process.communicate()
            raise RuntimeError("Inkscape operation timed out; the owned process was stopped") from exc
        result = {
            "host_pid": process.pid,
            "returncode": process.returncode,
            "stdout": stdout.decode("utf-8", errors="replace")[:65536],
            "stdout_bytes": len(stdout),
            "stdout_truncated": len(stdout) > 65536,
            "stderr": stderr.decode("utf-8", errors="replace")[-16000:],
            "command": command,
        }
        if process.returncode:
            raise RuntimeError("Inkscape failed: " + result["stderr"][-2000:])
        return result

    def capabilities(self):
        """Query the actual binary rather than advertising guessed support."""
        version = self._run(["--version"], timeout=30)["stdout"].strip()
        action_report = self._run(["--action-list"], timeout=60)
        return {
            "version": version,
            "executable": str(self.executable),
            "profile": str(self.profile),
            "font_config": str(self.font_config) if self.font_config else None,
            "actions": action_report["stdout"],
            "actions_truncated": action_report["stdout_truncated"],
            "runtime_shape": "standalone native CLI controller with host-invoked inkex effect",
            "native_formats": ["svg", "png", "pdf"],
            "limitations": ["No persistent live GUI document binding", "ICO and ICNS require packaging exported PNGs"],
        }

    def _output(self, filename, suffix):
        path = contained_path(self.workspace, filename, suffix=suffix)
        if path.exists():
            raise ValueError("Output already exists; use a new filename")
        path.parent.mkdir(parents=True, exist_ok=True)
        return path

    def _invocation(self):
        nonce = uuid.uuid4().hex
        directory = self.state / "invocations" / nonce
        directory.mkdir(parents=True)
        return nonce, directory

    def _commit(self, temporary, output, invocation, evidence=None):
        if not temporary.is_file() or temporary.stat().st_size == 0:
            raise RuntimeError("Inkscape did not produce a nonempty output")
        if output.exists():
            raise ValueError("Output appeared during processing; refusing to overwrite")
        # A hard link publishes the already-written file atomically without replacing an existing path.
        os.link(temporary, output)
        report = {
            "output_path": str(output),
            "bytes": output.stat().st_size,
            "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
            "producer": "Inkscape",
            "host_invocation": invocation,
        }
        if evidence is not None:
            report["native_effect"] = evidence
        report_path = temporary.parent / "invocation.json"
        report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
        report["evidence_path"] = str(report_path)
        return report

    def document_build(self, output_file, plan):
        """Build native layers and vector objects through an Inkscape effect."""
        plan = validate_plan(plan)
        output = self._output(output_file, ".svg")
        nonce, directory = self._invocation()
        evidence_path = directory / "effect.json"
        request_path = directory / "request.json"
        request_path.write_text(
            json.dumps({"nonce": nonce, "evidence_path": str(evidence_path), "plan": plan}), encoding="utf-8"
        )
        temporary = directory / "result.svg"
        environment = dict(self.environment, DCC_MCP_INKSCAPE_REQUEST=str(request_path))
        actions = f"file-new:;{ACTION};export-type:svg;export-filename:{safe_action_value(temporary)};export-do"
        invocation = self._run(["--actions=" + actions], environment=environment)
        if not evidence_path.is_file():
            raise RuntimeError("Inkscape did not execute the native extension; no effect evidence")
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        verify_evidence(evidence, nonce, invocation["host_pid"], self.executable)
        if evidence.get("object_count") != len(plan["nodes"]):
            raise RuntimeError("Native effect object count does not match the vector plan")
        verify_document(temporary, plan)
        return self._commit(temporary, output, invocation, evidence)

    def document_export(
        self,
        source_file,
        output_file,
        format="png",
        width=None,
        height=None,
        background="#ffffff",
        background_opacity=0,
        plain_svg=False,
        text_to_path=False,
    ):
        """Export or convert the document with actual Inkscape actions."""
        if format not in {"svg", "png", "pdf"}:
            raise ValueError("Supported native export formats are svg, png, and pdf")
        source = contained_path(self.workspace, source_file, suffix=".svg", existing=True)
        vector_preflight(source)
        output = self._output(output_file, "." + format)
        number(background_opacity, "background_opacity", 0, 1)
        if not isinstance(background, str) or not re.fullmatch(r"#[0-9a-fA-F]{6}", background):
            raise ValueError("background must be #RRGGBB")
        if plain_svg and format != "svg":
            raise ValueError("plain_svg only applies to SVG export")
        _, directory = self._invocation()
        temporary = directory / ("result." + format)
        actions = []
        if text_to_path:
            actions += ["select-by-element:text", "object-to-path", "select-clear"]
        actions += [
            "export-area-page",
            "export-type:" + format,
            "export-filename:" + safe_action_value(temporary),
            "export-background:" + background,
            "export-background-opacity:" + str(background_opacity),
        ]
        if plain_svg:
            actions.append("export-plain-svg")
        for name, value in (("width", width), ("height", height)):
            if value is not None:
                number(value, name, 1, 32768)
                actions.append(f"export-{name}:{value}")
        actions.append("export-do")
        invocation = self._run([str(source), "--actions=" + ";".join(actions)])
        if (
            format == "svg"
            and text_to_path
            and temporary.is_file()
            and any(element.tag.rsplit("}", 1)[-1] == "text" for element in ElementTree.parse(temporary).iter())
        ):
            raise RuntimeError("Inkscape did not convert every text object to paths")
        return self._commit(temporary, output, invocation)

    def document_inspect(self, source_file):
        """Reopen and query geometry using the native software."""
        source = contained_path(self.workspace, source_file, suffix=".svg", existing=True)
        vector_preflight(source)
        invocation = self._run([str(source), "--query-all"])
        counts = {}
        for element in ElementTree.parse(source).iter():
            name = element.tag.rsplit("}", 1)[-1]
            counts[name] = counts.get(name, 0) + 1
        return {
            "source_path": str(source),
            "geometry": invocation["stdout"],
            "element_counts": counts,
            "host_invocation": invocation,
        }

    def document_open(self, source_file):
        """Open a separate native GUI process for visual acceptance."""
        source = contained_path(self.workspace, source_file, suffix=".svg", existing=True)
        vector_preflight(source)
        process = subprocess.Popen(
            [str(self.executable), "--app-id-tag=dccmcp_" + uuid.uuid4().hex, "--with-gui", str(source)],
            env=self.environment,
            shell=False,
        )
        return {
            "host_pid": process.pid,
            "source_path": str(source),
            "producer": "Inkscape GUI",
            "accepted": False,
            "next_step": "Observe this exact process with the existing scoped app-ui workflow",
        }


def configured_runtime():
    """Read explicit operator configuration; no software installation or discovery."""
    return InkscapeRuntime(
        os.environ["DCC_MCP_INKSCAPE_EXE"],
        os.environ["DCC_MCP_INKSCAPE_WORKSPACE"],
        font_dirs=[value for value in os.environ.get("DCC_MCP_INKSCAPE_FONT_DIRS", "").split(os.pathsep) if value],
    )
