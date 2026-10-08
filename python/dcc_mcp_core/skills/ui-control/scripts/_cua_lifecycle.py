"""Report owned-task cleanup failures through the existing skill unload hooks."""

from __future__ import annotations

from copy import deepcopy
import json
from typing import Any

from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError


class OwnedPixelsCleanupError(RuntimeError):
    """Retain original failures while bounding the warning's safe evidence."""

    def __init__(self, failures: list[Exception]) -> None:
        self.failures = tuple(failures)
        self.report = {
            "failure_count": len(failures),
            "failures": [_failure_report(exc) for exc in failures[:4]],
            "omitted_failure_count": max(0, len(failures) - 4),
        }
        super().__init__("Owned pixels cleanup failed: " + json.dumps(self.report, separators=(",", ":")))


def _failure_report(exc: Exception) -> dict[str, Any]:
    """Never include arbitrary exception messages, commands or native text."""
    report: dict[str, Any] = {
        "code": "cleanup_failed" if getattr(exc, "code", None) == "cleanup_failed" else "cleanup_unknown"
    }
    if not isinstance(exc, OwnedCuaMcpError):
        return report
    evidence = exc.native_evidence
    if len(json.dumps(evidence, ensure_ascii=True, separators=(",", ":"))) <= 8192:
        report["native_evidence"] = deepcopy(evidence)
    else:
        # The cached original remains available on the exception. Large path
        # inventories must not turn an executor warning into a megabyte log.
        report["evidence_omitted"] = True
        report["cleanup_pending"] = evidence.get("cleanup_pending") is True
        ack = evidence.get("cleanup", {})
        video = ack.get("recording_video", {})
        report["recording_partial_retained"] = "current_partial" in video
        report["segment_count"] = len(video.get("segment_paths", []))
    return report


def stop_clients(entries: list[dict[str, Any]]) -> None:
    """Attempt all peers and report only owned pixels cleanup failures."""
    failures = []
    for entry in entries:
        client = entry["client"]
        try:
            client.stop()
        except Exception as exc:
            if isinstance(client, PixelsMcpHostClient):
                failures.append(exc)
    if failures:
        raise OwnedPixelsCleanupError(failures) from None
