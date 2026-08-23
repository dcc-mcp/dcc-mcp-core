"""Public Install SOP v1 contract regressions."""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def test_install_sop_schema_is_public_and_versioned() -> None:
    import dcc_mcp_core
    from dcc_mcp_core import deployment

    assert dcc_mcp_core.INSTALL_SOP_SCHEMA_VERSION == 1
    assert deployment.INSTALL_SOP_SCHEMA_VERSION == 1
    assert dcc_mcp_core.load_install_sop_schema is deployment.load_install_sop_schema
    assert "load_install_sop_schema" in dcc_mcp_core.__all__
    assert "load_install_sop_schema" in deployment.__all__

    schema = deployment.load_install_sop_schema()

    assert schema["$id"] == "https://dcc-mcp.github.io/schemas/adapter-install-sop-v1.schema.json"
    assert schema["properties"]["schema_version"] == {"const": 1, "type": "integer"}


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
