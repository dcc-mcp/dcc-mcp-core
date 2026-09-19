# 检查点 API

> **[English](../api/checkpoint.md)**

检查点/恢复机制，用于长时间运行的工具执行。可在循环中间隔保存进度，中断的任务从最近检查点恢复而非从头开始。

**导出符号：** `CHECKPOINT_FILE_NAME`, `CheckpointStore`, `configure_checkpoint_store`, `save_checkpoint`, `get_checkpoint`, `clear_checkpoint`, `list_checkpoints`, `checkpoint_every`, `register_checkpoint_tools`, `default_checkpoint_dir`, `default_checkpoint_path`, `resolve_checkpoint_path`

## 默认持久化（issue #2300）

迭代状态必须跨重启保留，因此服务端检查点存储**默认持久化**，adapter 无需额外接入：

- `DccServerBase` 使用 `resolve_checkpoint_path(dcc_name)` 构造存储，路径为 `<base>/<dcc>/checkpoints.json`。
- `<base>` 为 `DCC_MCP_CHECKPOINT_DIR`（若设置），否则为 `~/.dcc-mcp`。
- 启动时自动注册 `jobs_checkpoint_status` / `jobs_resume_context`，重启后可直接读取恢复状态。
- 写入为原子操作（临时文件 + rename），崩溃不会截断文件。
- 同名 DCC 的多个实例共享该文件。`CheckpointStore` 每次写入前重新读取并合并（按 job 取 `saved_at` 最新者），因此并发保存是叠加的，而非后写覆盖前写。

退出方式（任选其一）：

| 退出方式 | 作用范围 |
|---------|---------|
| `DCC_MCP_CHECKPOINT_IN_MEMORY=1` | 进程级环境变量 |
| `enable_checkpoint_persistence=False` | 单服务端选项 |

`checkpoint_path=...` **不是**退出方式——它是路径覆盖，**会启用**持久化到指定文件，且优先级高于上述两种退出方式（`resolve_checkpoint_path` 的优先级为：显式 `path` → 内存退出 → 持久化默认）。要关闭持久化请使用上表两种方式。

模块级兼容存储（`save_checkpoint` / `get_checkpoint` 使用）仍为内存存储，仅服务端存储默认持久化。

## CheckpointStore

线程安全的检查点存储。默认内存存储，设置 `path` 后使用 JSON 文件持久化。

- `.save(job_id, state, progress_hint="")` — 保存检查点
- `.get(job_id) -> dict | None` — 获取检查点（含 `job_id`、`saved_at`、`progress_hint`、`context`）
- `.clear(job_id) -> bool` — 删除指定检查点
- `.list_ids() -> list[str]` — 列出所有检查点 job ID
- `.clear_all() -> int` — 清空所有，返回删除数量
- `.path -> Path | None` — 持久化文件路径，内存存储为 `None`
- `.is_durable -> bool` — 是否可跨进程重启保留

## 路径助手

- `default_checkpoint_dir()` — `DCC_MCP_CHECKPOINT_DIR` 或 `~/.dcc-mcp`
- `default_checkpoint_path(dcc_name="dcc", instance_id=None)` — `<base>/<dcc>/[instance/]checkpoints.json`，名称经过清洗
- `resolve_checkpoint_path(...)` — 显式 `path` → 内存退出 → 持久化默认；返回 `None` 表示内存存储

## 便捷函数

- `save_checkpoint(job_id, state, *, progress_hint="", store=None)` — 保存检查点
- `get_checkpoint(job_id, *, store=None) -> dict | None` — 获取检查点
- `clear_checkpoint(job_id, *, store=None) -> bool` — 删除检查点
- `list_checkpoints(*, store=None) -> list[str]` — 列出 job ID
- `checkpoint_every(n, job_id, state_fn, *, progress_fn=None, store=None)` — 每 *n* 次迭代自动保存
- `register_checkpoint_tools(server, *, dcc_name="dcc", store=None)` — 注册 `jobs_checkpoint_status` 和 `jobs_resume_context` MCP 工具

详见 [English API 参考](../api/checkpoint.md)。
