"""Safety regressions for the native Inkscape typed-vector example.

The ordinary suite does not launch a DCC or write to an operator profile.
These tests exercise caller-controlled data before any host process starts.
Synthetic SVG documents are test fixtures, never production artefacts.
"""

from __future__ import annotations

import copy
import importlib.util
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
from unittest.mock import Mock
from unittest.mock import patch
from xml.etree import ElementTree

import pytest

from dcc_mcp_core import ToolValidator

EXAMPLE = Path(__file__).resolve().parents[1] / "examples" / "native-inkscape"


def _load_example_module(name, dependencies=None):
    spec = importlib.util.spec_from_file_location("native_inkscape_" + name, EXAMPLE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    with patch.dict(sys.modules, dependencies or {}):
        spec.loader.exec_module(module)
    return module


plan_module = _load_example_module("plan")
runtime_module = _load_example_module("runtime", {"plan": plan_module})


@pytest.fixture
def vector_plan():
    return {
        "width": 64,
        "height": 64,
        "nodes": [
            {"type": "layer", "id": "editable", "label": "Editable shapes"},
            {"type": "group", "id": "mark", "parent": "editable"},
            {"type": "path", "id": "shape", "parent": "mark", "d": "M8,8 L56,8 L32,56 Z", "fill": "#1696d2"},
        ],
    }


def test_plan_preserves_editable_hierarchy_without_generating_svg(vector_plan):
    before = copy.deepcopy(vector_plan)
    result = plan_module.validate_plan(vector_plan)
    assert result["nodes"] == before["nodes"]
    assert result["view_box"] == [0, 0, 64, 64]
    assert vector_plan == before


@pytest.mark.parametrize("field", ["svg", "xml", "code", "actions", "script", "href", "onload"])
def test_arbitrary_document_or_executable_fields_are_rejected(vector_plan, field):
    vector_plan["nodes"][-1][field] = "<svg onload='run()'/>"
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


@pytest.mark.parametrize("value", [True, float("nan"), float("inf"), -float("inf"), "64"])
def test_nonfinite_or_ambiguous_canvas_values_are_rejected(vector_plan, value):
    vector_plan["width"] = value
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


@pytest.mark.parametrize("parent", ["missing", "shape", "mark;quit", [], {}])
def test_parent_must_be_an_earlier_container(vector_plan, parent):
    vector_plan["nodes"][1]["parent"] = parent
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


def test_duplicate_ids_cannot_redirect_editable_parent_relationships(vector_plan):
    vector_plan["nodes"][-1]["id"] = "mark"
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("type", []),
        ("type", {}),
        ("id", []),
        ("fill_rule", []),
        ("stroke_linecap", {}),
        ("text_anchor", []),
    ],
)
def test_invalid_json_field_types_fail_with_validation_errors(vector_plan, field, value):
    vector_plan["nodes"][-1][field] = value
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("font_family", "Arial;fill:url(https://invalid.example/image.svg)"),
        ("font_family", "Arial\n;stroke:red"),
        ("font_weight", "bold;filter:url(https://invalid.example/filter.svg)"),
        ("fill", "url(https://invalid.example/paint.svg)"),
        ("stroke", "url(#paint);filter:url(https://invalid.example/filter.svg)"),
    ],
)
def test_style_fields_cannot_inject_declarations_or_external_resources(vector_plan, field, value):
    vector_plan["nodes"][-1][field] = value
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


@pytest.mark.parametrize("data", ["M0,0 L1e99999,1", "MNaN,0 L1,1", "M0,0 LInfinity,1"])
def test_path_coordinates_must_remain_finite(vector_plan, data):
    vector_plan["nodes"][-1]["d"] = data
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


@pytest.mark.parametrize(
    "transform",
    ["translate(1e99999,0)", "rotate(NaN)", "url(https://invalid.example/object.svg)", "translate(1,2);quit"],
)
def test_transform_fields_are_bounded_geometry(vector_plan, transform):
    vector_plan["nodes"][-1]["transform"] = transform
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


def test_gradient_references_must_point_to_local_gradient_objects(vector_plan):
    vector_plan["nodes"][-1]["fill"] = "url(#mark)"
    with pytest.raises(ValueError):
        plan_module.validate_plan(vector_plan)


def test_whole_plan_size_is_bounded_before_launch(vector_plan):
    vector_plan["nodes"] = [
        {"type": "path", "id": "path_" + str(index), "d": "M0,0 " + "L1,1 " * 16000} for index in range(30)
    ]
    with pytest.raises(ValueError, match="request limit"):
        plan_module.validate_plan(vector_plan)


@pytest.mark.parametrize("value", ["mark;quit", "mark\nquit", "mark\rquit", "mark\x00quit"])
def test_action_arguments_cannot_append_another_native_action(value):
    with pytest.raises(ValueError):
        runtime_module.safe_action_value(value)


def test_action_path_preserves_windows_drive_separator_and_spaces():
    value = "C:\\Assets Folder\\wordmark.svg"
    assert runtime_module.safe_action_value(value) == value


@pytest.mark.parametrize("relative", ["../outside.svg", "../../outside.svg"])
def test_workspace_resolution_rejects_traversal(tmp_path, relative):
    root = tmp_path / "workspace"
    root.mkdir()
    with pytest.raises(ValueError, match="outside"):
        runtime_module.contained_path(root, relative, suffix=".svg")


def test_workspace_resolution_rejects_an_absolute_other_root(tmp_path):
    root = tmp_path / "workspace"
    root.mkdir()
    with pytest.raises(ValueError, match="outside"):
        runtime_module.contained_path(root, tmp_path / "outside.svg", suffix=".svg")


def test_workspace_resolution_rejects_symlink_escape(tmp_path):
    root = tmp_path / "workspace"
    outside = tmp_path / "outside"
    root.mkdir()
    outside.mkdir()
    link = root / "redirect"
    try:
        link.symlink_to(outside, target_is_directory=True)
    except (OSError, NotImplementedError):
        pytest.skip("Directory symlinks are unavailable for this test account")
    with pytest.raises(ValueError, match="outside"):
        runtime_module.contained_path(root, "redirect/output.svg", suffix=".svg")


@pytest.fixture
def native_evidence(tmp_path):
    return {
        "nonce": "unique-request",
        "self_call": "true",
        "parent_pid": 4321,
        "extension_pid": 4322,
        "parent_executable": str(tmp_path / "inkscape.exe"),
    }


def test_native_evidence_accepts_only_the_correlated_host_process(native_evidence, tmp_path):
    runtime_module.verify_evidence(native_evidence, "unique-request", 4321, tmp_path / "inkscape.exe")


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("nonce", "earlier-request"),
        ("self_call", "false"),
        ("parent_pid", 9876),
        ("extension_pid", 4321),
        ("extension_pid", 0),
        ("extension_pid", -1),
        ("extension_pid", True),
        ("extension_pid", "4322"),
        ("parent_executable", "different-application.exe"),
    ],
)
def test_stale_or_standalone_effect_evidence_is_rejected(native_evidence, tmp_path, field, value):
    native_evidence[field] = value
    with pytest.raises(RuntimeError):
        runtime_module.verify_evidence(native_evidence, "unique-request", 4321, tmp_path / "inkscape.exe")


@pytest.mark.parametrize("field", ["nonce", "self_call", "parent_pid", "extension_pid", "parent_executable"])
def test_missing_native_provenance_is_rejected(native_evidence, tmp_path, field):
    native_evidence.pop(field)
    with pytest.raises(RuntimeError):
        runtime_module.verify_evidence(native_evidence, "unique-request", 4321, tmp_path / "inkscape.exe")


@pytest.fixture
def runtime(tmp_path):
    executable = tmp_path / "bin" / "inkscape.exe"
    executable.parent.mkdir()
    executable.write_bytes(b"test double - never executed")
    return runtime_module.InkscapeRuntime(executable, tmp_path / "workspace")


def test_process_invocation_uses_fixed_argv_and_an_isolated_profile(runtime, monkeypatch):
    child = Mock(pid=4321, returncode=0)
    child.communicate.return_value = (b"host stdout", b"")
    popen = Mock(return_value=child)
    monkeypatch.setattr(runtime_module.subprocess, "Popen", popen)
    monkeypatch.setattr(runtime_module.uuid, "uuid4", lambda: Mock(hex="0" * 32))
    result = runtime._run(["--version"], timeout=7)
    command = popen.call_args.args[0]
    options = popen.call_args.kwargs
    assert command[0] == str(runtime.executable)
    assert command[1].startswith("--app-id-tag=")
    tag = command[1].split("=", 1)[1]
    assert tag.isascii()
    assert tag[0] in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_"
    assert command[2:] == ["--version"]
    assert "--active-window" not in command
    assert options["shell"] is False
    assert options["env"]["INKSCAPE_PROFILE_DIR"] == str(runtime.profile)
    child.communicate.assert_called_once_with(timeout=7)
    assert result["host_pid"] == 4321
    assert result["stdout"] == "host stdout"


def test_timeout_stops_only_the_spawned_process(runtime, monkeypatch):
    child = Mock(pid=4321, returncode=0)
    child.communicate.side_effect = [subprocess.TimeoutExpired("inkscape", 7), (b"", b"")]
    monkeypatch.setattr(runtime_module.subprocess, "Popen", Mock(return_value=child))
    with pytest.raises(RuntimeError, match="timed out"):
        runtime._run(["--version"], timeout=7)
    child.kill.assert_called_once_with()
    assert child.communicate.call_count == 2


def test_invalid_plan_never_launches_a_host(runtime, monkeypatch, vector_plan):
    vector_plan["nodes"][-1]["actions"] = "quit"
    run = Mock(side_effect=AssertionError("Host launch must not occur"))
    monkeypatch.setattr(runtime, "_run", run)
    with pytest.raises(ValueError):
        runtime.document_build("rejected.svg", vector_plan)
    run.assert_not_called()
    assert not (runtime.workspace / "rejected.svg").exists()


@pytest.mark.parametrize("failure", ["missing", "wrong-parent", "stale-nonce"])
def test_failed_native_evidence_never_publishes_an_output(runtime, monkeypatch, vector_plan, failure):
    def simulated_host(arguments, environment=None, timeout=120):
        request = json.loads(Path(environment["DCC_MCP_INKSCAPE_REQUEST"]).read_text(encoding="utf-8"))
        if failure != "missing":
            evidence = {
                "nonce": "stale" if failure == "stale-nonce" else request["nonce"],
                "self_call": "true",
                "parent_pid": 9876 if failure == "wrong-parent" else 4321,
                "extension_pid": 4322,
                "parent_executable": str(runtime.executable),
                "object_count": len(vector_plan["nodes"]),
            }
            Path(request["evidence_path"]).write_text(json.dumps(evidence), encoding="utf-8")
        return {"host_pid": 4321, "returncode": 0, "stdout": "", "stderr": "", "command": arguments}

    monkeypatch.setattr(runtime, "_run", simulated_host)
    with pytest.raises(RuntimeError):
        runtime.document_build("untrusted.svg", vector_plan)
    assert not (runtime.workspace / "untrusted.svg").exists()


def test_output_publication_refuses_an_existing_file(runtime, tmp_path):
    temporary = tmp_path / "simulated-host-output.bin"
    temporary.write_bytes(b"new content")
    output = runtime.workspace / "existing.bin"
    output.write_bytes(b"existing content")
    with pytest.raises(ValueError, match="overwrite"):
        runtime._commit(temporary, output, {"host_pid": 4321})
    assert output.read_bytes() == b"existing content"


def test_atomic_output_race_cannot_overwrite_another_file(runtime, monkeypatch, tmp_path):
    temporary = tmp_path / "simulated-host-output.bin"
    temporary.write_bytes(b"new content")
    output = runtime.workspace / "race.bin"

    def raced_link(source, destination):
        Path(destination).write_bytes(b"concurrent content")
        raise FileExistsError("another producer won publication")

    monkeypatch.setattr(runtime_module.os, "link", raced_link)
    with pytest.raises(FileExistsError):
        runtime._commit(temporary, output, {"host_pid": 4321})
    assert output.read_bytes() == b"concurrent content"


def _public_validator(tool_name):
    descriptor = json.loads((EXAMPLE / "skills" / "inkscape-vector" / "tools.yaml").read_text(encoding="utf-8"))
    tool = next(tool for tool in descriptor["tools"] if tool["name"] == tool_name)
    return ToolValidator.from_schema_json(json.dumps(tool["input_schema"]))


@pytest.mark.parametrize("weight", [800, 800.0])
def test_public_numeric_font_weight_matches_native_plan_validation(vector_plan, weight):
    vector_plan["nodes"][-1]["font_weight"] = weight
    public_plan = {"canvas": {"width": 64, "height": 64, "view_box": "0 0 64 64"}, "nodes": vector_plan["nodes"]}
    arguments = {"output_file": "mark.svg", "plan": public_plan}
    ok, errors = _public_validator("document_build").validate(json.dumps(arguments))
    assert ok, errors
    normalized = plan_module.validate_plan(public_plan)
    assert normalized["nodes"][-1]["font_weight"] == "800"
    assert public_plan["nodes"][-1]["font_weight"] == weight


@pytest.mark.parametrize("location", ["arguments", "plan", "node"])
def test_public_core_schema_rejects_untyped_actions(vector_plan, location):
    arguments = {
        "output_file": "mark.svg",
        "plan": {"canvas": {"width": 64, "height": 64}, "nodes": vector_plan["nodes"]},
    }
    destination = {"arguments": arguments, "plan": arguments["plan"], "node": vector_plan["nodes"][-1]}[location]
    destination["actions"] = "file-open:outside.svg;quit"
    ok, errors = _public_validator("document_build").validate(json.dumps(arguments))
    assert not ok
    assert errors


@pytest.mark.parametrize("format_name", ["ico", "icns", "svg;file-open:outside.svg"])
def test_public_export_schema_never_claims_unsupported_native_formats(format_name):
    arguments = {"source_file": "mark.svg", "output_file": "mark." + format_name, "format": format_name}
    ok, errors = _public_validator("document_export").validate(json.dumps(arguments))
    assert not ok
    assert errors


@pytest.fixture
def synthetic_native_svg():
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" '
        'xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" '
        'xmlns:xlink="http://www.w3.org/1999/xlink" '
        'width="64px" height="64px" viewBox="0 0 64 64">'
        '<defs><linearGradient id="paint"><stop offset="0" stop-color="#1696d2"/>'
        '<stop offset="1" stop-color="#00cab5"/></linearGradient></defs>'
        '<g id="editable" inkscape:groupmode="layer"><g id="mark">'
        '<path id="shape" d="M8,8 L56,8 L32,56 Z" fill="#1696d2"/>'
        "</g></g></svg>"
    )


def _synthetic_document(tmp_path, content, encoding="utf-8"):
    document = tmp_path / "synthetic-test-document.svg"
    document.write_bytes(content if isinstance(content, bytes) else content.encode(encoding))
    return document


@pytest.mark.parametrize("encoding", ["utf-8", "utf-8-sig"])
def test_vector_preflight_accepts_safe_utf8_document(tmp_path, synthetic_native_svg, encoding):
    source = _synthetic_document(tmp_path, synthetic_native_svg, encoding)
    tree = runtime_module.vector_preflight(source)
    assert tree.getroot().tag == "{http://www.w3.org/2000/svg}svg"


@pytest.mark.parametrize("encoding", ["utf-16", "utf-16-le", "utf-16-be", "utf-32"])
def test_vector_preflight_rejects_alternate_xml_encodings_before_parse(tmp_path, synthetic_native_svg, encoding):
    source = _synthetic_document(tmp_path, synthetic_native_svg, encoding)
    with pytest.raises(ValueError):
        runtime_module.vector_preflight(source)


def test_vector_preflight_rejects_utf8_nul_bytes_before_parse(tmp_path, synthetic_native_svg):
    source = _synthetic_document(tmp_path, synthetic_native_svg.replace("<defs>", "\x00<defs>"))
    with pytest.raises(ValueError, match="NUL"):
        runtime_module.vector_preflight(source)


@pytest.mark.parametrize(
    "declaration",
    [
        '<!DOCTYPE svg SYSTEM "https://invalid.example/document.dtd">',
        '<!DOCTYPE svg [<!ENTITY test "fixture">]>',
        '<?xml-stylesheet type="text/css" href="https://invalid.example/style.css"?>',
    ],
)
def test_vector_preflight_rejects_dtd_and_stylesheet_instructions(tmp_path, synthetic_native_svg, declaration):
    source = _synthetic_document(tmp_path, declaration + synthetic_native_svg)
    with pytest.raises(ValueError, match=r"DTD|stylesheet"):
        runtime_module.vector_preflight(source)


@pytest.mark.parametrize(
    "style",
    ["<style>.mark { fill: red; }</style>", '<style>@import url("https://invalid.example/style.css");</style>'],
)
def test_vector_preflight_rejects_style_elements(tmp_path, synthetic_native_svg, style):
    source = _synthetic_document(tmp_path, synthetic_native_svg.replace("<defs>", style + "<defs>"))
    with pytest.raises(ValueError, match="vector"):
        runtime_module.vector_preflight(source)


@pytest.mark.parametrize(
    "attribute",
    ["fill", "stroke", "filter", "clip-path", "mask", "marker-start", "marker-mid", "marker-end"],
)
def test_vector_preflight_rejects_external_presentation_resources(tmp_path, synthetic_native_svg, attribute):
    source = _synthetic_document(
        tmp_path,
        synthetic_native_svg.replace('fill="#1696d2"', attribute + '="url(https://invalid.example/resource.svg)"'),
    )
    with pytest.raises(ValueError, match="External"):
        runtime_module.vector_preflight(source)


@pytest.mark.parametrize(
    ("attribute", "value"),
    [
        ("fill", r"u\72l(https://invalid.example/paint.svg)"),
        ("style", r"fill:u\72l(#paint)"),
        ("marker-mid", r"url\28 https://invalid.example/marker.svg\29"),
        ("marker-mid", r"u\72l(#marker)"),
        ("filter", r"url(\68ttps://invalid.example/filter.svg)"),
        ("style", "@import 'https://invalid.example/style.css'"),
    ],
)
def test_vector_preflight_cannot_bypass_css_guard_with_escapes(tmp_path, synthetic_native_svg, attribute, value):
    source = _synthetic_document(
        tmp_path, synthetic_native_svg.replace('fill="#1696d2"', attribute + '="' + value + '"')
    )
    with pytest.raises(ValueError, match="CSS"):
        runtime_module.vector_preflight(source)


@pytest.mark.parametrize("attribute", ["href", "xlink:href"])
@pytest.mark.parametrize(
    "target",
    ["https://invalid.example/document.svg#shape", "file:///private/document.svg", "data:image/svg+xml;base64,AAAA"],
)
def test_vector_preflight_rejects_external_href_namespaces(tmp_path, synthetic_native_svg, attribute, target):
    source = _synthetic_document(
        tmp_path, synthetic_native_svg.replace('fill="#1696d2"', attribute + '="' + target + '"')
    )
    with pytest.raises(ValueError, match="external"):
        runtime_module.vector_preflight(source)


@pytest.mark.parametrize("attribute", ["onclick", "onload", "xml:base"])
def test_vector_preflight_rejects_executable_attributes_and_base_urls(tmp_path, synthetic_native_svg, attribute):
    source = _synthetic_document(
        tmp_path, synthetic_native_svg.replace('fill="#1696d2"', attribute + '="https://invalid.example/"')
    )
    with pytest.raises(ValueError):
        runtime_module.vector_preflight(source)


@pytest.mark.parametrize("presentation", ['fill="url(#paint)"', "fill=\"url('#paint')\"", 'xlink:href="#shape"'])
def test_vector_preflight_preserves_local_vector_references(tmp_path, synthetic_native_svg, presentation):
    source = _synthetic_document(tmp_path, synthetic_native_svg.replace('fill="#1696d2"', presentation))
    runtime_module.vector_preflight(source)


def test_document_verification_accepts_requested_ids_grouping_and_canvas(tmp_path, synthetic_native_svg, vector_plan):
    source = _synthetic_document(tmp_path, synthetic_native_svg)
    runtime_module.verify_document(source, plan_module.validate_plan(vector_plan))


@pytest.mark.parametrize(
    "mutation",
    [
        "missing-id",
        "duplicate-id",
        "wrong-type",
        "wrong-namespace",
        "missing-layer-mode",
        "wrong-layer-mode",
        "parent",
        "width",
        "height",
        "viewbox",
        "missing-viewbox",
    ],
)
def test_failed_document_acceptance_never_publishes_output(
    runtime, monkeypatch, vector_plan, synthetic_native_svg, mutation
):
    fixture_tree = ElementTree.fromstring(synthetic_native_svg)
    layer = next(element for element in fixture_tree.iter() if element.get("id") == "editable")
    group = next(element for element in fixture_tree.iter() if element.get("id") == "mark")
    shape = next(element for element in fixture_tree.iter() if element.get("id") == "shape")
    if mutation == "missing-id":
        shape.attrib.pop("id")
    elif mutation == "duplicate-id":
        group.append(copy.deepcopy(shape))
    elif mutation == "wrong-type":
        shape.tag = "{http://www.w3.org/2000/svg}circle"
    elif mutation == "wrong-namespace":
        shape.tag = "{https://invalid.example/foreign}path"
    elif mutation == "missing-layer-mode":
        layer.attrib.pop("{http://www.inkscape.org/namespaces/inkscape}groupmode")
    elif mutation == "wrong-layer-mode":
        layer.set("{http://www.inkscape.org/namespaces/inkscape}groupmode", "group")
    elif mutation == "parent":
        group.remove(shape)
        layer.append(shape)
    elif mutation in {"width", "height"}:
        fixture_tree.set(mutation, "32px")
    elif mutation == "viewbox":
        fixture_tree.set("viewBox", "0 0 32 64")
    elif mutation == "missing-viewbox":
        fixture_tree.attrib.pop("viewBox")

    def simulated_host(arguments, environment=None, timeout=120):
        request_path = Path(environment["DCC_MCP_INKSCAPE_REQUEST"])
        request = json.loads(request_path.read_text(encoding="utf-8"))
        evidence = {
            "nonce": request["nonce"],
            "self_call": "true",
            "parent_pid": 4321,
            "extension_pid": 4322,
            "parent_executable": str(runtime.executable),
            "object_count": len(vector_plan["nodes"]),
        }
        Path(request["evidence_path"]).write_text(json.dumps(evidence), encoding="utf-8")
        ElementTree.ElementTree(fixture_tree).write(request_path.parent / "result.svg", encoding="utf-8")
        return {"host_pid": 4321, "returncode": 0, "stdout": "", "stderr": "", "command": arguments}

    monkeypatch.setattr(runtime, "_run", simulated_host)
    commit = Mock(side_effect=AssertionError("An unverified native document must never be published"))
    monkeypatch.setattr(runtime, "_commit", commit)
    with pytest.raises(RuntimeError):
        runtime.document_build("rejected-document.svg", vector_plan)
    commit.assert_not_called()
    assert not (runtime.workspace / "rejected-document.svg").exists()


def test_gui_invocation_uses_a_safe_separate_application_tag(runtime, monkeypatch, synthetic_native_svg):
    source = _synthetic_document(runtime.workspace, synthetic_native_svg)
    popen = Mock(return_value=Mock(pid=4321))
    monkeypatch.setattr(runtime_module.subprocess, "Popen", popen)
    monkeypatch.setattr(runtime_module.uuid, "uuid4", lambda: Mock(hex="0" * 32))
    runtime.document_open(str(source))
    command = popen.call_args.args[0]
    tag = command[1].split("=", 1)[1]
    assert tag.isascii()
    assert tag[0] in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_"
    assert "--active-window" not in command
    assert popen.call_args.kwargs["shell"] is False


@pytest.mark.parametrize("operation", ["document_export", "document_inspect", "document_open"])
def test_preflight_blocks_external_resources_before_every_native_entry(
    runtime, monkeypatch, synthetic_native_svg, operation
):
    unsafe_fixture = synthetic_native_svg.replace(
        'fill="#1696d2"', r'marker-mid="u\72l(https://invalid.example/marker.svg)"'
    )
    source = _synthetic_document(runtime.workspace, unsafe_fixture)
    run = Mock(side_effect=AssertionError("Untrusted document must never reach native execution"))
    popen = Mock(side_effect=AssertionError("Untrusted document must never open a GUI"))
    monkeypatch.setattr(runtime, "_run", run)
    monkeypatch.setattr(runtime_module.subprocess, "Popen", popen)
    arguments = [str(source), "blocked.png"] if operation == "document_export" else [str(source)]
    with pytest.raises(ValueError, match="CSS"):
        getattr(runtime, operation)(*arguments)
    run.assert_not_called()
    popen.assert_not_called()
    assert not (runtime.workspace / "blocked.png").exists()


@pytest.mark.dcc
@pytest.mark.skipif(
    os.environ.get("DCC_MCP_INKSCAPE_LIVE_TEST") != "1" or not os.environ.get("DCC_MCP_INKSCAPE_EXE"),
    reason="Native host smoke requires explicit live-test opt-in and an operator-configured Inkscape executable",
)
def test_opted_in_native_host_build_reopen_and_png_export(tmp_path, vector_plan):
    runtime = runtime_module.InkscapeRuntime(os.environ["DCC_MCP_INKSCAPE_EXE"], tmp_path / "isolated-live-workspace")
    built = runtime.document_build("editable.svg", vector_plan)
    evidence = built["native_effect"]
    assert evidence["object_count"] == 3
    assert evidence["parent_pid"] == built["host_invocation"]["host_pid"]
    assert evidence["self_call"] == "true"
    assert Path(evidence["parent_executable"]).resolve() == runtime.executable
    inspected = runtime.document_inspect("editable.svg")
    assert inspected["element_counts"]["path"] == 1
    assert inspected["element_counts"]["g"] >= 2
    root = ElementTree.parse(built["output_path"]).getroot()
    layer = next(element for element in root.iter() if element.get("id") == "editable")
    assert layer.get("{http://www.inkscape.org/namespaces/inkscape}groupmode") == "layer"
    assert any(element.get("id") == "mark" for element in layer)
    assert "shape," in inspected["geometry"]
    exported = runtime.document_export("editable.svg", "mark-32.png", width=32, height=32, background_opacity=0)
    png = Path(exported["output_path"]).read_bytes()
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    assert struct.unpack(">II", png[16:24]) == (32, 32)
    assert png[25] in {4, 6}, "Transparent PNG export must preserve an alpha channel"
