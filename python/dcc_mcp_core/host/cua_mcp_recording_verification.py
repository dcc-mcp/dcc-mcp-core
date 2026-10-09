"""Bind recording admission progress to the actual connected Host and source."""

from __future__ import annotations

from copy import deepcopy
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_contracts import typed_equal
from dcc_mcp_core.host.cua_mcp_recording_progress import RECORDING_PROGRESS_CONTRACT
from dcc_mcp_core.host.cua_mcp_recording_progress import compare_media_sample_progress
from dcc_mcp_core.host.cua_mcp_recording_progress import media_sample_progress


def connected_recording_host(value: dict[str, Any]) -> tuple[str, list[str]]:
    """Read the Native session's actual hello projection, never a requested grant."""
    if not isinstance(value, dict):
        raise CuaCliError("unsupported", "The actual connected Host proof is missing.")
    connection = value.get("host_connection_id")
    capabilities = value.get("connected_host_capabilities")
    if (
        not isinstance(connection, str)
        or not connection
        or len(connection) > 256
        or not isinstance(capabilities, list)
        or any(not isinstance(item, str) or not item for item in capabilities)
        or RECORDING_PROGRESS_CONTRACT not in capabilities
    ):
        raise CuaCliError("unsupported", "The connected Host did not prove recording sample progress support.")
    return connection, deepcopy(capabilities)


class RecordingProgressVerification:
    """Retain one source baseline; lifecycle transitions never prove advancement."""

    def __init__(self, client: Any, opened: dict[str, Any]) -> None:
        self.client = client
        self.connection, self.capabilities = connected_recording_host(opened)
        self.reset()

    def reset(self) -> None:
        self._binding: dict[str, Any] | None = None
        self._previous: dict[str, Any] | None = None

    def observe(self, state: dict[str, Any], context: dict[str, Any]) -> dict[str, Any]:
        self.check_context(context)
        video, source = state.get("video"), state.get("source")
        current = media_sample_progress(video.get("media_sample_progress")) if isinstance(video, dict) else None
        binding = self._source_binding(video, source) if current is not None else None
        if binding is None:
            self.reset()
            current = None
        elif not typed_equal(binding, self._binding):
            self.reset()
        status = compare_media_sample_progress(self._previous, current)
        if status in {"regressed", "unavailable"}:
            self.reset()
        else:
            self._binding, self._previous = deepcopy(binding), deepcopy(current)
        healthy = (
            state.get("status") == "active"
            and state.get("active") is True
            and state.get("healthy") is True
            and isinstance(video, dict)
            and video.get("active") is True
            and video.get("paused") is False
            and isinstance(source, dict)
            and source.get("active") is True
            and source.get("paused") is False
        )
        return {
            "status": status,
            "satisfied": status == "advanced" and healthy,
            "healthy": healthy,
            "sample_semantics": "openh264_media_sample_admission_with_source_provenance",
            "current": current,
            "binding": binding,
        }

    def check_context(self, context: dict[str, Any]) -> None:
        connection, capabilities = connected_recording_host(context)
        if connection != self.connection or not typed_equal(capabilities, self.capabilities):
            self.reset()
            raise CuaCliError("protocol_mismatch", "Recording changed its actual connected Host identity.")

    def _source_binding(self, video: Any, source: Any) -> dict[str, Any] | None:
        if not isinstance(video, dict) or not isinstance(source, dict) or video.get("backend") != "embedded-openh264":
            return None
        first = video.get("first_encoded_frame")
        provenance = first.get("capture_provenance") if isinstance(first, dict) else None
        stream = source.get("stream_id")
        if (
            not isinstance(provenance, dict)
            or provenance.get("kind") != "native_exact_window"
            or provenance.get("source") not in {"wgc", "verified_visible"}
            or type(stream) is not int
            or not 0 <= stream < 2**64
            or type(provenance.get("stream_id")) is not int
            or provenance["stream_id"] != stream
        ):
            return None
        instance = self.client._bind_recording_instance(provenance)
        return {
            "task_id": self.client.task_id,
            "host_connection_id": self.connection,
            "native_instance": instance,
            "stream_id": stream,
        }
