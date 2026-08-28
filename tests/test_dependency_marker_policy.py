"""Default-dependency proofs use tokens, never text inside quoted values."""

from __future__ import annotations

import pytest
from scripts.ci.check_python_wheel import _is_default_requirement
from scripts.ci.dependency_marker_policy import is_default_requirement
from scripts.ci.smoke_zero_typing_extensions import _is_default_requirement as smoke_policy


@pytest.mark.parametrize(
    "marker",
    [
        "",
        "python_version < '3.8'",
        "extra == ''",
        "extra != 'test'",
        "os_name != \"extra == 'test'\"",
        "'extra' == 'test'",
        "extra == 'test' or python_version < '3.8'",
        "extra == 'test' or extra == ''",
        "extra == 'test' garbage",
        "extra == 'test' and",
        "(extra == 'test'",
        "extra == 'test')",
        "unknown == 'value' and extra == 'test'",
        "extra === 'test'",
        "extra in 'test'",
        "extra not in 'test'",
        "extra == 'test' and os_name == 'bad\\escape'",
        "(" * 33 + "extra == 'test'" + ")" * 33,
        "extra == '" + "x" * 8192 + "'",
    ],
)
def test_unproven_or_malformed_markers_fail_closed(marker: str) -> None:
    assert is_default_requirement("typing-extensions; " + marker)


@pytest.mark.parametrize(
    "marker",
    [
        "extra == 'test'",
        "'test' == extra",
        'extra == "dev"',
        "python_version < '3.8' and extra == 'test'",
        "extra == 'test' or extra == 'dev'",
        "(extra == 'test' or extra == 'dev') and os_name != 'or'",
        "extra == 'test' and (python_version < '3.8' or os_name == 'nt')",
    ],
)
def test_complete_extra_constraints_prove_default_inactive(marker: str) -> None:
    assert not is_default_requirement("typing-extensions; " + marker)


def test_archive_consumers_share_the_exact_policy() -> None:
    assert _is_default_requirement is is_default_requirement
    assert smoke_policy is is_default_requirement
