"""Host-free calling-convention fixtures; these are not native DCC acceptance."""

from __future__ import annotations

from pathlib import Path
import sys

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
        "value: complex",
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
        ("value: complex", {"value": 1 + 2j}, 1 + 2j),
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
    [
        "params",
        "args=None",
        "request: Any",
        "payload: Mapping[str, Any]",
        "payload: Optional[dict] = None",
        "payload: Union[bool, Mapping[str, Any]]",
        "payload: HostValue",
        "payload: MissingHostValue",
    ],
)
def test_legacy_mapping_handlers_keep_the_complete_payload(tmp_path: Path, signature: str) -> None:
    parameter = signature.split(":")[0].split("=")[0]
    script = tmp_path / "legacy.py"
    script.write_text(
        "from __future__ import annotations\n"
        "from typing import Any, Mapping, Optional, Union\n"
        "class HostValue: pass\n"
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


@pytest.mark.parametrize("annotation", ["Optional[str]", "complex"])
def test_decorated_signature_rejects_unknown_arguments_before_the_wrapper(tmp_path: Path, annotation) -> None:
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
        f"def main(value: {annotation} = None):\n"
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


@pytest.mark.skipif(sys.version_info < (3, 8), reason="typing.Literal requires Python 3.8+")
@pytest.mark.parametrize("annotation", ["Literal[False]", "Literal['draft', 'final']", "Optional[Literal[1, 2]]"])
@pytest.mark.parametrize("decorated", [False, True])
def test_literal_scalar_arguments_reject_unknown_keywords_before_entry(tmp_path: Path, annotation, decorated) -> None:
    marker = tmp_path / "literal-handler-entered"
    script = tmp_path / "literal_handler.py"
    script.write_text(
        "from __future__ import annotations\n"
        "from functools import wraps\n"
        "from pathlib import Path\n"
        "from typing import Literal, Optional\n"
        "def wrap(handler):\n"
        "    @wraps(handler)\n"
        "    def invoke(*args, **kwargs):\n"
        f"        Path({str(marker)!r}).write_text('wrapper entered')\n"
        "        return handler(*args, **kwargs)\n"
        "    return invoke\n" + ("@wrap\n" if decorated else "") + f"def main(value: {annotation} = None):\n"
        f"    Path({str(marker)!r}).write_text('handler entered')\n"
        "    return {'success': True, 'value': value}\n",
        encoding="utf-8",
    )
    bridge = HostExecutionBridge()
    try:
        result = bridge.execute_script(str(script), {"unsupported_argument": True})
        assert result["success"] is False
        assert result["error"] == "TypeError"
        assert "unsupported_argument" in result["message"]
        assert not marker.exists()
    finally:
        bridge.shutdown_script_execution()


@pytest.mark.skipif(sys.version_info < (3, 8), reason="typing.Literal requires Python 3.8+")
@pytest.mark.parametrize("payload,expected", [({"value": False}, False), ({}, False)])
def test_literal_named_arguments_and_defaults_still_work(tmp_path: Path, payload, expected) -> None:
    script = tmp_path / "literal_named.py"
    script.write_text(
        "from typing import Literal\ndef main(value: Literal[False] = False):\n    return value\n",
        encoding="utf-8",
    )
    assert run_skill_script(str(script), payload) == expected


@pytest.mark.parametrize(
    "signature",
    ["value: bool = False) -> MissingReturn", "value: bool = False, *, other: MissingType = None)"],
)
def test_unrelated_unresolved_annotation_does_not_disable_parameter_guard(tmp_path: Path, signature: str) -> None:
    marker = tmp_path / "handler-entered"
    script = tmp_path / "unresolved.py"
    script.write_text(
        "from __future__ import annotations\n"
        "from pathlib import Path\n"
        f"def main({signature}:\n"
        f"    Path({str(marker)!r}).write_text('entered')\n"
        "    return {'success': True}\n",
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


def test_failed_optional_annotation_resolution_preserves_legacy_payload(tmp_path: Path) -> None:
    script = tmp_path / "opaque.py"
    script.write_text(
        "from __future__ import annotations\n"
        "def host_annotation():\n    raise RuntimeError('host type unavailable')\n"
        "def main(payload: host_annotation()):\n    return payload\n",
        encoding="utf-8",
    )
    assert run_skill_script(str(script), {"descriptor": "fixture"}) == {"descriptor": "fixture"}
