"""Public Install SOP v1 contract regressions."""

from __future__ import annotations

from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess

from jsonschema import Draft202012Validator
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
    assert "load_install_sop_schema" in dcc_mcp_core.__all__
    assert "load_install_sop_schema" in deployment.__all__

    schema = deployment.load_install_sop_schema()

    Draft202012Validator.check_schema(schema)
    assert schema["$id"] == "https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json"
    assert schema["properties"]["schema_version"] == {"const": 1, "type": "integer"}


def test_install_sop_schema_checkout_forces_the_canonical_git_blob_bytes() -> None:
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


def test_creator_guidance_routes_installers_to_the_sop_contract() -> None:
    creator = (REPO_ROOT / "skills" / "dcc-mcp-creator" / "SKILL.md").read_text(encoding="utf-8")
    testing = (REPO_ROOT / "skills" / "dcc-mcp-creator" / "references" / "TESTING_AND_RELEASE.md").read_text(
        encoding="utf-8"
    )

    for text in (creator, testing):
        assert "adapter-install-sop.md" in text
        assert "load_install_sop_schema" in text
        assert "INSTALL_EXIT_CODES" in text

    assert "plan -> execute -> verify -> status -> uninstall" in testing
