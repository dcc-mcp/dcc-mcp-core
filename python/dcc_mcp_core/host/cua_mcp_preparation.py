"""One retained task's explicitly authorized passive capture lifecycle."""

from __future__ import annotations

import base64
from copy import deepcopy
from pathlib import Path
import struct
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_preparation_contract import integer
from dcc_mcp_core.host.cua_preparation_contract import no_action_tokens
from dcc_mcp_core.host.cua_preparation_contract import rectangle
from dcc_mcp_core.host.cua_preparation_contract import require
from dcc_mcp_core.host.cua_preparation_contract import validate_status
from dcc_mcp_core.host.ui_control_options import _ordinary_absolute_path

METHODS = ("capture_preparation_begin", "capture_preparation_state", "capture_preparation_stop")


class PixelsMcpPreparation:
    """Native owns promotion/restoration and expiry; Core validates and retains receipts."""

    def __init__(self, client: Any) -> None:
        self.client = client
        self.status: dict[str, Any] | None = None
        self.attempted = False
        self._captured_at: int | None = None

    def register(self, methods: list, scopes: list) -> None:
        """Add only the independently granted passive lifecycle and image method."""
        options = self.client.options.capture_preparation
        if options is None:
            return
        methods.extend(METHODS)
        if options.allow_snapshot:
            methods.append("capture_preparation_snapshot")
        scopes.append(
            {
                "action": "capture_preparation_begin",
                "input_kind": "window_state",
                "secret_input": False,
                "authorization_category": "window_state",
            }
        )

    def _authorized(self) -> None:
        if self.client.options.capture_preparation is None:
            raise CuaCliError("permission_denied", "The owner did not grant passive capture preparation.")

    def validate(self, raw: Any, *, new: bool = False) -> dict[str, Any]:
        """Bind a complete status DTO to the retained exact native instance."""
        value = validate_status(raw)
        if self.status is not None:
            if new:
                require(value["preparation_id"] != self.status["preparation_id"], "Replayed preparation identity.")
            else:
                require(value["preparation_id"] == self.status["preparation_id"], "Preparation identity changed.")
                require(value["deadline_ms"] == self.status["deadline_ms"], "Preparation deadline changed.")
        original = value["original"]
        if self.status is not None and not new:
            if self.status["original"] or self.status["cleanup_verified"]:
                require(original == self.status["original"], "Preparation original state changed.")
            if self.status["journal_path"]:
                require(value["journal_path"] == self.status["journal_path"], "Preparation journal changed.")
        if not original:
            require(
                value["last_mutation"] is None
                and not value["affected_readback"]
                and value["last_completed_sequence"] is None,
                "Uninitialized preparation cannot report a mutation or window readback.",
            )
            if value["phase"] == "pending_promotion":
                require(value["pending_sequence"] == 1 and not value["cleanup_verified"])
            elif value["phase"] == "refused":
                require(value["capture_revoked"] and value["pending_sequence"] is None and value["failure"] is not None)
            else:
                require(
                    value["phase"] == "cleanup_unknown"
                    and value["capture_revoked"]
                    and not value["cleanup_verified"]
                    and value["failure"] is not None
                )
        else:
            require(len(original) == 1, "Public v1 preparation must bind exactly one root window.")
            expected = original[0]["identity"]
            self.client._check_target(expected)
            require(expected["native_instance"] == self.client._native_instance, "Preparation native instance changed.")
        groups = [value["affected_readback"]]
        mutation = value["last_mutation"]
        if mutation is not None:
            groups.append(mutation["readback"])
            require(
                all(
                    call["window_handle"] == original[0]["identity"]["window_handle"]
                    for call in mutation["native_calls"]
                )
            )
        for group in groups:
            require(
                len(group) <= 1 and all(item["identity"] == original[0]["identity"] for item in group),
                "Affected scope changed.",
            )
        if value["journal_path"]:
            journal = _ordinary_absolute_path(value["journal_path"])
            root = Path(self.client.options.capture_preparation.journal_root)
            require(root in journal.parents, "Preparation journal escaped the operator root.")
        return value

    def call(self, operation: str, *, window_state_id: str | None = None, lifetime_ms: int | None = None) -> dict:
        """Dispatch once with native task authority and consume prior action evidence."""
        self._authorized()
        require(operation in {"begin", "state", "stop", "snapshot"}, "Unsupported preparation operation.")
        params: dict[str, Any] = {}
        if operation == "begin":
            token = self.client._window_state_id
            self.client.invalidate_action_evidence()
            if not isinstance(window_state_id, str) or not token or token != window_state_id:
                raise CuaCliError("stale_observation", "Read fresh same-session window metadata before preparation.")
            if (
                len(window_state_id.encode("utf-8")) > 128
                or type(lifetime_ms) is not int
                or not 1 <= lifetime_ms <= 30000
            ):
                raise CuaCliError("invalid_request", "Preparation lifetime must be 1..30000 ms with bounded metadata.")
            if self.attempted and (self.status is None or not self.status["cleanup_verified"]):
                raise CuaCliError("cleanup_unknown", "Resolve prior preparation cleanup before beginning again.")
            params = {"request": {"window_state_id": token, "lifetime_ms": lifetime_ms}}
            self.attempted = True
        elif not self.attempted:
            raise CuaCliError("invalid_request", "This retained task has not attempted preparation.")
        if operation == "snapshot":
            if not self.client.options.capture_preparation.allow_snapshot:
                raise CuaCliError("permission_denied", "The owner did not grant passive prepared pixels.")
            if self.status is None or self.status["phase"] != "active" or self.status["capture_revoked"]:
                raise CuaCliError("stale_observation", "Read an active same-preparation state before capture.")
        self.client.invalidate_action_evidence()
        method = "capture_preparation_" + operation
        raw = self.client._call(method, params, method)
        require(raw.get("session_id") == "mcp-" + self.client.task_id, "Preparation Host session changed.")
        no_action_tokens(raw)
        if operation == "snapshot":
            return self._snapshot(raw)
        value = self.validate(raw.get("result"), new=operation == "begin")
        self.status = value
        if operation == "begin":
            self._captured_at = None
        return raw

    def _snapshot(self, raw: dict) -> dict:
        meta = raw.get("metadata")
        require(raw.get("passive") is True and raw.get("input_authorized") is False and isinstance(meta, dict))
        require(
            meta.get("schema") == "dcc-cua-passive-prepared-evidence-v1"
            and meta.get("passive") is True
            and meta.get("input_authorized") is False
        )
        self.client._check_target(meta)
        require(meta.get("native_instance") == self.client._native_instance)
        require(meta.get("preparation_id") == self.status["preparation_id"])
        timestamp = meta.get("captured_at_ms")
        require(
            integer(timestamp)
            and timestamp < self.status["deadline_ms"]
            and (self._captured_at is None or timestamp > self._captured_at),
            "Expired or replayed passive image.",
        )
        require(meta.get("whole_desktop_capture") is False)
        require(all(type(meta.get(key)) is bool for key in ("foreground_at_capture", "foreground_at_publication")))
        require(rectangle(meta.get("bounds")))
        width, height = meta.get("width"), meta.get("height")
        require(integer(width, 32) and integer(height, 32) and [width, height] == meta["bounds"][2:])
        images = [
            item for item in raw.pop("_mcp_content", []) if isinstance(item, dict) and item.get("type") == "image"
        ]
        require(len(images) == 1 and images[0].get("mimeType") == "image/png")
        encoded = images[0].get("data")
        require(isinstance(encoded, str) and len(encoded) <= 90 * 1024 * 1024)
        try:
            pixels = base64.b64decode(encoded, validate=True)
        except ValueError:
            raise CuaCliError("capture_failed", "Invalid passive PNG encoding.") from None
        descriptor = raw.get("image")
        require(len(pixels) >= 24 and pixels.startswith(b"\x89PNG\r\n\x1a\n") and pixels[12:16] == b"IHDR")
        require(
            struct.unpack(">II", pixels[16:24]) == (width, height)
            and isinstance(descriptor, dict)
            and descriptor.get("length") == len(pixels)
        )
        self._captured_at = timestamp
        raw["image_bytes"] = pixels
        return raw

    def cleanup(self, ack: dict) -> dict | None:
        """Validate an actual session-stop component; never infer restored from process exit."""
        raw = ack.get("capture_preparation")
        if raw is None and isinstance(ack.get("host_response"), dict):
            raw = ack["host_response"].get("capture_preparation")
        if raw is None:
            require(not self.attempted, "Native cleanup omitted the attempted preparation outcome.")
            return None
        self._authorized()
        value = self.validate(raw)
        self.status = deepcopy(value)
        return value
