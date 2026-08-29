"""Regression tests for fail-closed distribution archive payload policy."""

from __future__ import annotations

import stat
import tarfile
import zipfile

import pytest
from scripts.ci.archive_payload_policy import archive_member_errors


@pytest.mark.parametrize("name", ["../escape.py", "/absolute.py", "C:\\escape.py", "pkg//alias.py"])
def test_archive_policy_rejects_unsafe_paths(name: str) -> None:
    member = zipfile.ZipInfo(name)

    errors = archive_member_errors([member])

    assert any("unsafe archive member" in error for error in errors)


def test_archive_policy_rejects_symlink_members() -> None:
    member = zipfile.ZipInfo("pkg/link.py")
    member.create_system = 3
    member.external_attr = (stat.S_IFLNK | 0o777) << 16

    errors = archive_member_errors([member])

    assert any("symlink" in error for error in errors)


def test_archive_policy_rejects_normalized_aliases_and_duplicates() -> None:
    first = zipfile.ZipInfo("pkg/readme.py")
    alias = zipfile.ZipInfo("pkg/./README.py")

    errors = archive_member_errors([first, alias])

    assert any("duplicate portable member paths" in error for error in errors)


def test_archive_policy_rejects_typing_extensions_payload() -> None:
    member = tarfile.TarInfo("pkg/typing_extensions.py")

    errors = archive_member_errors([member])

    assert any("typing_extensions payload" in error for error in errors)
