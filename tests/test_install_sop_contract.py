"""Public Install SOP v1 contract regressions."""

from __future__ import annotations

from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
from types import SimpleNamespace

from jsonschema import Draft202012Validator
import pytest
from scripts.ci.python_support_contract import load_contract

REPO_ROOT = Path(__file__).resolve().parent.parent
INSTALL_SOP_SCHEMA_PATH = Path("python/dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json")


def _install_result_with_next_step(next_step: dict) -> dict:
    return {
        "schema_version": 1,
        "status": "planned",
        "dcc_type": "example",
        "adapter_version": "1.2.3",
        "core_version": "0.20.12",
        "steps": [{"id": "preflight", "status": "ok"}],
        "next_steps": [next_step],
        "receipt_path": None,
        "verify": {
            "directly_usable": False,
            "failure_stage": None,
            "failure_reason": None,
        },
    }


def _command_next_step() -> dict:
    return {
        "id": "execute",
        "description": "Execute the validated install plan.",
        "why": "Planning does not mutate the host.",
        "command": ["dcc-mcp-example", "install", "--json", "--yes"],
    }


def _file_edit_next_step(action: str, *, include_content: bool) -> dict:
    file_edit = {"path": "install.md", "action": action}
    if include_content:
        file_edit["content"] = "# Install\n"
    return {
        "id": f"{action}-install-guide",
        "description": "Update the adapter install guide.",
        "why": "The repository must publish its adapter-specific instructions.",
        "file_edit": file_edit,
    }


def test_install_sop_schema_is_public_and_versioned() -> None:
    import dcc_mcp_core
    from dcc_mcp_core import deployment

    assert dcc_mcp_core.INSTALL_SOP_SCHEMA_VERSION == 1
    assert deployment.INSTALL_SOP_SCHEMA_VERSION == 1
    assert dcc_mcp_core.load_install_sop_schema is deployment.load_install_sop_schema
    assert dcc_mcp_core.validate_install_sop_report is deployment.validate_install_sop_report
    assert "load_install_sop_schema" in dcc_mcp_core.__all__
    assert "validate_install_sop_report" in dcc_mcp_core.__all__
    assert "load_install_sop_schema" in deployment.__all__
    assert "validate_install_sop_report" in deployment.__all__

    schema = deployment.load_install_sop_schema()

    Draft202012Validator.check_schema(schema)
    assert schema["$id"] == "https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json"
    assert schema["properties"]["schema_version"] == {"const": 1, "type": "integer"}


def test_install_sop_schema_checkout_forces_the_canonical_git_blob_bytes() -> None:
    from dcc_mcp_core.deployment import install_sop

    contract = load_contract(REPO_ROOT)
    resource = contract["distributions"]["dcc-mcp-core"]["wheel_resources"][0]
    git_blob = subprocess.run(
        ["git", "cat-file", "blob", f"HEAD:{INSTALL_SOP_SCHEMA_PATH.as_posix()}"],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.PIPE,
    ).stdout
    attributes = subprocess.run(
        ["git", "check-attr", "eol", "--", INSTALL_SOP_SCHEMA_PATH.as_posix()],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    ).stdout

    assert attributes.rstrip().endswith(": eol: lf")
    assert resource["source"] == INSTALL_SOP_SCHEMA_PATH.as_posix()
    assert resource["canonical_url"] == "https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json"
    assert hashlib.sha256(git_blob).hexdigest() == resource["sha256"]
    assert resource["sha256"] == install_sop._INSTALL_SOP_SCHEMA_SHA256


def test_install_sop_schema_requires_agent_executable_results() -> None:
    from dcc_mcp_core import load_install_sop_schema

    schema = load_install_sop_schema()

    assert set(schema["required"]) == {
        "schema_version",
        "status",
        "dcc_type",
        "adapter_version",
        "core_version",
        "steps",
        "next_steps",
        "receipt_path",
        "verify",
    }
    next_step = schema["$defs"]["next_step"]
    assert set(next_step["required"]) == {"id", "description", "why"}
    assert next_step["oneOf"] == [
        {"required": ["command"]},
        {"required": ["file_edit"]},
    ]


def test_install_sop_schema_requires_nonblank_command_arguments() -> None:
    from dcc_mcp_core import load_install_sop_schema

    validator = Draft202012Validator(load_install_sop_schema())
    valid = _install_result_with_next_step(_command_next_step())

    assert validator.is_valid(valid)

    for bad_argument in ("", "   ", "\t\r\n"):
        invalid = deepcopy(valid)
        invalid["next_steps"][0]["command"][1] = bad_argument
        assert not validator.is_valid(invalid)


def test_install_sop_schema_requires_content_for_create_and_update_edits() -> None:
    from dcc_mcp_core import load_install_sop_schema

    validator = Draft202012Validator(load_install_sop_schema())

    for action in ("create", "update"):
        valid = _install_result_with_next_step(_file_edit_next_step(action, include_content=True))
        valid_empty_content = deepcopy(valid)
        valid_empty_content["next_steps"][0]["file_edit"]["content"] = ""
        missing_content = _install_result_with_next_step(_file_edit_next_step(action, include_content=False))

        assert validator.is_valid(valid)
        assert validator.is_valid(valid_empty_content)
        assert not validator.is_valid(missing_content)


def test_install_sop_schema_requires_nonblank_file_edit_paths() -> None:
    from dcc_mcp_core import load_install_sop_schema

    validator = Draft202012Validator(load_install_sop_schema())
    valid = _install_result_with_next_step(_file_edit_next_step("update", include_content=True))

    assert validator.is_valid(valid)

    for bad_path in ("", "   ", "\t\r\n"):
        invalid = deepcopy(valid)
        invalid["next_steps"][0]["file_edit"]["path"] = bad_path
        assert not validator.is_valid(invalid)


def test_install_sop_schema_forbids_content_for_remove_edits() -> None:
    from dcc_mcp_core import load_install_sop_schema

    validator = Draft202012Validator(load_install_sop_schema())
    valid = _install_result_with_next_step(_file_edit_next_step("remove", include_content=False))
    unexpected_content = _install_result_with_next_step(_file_edit_next_step("remove", include_content=True))

    assert validator.is_valid(valid)
    assert not validator.is_valid(unexpected_content)


def test_install_sop_schema_requires_exactly_one_executable_form() -> None:
    from dcc_mcp_core import load_install_sop_schema

    validator = Draft202012Validator(load_install_sop_schema())
    valid = _install_result_with_next_step(_command_next_step())

    dual_form = deepcopy(valid)
    dual_form["next_steps"][0]["file_edit"] = {"path": "install.md", "action": "remove"}
    missing_form = deepcopy(valid)
    del missing_form["next_steps"][0]["command"]

    assert not validator.is_valid(dual_form)
    assert not validator.is_valid(missing_form)


def test_install_sop_validator_enforces_full_draft_without_python_jsonschema(monkeypatch) -> None:
    from dcc_mcp_core import validate_install_sop_report

    valid = _install_result_with_next_step(_command_next_step())
    real_import = __import__

    def reject_jsonschema(name, globals=None, locals=None, fromlist=(), level=0):
        if name == "jsonschema" or name.startswith("jsonschema."):
            raise AssertionError("production validation imported Python jsonschema")
        return real_import(name, globals, locals, fromlist, level)

    monkeypatch.setattr("builtins.__import__", reject_jsonschema)
    validate_install_sop_report(valid)

    empty_command = deepcopy(valid)
    empty_command["next_steps"][0]["command"] = []
    with pytest.raises(ValueError) as empty_error:
        validate_install_sop_report(empty_command)
    assert "/next_steps/0/command" in str(empty_error.value)
    assert "minItems" in str(empty_error.value)

    dual_form = deepcopy(valid)
    dual_form["next_steps"][0]["file_edit"] = {"path": "install.md", "action": "remove"}
    with pytest.raises(ValueError) as dual_error:
        validate_install_sop_report(dual_form)
    assert "/next_steps/0" in str(dual_error.value)
    assert "oneOf" in str(dual_error.value)

    missing_content = _install_result_with_next_step(_file_edit_next_step("create", include_content=False))
    with pytest.raises(ValueError) as content_error:
        validate_install_sop_report(missing_content)
    assert "/next_steps/0/file_edit" in str(content_error.value)
    assert "required" in str(content_error.value)

    blank_path = _install_result_with_next_step(_file_edit_next_step("update", include_content=True))
    blank_path["next_steps"][0]["file_edit"]["path"] = "   "
    with pytest.raises(ValueError) as path_error:
        validate_install_sop_report(blank_path)
    assert "/next_steps/0/file_edit/path" in str(path_error.value)
    assert "pattern" in str(path_error.value)

    unsupported_version = deepcopy(valid)
    unsupported_version["schema_version"] = 2
    with pytest.raises(ValueError) as version_error:
        validate_install_sop_report(unsupported_version)
    assert "/schema_version" in str(version_error.value)
    assert "const" in str(version_error.value)


@pytest.mark.parametrize(
    ("schema_bytes", "error_code"),
    [
        (b'{"$id":"x","$id":"y"}', "schema_duplicate_key"),
        (
            b'{"$id":"https://attacker.invalid/schema","$schema":"https://json-schema.org/draft/2020-12/schema"}',
            "schema_identity_mismatch",
        ),
        (
            b'{"$id":"https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json","$schema":"https://attacker.invalid/draft"}',
            "schema_dialect_mismatch",
        ),
        (
            b'{"$id":"https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json","$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"file:///private/schema.json"}',
            "schema_external_ref",
        ),
        (
            b'{"$id":"https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json","$schema":"https://json-schema.org/draft/2020-12/schema","type":"object"}',
            "schema_digest_mismatch",
        ),
        (b"\xff", "schema_invalid_utf8"),
        (b'{"$id":', "schema_invalid_json"),
    ],
)
def test_install_sop_schema_loader_rejects_untrusted_bytes(
    monkeypatch, tmp_path: Path, schema_bytes: bytes, error_code: str
) -> None:
    from dcc_mcp_core.deployment import install_sop

    schema_path = tmp_path / "untrusted.schema.json"
    schema_path.write_bytes(schema_bytes)
    monkeypatch.setattr(install_sop, "_SCHEMA_PATH", schema_path)

    with pytest.raises(RuntimeError, match=rf"^Install SOP schema integrity error: {error_code}$") as error:
        install_sop.load_install_sop_schema()

    assert str(schema_path) not in str(error.value)


def test_install_sop_schema_loader_redacts_missing_path(monkeypatch, tmp_path: Path) -> None:
    from dcc_mcp_core.deployment import install_sop

    schema_path = tmp_path / "missing-private.schema.json"
    monkeypatch.setattr(install_sop, "_SCHEMA_PATH", schema_path)

    with pytest.raises(RuntimeError, match=r"^Install SOP schema integrity error: schema_unavailable$") as error:
        install_sop.load_install_sop_schema()

    assert str(schema_path) not in str(error.value)


def test_install_sop_validator_separates_schema_report_and_native_failures(monkeypatch, tmp_path: Path) -> None:
    import dcc_mcp_core
    from dcc_mcp_core.deployment import install_sop

    report = _install_result_with_next_step(_command_next_step())
    malformed_schema = tmp_path / "malformed-private.schema.json"
    malformed_schema.write_text('{"$id":', encoding="utf-8")
    monkeypatch.setattr(install_sop, "_SCHEMA_PATH", malformed_schema)
    with pytest.raises(RuntimeError, match=r"schema_invalid_json") as schema_error:
        install_sop.validate_install_sop_report(report)
    assert str(malformed_schema) not in str(schema_error.value)

    monkeypatch.undo()
    invalid_report = deepcopy(report)
    invalid_report["adapter_specific"] = object()
    with pytest.raises(ValueError, match=r"^Install SOP report is not JSON-compatible$"):
        install_sop.validate_install_sop_report(invalid_report)

    def fail_native(*_):
        raise ValueError("PRIVATE-NATIVE-DETAIL")

    monkeypatch.setattr(
        dcc_mcp_core,
        "_core",
        SimpleNamespace(_validate_install_sop_report_json=fail_native),
    )
    with pytest.raises(RuntimeError, match=r"^Install SOP validator runtime error: native_call_failed$") as error:
        install_sop.validate_install_sop_report(report)
    assert "PRIVATE-NATIVE-DETAIL" not in str(error.value)


def test_install_sop_native_validator_rejects_untrusted_schema_and_duplicate_report_keys() -> None:
    from dcc_mcp_core import _core

    assert not hasattr(_core, "_validate_json_schema_draft_2020_12")
    validator = _core._validate_install_sop_report_json
    canonical_schema = (REPO_ROOT / INSTALL_SOP_SCHEMA_PATH).read_text(encoding="utf-8")

    with pytest.raises(ValueError, match=r"^install_sop_schema_digest_mismatch$"):
        validator('{"type":"object"}', "{}")
    with pytest.raises(ValueError, match=r"^install_sop_schema_duplicate_key$"):
        validator('{"$id":"x","$id":"y"}', "{}")
    with pytest.raises(ValueError, match=r"^install_sop_schema_external_ref$"):
        validator(
            '{"$id":"https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json",'
            '"$schema":"https://json-schema.org/draft/2020-12/schema",'
            '"$ref":"https://attacker.invalid/schema"}',
            "{}",
        )
    with pytest.raises(ValueError, match=r"^install_sop_report_duplicate_key$"):
        validator(canonical_schema, '{"schema_version":1,"schema_version":2}')


@pytest.mark.parametrize(
    "mutate",
    [
        lambda report: report["steps"].append(deepcopy(report["steps"][0])),
        lambda report: report["next_steps"].append(deepcopy(report["next_steps"][0])),
        lambda report: report["next_steps"][0]["command"].__setitem__(1, "install\x00--yes"),
        lambda report: report["next_steps"][0]["command"].__setitem__(1, "install\n--yes"),
    ],
)
def test_install_sop_validator_rejects_duplicate_ids_and_executable_controls(mutate) -> None:
    from dcc_mcp_core import validate_install_sop_report

    report = _install_result_with_next_step(_command_next_step())
    mutate(report)

    with pytest.raises(ValueError, match=r"Install SOP report failed semantic validation"):
        validate_install_sop_report(report)


def test_install_sop_validator_uses_one_stable_document_for_schema_and_semantics(monkeypatch) -> None:
    import dcc_mcp_core
    from dcc_mcp_core import _core
    from dcc_mcp_core import validate_install_sop_report

    report = _install_result_with_next_step(_command_next_step())
    report["next_steps"].append(deepcopy(report["next_steps"][0]))
    native_validator = _core._validate_install_sop_report_json

    def validate_then_change_caller_mapping(schema_json: str, report_json: str):
        errors = native_validator(schema_json, report_json)
        report["next_steps"].pop()
        return errors

    monkeypatch.setattr(
        dcc_mcp_core,
        "_core",
        SimpleNamespace(_validate_install_sop_report_json=validate_then_change_caller_mapping),
    )

    with pytest.raises(ValueError, match=r"code=duplicate_next_step_id path=/next_steps/1/id"):
        validate_install_sop_report(report)

    assert len(report["next_steps"]) == 1


@pytest.mark.parametrize(
    ("build_report", "mutate", "expected_path"),
    [
        (
            lambda: _install_result_with_next_step(_command_next_step()),
            lambda report: report["steps"][0].__setitem__("id", "preflight\u0080host"),
            "/steps/0/id",
        ),
        (
            lambda: _install_result_with_next_step(_command_next_step()),
            lambda report: report["next_steps"][0].__setitem__("id", "execute\u0085install"),
            "/next_steps/0/id",
        ),
        (
            lambda: _install_result_with_next_step(_command_next_step()),
            lambda report: report["next_steps"][0]["command"].__setitem__(1, "install\u009f--yes"),
            "/next_steps/0/command/1",
        ),
        (
            lambda: _install_result_with_next_step(_file_edit_next_step("update", include_content=True)),
            lambda report: report["next_steps"][0]["file_edit"].__setitem__("path", "install\u0081outside.md"),
            "/next_steps/0/file_edit/path",
        ),
        (
            lambda: _install_result_with_next_step(_command_next_step()),
            lambda report: report.__setitem__("receipt_path", "receipts/install\u009f.json"),
            "/receipt_path",
        ),
    ],
)
def test_install_sop_validator_rejects_c1_control_characters(build_report, mutate, expected_path) -> None:
    from dcc_mcp_core import validate_install_sop_report

    report = build_report()
    mutate(report)

    with pytest.raises(ValueError, match=r"Install SOP report failed semantic validation") as error:
        validate_install_sop_report(report)

    assert f"code=control_character path={expected_path}" in str(error.value)


def test_install_sop_validator_allows_normal_unicode_text() -> None:
    from dcc_mcp_core import validate_install_sop_report

    report = _install_result_with_next_step(_command_next_step())
    report["steps"][0]["id"] = "preflight-检查"
    report["next_steps"][0]["id"] = "execute-安装"
    report["next_steps"][0]["command"][1] = "install\u00a0OBS"
    report["receipt_path"] = "状态/收据-😀.json"

    validate_install_sop_report(report)


def test_install_sop_validator_rejects_control_characters_in_file_edit_paths() -> None:
    from dcc_mcp_core import validate_install_sop_report

    report = _install_result_with_next_step(_file_edit_next_step("update", include_content=True))
    report["next_steps"][0]["file_edit"]["path"] = "install.md\routside"

    with pytest.raises(ValueError, match=r"Install SOP report failed semantic validation"):
        validate_install_sop_report(report)


def test_install_sop_validator_emits_bounded_value_free_diagnostics() -> None:
    from dcc_mcp_core import validate_install_sop_report

    report = _install_result_with_next_step(_command_next_step())
    private_value = "PRIVATE-VALUE-" + ("x" * 4096)
    report["status"] = private_value
    report["next_steps"] = [
        {
            "id": f"execute-{index}",
            "description": "Execute the validated install plan.",
            "why": "Planning does not mutate the host.",
            "command": ["dcc-mcp-example", " " * 4096],
        }
        for index in range(48)
    ]

    messages = []
    for _ in range(2):
        with pytest.raises(ValueError) as error:
            validate_install_sop_report(report)
        messages.append(str(error.value))

    assert messages[0] == messages[1]
    assert "PRIVATE-VALUE" not in messages[0]
    assert len(messages[0].encode("utf-8")) <= 16_384
    details = messages[0].splitlines()[1:]
    assert details
    assert all(len(line.encode("utf-8")) <= 640 for line in details)
    assert all("code=" in line and "instance=" in line and "schema=" in line for line in details)


@pytest.mark.parametrize(
    ("native_result", "error_code"),
    [
        (None, "native_result_type"),
        (("error",), "native_result_type"),
        ([1], "native_result_entry_type"),
        (["ok", 1], "native_result_entry_type"),
    ],
)
def test_install_sop_validator_rejects_native_result_shape(monkeypatch, native_result: object, error_code: str) -> None:
    import dcc_mcp_core
    from dcc_mcp_core import validate_install_sop_report

    fake_core = SimpleNamespace(_validate_install_sop_report_json=lambda *_: native_result)
    monkeypatch.setattr(dcc_mcp_core, "_core", fake_core)

    with pytest.raises(RuntimeError, match=rf"^Install SOP validator runtime error: {error_code}$"):
        validate_install_sop_report(_install_result_with_next_step(_command_next_step()))


def test_install_sop_validator_requires_callable_native_symbol(monkeypatch) -> None:
    import dcc_mcp_core
    from dcc_mcp_core import validate_install_sop_report

    monkeypatch.setattr(
        dcc_mcp_core,
        "_core",
        SimpleNamespace(_validate_install_sop_report_json="not-callable"),
    )

    with pytest.raises(RuntimeError, match=r"^Install SOP validator runtime error: native_symbol_unavailable$"):
        validate_install_sop_report(_install_result_with_next_step(_command_next_step()))


def test_install_sop_validation_is_not_path_or_argv_authorization() -> None:
    from dcc_mcp_core import validate_install_sop_report

    traversal = _install_result_with_next_step(_file_edit_next_step("update", include_content=True))
    traversal["next_steps"][0]["file_edit"]["path"] = "../../outside/install.md"
    absolute = _install_result_with_next_step(_command_next_step())
    absolute["next_steps"][0]["command"].append("C:\\private\\target")

    validate_install_sop_report(traversal)
    validate_install_sop_report(absolute)

    guide = (REPO_ROOT / "docs" / "guide" / "adapter-install-sop.md").read_text(encoding="utf-8").lower()
    for phrase in ("authorization boundary", "path traversal", "absolute paths", "command arguments"):
        assert phrase in guide


def test_install_sop_schema_allows_additive_adapter_fields() -> None:
    from dcc_mcp_core import load_install_sop_schema

    validator = Draft202012Validator(load_install_sop_schema())
    result = _install_result_with_next_step(_file_edit_next_step("update", include_content=True))
    result["adapter_diagnostic"] = {"code": "profile_selected"}
    result["steps"][0]["duration_ms"] = 12
    result["next_steps"][0]["confirmation"] = "operator"
    result["next_steps"][0]["file_edit"]["encoding"] = "utf-8"

    assert validator.is_valid(result)


def test_install_sop_exit_codes_are_stable_public_exports() -> None:
    import dcc_mcp_core
    from dcc_mcp_core import deployment

    expected = {
        "ok": 0,
        "preflight": 10,
        "acquire": 20,
        "install": 30,
        "verify": 40,
        "requires_restart": 50,
    }

    assert expected == dcc_mcp_core.INSTALL_EXIT_CODES
    assert expected == deployment.INSTALL_EXIT_CODES
    assert dcc_mcp_core.INSTALL_EXIT_OK == 0
    assert dcc_mcp_core.INSTALL_EXIT_PREFLIGHT == 10
    assert dcc_mcp_core.INSTALL_EXIT_ACQUIRE == 20
    assert dcc_mcp_core.INSTALL_EXIT_INSTALL == 30
    assert dcc_mcp_core.INSTALL_EXIT_VERIFY == 40
    assert dcc_mcp_core.INSTALL_EXIT_REQUIRES_RESTART == 50


def test_install_sop_guide_ships_a_reusable_install_template() -> None:
    guide = REPO_ROOT / "docs" / "guide" / "adapter-install-sop.md"
    template = REPO_ROOT / "docs" / "guide" / "templates" / "adapter-install.md"

    guide_text = guide.read_text(encoding="utf-8")
    template_text = template.read_text(encoding="utf-8")

    for heading in (
        "## Universal command surface",
        "## Plan and execution contract",
        "## JSON result contract",
        "## Preflight",
        "## Transaction and receipt contract",
        "## Verify to usable",
        "## Bootstrap diagnostics",
        "## CI acceptance",
        "## Reusable `install.md` template",
    ):
        assert heading in guide_text

    for heading in (
        "## Requirements",
        "## Supported versions",
        "## Agent quick path",
        "## Manual path",
        "## Verify",
        "## Upgrade",
        "## Uninstall",
        "## Troubleshooting",
    ):
        assert heading in template_text


def test_reusable_install_template_usable_result_is_schema_valid() -> None:
    from dcc_mcp_core import load_install_sop_schema

    template = REPO_ROOT / "docs" / "guide" / "templates" / "adapter-install.md"
    template_text = template.read_text(encoding="utf-8")
    marker = "A usable result has:\n\n```json\n"
    result_text = template_text.split(marker, maxsplit=1)[1].split("\n```", maxsplit=1)[0]

    Draft202012Validator(load_install_sop_schema()).validate(json.loads(result_text))


def test_rust_cli_failed_execution_report_fixture_is_schema_valid() -> None:
    from dcc_mcp_core import load_install_sop_schema

    fixture = REPO_ROOT / "tests" / "fixtures" / "install-execution-report-v1-failed.json"
    report = json.loads(fixture.read_text(encoding="utf-8"))

    Draft202012Validator(load_install_sop_schema()).validate(report)
    assert report["exit_code"] == 30
    assert report["stage"] == "install"
    assert report["error"]["code"] == "INSTALL_STEP_FAILED"
    assert report["rollback"] == {"attempted": True, "status": "ok", "failure_count": 0}
    assert [step["status"] for step in report["steps"]] == ["ok", "failed", "not_run"]


def test_rust_cli_success_execution_report_fixture_is_schema_valid() -> None:
    from dcc_mcp_core import load_install_sop_schema

    fixture = REPO_ROOT / "tests" / "fixtures" / "install-execution-report-v1-success.json"
    report = json.loads(fixture.read_text(encoding="utf-8"))

    Draft202012Validator(load_install_sop_schema()).validate(report)
    assert report["status"] == "ok"
    assert report["exit_code"] == 0
    assert report["receipt_path"] is None
    assert report["verify"] == {
        "directly_usable": False,
        "failure_stage": "host-readiness",
        "failure_reason": "LIVE_DCC_VERIFICATION_REQUIRED",
    }


def test_rust_cli_deferred_registration_report_fixture_is_schema_valid() -> None:
    from dcc_mcp_core import load_install_sop_schema

    fixture = REPO_ROOT / "tests" / "fixtures" / "install-execution-report-v1-deferred.json"
    report = json.loads(fixture.read_text(encoding="utf-8"))

    Draft202012Validator(load_install_sop_schema()).validate(report)
    assert report["status"] == "partial"
    assert report["exit_code"] == 0
    assert [step["status"] for step in report["steps"]] == ["ok", "deferred", "ok"]
    assert report["verify"]["directly_usable"] is False


def test_rust_cli_rollback_failure_report_fixture_is_schema_valid() -> None:
    from dcc_mcp_core import load_install_sop_schema

    fixture = REPO_ROOT / "tests" / "fixtures" / "install-execution-report-v1-rollback-failed.json"
    report = json.loads(fixture.read_text(encoding="utf-8"))

    Draft202012Validator(load_install_sop_schema()).validate(report)
    assert report["status"] == "partial"
    assert report["error"] == {
        "code": "ROLLBACK_FAILED",
        "stage": "rollback",
        "exit_code": 30,
        "primary_code": "INSTALL_STEP_FAILED",
    }
    assert report["rollback"] == {"attempted": True, "status": "failed", "failure_count": 1}


def test_adapter_onboarding_and_release_gate_the_install_sop() -> None:
    onboarding = (REPO_ROOT / "docs" / "guide" / "new-adapter-onboarding.md").read_text(encoding="utf-8")
    release = (REPO_ROOT / "docs" / "guide" / "adapter-release-checklist.md").read_text(encoding="utf-8")

    for text in (onboarding, release):
        assert "adapter-install-sop.md" in text
        assert "install|status|verify|uninstall|upgrade" in text
        assert "adapter-install-sop-v1.schema.json" in text
        assert "0/10/20/30/40/50" in text
        assert "instructions_url" in text
        assert "receipt" in text.lower()

    assert "install.md" in onboarding
    assert "plan -> execute -> verify -> status -> uninstall" in release
