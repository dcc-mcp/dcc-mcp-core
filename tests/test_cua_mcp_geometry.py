"""Physical source origin contracts; no capture, window or native process runs."""

from copy import deepcopy

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_geometry import validate_geometry
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import pixel_response
from test_cua_mcp_pixels import runtime


def geometry(origin):
    raw = pixel_response()
    obs = raw["observation"]
    proof = obs["capture_provenance"]
    if origin == "dwm_extended_frame":
        proof["native_window_bounds"] = [-653, -13, 666, 506]
    proof["native_visible_bounds"] = [-627, 0, 614, 467] if origin == "win32_window" else [-640, 0, 640, 480]
    proof["wgc_geometry"] = {
        "source_rect": [-640, 0, 640, 480],
        "origin": origin,
        "bgra_byte_len": 640 * 480 * 4,
        "frame": {
            **{
                key: [640, 480]
                for key in ("item_size_before", "item_size_after", "pool_size", "content_size", "texture_size")
            },
            "row_pitch_bytes": 2688,
        },
    }
    return raw


@pytest.mark.parametrize("origin", ["dwm_extended_frame", "win32_window", "identical_native_bounds"])
def test_actual_pixels_consumer_keeps_distinct_native_bounds_and_wgc_origin(runtime, origin):
    config, process, _ = runtime
    raw = geometry(origin)
    process.mutate_snapshot = lambda response: response.update(deepcopy(raw))
    owned = client(config)
    try:
        result = owned.snapshot(max_depth=1, max_nodes=1)
        assert result["observation"]["capture_provenance"]["wgc_geometry"]["origin"] == origin
        assert result["observation"]["source_rect"] == [-640, 0, 640, 480]
    finally:
        owned.stop()


def test_verified_visible_uses_dwm_and_legacy_does_not_gain_new_proof():
    obs = pixel_response()["observation"]
    proof = obs["capture_provenance"]
    validate_geometry(obs, proof)
    assert "native_visible_bounds" not in proof
    proof["native_window_bounds"] = [-653, -13, 666, 506]
    proof["native_visible_bounds"] = [-640, 0, 640, 480]
    validate_geometry(obs, proof)


@pytest.mark.parametrize(
    "fault",
    [
        "ambiguous_origin",
        "wrong_origin",
        "pool",
        "raw_bytes",
        "pitch",
        "source_size",
        "missing_visible",
        "visible_origin",
    ],
)
def test_geometry_refuses_ambiguous_scaled_or_incomplete_new_proof(runtime, fault):
    config, process, _ = runtime
    raw = geometry("dwm_extended_frame")
    obs = raw["observation"]
    proof = obs["capture_provenance"]
    wgc = proof["wgc_geometry"]
    if fault == "ambiguous_origin":
        proof["native_window_bounds"] = [-641, 0, 640, 480]
    elif fault == "wrong_origin":
        wgc["origin"] = "win32_window"
    elif fault == "pool":
        wgc["frame"]["pool_size"] = [666, 506]
    elif fault == "raw_bytes":
        wgc["bgra_byte_len"] -= 4
    elif fault == "pitch":
        wgc["frame"]["row_pitch_bytes"] = 1
    elif fault == "source_size":
        obs["source_rect"][2] = 639
    elif fault == "missing_visible":
        proof["native_visible_bounds"] = None
    else:
        proof.pop("wgc_geometry")
        proof["native_visible_bounds"][0] += 1
    process.mutate_snapshot = lambda response: response.update(deepcopy(raw))
    owned = client(config)
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=1, max_nodes=1)
    assert process.returncode == 0
