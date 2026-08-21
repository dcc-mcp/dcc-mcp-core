from pathlib import Path

from dcc_mcp_core import install_lifecycle
import dcc_mcp_core._install_lifecycle_process as process_lifecycle
import dcc_mcp_core._install_lifecycle_runtime as runtime_lifecycle
import dcc_mcp_core._install_lifecycle_sidecar as sidecar_lifecycle
from dcc_mcp_core._path_util import to_resolved_path


def test_to_resolved_path_rejects_empty_values() -> None:
    assert to_resolved_path(None) is None
    assert to_resolved_path("") is None


def test_to_resolved_path_expands_and_resolves(tmp_path: Path, monkeypatch) -> None:
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("USERPROFILE", str(tmp_path))

    assert to_resolved_path("~/assets/../scene.ma") == (tmp_path / "scene.ma").resolve()


def test_to_resolved_path_falls_back_when_resolve_fails(tmp_path: Path, monkeypatch) -> None:
    path = tmp_path / "transient" / "scene.ma"

    def fail_resolve(_path: Path, *args, **kwargs) -> Path:
        raise OSError("filesystem unavailable")

    monkeypatch.setattr(Path, "resolve", fail_resolve)

    assert to_resolved_path(path) == path.absolute()


def test_install_lifecycle_modules_share_path_coercion() -> None:
    assert install_lifecycle._to_path is to_resolved_path
    assert process_lifecycle._to_path is to_resolved_path
    assert runtime_lifecycle._to_path is to_resolved_path
    assert sidecar_lifecycle._to_path is to_resolved_path
