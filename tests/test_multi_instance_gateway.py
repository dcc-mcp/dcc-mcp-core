"""Tests for multi-instance gateway scenarios.

Covers:
- Multiple DCC instances registering with the same registry
- ServiceEntry with new fields: pid, display_name, documents
- update_documents: active document, document list, display_name updates
- Version-aware instance selection (rank_services, find_best_service)
- Session isolation: different sessions pinned to different instances
- Heartbeat and stale detection across multiple instances
- Instance disambiguation by document hint
- Gateway election version comparison logic

These tests validate the ideal behavior of multi-instance DCC workflows
where an AI agent must select, track, and communicate with specific instances.
"""

from __future__ import annotations

from pathlib import Path
import time
import uuid

import pytest

import dcc_mcp_core

# ── Fixtures ──────────────────────────────────────────────────────────────────


@pytest.fixture()
def registry(tmp_path: Path) -> dcc_mcp_core.TransportManager:
    """Fresh TransportManager backed by a temp directory."""
    import contextlib

    mgr = dcc_mcp_core.TransportManager(str(tmp_path))
    yield mgr
    with contextlib.suppress(Exception):
        mgr.shutdown()


# ── ServiceEntry with new fields ──────────────────────────────────────────────


class TestServiceEntryNewFields:
    """pid, display_name, and documents round-trip through the registry."""

    def test_register_with_pid(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Pid is stored and returned via get_service."""
        iid = registry.register_service("maya", "127.0.0.1", 18811, pid=12345)
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.pid == 12345

    def test_register_with_display_name(self, registry: dcc_mcp_core.TransportManager) -> None:
        """display_name is stored and returned via get_service."""
        iid = registry.register_service("maya", "127.0.0.1", 18812, display_name="Maya-Production")
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.display_name == "Maya-Production"

    def test_register_with_documents(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Documents list is stored and returned via get_service."""
        docs = ["scene.ma", "rig.ma", "anim.ma"]
        iid = registry.register_service("maya", "127.0.0.1", 18813, documents=docs)
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.documents == docs

    def test_register_all_new_fields(self, registry: dcc_mcp_core.TransportManager) -> None:
        """All new fields (pid, display_name, documents, scene) together."""
        docs = ["project.ma", "rig.ma"]
        iid = registry.register_service(
            "maya",
            "127.0.0.1",
            18814,
            pid=42000,
            display_name="Maya-Rigging",
            documents=docs,
            scene="project.ma",
            version="2025",
        )
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.pid == 42000
        assert entry.display_name == "Maya-Rigging"
        assert entry.documents == docs
        assert entry.scene == "project.ma"
        assert entry.version == "2025"

    def test_register_without_new_fields_has_defaults(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Old-style registration without new fields still works; fields default to None/[]."""
        import os

        iid = registry.register_service("maya", "127.0.0.1", 18815)
        entry = registry.get_service("maya", iid)
        assert entry is not None
        # `pid` is auto-populated with the current process id so the registry
        # can reap ghost rows via `prune_dead_pids()` (issue #227).
        assert entry.pid == os.getpid()
        assert entry.display_name is None
        assert entry.documents == []
        assert entry.extras == {}

    def test_register_with_extras_scalar_values(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Extras round-trip scalar JSON values (int / str / bool / None / float)."""
        extras = {
            "cdp_port": 9222,
            "url": "http://localhost:3000",
            "debug": True,
            "token": None,
            "ratio": 1.5,
        }
        iid = registry.register_service("webview", "127.0.0.1", 3000, extras=extras)
        entry = registry.get_service("webview", iid)
        assert entry is not None
        assert entry.extras == extras

    def test_register_with_extras_nested_values(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Extras round-trip nested dicts and lists (lossless JSON storage)."""
        extras = {
            "capabilities": {"scene": False, "timeline": True, "selection": False},
            "tags": ["preview", "cdp", "webview"],
            "host_dcc": {"name": "maya", "pid": 42000, "version": "2025"},
        }
        iid = registry.register_service("webview-maya", "127.0.0.1", 3001, extras=extras)
        entry = registry.get_service("webview-maya", iid)
        assert entry is not None
        assert entry.extras == extras
        assert entry.extras["capabilities"]["timeline"] is True
        assert entry.extras["host_dcc"]["pid"] == 42000

    def test_extras_returns_fresh_dict_each_call(self, registry: dcc_mcp_core.TransportManager) -> None:
        """The ``extras`` getter materialises a new dict each access so mutations stay local."""
        iid = registry.register_service("blender", "127.0.0.1", 8080, extras={"key": "value"})
        entry = registry.get_service("blender", iid)
        assert entry is not None
        first = entry.extras
        first["key"] = "mutated"
        second = entry.extras
        assert second == {"key": "value"}

    def test_extras_persists_across_to_dict(self, registry: dcc_mcp_core.TransportManager) -> None:
        """``to_dict()`` includes the extras field for downstream serialisation."""
        iid = registry.register_service("houdini", "127.0.0.1", 9090, extras={"a": 1, "b": [2, 3]})
        entry = registry.get_service("houdini", iid)
        assert entry is not None
        as_dict = entry.to_dict()
        assert as_dict["extras"] == {"a": 1, "b": [2, 3]}

    def test_extras_empty_default(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Not providing extras yields an empty dict, not ``None``."""
        iid = registry.register_service("nuke", "127.0.0.1", 7070)
        entry = registry.get_service("nuke", iid)
        assert entry is not None
        assert entry.extras == {}


# ── update_documents ──────────────────────────────────────────────────────────


class TestUpdateDocuments:
    """update_documents updates scene, documents, and display_name atomically."""

    def test_update_active_document_sets_scene(self, registry: dcc_mcp_core.TransportManager) -> None:
        """active_document argument updates the scene field."""
        iid = registry.register_service("photoshop", "127.0.0.1", 18820, scene="old.psd")
        ok = registry.update_documents("photoshop", iid, active_document="new.psd")
        assert ok is True
        entry = registry.get_service("photoshop", iid)
        assert entry is not None
        assert entry.scene == "new.psd"

    def test_update_documents_replaces_list(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Documents kwarg replaces the existing list entirely."""
        iid = registry.register_service("photoshop", "127.0.0.1", 18821, documents=["old1.psd", "old2.psd"])
        new_docs = ["banner.psd", "icon.psd", "logo.psd"]
        registry.update_documents("photoshop", iid, documents=new_docs)
        entry = registry.get_service("photoshop", iid)
        assert entry is not None
        assert entry.documents == new_docs

    def test_update_display_name(self, registry: dcc_mcp_core.TransportManager) -> None:
        """display_name kwarg updates the human-readable label."""
        iid = registry.register_service("photoshop", "127.0.0.1", 18822, display_name="PS-Old")
        registry.update_documents("photoshop", iid, display_name="PS-Marketing")
        entry = registry.get_service("photoshop", iid)
        assert entry is not None
        assert entry.display_name == "PS-Marketing"

    def test_update_documents_all_fields_at_once(self, registry: dcc_mcp_core.TransportManager) -> None:
        """All three fields updated in a single call."""
        iid = registry.register_service("photoshop", "127.0.0.1", 18823)
        registry.update_documents(
            "photoshop",
            iid,
            active_document="hero.psd",
            documents=["hero.psd", "thumb.psd"],
            display_name="PS-Marketing",
        )
        entry = registry.get_service("photoshop", iid)
        assert entry is not None
        assert entry.scene == "hero.psd"
        assert entry.documents == ["hero.psd", "thumb.psd"]
        assert entry.display_name == "PS-Marketing"

    def test_update_documents_returns_false_for_unknown_instance(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Returns False when the instance_id does not exist."""
        fake_id = str(uuid.uuid4())
        result = registry.update_documents("maya", fake_id, active_document="noop.ma")
        assert result is False

    def test_clear_active_document_with_empty_string(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Passing empty string for active_document clears the scene field."""
        iid = registry.register_service("maya", "127.0.0.1", 18824, scene="existing.ma")
        registry.update_documents("maya", iid, active_document="")
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.scene is None

    def test_none_active_document_leaves_scene_unchanged(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Passing None for active_document leaves scene unchanged."""
        iid = registry.register_service("maya", "127.0.0.1", 18825, scene="keep.ma")
        registry.update_documents("maya", iid, active_document=None)
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.scene == "keep.ma"


# ── Multi-instance registration ───────────────────────────────────────────────


class TestMultiInstanceRegistration:
    """Multiple DCC instances of the same type coexist in the registry."""

    def test_two_maya_instances_registered(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Two Maya instances can be registered simultaneously."""
        iid1 = registry.register_service("maya", "127.0.0.1", 18830, display_name="Maya-Anim")
        iid2 = registry.register_service("maya", "127.0.0.1", 18831, display_name="Maya-Rig")
        assert iid1 != iid2

        instances = registry.list_instances("maya")
        assert len(instances) >= 2

        names = [e.display_name for e in instances]
        assert "Maya-Anim" in names
        assert "Maya-Rig" in names

    def test_different_dcc_types_coexist(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Maya, Blender, and Houdini instances can all be registered."""
        registry.register_service("maya", "127.0.0.1", 18840, display_name="Maya-1")
        registry.register_service("blender", "127.0.0.1", 18841, display_name="Blender-1")
        registry.register_service("houdini", "127.0.0.1", 18842, display_name="Houdini-1")

        assert len(registry.list_instances("maya")) >= 1
        assert len(registry.list_instances("blender")) >= 1
        assert len(registry.list_instances("houdini")) >= 1

    def test_list_all_services_returns_all_types(self, registry: dcc_mcp_core.TransportManager) -> None:
        """list_all_services() returns entries across all DCC types."""
        registry.register_service("maya", "127.0.0.1", 18850)
        registry.register_service("blender", "127.0.0.1", 18851)
        all_svcs = registry.list_all_services()
        dcc_types = {e.dcc_type for e in all_svcs}
        assert "maya" in dcc_types
        assert "blender" in dcc_types

    def test_deregister_removes_specific_instance(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Deregistering one instance leaves the other intact."""
        iid1 = registry.register_service("maya", "127.0.0.1", 18860)
        iid2 = registry.register_service("maya", "127.0.0.1", 18861)

        registry.deregister_service("maya", iid1)

        assert registry.get_service("maya", iid1) is None
        assert registry.get_service("maya", iid2) is not None

    def test_rank_services_returns_sorted_list(self, registry: dcc_mcp_core.TransportManager) -> None:
        """rank_services() returns instances sorted by connection preference."""
        registry.register_service("maya", "127.0.0.1", 18870)
        registry.register_service("maya", "127.0.0.1", 18871)

        ranked = registry.rank_services("maya")
        assert isinstance(ranked, list)
        assert len(ranked) >= 2
        # All returned entries are maya type
        assert all(e.dcc_type == "maya" for e in ranked)

    def test_find_best_service_returns_single_entry(self, registry: dcc_mcp_core.TransportManager) -> None:
        """find_best_service() returns the top-ranked ServiceEntry."""
        registry.register_service("maya", "127.0.0.1", 18880)
        best = registry.find_best_service("maya")
        assert isinstance(best, dcc_mcp_core.ServiceEntry)
        assert best.dcc_type == "maya"

    def test_find_best_service_prefers_available(self, registry: dcc_mcp_core.TransportManager) -> None:
        """AVAILABLE status is preferred over BUSY in find_best_service."""
        iid_busy = registry.register_service("maya", "127.0.0.1", 18882)
        iid_avail = registry.register_service("maya", "127.0.0.1", 18883)

        # Mark first instance as busy
        registry.update_service_status("maya", iid_busy, dcc_mcp_core.ServiceStatus.BUSY)

        best = registry.find_best_service("maya")
        # Should prefer the AVAILABLE instance
        assert best.instance_id == iid_avail


# ── Session isolation ─────────────────────────────────────────────────────────


class TestSessionIsolation:
    """Sessions are pinned to specific DCC instances to prevent context bleeding."""

    def test_session_created_for_instance(self, registry: dcc_mcp_core.TransportManager) -> None:
        """get_or_create_session returns a session UUID for a specific instance."""
        iid = registry.register_service("maya", "127.0.0.1", 18890)
        session_id = registry.get_or_create_session("maya", iid)
        assert isinstance(session_id, str)
        assert len(session_id) > 0

    def test_two_sessions_for_same_instance_return_same_id(self, registry: dcc_mcp_core.TransportManager) -> None:
        """get_or_create_session is idempotent for the same instance."""
        iid = registry.register_service("maya", "127.0.0.1", 18891)
        sid1 = registry.get_or_create_session("maya", iid)
        sid2 = registry.get_or_create_session("maya", iid)
        assert sid1 == sid2

    def test_different_instances_get_different_sessions(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Two Maya instances produce distinct sessions."""
        iid1 = registry.register_service("maya", "127.0.0.1", 18892)
        iid2 = registry.register_service("maya", "127.0.0.1", 18893)
        sid1 = registry.get_or_create_session("maya", iid1)
        sid2 = registry.get_or_create_session("maya", iid2)
        assert sid1 != sid2

    def test_session_count_tracks_open_sessions(self, registry: dcc_mcp_core.TransportManager) -> None:
        """session_count() increases as sessions are created."""
        before = registry.session_count()
        iid = registry.register_service("blender", "127.0.0.1", 18894)
        registry.get_or_create_session("blender", iid)
        after = registry.session_count()
        assert after >= before + 1


# ── Heartbeat and instance health ─────────────────────────────────────────────


class TestInstanceHealth:
    """Heartbeat keeps instances alive; stale instances are detected."""

    def test_heartbeat_returns_true_for_live_instance(self, registry: dcc_mcp_core.TransportManager) -> None:
        """heartbeat() updates last_heartbeat_ms for an active instance."""
        iid = registry.register_service("maya", "127.0.0.1", 18900)
        result = registry.heartbeat("maya", iid)
        assert result is True

    def test_heartbeat_returns_false_for_unknown_instance(self, registry: dcc_mcp_core.TransportManager) -> None:
        """heartbeat() returns False when the instance is not found."""
        fake_id = str(uuid.uuid4())
        result = registry.heartbeat("maya", fake_id)
        assert result is False

    def test_heartbeat_updates_last_heartbeat_ms(self, registry: dcc_mcp_core.TransportManager) -> None:
        """last_heartbeat_ms advances after a heartbeat call."""
        iid = registry.register_service("maya", "127.0.0.1", 18901)
        entry_before = registry.get_service("maya", iid)
        time.sleep(0.02)
        registry.heartbeat("maya", iid)
        entry_after = registry.get_service("maya", iid)
        assert entry_after is not None
        assert entry_after.last_heartbeat_ms >= entry_before.last_heartbeat_ms

    def test_multi_instance_heartbeat_selective(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Heartbeat for one instance does not affect another's timestamp."""
        iid1 = registry.register_service("maya", "127.0.0.1", 18902)
        iid2 = registry.register_service("maya", "127.0.0.1", 18903)
        before2 = registry.get_service("maya", iid2).last_heartbeat_ms
        time.sleep(0.02)
        registry.heartbeat("maya", iid1)
        after2 = registry.get_service("maya", iid2).last_heartbeat_ms
        # Instance 2's timestamp should NOT have changed
        assert after2 == before2


# ── Version-aware instance registration ───────────────────────────────────────


class TestVersionAwareRegistration:
    """Instance version is tracked and accessible for gateway election logic."""

    def test_instance_version_stored(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Version parameter is stored and retrievable."""
        iid = registry.register_service("maya", "127.0.0.1", 18910, version="2025")
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.version == "2025"

    def test_multiple_versions_coexist(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Maya 2024 and Maya 2025 can both be registered simultaneously."""
        iid24 = registry.register_service("maya", "127.0.0.1", 18911, version="2024")
        iid25 = registry.register_service("maya", "127.0.0.1", 18912, version="2025")

        entry24 = registry.get_service("maya", iid24)
        entry25 = registry.get_service("maya", iid25)

        assert entry24.version == "2024"
        assert entry25.version == "2025"

    def test_rank_services_includes_all_versions(self, registry: dcc_mcp_core.TransportManager) -> None:
        """rank_services includes all registered versions of a DCC."""
        registry.register_service("maya", "127.0.0.1", 18913, version="2024")
        registry.register_service("maya", "127.0.0.1", 18914, version="2025")

        ranked = registry.rank_services("maya")
        versions = {e.version for e in ranked}
        assert "2024" in versions
        assert "2025" in versions

    def test_update_scene_after_registration(self, registry: dcc_mcp_core.TransportManager) -> None:
        """update_scene() updates the active scene for a live instance."""
        iid = registry.register_service("maya", "127.0.0.1", 18915)
        ok = registry.update_scene("maya", iid, scene="production.ma")
        assert ok is True
        entry = registry.get_service("maya", iid)
        assert entry is not None
        assert entry.scene == "production.ma"

    def test_scene_change_simulates_file_open(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Simulate a user opening a new file by calling update_documents."""
        iid = registry.register_service(
            "maya",
            "127.0.0.1",
            18916,
            scene="old_scene.ma",
            documents=["old_scene.ma"],
        )
        # User opens a new scene
        registry.update_documents(
            "maya",
            iid,
            active_document="new_project.ma",
            documents=["new_project.ma", "references.ma"],
        )
        entry = registry.get_service("maya", iid)
        assert entry.scene == "new_project.ma"
        assert "new_project.ma" in entry.documents
        assert "references.ma" in entry.documents
        # Old document should no longer be in the list
        assert "old_scene.ma" not in entry.documents


# ── E2E multi-instance scenario ───────────────────────────────────────────────


class TestMultiInstanceE2EScenario:
    """End-to-end scenario: three DCC instances compete for work."""

    def test_photoshop_multi_document_workflow(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Simulate Photoshop with multiple open documents.

        Scenario:
        1. PS starts, opens 3 documents
        2. AI agent connects, gets list of open documents
        3. User switches active document → AI must see updated scene
        4. User opens a new document → documents list updated
        """
        # Step 1: Photoshop registers with initial documents
        iid = registry.register_service(
            "photoshop",
            "127.0.0.1",
            18920,
            pid=55001,
            display_name="PS-Marketing",
            scene="logo.psd",
            documents=["logo.psd", "banner.psd"],
            version="2025",
        )
        entry = registry.get_service("photoshop", iid)
        assert entry.pid == 55001
        assert entry.display_name == "PS-Marketing"
        assert entry.scene == "logo.psd"
        assert len(entry.documents) == 2

        # Step 2: User switches active document — must pass documents to keep them
        registry.update_documents(
            "photoshop",
            iid,
            active_document="banner.psd",
            documents=["logo.psd", "banner.psd"],  # documents must be passed explicitly
        )
        entry = registry.get_service("photoshop", iid)
        assert entry.scene == "banner.psd"
        assert entry.documents == ["logo.psd", "banner.psd"]  # list preserved

        # Step 3: User opens a third document
        registry.update_documents(
            "photoshop",
            iid,
            active_document="icon.psd",
            documents=["logo.psd", "banner.psd", "icon.psd"],
        )
        entry = registry.get_service("photoshop", iid)
        assert entry.scene == "icon.psd"
        assert len(entry.documents) == 3
        assert "icon.psd" in entry.documents

    def test_maya_multi_instance_session_routing(self, registry: dcc_mcp_core.TransportManager) -> None:
        """Simulate AI agents routing to correct Maya instances.

        Scenario:
        - Maya #1: rigging work on rig.ma
        - Maya #2: animation work on anim_shot001.ma
        - AI Agent A: needs Maya with rig.ma → routes to instance 1
        - AI Agent B: needs Maya for animation → routes to instance 2
        """
        # Register two Maya instances
        iid_rig = registry.register_service(
            "maya",
            "127.0.0.1",
            18930,
            display_name="Maya-Rigging",
            scene="rig.ma",
            documents=["rig.ma"],
        )
        iid_anim = registry.register_service(
            "maya",
            "127.0.0.1",
            18931,
            display_name="Maya-Animation",
            scene="anim_shot001.ma",
            documents=["anim_shot001.ma", "anim_shot002.ma"],
        )

        # Both instances are available
        assert registry.get_service("maya", iid_rig) is not None
        assert registry.get_service("maya", iid_anim) is not None

        # Create isolated sessions for each agent
        session_a = registry.get_or_create_session("maya", iid_rig)
        session_b = registry.get_or_create_session("maya", iid_anim)

        # Sessions are different (isolated)
        assert session_a != session_b

        # AI can find the specific instance by document
        rig_entry = registry.get_service("maya", iid_rig)
        anim_entry = registry.get_service("maya", iid_anim)

        assert "rig.ma" in rig_entry.documents
        assert "anim_shot001.ma" in anim_entry.documents
        assert "rig.ma" not in anim_entry.documents
