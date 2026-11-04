"""Validate secondary popup pixels without minting native input authority."""

from __future__ import annotations

import base64
import struct
from typing import Any
import zlib

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_geometry import validate_geometry


def requested_popup_handles(value: Any, *, allowed: bool, root_handle: int) -> list[int]:
    """Require an explicit bounded secondary observation selection."""
    if value is None or value == []:
        return []
    if not allowed:
        raise CuaCliError("unsupported_action", "The owner did not grant native capture popup observation.")
    if (
        not isinstance(value, list)
        or not 1 <= len(value) <= 8
        or any(type(handle) is not int or not 0 < handle < 2**64 or handle == root_handle for handle in value)
        or len(set(value)) != len(value)
    ):
        raise CuaCliError("invalid_action", "Select one through eight unique exact popup window handles.")
    return list(value)


def decode_popup_observation(
    raw: dict[str, Any], root: dict[str, Any], selected: list[int], images: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    """Correlate every secondary native frame, instance and PNG independently."""

    def invalid() -> CuaCliError:
        return CuaCliError("protocol_mismatch", "Native capture popup pixels do not match their exact observations.")

    context = raw.get("native_capture_popup_observation")
    if not selected:
        if context is not None or images:
            raise invalid()
        return []
    if (
        not isinstance(context, dict)
        or context.get("purpose") != "main_root_keyboard"
        or context.get("one_use") is not True
        or type(context.get("max_age_ms")) is not int
        or context["max_age_ms"] != 5000
        or context.get("root_observation_id") != root["observation_id"]
        or context.get("selected_window_handles") != selected
        or type(context.get("capture_window_handle")) is not int
        or context["capture_window_handle"] not in selected
        or not isinstance(context.get("overlays"), list)
        or len(context["overlays"]) != len(selected)
        or len(images) != len(selected)
    ):
        raise invalid()
    root_instance = root["capture_provenance"]["native_instance"]
    identities = {root["observation_id"]}
    generations = {root["capture_provenance"]["capture_generation"]}
    result = []
    encoded_size = 0
    image_offset = raw["image"]["length"]
    for index, (handle, overlay, image) in enumerate(zip(selected, context["overlays"], images), 1):
        if not isinstance(overlay, dict) or not isinstance(image, dict):
            raise invalid()
        observation = overlay.get("observation")
        if not isinstance(observation, dict):
            raise invalid()
        provenance = observation.get("capture_provenance")
        instance = overlay.get("native_instance")
        identity = overlay.get("observation_id")
        if (
            type(overlay.get("window_handle")) is not int
            or overlay["window_handle"] != handle
            or not isinstance(identity, str)
            or not 1 <= len(identity) <= 256
            or identity in identities
            or identity != "{}-popup-{}".format(root["observation_id"], handle)
            or observation.get("observation_id") != identity
            or observation.get("session_id") != root["session_id"]
            or type(observation.get("process_id")) is not int
            or observation["process_id"] != root["process_id"]
            or type(observation.get("window_handle")) is not int
            or observation["window_handle"] != handle
            or not isinstance(provenance, dict)
            or type(provenance.get("process_id")) is not int
            or provenance["process_id"] != root["process_id"]
            or type(provenance.get("window_handle")) is not int
            or provenance["window_handle"] != handle
            or provenance.get("observation_mode") != "pixels_only"
            or provenance.get("pixels_captured") is not True
            or provenance.get("whole_desktop_capture") is not False
            or provenance.get("accessibility_available") is not False
            or not isinstance(instance, dict)
            or set(instance) != set(root_instance)
            or instance != provenance.get("native_instance")
            or any(type(instance[key]) is not int or not 0 <= instance[key] < 2**64 for key in root_instance)
            or instance["process_creation_time_100ns"] != root_instance["process_creation_time_100ns"]
            or instance["window_thread_id"] != root_instance["window_thread_id"]
            or instance["window_class_hash"] == 0
            or type(provenance.get("window_dpi")) is not int
            or provenance["window_dpi"] <= 0
            or type(provenance.get("capture_generation")) is not int
            or provenance["capture_generation"] <= 0
            or provenance["capture_generation"] in generations
            or not isinstance(provenance.get("backend"), str)
            or not provenance["backend"]
            or observation.get("capture_backend") != provenance["backend"]
        ):
            raise invalid()
        rect = observation.get("source_rect")
        width, height = observation.get("width"), observation.get("height")
        if (
            type(width) is not int
            or type(height) is not int
            or not 0 < width <= 32768
            or not 0 < height <= 32768
            or width * height > 64 * 1024 * 1024
            or not isinstance(rect, list)
            or len(rect) != 4
            or any(type(value) is not int or not -(2**31) <= value < 2**31 for value in rect)
            or rect[2:] != [width, height]
            or any(not -(2**31) <= rect[index] + rect[index + 2] < 2**31 for index in (0, 1))
            or any(overlay.get(key) != observation[key] for key in ("source_rect", "width", "height"))
        ):
            raise invalid()
        if "native_visible_bounds" not in provenance or not isinstance(provenance.get("native_window_bounds"), list):
            raise invalid()
        validate_geometry(observation, provenance)
        encoded = image.get("data")
        if image.get("type") != "image" or image.get("mimeType") != "image/png" or not isinstance(encoded, str):
            raise invalid()
        encoded_size += len(encoded)
        if encoded_size > 90 * 1024 * 1024:
            raise invalid()
        pixels = base64.b64decode(encoded, validate=True)
        descriptor = overlay.get("image")
        if (
            len(pixels) < 24
            or not pixels.startswith(b"\x89PNG\r\n\x1a\n")
            or pixels[12:16] != b"IHDR"
            or struct.unpack(">II", pixels[16:24]) != (width, height)
            or not isinstance(descriptor, dict)
            or descriptor.get("mime_type") != "image/png"
            or descriptor.get("encoding") != "binary_frame"
            or type(descriptor.get("index")) is not int
            or descriptor["index"] != index
            or type(descriptor.get("offset")) is not int
            or descriptor["offset"] != image_offset
            or type(descriptor.get("length")) is not int
            or descriptor["length"] != len(pixels)
            or not _complete_png(pixels, width, height)
        ):
            raise invalid()
        identities.add(identity)
        generations.add(provenance["capture_generation"])
        image_offset += len(pixels)
        result.append({**overlay, "image_bytes": pixels})
    return result


def _complete_png(pixels: bytes, width: int, height: int) -> bool:
    """Check the bounded native PNG stream without retaining decoded pixels."""
    offset, decoded, expected, stride = 8, 0, 0, 1
    decoder = zlib.decompressobj()
    try:
        while offset + 12 <= len(pixels):
            size = struct.unpack_from(">I", pixels, offset)[0]
            end = offset + 12 + size
            if end > len(pixels):
                return False
            kind = pixels[offset + 4 : offset + 8]
            payload = memoryview(pixels)[offset + 8 : end - 4]
            if zlib.crc32(payload, zlib.crc32(kind)) != struct.unpack_from(">I", pixels, end - 4)[0]:
                return False
            if offset == 8:
                if kind != b"IHDR" or size != 13:
                    return False
                w, h, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
                channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color)
                if (
                    (w, h) != (width, height)
                    or depth != 8
                    or not channels
                    or (compression, filtering, interlace) != (0, 0, 0)
                ):
                    return False
                stride = width * channels + 1
                expected = stride * height
            elif kind == b"IHDR":
                return False
            if kind == b"IDAT":
                pending = payload
                while True:
                    block = decoder.decompress(pending, 65536)
                    pending = decoder.unconsumed_tail
                    start = decoded
                    decoded += len(block)
                    if decoded > expected or decoder.unused_data:
                        return False
                    if any(value > 4 for value in block[(-start) % stride :: stride]):
                        return False
                    if not pending and len(block) < 65536:
                        break
            elif kind == b"IEND":
                return size == 0 and end == len(pixels) and decoder.eof and decoded == expected
            offset = end
    except (ValueError, struct.error, zlib.error):
        pass
    return False
