"""Host-free calling-convention fixtures; these are not native DCC acceptance."""

from __future__ import annotations

from pathlib import Path

import pytest

from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core._server.inprocess_executor import run_skill_script


@pytest.mark.parametrize(
    "signature",
    [
        "value: bool = False",
        "value: float",
        "value: str",
        "value: int",
        "value: Optional[str] = None",
        "value: List[str]",
        "value: Optional[List[str]] = None",
        "value: Tuple[str, ...]",
        "value: Sequence[str]",
        "value: MutableSequence[str]",
        "value: AbstractSet[str]",
    ],
)
def test_unknown_arguments_do_not_enter_a_typed_single_parameter_handler(tmp_path: Path, signature: str) -> None:
    marker = tmp_path / "handler-entered"
    script = tmp_path / "typed.py"
    script.write_text(
        "from __future__ import annotations\n"
        "from pathlib import Path\n"
        "from typing import AbstractSet, List, MutableSequence, Optional, Sequence, Tuple\n"
        f"def main({signature}):\n"
        f"    Path({str(marker)!r}).write_text('entered')\n"
        "    return {'success': True, 'value': value}\n",
        encoding="utf-8",
    )
    bridge = HostExecutionBridge()
    try:
        result = bridge.execute_script(str(script), {"unsupported_argument": True})
        assert result["success"] is False
        assert result["error"] == "TypeError"
        assert result["_meta"]["dcc.error"]["type"] == "TypeError"
        assert "unsupported_argument" in result["message"]
        assert not marker.exists()
    finally:
        bridge.shutdown_script_execution()


@pytest.mark.parametrize(
    "signature,payload,expected",
    [
        ("value: bool = False", {"value": False}, False),
        ("value: bool = False", {}, False),
        ("value: float", {"value": 12.5}, 12.5),
        ("value: Optional[str] = None", {"value": None}, None),
        ("value: List[str]", {"value": ["mesh"]}, ["mesh"]),
    ],
)
def test_named_typed_arguments_and_defaults_still_work(tmp_path: Path, signature, payload, expected) -> None:
    script = tmp_path / "typed.py"
    script.write_text(
        "from __future__ import annotations\n"
        "from typing import List, Optional\n"
        f"def main({signature}):\n    return value\n",
        encoding="utf-8",
    )
    assert run_skill_script(str(script), payload) == expected


@pytest.mark.parametrize(
    "signature",
    ["params", "args=None", "request: Any", "payload: Mapping[str, Any]", "payload: Optional[dict] = None"],
)
def test_legacy_mapping_handlers_keep_the_complete_payload(tmp_path: Path, signature: str) -> None:
    parameter = signature.split(":")[0].split("=")[0]
    script = tmp_path / "legacy.py"
    script.write_text(
        "from __future__ import annotations\n"
        "from typing import Any, Mapping, Optional\n"
        f"def main({signature}):\n    return {parameter}\n",
        encoding="utf-8",
    )
    payload = {"descriptor": {"asset_id": "fixture"}, "placement": {"scale": [1, 1, 1]}}
    assert run_skill_script(str(script), payload) == payload


def test_open_handlers_keep_extension_arguments(tmp_path: Path) -> None:
    script = tmp_path / "open_handler.py"
    script.write_text("def main(**params):\n    return params\n", encoding="utf-8")
    payload = {"name": "mesh", "adapter_extension": {"enabled": False}}
    assert run_skill_script(str(script), payload) == payload


def test_decorated_signature_rejects_unknown_arguments_before_the_wrapper(tmp_path: Path) -> None:
    marker = tmp_path / "wrapper-entered"
    (tmp_path / "helper.py").write_text(
        "from functools import wraps\n"
        "from pathlib import Path\n"
        "def wrap(handler):\n"
        "    @wraps(handler)\n"
        "    def invoke(*args, **kwargs):\n"
        f"        Path({str(marker)!r}).write_text('entered')\n"
        "        return handler(*args, **kwargs)\n"
        "    return invoke\n",
        encoding="utf-8",
    )
    script = tmp_path / "decorated.py"
    script.write_text(
        "from __future__ import annotations\n"
        "from typing import Optional\n"
        "from .helper import wrap\n"
        "@wrap\n"
        "def main(value: Optional[str] = None):\n"
        "    return {'success': True, 'value': value}\n",
        encoding="utf-8",
    )
    bridge = HostExecutionBridge()
    try:
        result = bridge.execute_script(str(script), {"unsupported_argument": True})
        assert result["success"] is False
        assert result["error"] == "TypeError"
        assert not marker.exists()
    finally:
        bridge.shutdown_script_execution()
