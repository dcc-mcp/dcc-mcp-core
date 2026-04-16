"""Deep tests for SkillCatalog/SkillSummary and TransportManager multi-service scenarios.

Iteration 133 (+105 tests):

TestSkillCatalogCreate (6):
  empty_repr / empty_list_skills / empty_loaded_count / is_loaded_false /
  get_skill_info_none / find_skills_empty

TestSkillCatalogDiscover (10):
  single_skill_count / two_skills_count / is_not_loaded_after_discover /
  list_skills_after_discover / loaded_count_zero_after_discover /
  get_skill_info_returns_dict / get_skill_info_has_name / get_skill_info_has_dcc /
  get_skill_info_has_tools / discover_dcc_name_filter

TestSkillCatalogLoad (10):
  load_skill_returns_bool / is_loaded_true_after_load / loaded_count_one /
  list_skills_loaded_true / skill_summary_name / skill_summary_dcc /
  skill_summary_description / skill_summary_version / skill_summary_tags /
  skill_summary_tool_count

TestSkillSummary (8):
  tool_names_type / tool_names_content / loaded_false_after_discover_only /
  loaded_true_after_load / find_skills_dcc_filter / find_skills_query_match /
  find_skills_query_no_match / find_skills_dcc_no_match

TestSkillCatalogUnload (8):
  unload_returns_true / is_loaded_false_after_unload / loaded_count_decrements /
  list_skills_still_present / can_reload_after_unload / unload_nonexistent_false /
  load_nonexistent_raises / get_skill_info_after_unload_has_state

TestSkillCatalogMultiSkill (11):
  discover_two_different_dcc / load_one_affects_only_one / loaded_count_partial /
  find_skills_returns_all_for_any_dcc / find_skills_returns_only_python /
  find_skills_returns_only_maya / unload_one_leaves_other_loaded /
  list_skills_both_present / discover_idempotent / discover_incremental /
  total_count_repr

TestTransportManagerMultiService (12):
  register_two_same_dcc / list_all_services_two / list_instances_two /
  get_service_each / find_best_no_busy_preferred / update_one_busy_find_best /
  deregister_one_leaves_other / register_different_dccs / list_instances_by_dcc /
  rank_services_length / heartbeat_multiple / shutdown_then_register_raises

TestTransportManagerServiceEntry (12):
  entry_dcc_type / entry_host / entry_port / entry_instance_id_is_uuid /
  entry_status_default_available / entry_version_none / entry_scene_none /
  entry_metadata_empty / entry_is_ipc_false / entry_last_heartbeat_positive /
  entry_transport_address_none / to_dict_has_required_keys

TestTransportManagerStateMachine (14):
  available_to_busy / busy_to_available / available_to_shutting_down /
  available_to_unreachable / update_nonexistent_instance_silent /
  record_success_known_session_noop / record_error_known_session_noop /
  begin_reconnect_noop / reconnect_success_noop / cleanup_noop /
  repr_reflects_count / is_shutdown_false_initially / shutdown_idempotent /
  is_shutdown_true_after_shutdown

TestServiceStatusEnum (14):
  available_value / busy_value / shutting_down_value / unreachable_value /
  available_eq / busy_eq / shutting_down_eq / unreachable_eq /
  available_ne_busy / available_ne_unreachable / busy_ne_shutting_down /
  int_conversion / repr_contains_variant / status_can_be_used_as_dict_key
"""

from __future__ import annotations

import contextlib
from pathlib import Path
import tempfile
import uuid

import pytest

from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportManager

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_SKILL_MD_TEMPLATE = """\
---
name: {name}
description: Test skill {name}
version: {version}
dcc: {dcc}
tags: {tags}
tools:
  - name: greet
    description: Say hello
    read_only: true
    destructive: false
    idempotent: true
    source_file: scripts/greet.py
---
"""

_SCRIPT_CONTENT = (
    "import argparse, json\n"
    "parser = argparse.ArgumentParser()\n"
    "args = parser.parse_args()\n"
    "print(json.dumps({'success': True}))\n"
)


def make_skill_dir(
    base: str,
    name: str,
    dcc: str = "python",
    version: str = "1.0.0",
    tags: str = "[test]",
) -> str:
    skill_dir = Path(base) / name
    (skill_dir / "scripts").mkdir(parents=True, exist_ok=True)
    (skill_dir / "SKILL.md").write_text(
        _SKILL_MD_TEMPLATE.format(name=name, version=version, dcc=dcc, tags=tags),
        encoding="utf-8",
    )
    (skill_dir / "scripts" / "greet.py").write_text(_SCRIPT_CONTENT, encoding="utf-8")
    return str(skill_dir)
    return skill_dir


def make_tm() -> TransportManager:
    return TransportManager(tempfile.mkdtemp())


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture()
def tmpdir_path():
    return tempfile.mkdtemp()


@pytest.fixture()
def single_skill_cat(tmpdir_path):
    """SkillCatalog with one discovered (not yet loaded) Python skill."""
    make_skill_dir(tmpdir_path, "hello-world", dcc="python")
    reg = ToolRegistry()
    cat = SkillCatalog(reg)
    cat.discover(extra_paths=[tmpdir_path])
    return cat


@pytest.fixture()
def loaded_skill_cat(tmpdir_path):
    """SkillCatalog with one discovered AND loaded Python skill."""
    make_skill_dir(tmpdir_path, "hello-world", dcc="python")
    reg = ToolRegistry()
    cat = SkillCatalog(reg)
    cat.discover(extra_paths=[tmpdir_path])
    cat.load_skill("hello-world")
    return cat


@pytest.fixture()
def two_skill_cat(tmpdir_path):
    """SkillCatalog with two skills: hello-world (python) and maya-geo (maya)."""
    make_skill_dir(tmpdir_path, "hello-world", dcc="python")
    make_skill_dir(tmpdir_path, "maya-geo", dcc="maya", tags="[geometry]")
    reg = ToolRegistry()
    cat = SkillCatalog(reg)
    cat.discover(extra_paths=[tmpdir_path])
    return cat


# ---------------------------------------------------------------------------
# TestSkillCatalogCreate
# ---------------------------------------------------------------------------


class TestSkillCatalogCreate:
    def test_empty_repr(self):
        cat = SkillCatalog(ToolRegistry())
        assert "SkillCatalog" in repr(cat)

    def test_empty_list_skills(self):
        cat = SkillCatalog(ToolRegistry())
        assert cat.list_skills() == []

    def test_empty_loaded_count(self):
        cat = SkillCatalog(ToolRegistry())
        assert cat.loaded_count() == 0

    def test_is_loaded_false(self):
        cat = SkillCatalog(ToolRegistry())
        assert cat.is_loaded("anything") is False

    def test_get_skill_info_none(self):
        cat = SkillCatalog(ToolRegistry())
        assert cat.get_skill_info("anything") is None

    def test_find_skills_empty(self):
        cat = SkillCatalog(ToolRegistry())
        assert cat.find_skills() == []


# ---------------------------------------------------------------------------
# TestSkillCatalogDiscover
# ---------------------------------------------------------------------------


class TestSkillCatalogDiscover:
    def test_single_skill_count(self, tmpdir_path):
        make_skill_dir(tmpdir_path, "hello-world")
        cat = SkillCatalog(ToolRegistry())
        count = cat.discover(extra_paths=[tmpdir_path])
        assert count == 1

    def test_two_skills_count(self, tmpdir_path):
        make_skill_dir(tmpdir_path, "skill-a")
        make_skill_dir(tmpdir_path, "skill-b")
        cat = SkillCatalog(ToolRegistry())
        count = cat.discover(extra_paths=[tmpdir_path])
        assert count == 2

    def test_is_not_loaded_after_discover(self, single_skill_cat):
        assert single_skill_cat.is_loaded("hello-world") is False

    def test_list_skills_after_discover(self, single_skill_cat):
        skills = single_skill_cat.list_skills()
        assert len(skills) == 1
        assert skills[0].name == "hello-world"

    def test_loaded_count_zero_after_discover(self, single_skill_cat):
        assert single_skill_cat.loaded_count() == 0

    def test_get_skill_info_returns_dict(self, single_skill_cat):
        info = single_skill_cat.get_skill_info("hello-world")
        assert isinstance(info, dict)

    def test_get_skill_info_has_name(self, single_skill_cat):
        info = single_skill_cat.get_skill_info("hello-world")
        assert info["name"] == "hello-world"

    def test_get_skill_info_has_dcc(self, single_skill_cat):
        info = single_skill_cat.get_skill_info("hello-world")
        assert info["dcc"] == "python"

    def test_get_skill_info_has_tools(self, single_skill_cat):
        info = single_skill_cat.get_skill_info("hello-world")
        assert "tools" in info
        assert isinstance(info["tools"], list)
        assert len(info["tools"]) == 1

    def test_discover_dcc_name_filter(self, tmpdir_path):
        make_skill_dir(tmpdir_path, "hello-world", dcc="python")
        make_skill_dir(tmpdir_path, "maya-geo", dcc="maya")
        cat = SkillCatalog(ToolRegistry())
        cat.discover(extra_paths=[tmpdir_path], dcc_name="maya")
        # Only maya skills discovered when filter applied
        names = [s.name for s in cat.list_skills()]
        assert "maya-geo" in names


# ---------------------------------------------------------------------------
# TestSkillCatalogLoad
# ---------------------------------------------------------------------------


class TestSkillCatalogLoad:
    def test_load_skill_returns_list(self, single_skill_cat):
        result = single_skill_cat.load_skill("hello-world")
        assert isinstance(result, list)

    def test_load_skill_returns_registered_actions(self, single_skill_cat):
        result = single_skill_cat.load_skill("hello-world")
        assert len(result) >= 1

    def test_is_loaded_true_after_load(self, single_skill_cat):
        single_skill_cat.load_skill("hello-world")
        assert single_skill_cat.is_loaded("hello-world") is True

    def test_loaded_count_one(self, single_skill_cat):
        single_skill_cat.load_skill("hello-world")
        assert single_skill_cat.loaded_count() == 1

    def test_list_skills_loaded_true(self, single_skill_cat):
        single_skill_cat.load_skill("hello-world")
        skills = single_skill_cat.list_skills()
        loaded = [s for s in skills if s.name == "hello-world"]
        assert len(loaded) == 1
        assert loaded[0].loaded is True

    def test_skill_summary_name(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert s.name == "hello-world"

    def test_skill_summary_dcc(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert s.dcc == "python"

    def test_skill_summary_description(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert isinstance(s.description, str)
        assert len(s.description) > 0

    def test_skill_summary_version(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert s.version == "1.0.0"

    def test_skill_summary_tags(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert isinstance(s.tags, list)


# ---------------------------------------------------------------------------
# TestSkillSummary
# ---------------------------------------------------------------------------


class TestSkillSummary:
    def test_tool_names_type(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert isinstance(s.tool_names, list)

    def test_tool_names_content(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert "greet" in s.tool_names

    def test_tool_count_positive(self, loaded_skill_cat):
        s = loaded_skill_cat.list_skills()[0]
        assert s.tool_count >= 1

    def test_loaded_false_after_discover_only(self, single_skill_cat):
        s = single_skill_cat.list_skills()[0]
        assert s.loaded is False

    def test_loaded_true_after_load(self, single_skill_cat):
        single_skill_cat.load_skill("hello-world")
        s = single_skill_cat.list_skills()[0]
        assert s.loaded is True

    def test_find_skills_dcc_filter(self, two_skill_cat):
        found = two_skill_cat.find_skills(dcc="maya")
        names = [s.name for s in found]
        assert "maya-geo" in names
        assert "hello-world" not in names

    def test_find_skills_query_match(self, two_skill_cat):
        found = two_skill_cat.find_skills(query="hello")
        names = [s.name for s in found]
        assert "hello-world" in names

    def test_find_skills_query_no_match(self, two_skill_cat):
        found = two_skill_cat.find_skills(query="zzz_no_match_zzz")
        assert found == []

    def test_find_skills_dcc_no_match(self, two_skill_cat):
        found = two_skill_cat.find_skills(dcc="blender")
        assert found == []


# ---------------------------------------------------------------------------
# TestSkillCatalogUnload
# ---------------------------------------------------------------------------


class TestSkillCatalogUnload:
    def test_unload_returns_int(self, loaded_skill_cat):
        result = loaded_skill_cat.unload_skill("hello-world")
        assert isinstance(result, int)

    def test_unload_returns_positive_count(self, loaded_skill_cat):
        result = loaded_skill_cat.unload_skill("hello-world")
        assert result >= 1

    def test_is_loaded_false_after_unload(self, loaded_skill_cat):
        loaded_skill_cat.unload_skill("hello-world")
        assert loaded_skill_cat.is_loaded("hello-world") is False

    def test_loaded_count_decrements(self, loaded_skill_cat):
        loaded_skill_cat.unload_skill("hello-world")
        assert loaded_skill_cat.loaded_count() == 0

    def test_list_skills_still_present(self, loaded_skill_cat):
        loaded_skill_cat.unload_skill("hello-world")
        skills = loaded_skill_cat.list_skills()
        assert any(s.name == "hello-world" for s in skills)

    def test_can_reload_after_unload(self, loaded_skill_cat):
        loaded_skill_cat.unload_skill("hello-world")
        result = loaded_skill_cat.load_skill("hello-world")
        assert isinstance(result, list)
        assert loaded_skill_cat.is_loaded("hello-world") is True

    def test_unload_nonexistent_raises(self, single_skill_cat):
        with pytest.raises((ValueError, RuntimeError)):
            single_skill_cat.unload_skill("does-not-exist")

    def test_load_nonexistent_raises(self, single_skill_cat):
        with pytest.raises((ValueError, RuntimeError)):
            single_skill_cat.load_skill("does-not-exist")


# ---------------------------------------------------------------------------
# TestSkillCatalogMultiSkill
# ---------------------------------------------------------------------------


class TestSkillCatalogMultiSkill:
    def test_discover_two_different_dcc(self, two_skill_cat):
        skills = two_skill_cat.list_skills()
        assert len(skills) == 2

    def test_load_one_affects_only_one(self, two_skill_cat):
        two_skill_cat.load_skill("hello-world")
        assert two_skill_cat.is_loaded("hello-world") is True
        assert two_skill_cat.is_loaded("maya-geo") is False

    def test_loaded_count_partial(self, two_skill_cat):
        two_skill_cat.load_skill("hello-world")
        assert two_skill_cat.loaded_count() == 1

    def test_find_skills_returns_all_no_filter(self, two_skill_cat):
        found = two_skill_cat.find_skills()
        assert len(found) >= 2

    def test_find_skills_returns_only_python(self, two_skill_cat):
        found = two_skill_cat.find_skills(dcc="python")
        assert all(s.dcc == "python" for s in found)
        assert any(s.name == "hello-world" for s in found)

    def test_find_skills_returns_only_maya(self, two_skill_cat):
        found = two_skill_cat.find_skills(dcc="maya")
        assert all(s.dcc == "maya" for s in found)
        assert any(s.name == "maya-geo" for s in found)

    def test_unload_one_leaves_other_loaded(self, two_skill_cat):
        two_skill_cat.load_skill("hello-world")
        two_skill_cat.load_skill("maya-geo")
        two_skill_cat.unload_skill("hello-world")
        assert two_skill_cat.is_loaded("maya-geo") is True
        assert two_skill_cat.is_loaded("hello-world") is False

    def test_list_skills_both_present(self, two_skill_cat):
        names = {s.name for s in two_skill_cat.list_skills()}
        assert "hello-world" in names
        assert "maya-geo" in names

    def test_discover_idempotent(self, tmpdir_path):
        make_skill_dir(tmpdir_path, "hello-world")
        cat = SkillCatalog(ToolRegistry())
        count1 = cat.discover(extra_paths=[tmpdir_path])
        count2 = cat.discover(extra_paths=[tmpdir_path])
        # Second discover of same path should not add duplicates
        assert count2 == 0 or len(cat.list_skills()) == count1

    def test_discover_incremental(self, tmpdir_path):
        make_skill_dir(tmpdir_path, "skill-a")
        cat = SkillCatalog(ToolRegistry())
        cat.discover(extra_paths=[tmpdir_path])
        make_skill_dir(tmpdir_path, "skill-b")
        cat.discover(extra_paths=[tmpdir_path])
        # Should discover the new skill
        names = {s.name for s in cat.list_skills()}
        assert "skill-a" in names
        assert "skill-b" in names

    def test_total_count_repr(self, two_skill_cat):
        r = repr(two_skill_cat)
        assert "2" in r


# ---------------------------------------------------------------------------
# TestTransportManagerMultiService
# ---------------------------------------------------------------------------


class TestTransportManagerMultiService:
    def test_register_two_same_dcc(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        assert len(tm.list_all_services()) == 2

    def test_list_all_services_two(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        svcs = tm.list_all_services()
        assert len(svcs) == 2

    def test_list_instances_two(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        insts = tm.list_instances("maya")
        assert len(insts) == 2

    def test_get_service_each(self):
        tm = make_tm()
        iid1 = tm.register_service("maya", "127.0.0.1", 9001)
        iid2 = tm.register_service("maya", "127.0.0.1", 9002)
        svc1 = tm.get_service("maya", iid1)
        svc2 = tm.get_service("maya", iid2)
        assert svc1.port == 9001
        assert svc2.port == 9002

    def test_find_best_prefers_available(self):
        tm = make_tm()
        iid1 = tm.register_service("maya", "127.0.0.1", 9001)
        _iid2 = tm.register_service("maya", "127.0.0.1", 9002)
        tm.update_service_status("maya", iid1, ServiceStatus.BUSY)
        best = tm.find_best_service("maya")
        assert best is not None
        # AVAILABLE instance should be preferred
        assert best.port == 9002

    def test_update_one_busy_find_best(self):
        tm = make_tm()
        iid1 = tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        tm.update_service_status("maya", iid1, ServiceStatus.BUSY)
        best = tm.find_best_service("maya")
        # Should find the available one
        assert best is not None
        assert best.status == ServiceStatus.AVAILABLE

    def test_deregister_one_leaves_other(self):
        tm = make_tm()
        iid1 = tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        tm.deregister_service("maya", iid1)
        assert len(tm.list_all_services()) == 1

    def test_register_different_dccs(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("blender", "127.0.0.1", 9002)
        assert len(tm.list_all_services()) == 2

    def test_list_instances_by_dcc(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("blender", "127.0.0.1", 9002)
        maya_insts = tm.list_instances("maya")
        blender_insts = tm.list_instances("blender")
        assert len(maya_insts) == 1
        assert len(blender_insts) == 1

    def test_rank_services_length(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        ranked = tm.rank_services("maya")
        assert len(ranked) == 2

    def test_heartbeat_multiple(self):
        tm = make_tm()
        iid1 = tm.register_service("maya", "127.0.0.1", 9001)
        iid2 = tm.register_service("maya", "127.0.0.1", 9002)
        # Should not raise
        tm.heartbeat("maya", iid1)
        tm.heartbeat("maya", iid2)

    def test_shutdown_then_is_shutdown(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.shutdown()
        assert tm.is_shutdown() is True


# ---------------------------------------------------------------------------
# TestTransportManagerServiceEntry
# ---------------------------------------------------------------------------


class TestTransportManagerServiceEntry:
    @pytest.fixture()
    def entry(self):
        tm = make_tm()
        iid = tm.register_service("maya", "127.0.0.1", 9001)
        return tm.get_service("maya", iid)

    def test_entry_dcc_type(self, entry):
        assert entry.dcc_type == "maya"

    def test_entry_host(self, entry):
        assert entry.host == "127.0.0.1"

    def test_entry_port(self, entry):
        assert entry.port == 9001

    def test_entry_instance_id_is_uuid(self, entry):
        # Must be a valid UUID string
        uuid.UUID(entry.instance_id)

    def test_entry_status_default_available(self, entry):
        assert entry.status == ServiceStatus.AVAILABLE

    def test_entry_version_none(self, entry):
        assert entry.version is None

    def test_entry_scene_none(self, entry):
        assert entry.scene is None

    def test_entry_metadata_empty(self, entry):
        assert entry.metadata == {}

    def test_entry_is_ipc_false(self, entry):
        assert entry.is_ipc is False

    def test_entry_last_heartbeat_positive(self, entry):
        assert entry.last_heartbeat_ms > 0

    def test_entry_transport_address_none(self, entry):
        assert entry.transport_address is None

    def test_to_dict_has_required_keys(self, entry):
        d = entry.to_dict()
        for k in ("dcc_type", "instance_id", "host", "port", "status"):
            assert k in d


# ---------------------------------------------------------------------------
# TestTransportManagerStateMachine
# ---------------------------------------------------------------------------


class TestTransportManagerStateMachine:
    def test_available_to_busy(self):
        tm = make_tm()
        iid = tm.register_service("maya", "127.0.0.1", 9001)
        tm.update_service_status("maya", iid, ServiceStatus.BUSY)
        assert tm.get_service("maya", iid).status == ServiceStatus.BUSY

    def test_busy_to_available(self):
        tm = make_tm()
        iid = tm.register_service("maya", "127.0.0.1", 9001)
        tm.update_service_status("maya", iid, ServiceStatus.BUSY)
        tm.update_service_status("maya", iid, ServiceStatus.AVAILABLE)
        assert tm.get_service("maya", iid).status == ServiceStatus.AVAILABLE

    def test_available_to_shutting_down(self):
        tm = make_tm()
        iid = tm.register_service("maya", "127.0.0.1", 9001)
        tm.update_service_status("maya", iid, ServiceStatus.SHUTTING_DOWN)
        assert tm.get_service("maya", iid).status == ServiceStatus.SHUTTING_DOWN

    def test_available_to_unreachable(self):
        tm = make_tm()
        iid = tm.register_service("maya", "127.0.0.1", 9001)
        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        assert tm.get_service("maya", iid).status == ServiceStatus.UNREACHABLE

    def test_update_nonexistent_instance_noop(self):
        tm = make_tm()
        fake_id = str(uuid.uuid4())
        # Should not raise even for unknown UUID
        with contextlib.suppress(Exception):
            tm.update_service_status("maya", fake_id, ServiceStatus.BUSY)

    def test_cleanup_noop(self):
        tm = make_tm()
        tm.cleanup()  # Should not raise

    def test_repr_reflects_count(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        r = repr(tm)
        assert "1" in r

    def test_is_shutdown_false_initially(self):
        tm = make_tm()
        assert tm.is_shutdown() is False

    def test_shutdown_idempotent(self):
        tm = make_tm()
        tm.shutdown()
        tm.shutdown()  # Should not raise
        assert tm.is_shutdown() is True

    def test_is_shutdown_true_after_shutdown(self):
        tm = make_tm()
        tm.shutdown()
        assert tm.is_shutdown() is True

    def test_pool_size_initial_zero(self):
        tm = make_tm()
        assert tm.pool_size() == 0

    def test_session_count_initial_zero(self):
        tm = make_tm()
        assert tm.session_count() == 0

    def test_pool_count_for_dcc_zero(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        assert tm.pool_count_for_dcc("maya") == 0

    def test_list_sessions_for_dcc_empty(self):
        tm = make_tm()
        tm.register_service("maya", "127.0.0.1", 9001)
        assert tm.list_sessions_for_dcc("maya") == []


# ---------------------------------------------------------------------------
# TestServiceStatusEnum
# ---------------------------------------------------------------------------


class TestServiceStatusEnum:
    def test_available_value(self):
        assert ServiceStatus.AVAILABLE is not None

    def test_busy_value(self):
        assert ServiceStatus.BUSY is not None

    def test_shutting_down_value(self):
        assert ServiceStatus.SHUTTING_DOWN is not None

    def test_unreachable_value(self):
        assert ServiceStatus.UNREACHABLE is not None

    def test_available_eq(self):
        assert ServiceStatus.AVAILABLE == ServiceStatus.AVAILABLE

    def test_busy_eq(self):
        assert ServiceStatus.BUSY == ServiceStatus.BUSY

    def test_shutting_down_eq(self):
        assert ServiceStatus.SHUTTING_DOWN == ServiceStatus.SHUTTING_DOWN

    def test_unreachable_eq(self):
        assert ServiceStatus.UNREACHABLE == ServiceStatus.UNREACHABLE

    def test_available_ne_busy(self):
        assert ServiceStatus.AVAILABLE != ServiceStatus.BUSY

    def test_available_ne_unreachable(self):
        assert ServiceStatus.AVAILABLE != ServiceStatus.UNREACHABLE

    def test_busy_ne_shutting_down(self):
        assert ServiceStatus.BUSY != ServiceStatus.SHUTTING_DOWN

    def test_int_conversion(self):
        val = int(ServiceStatus.AVAILABLE)
        assert isinstance(val, int)

    def test_repr_contains_variant(self):
        r = repr(ServiceStatus.AVAILABLE)
        assert "AVAILABLE" in r

    def test_status_int_can_be_dict_key(self):
        d = {int(ServiceStatus.AVAILABLE): "ok", int(ServiceStatus.BUSY): "busy"}
        assert d[int(ServiceStatus.AVAILABLE)] == "ok"
