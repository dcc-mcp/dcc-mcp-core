"""Tests for SandboxPolicy, SandboxContext, TransportManager and related APIs.

find_best_service, rank_services, begin_reconnect, heartbeat covered.
Iteration 89: +128 tests covering SandboxPolicy, SandboxContext, ServiceStatus,
TransportManager pool/session/routing/heartbeat/reconnect operations.
  - SandboxPolicy: allow_actions, deny_actions, allow_paths, set_read_only,
    set_max_actions, set_timeout_ms, repr
  - SandboxContext: construction, is_allowed, is_path_allowed, execute_json,
    action_count, set_actor, audit_log passthrough
  - TransportManager: register_service, get_service, list_all_services,
    list_all_instances, pool_size, pool_count_for_dcc, acquire_connection (error),
    release_connection, find_best_service, rank_services, update_service_status,
    deregister_service, heartbeat, begin_reconnect, reconnect_success,
    get_or_create_session_routed, session_count
  - ServiceStatus: enum variants and equality

Notes on allow_paths behavior:
  - On Windows, allow_paths performs file-system existence check on the prefix.
    Paths like "/tmp" exist (Windows resolves as system path), but "/projects"
    does not. Tests use tmp_path fixture to guarantee real directories.
Notes on UUID validation:
  - APIs that accept instance_id validate strict UUID format.
    Use uuid.uuid4() to generate valid-but-nonexistent UUIDs for "unknown" tests.
"""

from __future__ import annotations

from pathlib import Path
import uuid

import pytest

import dcc_mcp_core as c

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture()
def sandbox_policy() -> c.SandboxPolicy:
    return c.SandboxPolicy()


@pytest.fixture()
def sandbox_ctx(sandbox_policy: c.SandboxPolicy) -> c.SandboxContext:
    return c.SandboxContext(sandbox_policy)


@pytest.fixture()
def tmp_registry(tmp_path) -> str:
    return str(tmp_path / "registry")


@pytest.fixture()
def transport_manager(tmp_registry: str) -> c.TransportManager:
    Path(tmp_registry).mkdir(parents=True, exist_ok=True)
    return c.TransportManager(tmp_registry)


@pytest.fixture()
def tm_with_maya(transport_manager: c.TransportManager):
    """TransportManager pre-loaded with one maya service."""
    inst_id = transport_manager.register_service("maya", "localhost", 17001)
    return transport_manager, inst_id


@pytest.fixture()
def tm_with_two_maya(transport_manager: c.TransportManager):
    """TransportManager pre-loaded with two maya services."""
    id1 = transport_manager.register_service("maya", "localhost", 17001)
    id2 = transport_manager.register_service("maya", "localhost", 17002)
    return transport_manager, id1, id2


# ===========================================================================
# SandboxPolicy tests
# ===========================================================================


class TestSandboxPolicyConstruction:
    def test_default_not_read_only(self) -> None:
        sp = c.SandboxPolicy()
        assert sp.is_read_only is False

    def test_repr_default(self) -> None:
        sp = c.SandboxPolicy()
        r = repr(sp)
        assert "SandboxPolicy" in r
        assert "ReadWrite" in r
        assert "timeout=None" in r
        assert "max_actions=None" in r

    def test_repr_after_read_only(self) -> None:
        sp = c.SandboxPolicy()
        sp.set_read_only(True)
        r = repr(sp)
        assert "ReadOnly" in r

    def test_repr_after_set_max_actions(self) -> None:
        sp = c.SandboxPolicy()
        sp.set_max_actions(10)
        r = repr(sp)
        assert "10" in r

    def test_repr_after_set_timeout_ms(self) -> None:
        sp = c.SandboxPolicy()
        sp.set_timeout_ms(500)
        r = repr(sp)
        assert "500" in r


class TestSandboxPolicySetters:
    def test_set_read_only_true(self) -> None:
        sp = c.SandboxPolicy()
        sp.set_read_only(True)
        assert sp.is_read_only is True

    def test_set_read_only_false(self) -> None:
        sp = c.SandboxPolicy()
        sp.set_read_only(True)
        sp.set_read_only(False)
        assert sp.is_read_only is False

    def test_set_read_only_returns_none(self) -> None:
        sp = c.SandboxPolicy()
        result = sp.set_read_only(True)
        assert result is None

    def test_set_max_actions_returns_none(self) -> None:
        sp = c.SandboxPolicy()
        result = sp.set_max_actions(5)
        assert result is None

    def test_set_timeout_ms_returns_none(self) -> None:
        sp = c.SandboxPolicy()
        result = sp.set_timeout_ms(1000)
        assert result is None

    def test_is_read_only_is_bool(self) -> None:
        sp = c.SandboxPolicy()
        assert isinstance(sp.is_read_only, bool)


class TestSandboxPolicyAllowDenyActions:
    def test_allow_actions_restricts_to_whitelist(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions(["echo", "ping"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("echo") is True
        assert ctx.is_allowed("ping") is True

    def test_allow_actions_rejects_not_in_whitelist(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions(["echo"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("other_action") is False

    def test_allow_actions_returns_none(self) -> None:
        sp = c.SandboxPolicy()
        result = sp.allow_actions(["echo"])
        assert result is None

    def test_deny_actions_blocks_listed(self) -> None:
        sp = c.SandboxPolicy()
        sp.deny_actions(["forbidden"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("forbidden") is False

    def test_deny_actions_allows_others(self) -> None:
        sp = c.SandboxPolicy()
        sp.deny_actions(["forbidden"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("allowed_action") is True

    def test_deny_actions_returns_none(self) -> None:
        sp = c.SandboxPolicy()
        result = sp.deny_actions(["forbidden"])
        assert result is None

    def test_empty_allow_actions_blocks_all(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions([])
        ctx = c.SandboxContext(sp)
        # Empty whitelist means nothing is allowed
        assert ctx.is_allowed("any_action") is False

    def test_default_policy_allows_any(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("any_action") is True
        assert ctx.is_allowed("create_sphere") is True


class TestSandboxPolicyAllowPaths:
    def test_allow_paths_returns_none(self) -> None:
        sp = c.SandboxPolicy()
        result = sp.allow_paths(["/tmp"])
        assert result is None

    def test_allow_paths_permits_sub_path(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_paths(["/tmp"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed("/tmp/test.ma") is True

    def test_allow_paths_rejects_outside(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_paths(["/tmp"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed("/other/path") is False

    def test_allow_paths_exact_dir(self, tmp_path) -> None:
        allowed_dir = tmp_path / "projects"
        allowed_dir.mkdir(parents=True, exist_ok=True)
        sp = c.SandboxPolicy()
        sp.allow_paths([str(allowed_dir)])
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed(str(allowed_dir / "scene.usd")) is True

    def test_no_allow_paths_permits_any(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed("/any/path") is True

    def test_multiple_paths_both_permitted(self, tmp_path) -> None:
        dir_a = tmp_path / "dir_a"
        dir_b = tmp_path / "dir_b"
        dir_a.mkdir(parents=True, exist_ok=True)
        dir_b.mkdir(parents=True, exist_ok=True)
        sp = c.SandboxPolicy()
        sp.allow_paths([str(dir_a), str(dir_b)])
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed(str(dir_a / "scene.ma")) is True
        assert ctx.is_path_allowed(str(dir_b / "asset.usd")) is True

    def test_multiple_paths_others_rejected(self, tmp_path) -> None:
        dir_a = tmp_path / "dir_a"
        dir_a.mkdir(parents=True, exist_ok=True)
        sp = c.SandboxPolicy()
        sp.allow_paths([str(dir_a)])
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed("/home/user/unrelated_file") is False


# ===========================================================================
# SandboxContext tests
# ===========================================================================


class TestSandboxContextConstruction:
    def test_construction_requires_policy(self) -> None:
        with pytest.raises(TypeError):
            c.SandboxContext()  # type: ignore[call-arg]

    def test_construction_with_policy(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert ctx is not None

    def test_initial_action_count_is_zero(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert ctx.action_count == 0

    def test_action_count_is_int(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert isinstance(ctx.action_count, int)


class TestSandboxContextIsAllowed:
    def test_default_policy_allows_everything(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("create_sphere") is True
        assert ctx.is_allowed("delete_mesh") is True

    def test_whitelist_allows_listed(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions(["create_sphere"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("create_sphere") is True

    def test_whitelist_blocks_unlisted(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions(["create_sphere"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("delete_mesh") is False

    def test_blacklist_blocks_listed(self) -> None:
        sp = c.SandboxPolicy()
        sp.deny_actions(["dangerous_action"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("dangerous_action") is False

    def test_blacklist_allows_others(self) -> None:
        sp = c.SandboxPolicy()
        sp.deny_actions(["dangerous_action"])
        ctx = c.SandboxContext(sp)
        assert ctx.is_allowed("safe_action") is True

    def test_is_allowed_returns_bool(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        result = ctx.is_allowed("any")
        assert isinstance(result, bool)


class TestSandboxContextIsPathAllowed:
    def test_no_paths_set_allows_any(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed("/any/path") is True

    def test_path_in_allowed_set(self, tmp_path) -> None:
        allowed_dir = tmp_path / "allowed"
        allowed_dir.mkdir(parents=True, exist_ok=True)
        sp = c.SandboxPolicy()
        sp.allow_paths([str(allowed_dir)])
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed(str(allowed_dir / "scene.ma")) is True

    def test_path_outside_allowed_set(self, tmp_path) -> None:
        allowed_dir = tmp_path / "allowed"
        allowed_dir.mkdir(parents=True, exist_ok=True)
        sp = c.SandboxPolicy()
        sp.allow_paths([str(allowed_dir)])
        ctx = c.SandboxContext(sp)
        assert ctx.is_path_allowed("/home/evil/file") is False

    def test_is_path_allowed_returns_bool(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        result = ctx.is_path_allowed("/tmp")
        assert isinstance(result, bool)


class TestSandboxContextExecuteJson:
    def test_execute_json_increments_action_count(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        ctx.execute_json("some_action", "{}")
        assert ctx.action_count == 1

    def test_execute_json_twice_increments_twice(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        ctx.execute_json("action_a", "{}")
        ctx.execute_json("action_b", "{}")
        assert ctx.action_count == 2

    def test_execute_json_denied_raises_runtime_error(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions(["allowed_only"])
        ctx = c.SandboxContext(sp)
        with pytest.raises(RuntimeError, match="not allowed"):
            ctx.execute_json("forbidden_action", "{}")

    def test_execute_json_denied_does_not_increment_count(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions(["allowed_only"])
        ctx = c.SandboxContext(sp)
        with pytest.raises(RuntimeError):
            ctx.execute_json("forbidden_action", "{}")
        assert ctx.action_count == 0

    def test_execute_json_denied_error_message_mentions_action(self) -> None:
        sp = c.SandboxPolicy()
        sp.allow_actions(["allowed"])
        ctx = c.SandboxContext(sp)
        with pytest.raises(RuntimeError) as exc_info:
            ctx.execute_json("the_blocked_action", "{}")
        assert "the_blocked_action" in str(exc_info.value)


class TestSandboxContextSetActor:
    def test_set_actor_returns_none(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        result = ctx.set_actor("my-agent")
        assert result is None

    def test_set_actor_accepts_string(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        ctx.set_actor("agent-v2")

    def test_set_actor_multiple_times(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        ctx.set_actor("agent1")
        ctx.set_actor("agent2")


class TestSandboxContextAuditLog:
    def test_audit_log_accessible(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        log = ctx.audit_log
        assert log is not None

    def test_audit_log_type(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        log = ctx.audit_log
        assert type(log).__name__ == "AuditLog"

    def test_audit_log_has_entries(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert hasattr(ctx.audit_log, "entries")

    def test_audit_log_has_successes(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert hasattr(ctx.audit_log, "successes")

    def test_audit_log_has_denials(self) -> None:
        sp = c.SandboxPolicy()
        ctx = c.SandboxContext(sp)
        assert hasattr(ctx.audit_log, "denials")


# ===========================================================================
# ServiceStatus enum tests
# ===========================================================================


class TestServiceStatus:
    def test_available_variant(self) -> None:
        assert c.ServiceStatus.AVAILABLE is not None

    def test_busy_variant(self) -> None:
        assert c.ServiceStatus.BUSY is not None

    def test_unreachable_variant(self) -> None:
        assert c.ServiceStatus.UNREACHABLE is not None

    def test_shutting_down_variant(self) -> None:
        assert c.ServiceStatus.SHUTTING_DOWN is not None

    def test_available_str(self) -> None:
        assert "AVAILABLE" in str(c.ServiceStatus.AVAILABLE)

    def test_busy_str(self) -> None:
        assert "BUSY" in str(c.ServiceStatus.BUSY)

    def test_same_variant_eq(self) -> None:
        assert c.ServiceStatus.AVAILABLE == c.ServiceStatus.AVAILABLE

    def test_diff_variant_ne(self) -> None:
        assert c.ServiceStatus.AVAILABLE != c.ServiceStatus.BUSY


# ===========================================================================
# TransportManager pool and service registration tests
# ===========================================================================


class TestTransportManagerPoolBasics:
    def test_initial_pool_size_is_zero(self, transport_manager: c.TransportManager) -> None:
        assert transport_manager.pool_size() == 0

    def test_initial_pool_count_for_unknown_dcc_is_zero(self, transport_manager: c.TransportManager) -> None:
        assert transport_manager.pool_count_for_dcc("nonexistent") == 0

    def test_pool_count_for_dcc_returns_int(self, transport_manager: c.TransportManager) -> None:
        result = transport_manager.pool_count_for_dcc("maya")
        assert isinstance(result, int)

    def test_pool_size_returns_int(self, transport_manager: c.TransportManager) -> None:
        result = transport_manager.pool_size()
        assert isinstance(result, int)

    def test_pool_count_after_register_still_zero(self, transport_manager: c.TransportManager) -> None:
        # Pool only grows with actual connections
        transport_manager.register_service("maya", "localhost", 17001)
        assert transport_manager.pool_count_for_dcc("maya") == 0

    def test_release_connection_no_error(self, transport_manager: c.TransportManager) -> None:
        # release when nothing acquired should not raise, uses valid UUID format
        fake_uuid = str(uuid.uuid4())
        result = transport_manager.release_connection("maya", fake_uuid)
        assert result is None

    def test_acquire_connection_no_service_raises(self, transport_manager: c.TransportManager) -> None:
        with pytest.raises(RuntimeError, match="service not found"):
            transport_manager.acquire_connection("maya")

    def test_acquire_connection_wrong_dcc_raises(self, transport_manager: c.TransportManager) -> None:
        with pytest.raises(RuntimeError):
            transport_manager.acquire_connection("nonexistent_dcc")


class TestTransportManagerRegisterService:
    def test_register_returns_uuid_string(self, transport_manager: c.TransportManager) -> None:
        inst_id = transport_manager.register_service("maya", "localhost", 17001)
        assert isinstance(inst_id, str)
        assert len(inst_id) == 36  # UUID format

    def test_register_two_services_different_ids(self, transport_manager: c.TransportManager) -> None:
        id1 = transport_manager.register_service("maya", "localhost", 17001)
        id2 = transport_manager.register_service("maya", "localhost", 17002)
        assert id1 != id2

    def test_register_different_dccs(self, transport_manager: c.TransportManager) -> None:
        id_maya = transport_manager.register_service("maya", "localhost", 17001)
        id_blender = transport_manager.register_service("blender", "localhost", 18001)
        assert id_maya != id_blender

    def test_list_all_services_empty_initially(self, transport_manager: c.TransportManager) -> None:
        assert transport_manager.list_all_services() == []

    def test_list_all_services_after_register(self, transport_manager: c.TransportManager) -> None:
        transport_manager.register_service("maya", "localhost", 17001)
        services = transport_manager.list_all_services()
        assert len(services) == 1

    def test_list_all_services_multiple(self, transport_manager: c.TransportManager) -> None:
        transport_manager.register_service("maya", "localhost", 17001)
        transport_manager.register_service("blender", "localhost", 18001)
        services = transport_manager.list_all_services()
        assert len(services) == 2

    def test_list_all_instances_alias(self, transport_manager: c.TransportManager) -> None:
        transport_manager.register_service("maya", "localhost", 17001)
        assert len(transport_manager.list_all_instances()) == 1

    def test_list_all_instances_same_as_services(self, transport_manager: c.TransportManager) -> None:
        transport_manager.register_service("maya", "localhost", 17001)
        assert len(transport_manager.list_all_instances()) == len(transport_manager.list_all_services())


class TestTransportManagerGetService:
    def test_get_service_returns_entry(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        entry = tm.get_service("maya", inst_id)
        assert entry is not None

    def test_get_service_dcc_type(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        entry = tm.get_service("maya", inst_id)
        assert entry.dcc_type == "maya"

    def test_get_service_instance_id(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        entry = tm.get_service("maya", inst_id)
        assert entry.instance_id == inst_id

    def test_get_service_host(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        entry = tm.get_service("maya", inst_id)
        assert entry.host == "localhost"

    def test_get_service_port(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        entry = tm.get_service("maya", inst_id)
        assert entry.port == 17001

    def test_get_service_initial_status_available(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        entry = tm.get_service("maya", inst_id)
        assert entry.status == c.ServiceStatus.AVAILABLE

    def test_get_service_unknown_id_returns_none(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        fake_uuid = str(uuid.uuid4())
        entry = tm.get_service("maya", fake_uuid)
        assert entry is None

    def test_get_service_wrong_dcc_returns_none(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        entry = tm.get_service("blender", inst_id)
        assert entry is None


class TestTransportManagerUpdateServiceStatus:
    def test_update_to_busy(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        result = tm.update_service_status("maya", inst_id, c.ServiceStatus.BUSY)
        assert result is True

    def test_update_to_unreachable(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        result = tm.update_service_status("maya", inst_id, c.ServiceStatus.UNREACHABLE)
        assert result is True

    def test_update_to_available(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        tm.update_service_status("maya", inst_id, c.ServiceStatus.BUSY)
        result = tm.update_service_status("maya", inst_id, c.ServiceStatus.AVAILABLE)
        assert result is True

    def test_update_unknown_instance_returns_false(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        fake_uuid = str(uuid.uuid4())
        result = tm.update_service_status("maya", fake_uuid, c.ServiceStatus.BUSY)
        assert result is False

    def test_status_reflected_in_get_service(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        tm.update_service_status("maya", inst_id, c.ServiceStatus.BUSY)
        entry = tm.get_service("maya", inst_id)
        assert entry.status == c.ServiceStatus.BUSY

    def test_update_requires_service_status_enum(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        with pytest.raises(TypeError):
            tm.update_service_status("maya", inst_id, "BUSY")  # type: ignore[arg-type]


class TestTransportManagerFindBestService:
    def test_find_best_no_service_raises(self, transport_manager: c.TransportManager) -> None:
        with pytest.raises(RuntimeError, match="service not found"):
            transport_manager.find_best_service("maya")

    def test_find_best_returns_available_first(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.BUSY)
        best = tm.find_best_service("maya")
        assert best.instance_id == id2
        assert best.status == c.ServiceStatus.AVAILABLE

    def test_find_best_available_preferred_over_busy(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id2, c.ServiceStatus.BUSY)
        best = tm.find_best_service("maya")
        assert best.instance_id == id1
        assert best.status == c.ServiceStatus.AVAILABLE

    def test_find_best_excludes_unreachable(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.UNREACHABLE)
        best = tm.find_best_service("maya")
        assert best.instance_id == id2

    def test_find_best_all_unreachable_raises(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.UNREACHABLE)
        tm.update_service_status("maya", id2, c.ServiceStatus.UNREACHABLE)
        with pytest.raises(RuntimeError):
            tm.find_best_service("maya")

    def test_find_best_excludes_shutting_down(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.SHUTTING_DOWN)
        best = tm.find_best_service("maya")
        assert best.instance_id == id2

    def test_find_best_wrong_dcc_raises(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        with pytest.raises(RuntimeError):
            tm.find_best_service("blender")


class TestTransportManagerRankServices:
    def test_rank_no_service_raises(self, transport_manager: c.TransportManager) -> None:
        with pytest.raises(RuntimeError, match="service not found"):
            transport_manager.rank_services("maya")

    def test_rank_returns_list(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        result = tm.rank_services("maya")
        assert isinstance(result, list)

    def test_rank_single_service(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        ranked = tm.rank_services("maya")
        assert len(ranked) == 1
        assert ranked[0].instance_id == inst_id

    def test_rank_two_services_count(self, tm_with_two_maya) -> None:
        tm, _id1, _id2 = tm_with_two_maya
        ranked = tm.rank_services("maya")
        assert len(ranked) == 2

    def test_rank_available_before_busy(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.BUSY)
        ranked = tm.rank_services("maya")
        # AVAILABLE (id2) should come before BUSY (id1)
        assert ranked[0].instance_id == id2
        assert ranked[0].status == c.ServiceStatus.AVAILABLE

    def test_rank_excludes_unreachable(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.UNREACHABLE)
        ranked = tm.rank_services("maya")
        assert len(ranked) == 1
        assert ranked[0].instance_id == id2

    def test_rank_all_unreachable_raises(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.UNREACHABLE)
        tm.update_service_status("maya", id2, c.ServiceStatus.UNREACHABLE)
        with pytest.raises(RuntimeError):
            tm.rank_services("maya")

    def test_rank_excludes_shutting_down(self, tm_with_two_maya) -> None:
        tm, id1, id2 = tm_with_two_maya
        tm.update_service_status("maya", id1, c.ServiceStatus.SHUTTING_DOWN)
        ranked = tm.rank_services("maya")
        assert len(ranked) == 1
        assert ranked[0].instance_id == id2


class TestTransportManagerDeregisterService:
    def test_deregister_returns_true(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        result = tm.deregister_service("maya", inst_id)
        assert result is True

    def test_deregister_reduces_service_count(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        assert len(tm.list_all_services()) == 1
        tm.deregister_service("maya", inst_id)
        assert len(tm.list_all_services()) == 0

    def test_deregister_unknown_returns_false(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        fake_uuid = str(uuid.uuid4())
        result = tm.deregister_service("maya", fake_uuid)
        assert result is False

    def test_deregister_then_find_best_raises(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        tm.deregister_service("maya", inst_id)
        with pytest.raises(RuntimeError):
            tm.find_best_service("maya")


class TestTransportManagerHeartbeat:
    def test_heartbeat_returns_true(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        result = tm.heartbeat("maya", inst_id)
        assert result is True

    def test_heartbeat_unknown_instance_returns_false(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        fake_uuid = str(uuid.uuid4())
        result = tm.heartbeat("maya", fake_uuid)
        assert result is False

    def test_heartbeat_wrong_dcc_returns_false(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        result = tm.heartbeat("blender", inst_id)
        assert result is False


class TestTransportManagerReconnect:
    def test_begin_reconnect_returns_int(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        session_id = tm.get_or_create_session("maya", inst_id)
        delay = tm.begin_reconnect(session_id)
        assert isinstance(delay, int)

    def test_begin_reconnect_positive_delay(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        session_id = tm.get_or_create_session("maya", inst_id)
        delay = tm.begin_reconnect(session_id)
        assert delay > 0

    def test_reconnect_success_returns_none(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        session_id = tm.get_or_create_session("maya", inst_id)
        tm.begin_reconnect(session_id)
        result = tm.reconnect_success(session_id)
        assert result is None

    def test_begin_reconnect_twice_backoff_grows(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        session_id = tm.get_or_create_session("maya", inst_id)
        delay1 = tm.begin_reconnect(session_id)
        tm.reconnect_success(session_id)
        delay2 = tm.begin_reconnect(session_id)
        # Backoff should be >= first delay
        assert delay2 >= delay1


class TestTransportManagerGetOrCreateSessionRouted:
    def test_routed_no_service_raises(self, transport_manager: c.TransportManager) -> None:
        with pytest.raises(RuntimeError):
            transport_manager.get_or_create_session_routed("maya")

    def test_routed_returns_uuid_string(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        sid = tm.get_or_create_session_routed("maya")
        assert isinstance(sid, str)
        assert len(sid) == 36

    def test_routed_first_available_strategy(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        sid = tm.get_or_create_session_routed("maya", c.RoutingStrategy.FIRST_AVAILABLE)
        assert isinstance(sid, str)

    def test_routed_round_robin_strategy(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        sid = tm.get_or_create_session_routed("maya", c.RoutingStrategy.ROUND_ROBIN)
        assert isinstance(sid, str)

    def test_routed_none_strategy(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        sid = tm.get_or_create_session_routed("maya", None)
        assert isinstance(sid, str)

    def test_routed_same_call_same_session(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        sid1 = tm.get_or_create_session_routed("maya")
        sid2 = tm.get_or_create_session_routed("maya")
        # Should return same session for same instance
        assert sid1 == sid2

    def test_routed_increments_session_count(self, tm_with_maya) -> None:
        tm, _ = tm_with_maya
        assert tm.session_count() == 0
        tm.get_or_create_session_routed("maya")
        assert tm.session_count() == 1

    def test_routed_all_unreachable_raises(self, tm_with_maya) -> None:
        tm, inst_id = tm_with_maya
        tm.update_service_status("maya", inst_id, c.ServiceStatus.UNREACHABLE)
        with pytest.raises(RuntimeError):
            tm.get_or_create_session_routed("maya")


class TestTransportManagerMultiInstanceScenario:
    def test_multi_instance_rank_all_available(self, transport_manager: c.TransportManager) -> None:
        transport_manager.register_service("maya", "localhost", 17001)
        transport_manager.register_service("maya", "localhost", 17002)
        transport_manager.register_service("maya", "localhost", 17003)
        ranked = transport_manager.rank_services("maya")
        assert len(ranked) == 3

    def test_multi_dcc_list_all_instances(self, transport_manager: c.TransportManager) -> None:
        transport_manager.register_service("maya", "localhost", 17001)
        transport_manager.register_service("blender", "localhost", 18001)
        transport_manager.register_service("houdini", "localhost", 19001)
        insts = transport_manager.list_all_instances()
        assert len(insts) == 3

    def test_pool_count_dcc_specific(self, transport_manager: c.TransportManager) -> None:
        transport_manager.register_service("maya", "localhost", 17001)
        transport_manager.register_service("blender", "localhost", 18001)
        # Pool only grows with actual connections
        assert transport_manager.pool_count_for_dcc("maya") == 0
        assert transport_manager.pool_count_for_dcc("blender") == 0
        assert transport_manager.pool_count_for_dcc("houdini") == 0
