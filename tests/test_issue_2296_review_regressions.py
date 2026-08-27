from __future__ import annotations

import json
from pathlib import Path

import pytest

from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core.dcc_api_executor import DccApiExecutor
from dcc_mcp_core.dcc_api_executor import register_dcc_api_executor
from dcc_mcp_core.schema import derive_script_parameters_schema
from dcc_mcp_core.script_materialization import materialize_script
from dcc_mcp_core.skills_helper import ToolValidator


class _Dispatcher:
    def dispatch(self, name: str, json_str: str) -> dict:
        assert name == "double"
        value = json.loads(json_str)["value"]
        return {"action": name, "output": {"success": True, "result": value * 2}}


def _metadata_path(root: Path) -> Path:
    matches = list(root.rglob("*.json"))
    assert len(matches) == 1
    return matches[0]


def test_structured_params_preserve_dispatcher_execution_capability(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "def main(scale: float) -> float:\n"
        "    response = dispatch('double', {'value': scale})\n"
        "    return response['output']['result']\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )
    executor = DccApiExecutor(
        "maya",
        dispatcher=_Dispatcher(),
        script_materialization_root=tmp_path,
    )

    result = executor.execute_params(
        {"file_path": descriptor.file_path, "params": {"scale": 2.5}},
    )

    assert result["success"] is True
    assert result["output"] == 5.0


def test_registered_tools_call_preserves_dispatcher_execution_capability(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "def main(scale: float) -> float:\n"
        "    response = dispatch('double', {'value': scale})\n"
        "    return response['output']['result']\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0, server_name="params-capability-test"))
    register_dcc_api_executor(
        server,
        DccApiExecutor("maya", dispatcher=_Dispatcher(), script_materialization_root=tmp_path),
    )

    handle = server.start()
    try:
        code, body = McpClient(handle.mcp_url()).post(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "dcc_execute",
                    "arguments": {"file_path": descriptor.file_path, "params": {"scale": 2.5}},
                },
            },
        )
    finally:
        handle.shutdown()

    assert code == 200
    result = body["result"]
    assert result["isError"] is False
    payload = json.loads(result["content"][0]["text"])
    assert payload["success"] is True
    assert payload["output"] == 5.0


def test_structured_params_preserve_sandbox_builtins(tmp_path: Path) -> None:
    marker = tmp_path / "must-not-write"
    descriptor = materialize_script(
        "def main(path: str) -> str:\n"
        "    with open(path, 'w') as stream:\n"
        "        stream.write('unsafe')\n"
        "    return path\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path / "store",
    )

    result = DccApiExecutor(
        "maya",
        script_materialization_root=tmp_path / "store",
    ).execute_params({"file_path": descriptor.file_path, "params": {"path": str(marker)}})

    assert result["success"] is False
    assert not marker.exists()


def test_structured_params_typing_annotations_do_not_expose_runtime_modules(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "import typing\ndef main(value: int) -> str:\n    return typing.sys.modules['os'].getcwd()\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )

    result = DccApiExecutor("maya", script_materialization_root=tmp_path).execute_params(
        {"file_path": descriptor.file_path, "params": {"value": 1}},
    )

    assert result["success"] is False


def test_structured_params_rederive_missing_legacy_sidecar_schema(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "def main(scale: float) -> float:\n    return scale * 2\n",
        dcc_type="blender",
        instance_id="blender-1",
        session_id="session-1",
        root=tmp_path,
    )
    metadata_path = _metadata_path(tmp_path)
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    metadata.pop("parameters_schema", None)
    metadata_path.write_text(json.dumps(metadata), encoding="utf-8")

    result = DccApiExecutor(
        "blender",
        script_materialization_root=tmp_path,
    ).execute_params(
        {"file_path": descriptor.file_path, "params": {"scale": 2.5}},
    )

    assert result["success"] is True
    assert result["output"] == 5.0


def test_structured_params_reject_sidecar_schema_drift(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "def main(scale: float) -> float:\n    return scale * 2\n",
        dcc_type="houdini",
        instance_id="houdini-1",
        session_id="session-1",
        root=tmp_path,
    )
    metadata_path = _metadata_path(tmp_path)
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    metadata["parameters_schema"]["properties"]["scale"] = {"type": "string"}
    metadata_path.write_text(json.dumps(metadata), encoding="utf-8")

    result = DccApiExecutor(
        "houdini",
        script_materialization_root=tmp_path,
    ).execute_params(
        {"file_path": descriptor.file_path, "params": {"scale": "oops"}},
    )

    assert result["success"] is False
    assert "parameters_schema" in result["error"]


def test_module_main_self_parameter_is_not_treated_as_a_method_receiver() -> None:
    schema = derive_script_parameters_schema("def main(self: int) -> int:\n    return self\n")

    assert schema["required"] == ["self"]
    assert schema["properties"]["self"] == {"type": "integer"}


def test_module_main_rejects_params_removed_by_runtime_key_filter() -> None:
    assert (
        derive_script_parameters_schema(
            "def main(_scale: float) -> float:\n    return _scale * 2\n",
        )
        is None
    )


@pytest.mark.parametrize(
    "source",
    [
        "from pathlib import Path\ndef main(value: Path):\n    return value\n",
        "from uuid import UUID\ndef main(value: UUID):\n    return value\n",
        "from datetime import date\ndef main(value: date):\n    return value\n",
        "from typing import Set\ndef main(value: Set[int]):\n    return value\n",
    ],
)
def test_module_main_rejects_non_json_preserving_annotations(source: str) -> None:
    assert derive_script_parameters_schema(source) is None


def test_generated_fixed_tuple_schema_is_enforced_by_validator() -> None:
    schema = derive_script_parameters_schema(
        "from typing import Tuple\ndef main(pair: Tuple[int, int]) -> int:\n    return pair[0] + pair[1]\n",
    )
    validator = ToolValidator.from_schema_json(json.dumps(schema))

    valid, valid_errors = validator.validate(json.dumps({"pair": [1, 2]}))
    too_short, too_short_errors = validator.validate(json.dumps({"pair": [1]}))
    wrong_second, wrong_second_errors = validator.validate(json.dumps({"pair": [1, "two"]}))

    assert valid is True, valid_errors
    assert too_short is False
    assert too_short_errors
    assert wrong_second is False
    assert wrong_second_errors


def test_generated_fixed_tuple_schema_matches_runtime_call(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "from typing import Tuple\ndef main(pair: Tuple[int, int]) -> int:\n    return pair[0] + pair[1]\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )
    executor = DccApiExecutor("maya", script_materialization_root=tmp_path)

    valid = executor.execute_params({"file_path": descriptor.file_path, "params": {"pair": [1, 2]}})
    too_short = executor.execute_params({"file_path": descriptor.file_path, "params": {"pair": [1]}})

    assert valid["success"] is True
    assert valid["output"] == 3
    assert too_short["success"] is False
    assert "schema validation" in too_short["error"]
