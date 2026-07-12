from __future__ import annotations

from dcc_mcp_core.admin_tools import register_admin_tools


class _Registry:
    def __init__(self) -> None:
        self.registered: dict = {}

    def register(self, **kwargs) -> None:
        self.registered[kwargs["name"]] = kwargs


class _Server:
    def __init__(self) -> None:
        self.registry = _Registry()
        self.handlers: dict = {}

    def register_handler(self, name, handler) -> None:
        self.handlers[name] = handler


def test_reload_skills_tool_invokes_adapter_rescan() -> None:
    server = _Server()
    calls = 0

    def reload_skills() -> int:
        nonlocal calls
        calls += 1
        return 17

    register_admin_tools(server, dcc_name="houdini", reload_skills=reload_skills)

    result = server.handlers["dcc_admin__reload_skills"]({})
    assert result == {"success": True, "reloaded": True, "skill_count": 17}
    assert calls == 1
    assert server.registry.registered["dcc_admin__reload_skills"]["category"] == "admin"
