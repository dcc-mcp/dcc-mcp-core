from __future__ import annotations

import json
from pathlib import Path
import time

import pytest

from conftest import McpClient
from dcc_mcp_core import CancelToken
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolPipeline
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import reset_cancel_token
from dcc_mcp_core import set_cancel_token
from dcc_mcp_core.dcc_api_executor import DccApiExecutor
from dcc_mcp_core.dcc_api_executor import register_dcc_api_executor
from dcc_mcp_core.schema import derive_script_parameters_schema
from dcc_mcp_core.script_materialization import materialize_script
from dcc_mcp_core.skills_helper import ToolValidator


def _real_dispatch_pipeline(
    events: list[str],
    *,
    delay_seconds: float = 0.0,
    observe_cancel: bool = False,
) -> ToolPipeline:
    registry = ToolRegistry()
    registry.register(
        name="double",
        description="Double one number.",
        dcc="maya",
        input_schema=json.dumps(
            {
                "type": "object",
                "required": ["value"],
                "properties": {"value": {"type": "number"}},
                "additionalProperties": False,
            },
        ),
    )
    dispatcher = ToolDispatcher(registry)

    def _double(params: dict) -> dict:
        if delay_seconds:
            time.sleep(delay_seconds)
        if observe_cancel:
            from dcc_mcp_core import check_cancelled

            check_cancelled()
        return {"success": True, "result": params["value"] * 2}

    dispatcher.register_handler("double", _double)
    pipeline = ToolPipeline(dispatcher)
    pipeline.add_callable(
        before_fn=lambda name: events.append(f"before:{name}"),
        after_fn=lambda name, success: events.append(f"after:{name}:{success}"),
    )
    return pipeline


def _metadata_path(root: Path) -> Path:
    matches = list(root.rglob("*.json"))
    assert len(matches) == 1
    return matches[0]


def test_structured_params_preserve_dispatcher_execution_capability(tmp_path: Path) -> None:
    events: list[str] = []
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
        dispatcher=_real_dispatch_pipeline(events),
        script_materialization_root=tmp_path,
    )

    result = executor.execute_params(
        {"file_path": descriptor.file_path, "params": {"scale": 2.5}},
    )

    assert result["success"] is True
    assert result["output"] == 5.0
    assert events == ["before:double", "after:double:True"]


def test_registered_tools_call_preserves_dispatcher_execution_capability(tmp_path: Path) -> None:
    events: list[str] = []
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
        DccApiExecutor(
            "maya",
            dispatcher=_real_dispatch_pipeline(events),
            script_materialization_root=tmp_path,
        ),
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
    assert events == ["before:double", "after:double:True"]


def test_registered_tools_call_preserves_structured_and_legacy_deadline_and_hooks(tmp_path: Path) -> None:
    events: list[str] = []
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0, server_name="params-deadline-test"))
    register_dcc_api_executor(
        server,
        DccApiExecutor(
            "maya",
            dispatcher=_real_dispatch_pipeline(events, delay_seconds=1.05),
            script_materialization_root=tmp_path,
        ),
    )
    descriptor = materialize_script(
        "def main(scale: float) -> float:\n"
        "    response = dispatch('double', {'value': scale})\n"
        "    return response['output']['result']\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )

    handle = server.start()
    try:
        client = McpClient(handle.mcp_url())
        responses = []
        for arguments in (
            {"file_path": descriptor.file_path, "params": {"scale": 2.5}, "timeout_secs": 1},
            {
                "code": "response = dispatch('double', {'value': 2.5})\nreturn response['output']['result']",
                "timeout_secs": 1,
            },
        ):
            code, body = client.post(
                {
                    "jsonrpc": "2.0",
                    "id": len(responses) + 1,
                    "method": "tools/call",
                    "params": {"name": "dcc_execute", "arguments": arguments},
                },
            )
            assert code == 200
            responses.append(json.loads(body["result"]["content"][0]["text"]))
    finally:
        handle.shutdown()

    assert [response["success"] for response in responses] == [False, False]
    assert all("timed out" in response["message"].lower() for response in responses)
    assert events == [
        "before:double",
        "after:double:True",
        "before:double",
        "after:double:True",
    ]


def test_structured_and_legacy_dispatch_share_cancellation_and_hooks(tmp_path: Path) -> None:
    events: list[str] = []
    executor = DccApiExecutor(
        "maya",
        dispatcher=_real_dispatch_pipeline(events, observe_cancel=True),
        script_materialization_root=tmp_path,
    )
    descriptor = materialize_script(
        "def main(scale: float) -> float:\n"
        "    response = dispatch('double', {'value': scale})\n"
        "    if not response['output'].get('success', True):\n"
        "        raise RuntimeError(response['output']['message'])\n"
        "    return response['output']['result']\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )
    token = CancelToken()
    token.cancel()
    reset = set_cancel_token(token)
    try:
        structured = executor.execute_params(
            {"file_path": descriptor.file_path, "params": {"scale": 2.5}},
        )
        legacy = executor.execute_params(
            {
                "code": "response = dispatch('double', {'value': 2.5})\n"
                "if not response['output'].get('success', True):\n"
                "    raise RuntimeError(response['output']['message'])\n"
                "return response['output']['result']",
            },
        )
    finally:
        reset_cancel_token(reset)

    assert structured["success"] is False
    assert legacy["success"] is False
    assert events == [
        "before:double",
        "after:double:True",
        "before:double",
        "after:double:True",
    ]


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


def test_structured_params_support_qualified_typing_extensions_annotations(tmp_path: Path) -> None:
    source = (
        "import typing_extensions\n"
        "def main(\n"
        "    value: typing_extensions.Optional[int],\n"
        "    values: typing_extensions.List[int],\n"
        "    mode: typing_extensions.Literal['sum', 'count'],\n"
        ") -> int:\n"
        "    if mode == 'count':\n"
        "        return len(values)\n"
        "    return (value or 0) + sum(values)\n"
    )

    schema = derive_script_parameters_schema(source)
    assert schema is not None
    assert schema["required"] == ["value", "values", "mode"]
    assert schema["properties"]["value"] == {
        "anyOf": [{"type": "integer"}, {"type": "null"}],
    }
    assert schema["properties"]["values"] == {
        "type": "array",
        "items": {"type": "integer"},
    }
    assert schema["properties"]["mode"] == {
        "type": "string",
        "enum": ["sum", "count"],
    }

    descriptor = materialize_script(
        source,
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )
    result = DccApiExecutor("maya", script_materialization_root=tmp_path).execute_params(
        {
            "file_path": descriptor.file_path,
            "params": {"value": None, "values": [2, 3], "mode": "sum"},
        },
    )

    assert result["success"] is True
    assert result["output"] == 5


@pytest.mark.parametrize(
    ("module_name", "escape"),
    [
        (
            "typing",
            "typing.Any.__new__.__globals__['sys'].modules['os'].getcwd()",
        ),
        (
            "typing",
            "typing.Optional.__getitem__.__globals__['sys'].modules['os'].getcwd()",
        ),
        (
            "typing_extensions",
            "typing_extensions.Any.__new__.__globals__['sys'].modules['os'].getcwd()",
        ),
        (
            "typing_extensions",
            "typing_extensions.Optional.__getitem__.__globals__['sys'].modules['os'].getcwd()",
        ),
    ],
)
def test_structured_params_typing_proxy_objects_have_no_module_global_escape(
    tmp_path: Path,
    module_name: str,
    escape: str,
) -> None:
    descriptor = materialize_script(
        f"import {module_name}\ndef main(value: {module_name}.Optional[int]) -> str:\n    return {escape}\n",
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )
    assert descriptor.parameters_schema is not None

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


def test_module_main_leading_underscore_parameter_matches_runtime_call(tmp_path: Path) -> None:
    source = "def main(_scale: float) -> float:\n    return _scale * 2\n"
    schema = derive_script_parameters_schema(source)
    descriptor = materialize_script(
        source,
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )
    result = DccApiExecutor("maya", script_materialization_root=tmp_path).execute_params(
        {"file_path": descriptor.file_path, "params": {"_scale": 2.5}},
    )

    assert schema is not None
    assert schema["required"] == ["_scale"]
    assert schema["properties"]["_scale"] == {"type": "number"}
    assert result["success"] is True
    assert result["output"] == 5.0


def test_optional_without_default_is_required_but_nullable() -> None:
    schema = derive_script_parameters_schema(
        "from typing import Optional\ndef main(value: Optional[int]) -> int:\n    return 0 if value is None else value\n",
    )
    validator = ToolValidator.from_schema_json(json.dumps(schema))

    missing, missing_errors = validator.validate("{}")
    explicit_null, null_errors = validator.validate('{"value": null}')

    assert schema is not None
    assert schema["required"] == ["value"]
    assert missing is False
    assert missing_errors
    assert explicit_null is True, null_errors


@pytest.mark.parametrize(
    "source",
    [
        "def main(values: list[int]) -> int:\n    return len(values)\n",
        "def main(values: dict[str, int]) -> int:\n    return len(values)\n",
        "def main(values: tuple[int, int]) -> int:\n    return len(values)\n",
    ],
)
def test_python38_incompatible_pep585_annotations_are_not_published(source: str) -> None:
    assert derive_script_parameters_schema(source) is None


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


@pytest.mark.parametrize("literal", ["b'x'", "..."])
def test_module_main_rejects_non_json_literal_values(literal: str) -> None:
    source = f"from typing import Literal\ndef main(value: Literal[{literal}]):\n    return value\n"

    assert derive_script_parameters_schema(source) is None


@pytest.mark.parametrize("literal", ["b'x'", "..."])
def test_non_json_literal_keeps_legacy_materialization_available(tmp_path: Path, literal: str) -> None:
    source = f"from typing import Literal\ndef main(value: Literal[{literal}]):\n    return value\n"

    descriptor = materialize_script(
        source,
        dcc_type="maya",
        instance_id="maya-1",
        session_id="session-1",
        root=tmp_path,
    )

    assert descriptor.parameters_schema is None


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
