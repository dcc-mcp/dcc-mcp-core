"""Secondary frame contracts; fake pipes never observe or operate an application."""

import base64
from copy import deepcopy
from dataclasses import replace
import importlib.util
import json
import struct
import zlib

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_popup import requested_popup_handles
from test_cua_mcp_pixels import PNG
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import pixel_response
from test_cua_mcp_pixels import runtime


def png_chunk(kind, payload):
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))


def popup_png(width=80, height=40, *, compressed=None):
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    rows = (b"\0" + b"\x44" * width * 4) * height
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", header)
        + png_chunk(b"IDAT", zlib.compress(rows) if compressed is None else compressed)
        + png_chunk(b"IEND", b"")
    )


POPUP_PNG = popup_png()


def enable(config, process, handles=(501,)):
    def catalog(value):
        value["tools"][0]["inputSchema"]["properties"]["allow_native_capture_popup_keyboard"] = {"type": "boolean"}

    def snapshot(raw):
        overlays = []
        offset = len(PNG)
        for index, handle in enumerate(handles, 1):
            observation = deepcopy(raw["observation"])
            observation.update(
                window_handle=handle,
                observation_id="{}-popup-{}".format(raw["observation_id"], handle),
                width=79 + index,
                height=40,
                source_rect=[-600, 20, 79 + index, 40],
            )
            observation["capture_provenance"].update(
                window_handle=handle,
                native_window_bounds=observation["source_rect"],
                native_visible_bounds=observation["source_rect"],
                capture_generation=raw["observation"]["capture_provenance"]["capture_generation"] + index,
            )
            overlay = {
                key: observation[key] for key in ("window_handle", "observation_id", "source_rect", "width", "height")
            }
            length = len(popup_png(79 + index))
            overlay.update(
                observation=observation,
                native_instance=deepcopy(observation["capture_provenance"]["native_instance"]),
                image={
                    "index": index,
                    "offset": offset,
                    "length": length,
                    "mime_type": "image/png",
                    "encoding": "binary_frame",
                },
            )
            offset += length
            overlays.append(overlay)
        raw["native_capture_popup_observation"] = {
            "purpose": "main_root_keyboard",
            "one_use": True,
            "max_age_ms": 5000,
            "root_observation_id": raw["observation_id"],
            "capture_window_handle": handles[0],
            "selected_window_handles": list(handles),
            "overlays": overlays,
        }

    process.mutate_catalog = catalog
    process.mutate_open = lambda raw: raw.update(allow_native_capture_popup_keyboard=True)
    process.expected_snapshot_params = {"native_capture_popup_window_handles": list(handles)}
    process.extra_images = [
        {"type": "image", "mimeType": "image/png", "data": base64.b64encode(popup_png(80 + index)).decode()}
        for index in range(len(handles))
    ]
    process.mutate_snapshot = snapshot
    return replace(config, allow_native_capture_popup_keyboard=True)


@pytest.mark.parametrize("handles", [[True], [0], [500], [501, 501], list(range(501, 510)), [2**64], "501"])
def test_secondary_selection_is_explicit_unique_and_bounded(handles):
    with pytest.raises(CuaCliError):
        requested_popup_handles(handles, allowed=True, root_handle=500)


def test_no_ceiling_never_authorizes_secondary_observation():
    assert requested_popup_handles(None, allowed=False, root_handle=500) == []
    with pytest.raises(CuaCliError, match="owner did not grant"):
        requested_popup_handles([501], allowed=False, root_handle=500)


@pytest.mark.parametrize("dcc_type", ["nuke", "maya"])
def test_separate_popup_png_and_single_keyboard_keep_main_target(runtime, dcc_type):
    config, process, _ = runtime
    owned = client(enable(config, process), dcc_type)
    captured = owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[501])
    assert captured["image_bytes"] == PNG
    assert captured["observation"]["window_handle"] == TARGET["window_handle"]
    assert captured["native_capture_popup_overlays"][0]["image_bytes"] == POPUP_PNG
    result = owned.execute({"action": "type", "input_kind": "raw_input", "text": "NoOp"})
    assert result["success"] is True
    sent = process.requests[-1]["params"]["arguments"]
    assert sent["params"]["observation_id"] == "native-obs-1"
    assert sent["params"]["capture_after"] is False
    with pytest.raises(CuaCliError, match="fresh pixels"):
        owned.execute({"action": "type", "input_kind": "raw_input", "text": "second"})
    owned.stop()


@pytest.mark.parametrize(
    "action", [{"action": "click", "x": 1, "y": 1}, {"action": "keypress", "keys": ["W"], "duration_ms": 10}]
)
def test_composite_cannot_authorize_pointer_or_held_keys_and_is_consumed(runtime, action):
    config, process, _ = runtime
    owned = client(enable(config, process))
    owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[501])
    before = len(process.requests)
    with pytest.raises(CuaCliError, match="unheld keyboard"):
        owned.execute({"input_kind": "raw_input", **action})
    assert len(process.requests) == before
    with pytest.raises(CuaCliError, match="fresh pixels"):
        owned.execute({"action": "type", "input_kind": "raw_input", "text": "retry"})
    owned.stop()


@pytest.mark.parametrize(
    "path,value",
    [
        (("purpose",), "pointer"),
        (("one_use",), False),
        (("max_age_ms",), True),
        (("root_observation_id",), "foreign"),
        (("capture_window_handle",), 502),
        (("selected_window_handles",), [502]),
        (("overlays", 0, "window_handle"), 502),
        (("overlays", 0, "observation_id"), "native-obs-1"),
        (("overlays", 0, "observation_id"), "native-obs-0-popup-501"),
        (("overlays", 0, "observation", "process_id"), 43),
        (("overlays", 0, "observation", "session_id"), "different-session"),
        (("overlays", 0, "native_instance", "process_creation_time_100ns"), 999),
        (("overlays", 0, "observation", "capture_provenance", "window_handle"), 502),
        (("overlays", 0, "observation", "capture_provenance", "capture_generation"), 1),
        (("overlays", 0, "source_rect"), [-600, 20, 81, 40]),
        (("overlays", 0, "image", "length"), 1000),
        (("overlays", 0, "image", "mime_type"), "image/jpeg"),
        (("overlays", 0, "image", "encoding"), "shared_memory"),
        (("overlays", 0, "image", "index"), 0),
        (("overlays", 0, "image", "offset"), 0),
    ],
)
def test_malformed_secondary_evidence_closes_owned_task(runtime, path, value):
    config, process, _ = runtime
    config = enable(config, process)
    original = process.mutate_snapshot

    def corrupt(raw):
        original(raw)
        destination = raw["native_capture_popup_observation"]
        for key in path[:-1]:
            destination = destination[key]
        destination[path[-1]] = value

    process.mutate_snapshot = corrupt
    owned = client(config)
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[501])
    assert process.returncode == 0


def test_bundled_projection_preserves_separate_labeled_artifacts(runtime, monkeypatch):
    config, process, launches = runtime
    config = enable(config, process)
    spec = importlib.util.spec_from_file_location("popup_backend_test", SCRIPTS / "_cua_backend.py")
    backend = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(backend)
    params = {
        "session_id": "popup-test",
        "trusted_ui_control_runtime": config,
        "trusted_adapter_scope": {"dcc_type": "nuke", **TARGET},
        "native_capture_popup_window_handles": [501],
    }
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "1")
    try:
        output = backend.snapshot_tool(params)
        assert output["success"] is True
        context = output["context"]
        assert base64.b64decode(context["__rich__"]["data"]) == PNG
        popup = context["native_capture_popup_overlays"][0]
        assert base64.b64decode(popup["__rich__"]["data"]) == POPUP_PNG
        assert "501" in popup["__rich__"]["alt"]
        assert "image_bytes" not in popup
        json.dumps(output)
    finally:
        backend.cleanup()
    assert len(launches) == 1


def test_invalid_owner_flag_rejected(runtime):
    config, _, _ = runtime
    with pytest.raises(TypeError):
        replace(config, allow_native_capture_popup_keyboard=1)


@pytest.mark.parametrize("trusted", [False, True])
def test_disabled_popup_ceiling_refuses_before_any_child_is_opened(runtime, monkeypatch, trusted):
    config, process, launches = runtime
    spec = importlib.util.spec_from_file_location("popup_disabled_backend", SCRIPTS / "_cua_backend.py")
    backend = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(backend)
    params = {
        "session_id": "disabled-popup",
        "trusted_adapter_scope": {"dcc_type": "nuke", **TARGET},
        "native_capture_popup_window_handles": [501],
    }
    if trusted:
        params["trusted_ui_control_runtime"] = config
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "1")
    try:
        output = backend.snapshot_tool(params)
        assert output["success"] is False
        assert not launches and not process.requests
    finally:
        backend.cleanup()


def test_requested_popup_feature_requires_catalog_and_start_ack(runtime):
    config, process, _ = runtime
    config = enable(config, process)
    process.mutate_catalog = lambda raw: None
    with pytest.raises(CuaCliError, match="lacks the public"):
        client(config)
    assert not any(request.get("params", {}).get("name") == "start_task" for request in process.requests)


def test_missing_popup_ceiling_ack_stops_the_returned_owned_task(runtime):
    config, process, _ = runtime
    config = enable(config, process)
    process.mutate_open = lambda raw: None
    with pytest.raises(CuaCliError, match="expected bounded pixels task"):
        client(config)
    assert any(request.get("params", {}).get("name") == "stop_task" for request in process.requests)
    assert process.returncode == 0


@pytest.mark.parametrize("reorder", [False, True])
def test_two_popup_order_matches_explicit_selection_and_unique_generations(runtime, reorder):
    config, process, _ = runtime
    config = enable(config, process, handles=(502, 501))
    original = process.mutate_snapshot

    def mutate(raw):
        original(raw)
        if reorder:
            raw["native_capture_popup_observation"]["overlays"].reverse()

    process.mutate_snapshot = mutate
    owned = client(config)
    if reorder:
        with pytest.raises(CuaCliError):
            owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[502, 501])
        assert process.returncode == 0
    else:
        value = owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[502, 501])
        overlays = value["native_capture_popup_overlays"]
        assert [overlay["window_handle"] for overlay in overlays] == [502, 501]
        assert [overlay["image_bytes"] for overlay in overlays] == [popup_png(80), popup_png(81)]
        owned.stop()


@pytest.mark.parametrize("mode", ["truncated", "crc", "trailing", "zlib", "short_raster", "long_raster"])
def test_corrupt_secondary_png_is_refused_even_with_matching_descriptor_length(runtime, mode):
    config, process, _ = runtime
    config = enable(config, process)
    original = process.mutate_snapshot
    invalid = {
        "truncated": POPUP_PNG[:-12],
        "crc": POPUP_PNG[:29] + bytes([POPUP_PNG[29] ^ 1]) + POPUP_PNG[30:],
        "trailing": POPUP_PNG + b"trailing",
        "zlib": popup_png(compressed=b"not a zlib stream"),
        "short_raster": popup_png(compressed=zlib.compress(b"\0")),
        "long_raster": popup_png(compressed=zlib.compress(b"\0" * ((80 * 4 + 1) * 40 + 1))),
    }[mode]

    def mutate(raw):
        original(raw)
        raw["native_capture_popup_observation"]["overlays"][0]["image"]["length"] = len(invalid)

    process.mutate_snapshot = mutate
    process.extra_images[0]["data"] = base64.b64encode(invalid).decode()
    owned = client(config)
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[501])
    assert process.returncode == 0


def test_reused_popup_identity_cannot_be_rebound_to_a_new_root_observation(runtime):
    config, process, _ = runtime
    config = enable(config, process)
    owned = client(config)
    first = owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[501])
    old_identity = first["native_capture_popup_overlays"][0]["observation_id"]
    original = process.mutate_snapshot

    def reuse(raw):
        original(raw)
        overlay = raw["native_capture_popup_observation"]["overlays"][0]
        overlay["observation_id"] = old_identity
        overlay["observation"]["observation_id"] = old_identity

    process.mutate_snapshot = reuse
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[501])
    assert process.returncode == 0


def test_popup_frames_cannot_reuse_each_others_capture_generation(runtime):
    config, process, _ = runtime
    config = enable(config, process, handles=(501, 502))
    original = process.mutate_snapshot

    def reuse(raw):
        original(raw)
        first, second = raw["native_capture_popup_observation"]["overlays"]
        second["observation"]["capture_provenance"]["capture_generation"] = first["observation"]["capture_provenance"][
            "capture_generation"
        ]

    process.mutate_snapshot = reuse
    owned = client(config)
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=5, max_nodes=250, native_capture_popup_window_handles=[501, 502])
    assert process.returncode == 0


def test_complete_png_validation_streams_large_valid_rasters():
    from dcc_mcp_core.host.cua_mcp_popup import _complete_png

    assert _complete_png(popup_png(1024, 128), 1024, 128)
