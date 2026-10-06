"""Tests for the adapter to core version requirement contract.

Covers the three behaviours the contract exists to guarantee: a PEP 440
requirement and a package-environment requirement describe the same bounds, an
adapter's package-environment requirement is never *wider* than the range its
distribution metadata declares, and a running core outside that range fails at
the adapter's front door with a message naming the version.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import os

# Import third-party modules
import pytest

from dcc_mcp_core.version_compat.core_requirement import CORE_IMPORT_NAME
from dcc_mcp_core.version_compat.core_requirement import CORE_PACKAGE

# Import local modules
from dcc_mcp_core.version_compat.core_requirement import ENV_ENFORCE
from dcc_mcp_core.version_compat.core_requirement import CoreRequirementError
from dcc_mcp_core.version_compat.core_requirement import Requirement
from dcc_mcp_core.version_compat.core_requirement import check_requirement
from dcc_mcp_core.version_compat.core_requirement import compare_requirements
from dcc_mcp_core.version_compat.core_requirement import compare_versions
from dcc_mcp_core.version_compat.core_requirement import enforce_core_compatibility
from dcc_mcp_core.version_compat.core_requirement import open_ceiling
from dcc_mcp_core.version_compat.core_requirement import package_environment_name
from dcc_mcp_core.version_compat.core_requirement import requirement_from_package_environment
from dcc_mcp_core.version_compat.core_requirement import requirement_from_pep440
from dcc_mcp_core.version_compat.core_requirement import requirement_from_text
from dcc_mcp_core.version_compat.core_requirement import runtime_core_version


def test_package_environment_name_uses_the_import_spelling() -> None:
    """Both spellings of the core package render as one package name."""
    assert package_environment_name(CORE_PACKAGE) == CORE_IMPORT_NAME
    assert package_environment_name("dcc_mcp_core") == CORE_IMPORT_NAME


@pytest.mark.parametrize(
    "specifier, lower, upper",
    [
        (">=0.19.3,<0.19.5", "0.19.3", "0.19.5"),
        ("dcc-mcp-core>=0.19.3,<0.19.5", "0.19.3", "0.19.5"),
        ("dcc_mcp_core>=0.19.3,<0.19.5", "0.19.3", "0.19.5"),
        (">=0.18.21,<1.0.0", "0.18.21", "1.0.0"),
        ("~=0.19.3", "0.19.3", "0.20.0"),
        ("==0.19.*", "0.19.0", "0.20.0"),
    ],
)
def test_pep440_parsing_reads_both_bounds(specifier: str, lower: str, upper: str) -> None:
    """A declared range keeps the floor and the exclusive ceiling."""
    requirement = requirement_from_pep440(specifier)
    assert requirement.lower == lower
    assert requirement.upper == upper
    assert requirement.upper_inclusive is False


def test_pep440_parsing_keeps_the_tightest_bound() -> None:
    """Overlapping clauses narrow the range instead of the last one winning."""
    requirement = requirement_from_pep440(">=0.19.3,>=0.19.10,<0.19.5,<0.20.0")
    assert requirement.lower == "0.19.10"
    assert requirement.upper == "0.19.5"


def test_pep440_parsing_ignores_exclusions() -> None:
    """``!=`` narrows a range without bounding it in either direction."""
    requirement = requirement_from_pep440(">=0.19.3,!=0.19.7,<0.19.5")
    assert requirement.lower == "0.19.3"
    assert requirement.upper == "0.19.5"


def test_pep440_parsing_rejects_an_empty_requirement() -> None:
    """An empty requirement is a mistake, not an unbounded one."""
    with pytest.raises(ValueError, match="empty core requirement"):
        requirement_from_pep440("   ")


def test_pep440_parsing_rejects_an_unsupported_clause() -> None:
    """A clause the parser cannot read is reported instead of skipped."""
    with pytest.raises(ValueError, match="unsupported core requirement clause"):
        requirement_from_pep440(">=0.19.3,latest")


@pytest.mark.parametrize(
    "token, lower, upper",
    [
        ("dcc_mcp_core", None, None),
        ("dcc_mcp_core-0", "0.0.0", "1.0.0"),
        ("dcc_mcp_core-0.20", "0.20.0", "0.21.0"),
        ("dcc_mcp_core-0.19.3+", "0.19.3", None),
        ("dcc_mcp_core-0.12.18..1", "0.12.18", "1"),
        ("dcc_mcp_core-0.19.3..0.19.5", "0.19.3", "0.19.5"),
        ("dcc_mcp_core-..1.0.0", None, "1.0.0"),
    ],
)
def test_package_environment_parsing(token: str, lower: str | None, upper: str | None) -> None:
    """Every shape an adapter can write maps onto one pair of bounds."""
    requirement = requirement_from_package_environment(token)
    assert requirement.lower == lower
    assert requirement.upper == upper


def test_package_environment_parsing_rejects_a_missing_name() -> None:
    """A token with no package name cannot be resolved."""
    with pytest.raises(ValueError, match="missing a package name"):
        requirement_from_package_environment("-0.19.3")


def test_requirement_from_text_sniffs_the_syntax() -> None:
    """Either syntax is accepted without the caller having to say which."""
    assert requirement_from_text(">=0.19.3,<0.19.5").lower == "0.19.3"
    assert requirement_from_text("dcc_mcp_core-0.19.3..0.19.5").upper == "0.19.5"


def test_rendering_a_bounded_requirement_produces_a_range_token() -> None:
    """The token adapters must generate is derived, not written by hand."""
    requirement = requirement_from_pep440("dcc-mcp-core>=0.19.3,<0.19.5")
    assert requirement.to_package_environment() == "dcc_mcp_core-0.19.3..0.19.5"


def test_rendering_rejects_an_inclusive_ceiling() -> None:
    """An inclusive ceiling has no token, so it is refused, not widened."""
    requirement = requirement_from_pep440("dcc-mcp-core>=0.19.3,<=0.19.5")
    with pytest.raises(ValueError, match="inclusive upper bound"):
        requirement.to_package_environment()


def test_contains_respects_both_ends_of_the_range() -> None:
    """The ceiling is exclusive and the floor is inclusive."""
    requirement = requirement_from_pep440(">=0.19.3,<0.19.5")
    assert requirement.contains("0.19.3") is True
    assert requirement.contains("0.19.4") is True
    assert requirement.contains("0.19.5") is False
    assert requirement.contains("0.20.28") is False
    assert requirement.contains("0.19.2") is False


def test_compare_versions_orders_numerically() -> None:
    """Version ordering is numeric, so 0.19.30 outranks 0.19.4."""
    assert compare_versions("0.19.30", "0.19.4") == 1
    assert compare_versions("0.19.4", "0.19.30") == -1
    assert compare_versions("0.19.4", "0.19.4") == 0
    assert compare_versions("0.20.28", "0.19.5") == 1


def test_compare_requirements_flags_a_wider_package_environment() -> None:
    """``dcc_mcp_core-0`` admits everything the declared range excludes."""
    declared = requirement_from_pep440(">=0.19.3,<0.19.5")
    observed = requirement_from_package_environment("dcc_mcp_core-0")
    assert compare_requirements(declared, observed) == ["lower_bound_too_low", "upper_bound_too_high"]


def test_compare_requirements_flags_a_missing_ceiling() -> None:
    """A floor with no ceiling is the shape that let the resolver drift."""
    declared = requirement_from_pep440(">=0.18.21,<1.0.0")
    observed = requirement_from_package_environment("dcc_mcp_core-0.18.21+")
    assert compare_requirements(declared, observed) == ["missing_upper_bound"]


def test_compare_requirements_accepts_a_narrower_package_environment() -> None:
    """Over-constraining is safe: the resolver still cannot leave the range."""
    declared = requirement_from_pep440(">=0.20.0,<1.0.0")
    observed = requirement_from_package_environment("dcc_mcp_core-0.20")
    assert compare_requirements(declared, observed) == []


def test_compare_requirements_accepts_a_derived_token() -> None:
    """A token generated from the declared requirement round-trips cleanly."""
    declared = requirement_from_pep440(">=0.19.3,<0.19.5")
    observed = requirement_from_package_environment(declared.to_package_environment())
    assert compare_requirements(declared, observed) == []


def test_open_ceiling_detects_the_one_point_zero_placeholder() -> None:
    """``<1.0.0`` excludes nothing that exists while core is 0.x."""
    assert open_ceiling(requirement_from_pep440(">=0.20.14,<1.0.0")) is True
    assert open_ceiling(requirement_from_pep440(">=0.19.3,<0.19.5")) is False


def test_check_requirement_reports_an_unbounded_requirement() -> None:
    """An unbounded requirement is a violation even when the core matches."""
    assert check_requirement(requirement_from_text("dcc_mcp_core-0.19.3+"), "0.19.4") == ["unbounded"]
    assert check_requirement(requirement_from_text("dcc_mcp_core"), "0.19.4") == ["unbounded"]


def test_check_requirement_reports_an_out_of_range_core() -> None:
    """A core the resolver picked outside the range is named as such."""
    requirement = requirement_from_pep440(">=0.19.3,<0.19.5")
    assert check_requirement(requirement, "0.20.28") == ["out_of_range"]
    assert check_requirement(requirement, "0.19.4") == []


def test_enforce_core_compatibility_accepts_a_matching_core() -> None:
    """A satisfied requirement returns the requirement and raises nothing."""
    requirement = enforce_core_compatibility(">=0.19.3,<0.19.5", core_version="0.19.4")
    assert requirement is not None
    assert requirement.to_package_environment() == "dcc_mcp_core-0.19.3..0.19.5"


def test_enforce_core_compatibility_raises_on_a_drifted_core() -> None:
    """The failure names the declared range, the core, and the remedy."""
    with pytest.raises(CoreRequirementError) as raised:
        enforce_core_compatibility(">=0.19.3,<0.19.5", adapter="dcc-mcp-maya", core_version="0.20.28")
    message = str(raised.value)
    assert "dcc-mcp-maya" in message
    assert "0.20.28" in message
    assert "<0.19.5" in message


def test_enforce_core_compatibility_raises_on_an_unbounded_requirement() -> None:
    """An unbounded requirement is as much a violation as a drifted core."""
    with pytest.raises(CoreRequirementError) as raised:
        enforce_core_compatibility("dcc_mcp_core-0.19.3+", core_version="0.19.4")
    assert "no upper bound" in str(raised.value)


def test_enforce_core_compatibility_downgrades_to_a_warning(monkeypatch: pytest.MonkeyPatch) -> None:
    """An environment that must keep running can opt out of the hard failure."""
    monkeypatch.setenv(ENV_ENFORCE, "0")
    requirement = enforce_core_compatibility(">=0.19.3,<0.19.5", core_version="0.20.28")
    assert requirement is not None
    assert requirement.upper == "0.19.5"


def test_enforce_core_compatibility_is_a_no_op_without_a_requirement() -> None:
    """An unknown requirement is not a violation."""
    assert enforce_core_compatibility(adapter="definitely-not-installed-42") is None
    assert enforce_core_compatibility() is None


def test_requirement_equality_ignores_the_raw_text() -> None:
    """Two spellings of one range compare equal for dedupe purposes."""
    assert requirement_from_pep440(">=0.19.3,<0.19.5") == requirement_from_text("dcc_mcp_core-0.19.3..0.19.5")
    assert hash(requirement_from_pep440(">=0.19.3,<0.19.5")) == hash(
        requirement_from_text("dcc_mcp_core-0.19.3..0.19.5")
    )
    assert Requirement(package="dcc_mcp_core") != "dcc_mcp_core"


def test_runtime_core_version_matches_the_distribution() -> None:
    """The running core version is resolvable, whichever side reports it."""
    version = runtime_core_version()
    assert version
    assert version.split(".")[0].isdigit()


def test_environment_opt_out_defaults_to_enforcing(monkeypatch: pytest.MonkeyPatch) -> None:
    """The default is to fail; only an explicit opt-out downgrades."""
    monkeypatch.delenv(ENV_ENFORCE, raising=False)
    assert os.environ.get(ENV_ENFORCE) is None
    with pytest.raises(CoreRequirementError):
        enforce_core_compatibility(">=0.19.3,<0.19.5", core_version="0.20.28")
