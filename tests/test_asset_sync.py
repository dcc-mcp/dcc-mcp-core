"""Contract tests for content-addressed cross-DCC asset synchronization."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
from threading import Barrier

import pytest

import dcc_mcp_core
from dcc_mcp_core.asset_sync import AssetSyncConflictError
from dcc_mcp_core.asset_sync import AssetSyncRevision
from dcc_mcp_core.asset_sync import AssetSyncValidationError
from dcc_mcp_core.asset_sync import FileAssetSyncStore


def test_publish_first_revision_creates_content_addressed_head(tmp_path: Path) -> None:
    source = tmp_path / "houdini_model.obj"
    source.write_bytes(b"houdini-geometry-v1")
    store = FileAssetSyncStore(tmp_path / "sync")

    revision = store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        source_instance_id="houdini-instance",
        expected_head_revision=0,
    )

    assert revision.revision == 1
    assert revision.base_revision is None
    assert revision.file_ref["uri"].startswith("artefact://sha256/")
    assert revision.file_ref["digest"] == revision.digest
    assert store.resolve(revision).read_bytes() == b"houdini-geometry-v1"
    assert "local_path" not in revision.to_dict()

    head_path = tmp_path / "sync" / "channels" / "houdini-comfyui" / "heads" / "test-model.json"
    assert json.loads(head_path.read_text(encoding="utf-8")) == revision.to_dict()
    assert dcc_mcp_core.AssetSyncRevision is not None
    assert dcc_mcp_core.FileAssetSyncStore is FileAssetSyncStore


def test_publish_rejects_stale_expected_head(tmp_path: Path) -> None:
    source = tmp_path / "model.obj"
    source.write_bytes(b"revision-one")
    store = FileAssetSyncStore(tmp_path / "sync")
    store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        expected_head_revision=0,
    )
    source.write_bytes(b"revision-two")

    with pytest.raises(AssetSyncConflictError, match="revision 1, expected 0"):
        store.publish(
            source,
            channel_id="houdini-comfyui",
            asset_id="test-model",
            format="obj",
            mime="model/obj",
            expected_head_revision=0,
        )


def test_materialize_revision_into_consumer_owned_root(tmp_path: Path) -> None:
    source = tmp_path / "model.obj"
    source.write_bytes(b"houdini-model")
    store = FileAssetSyncStore(tmp_path / "sync")
    revision = store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        expected_head_revision=0,
    )

    staged = store.materialize(revision, tmp_path / "comfyui-input", subfolder="3d")

    assert staged.parent == tmp_path / "comfyui-input" / "3d"
    assert staged.name.startswith("test-model_r1_")
    assert staged.read_bytes() == b"houdini-model"


def test_publish_update_preserves_history_and_identical_content_is_idempotent(tmp_path: Path) -> None:
    source = tmp_path / "model.obj"
    source.write_bytes(b"revision-one")
    store = FileAssetSyncStore(tmp_path / "sync")
    first = store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        expected_head_revision=0,
    )
    repeated = store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        expected_head_revision=1,
    )
    source.write_bytes(b"revision-two")
    second = store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        expected_head_revision=1,
    )

    assert repeated == first
    assert second.revision == 2
    assert second.base_revision == 1
    assert store.resolve(first).read_bytes() == b"revision-one"
    assert store.resolve(second).read_bytes() == b"revision-two"


def test_materialize_rejects_parent_traversal(tmp_path: Path) -> None:
    source = tmp_path / "model.obj"
    source.write_bytes(b"houdini-model")
    store = FileAssetSyncStore(tmp_path / "sync")
    revision = store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        expected_head_revision=0,
    )

    with pytest.raises(AssetSyncValidationError):
        store.materialize(revision, tmp_path / "comfyui-input", subfolder="../outside")


def test_concurrent_publish_allows_only_one_writer(tmp_path: Path, monkeypatch) -> None:
    import dcc_mcp_core.asset_sync as asset_sync

    source = tmp_path / "model.obj"
    source.write_bytes(b"concurrent-model")
    store = FileAssetSyncStore(tmp_path / "sync")
    barrier = Barrier(2)
    original_sha256_file = asset_sync._sha256_file

    def synchronized_sha256(path: Path) -> str:
        digest = original_sha256_file(path)
        barrier.wait(timeout=5)
        return digest

    monkeypatch.setattr(asset_sync, "_sha256_file", synchronized_sha256)

    def publish() -> AssetSyncRevision:
        return store.publish(
            source,
            channel_id="houdini-comfyui",
            asset_id="test-model",
            format="obj",
            mime="model/obj",
            expected_head_revision=0,
        )

    with ThreadPoolExecutor(max_workers=2) as executor:
        futures = [executor.submit(publish) for _ in range(2)]
    successes = [future.result() for future in futures if future.exception() is None]
    failures = [future.exception() for future in futures if future.exception() is not None]

    assert len(successes) == 1
    assert successes[0].revision == 1
    assert len(failures) == 1
    assert isinstance(failures[0], AssetSyncConflictError)


def test_resolve_rejects_tampered_content_addressed_object(tmp_path: Path) -> None:
    source = tmp_path / "model.obj"
    source.write_bytes(b"trusted-model")
    store = FileAssetSyncStore(tmp_path / "sync")
    revision = store.publish(
        source,
        channel_id="houdini-comfyui",
        asset_id="test-model",
        format="obj",
        mime="model/obj",
        expected_head_revision=0,
    )
    store.resolve(revision).write_bytes(b"tampered")

    with pytest.raises(AssetSyncValidationError, match="digest"):
        store.resolve(revision)
