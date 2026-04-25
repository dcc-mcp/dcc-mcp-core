# 检查点 API

> **[English](../api/checkpoint.md)**

检查点/恢复机制，用于长时间运行的工具执行。可在循环中间隔保存进度，中断的任务从最近检查点恢复而非从头开始。

**导出符号：** `CheckpointStore`, `configure_checkpoint_store`, `save_checkpoint`, `get_checkpoint`, `clear_checkpoint`, `list_checkpoints`, `checkpoint_every`, `register_checkpoint_tools`

## CheckpointStore

线程安全的检查点存储。默认内存存储，设置 `path` 后使用 JSON 文件持久化。

- `.save(job_id, state, progress_hint="")` — 保存检查点
- `.get(job_id) -> dict | None` — 获取检查点（含 `job_id`、`saved_at`、`progress_hint`、`context`）
- `.clear(job_id) -> bool` — 删除指定检查点
- `.list_ids() -> list[str]` — 列出所有检查点 job ID
- `.clear_all() -> int` — 清空所有，返回删除数量

## 便捷函数

- `save_checkpoint(job_id, state, *, progress_hint="", store=None)` — 保存检查点
- `get_checkpoint(job_id, *, store=None) -> dict | None` — 获取检查点
- `clear_checkpoint(job_id, *, store=None) -> bool` — 删除检查点
- `list_checkpoints(*, store=None) -> list[str]` — 列出 job ID
- `checkpoint_every(n, job_id, state_fn, *, progress_fn=None, store=None)` — 每 *n* 次迭代自动保存
- `register_checkpoint_tools(server, *, dcc_name="dcc", store=None)` — 注册 `jobs.checkpoint_status` 和 `jobs.resume_context` MCP 工具

详见 [English API 参考](../api/checkpoint.md)。
