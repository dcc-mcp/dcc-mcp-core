"""Historical dependency preparation never grants current code an exemption."""

from __future__ import annotations

import pytest
from scripts.ci.python37_wheel_smoke import historical_release
from scripts.ci.python37_wheel_smoke import smoke_wheel

_COMMIT = "e8d070fd703164a380895af2d6b4e17b3cb2459c"


def test_only_exact_pre_cutover_tag_can_prepare_declared_dependencies() -> None:
    assert historical_release("v0.20.21", "0.20.21", _COMMIT, _COMMIT, "0.20.22")
    assert not historical_release("", "0.20.21", _COMMIT, _COMMIT, "0.20.22")
    assert not historical_release("main", "0.20.21", _COMMIT, _COMMIT, "0.20.22")
    assert not historical_release("v0.20.22", "0.20.22", _COMMIT, _COMMIT, "0.20.22")
    assert not historical_release("v1.0.0", "1.0.0", _COMMIT, _COMMIT, "0.20.22")
    assert historical_release("refs/tags/v0.20.21", "0.20.21", _COMMIT, _COMMIT, "0.20.22")
    assert not historical_release("refs/heads/v0.20.21", "0.20.21", _COMMIT, _COMMIT, "0.20.22")


@pytest.mark.parametrize(
    "version,head,tag",
    [
        ("0.20.20", _COMMIT, _COMMIT),
        ("0.20.21", "a" * 40, _COMMIT),
        ("0.20.21", _COMMIT, ""),
        ("0.20.21", "", ""),
    ],
)
def test_historical_ref_version_or_identity_mismatch_is_rejected(version: str, head: str, tag: str) -> None:
    with pytest.raises(ValueError, match="historical"):
        historical_release("v0.20.21", version, head, tag, "0.20.22")


@pytest.mark.parametrize("profile", ["lite_py37", "native_py37"])
@pytest.mark.parametrize("historical", [False, True])
@pytest.mark.parametrize("has_contract", [False, True])
def test_install_and_smoke_commands_enforce_the_same_profile_boundary(
    tmp_path, monkeypatch, profile: str, historical: bool, has_contract: bool
) -> None:
    commands = []
    monkeypatch.setattr(
        "scripts.ci.python37_wheel_smoke.subprocess.run", lambda command, **kwargs: commands.append(command)
    )
    wheel = tmp_path / "dcc_mcp_core-0.20.21-py3-none-any.whl"
    smoke_wheel(wheel, profile, historical, has_contract)
    install = next(command for command in commands if "pip" in command)
    assert ("--no-deps" in install) is not historical
    assert install[-1] == str(wheel)
    isolated = [command for command in commands if any("smoke_zero_typing_extensions.py" in part for part in command)]
    assert bool(isolated) is not historical
    if not historical:
        assert commands.index(isolated[0]) < commands.index(install)
    if historical and not has_contract:
        assert commands[-1][-2:] == ["-c", "import dcc_mcp_core"]
    else:
        assert commands[-1][-2:] == ["--profile", profile]
