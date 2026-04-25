# 热重载 API

> **[English](../api/hot-reload.md)**

Skill 目录热重载管理器。监控 skill 目录的文件变更，自动触发重新加载，无需重启 MCP 服务器。适用于开发阶段的快速迭代。

**导出符号：** `DccSkillHotReloader`

## DccSkillHotReloader

通用的 skill 热重载管理器。

- `DccSkillHotReloader(dcc_name, server)` — 创建热重载管理器
- `.enable(skill_paths, debounce_ms=300) -> bool` — 启用指定目录的热重载
- `.disable()` — 禁用热重载
- `.reload_now() -> int` — 手动触发重载，返回重载数量
- `.is_enabled -> bool` — 是否已启用
- `.reload_count -> int` — 累计重载次数
- `.watched_paths -> list[str]` — 当前监控的路径
- `.get_stats() -> dict` — 返回 `{enabled, watched_paths, reload_count}`

详见 [English API 参考](../api/hot-reload.md)。
