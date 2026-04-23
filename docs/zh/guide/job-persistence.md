# 作业持久化

`JobManager` 追踪长时间运行的异步工具执行（参见 issues \#316、\#318、
\#326、\#371）。默认情况下它将所有作业状态保存在进程内，这意味着
如果服务器崩溃或重启，每个 `Pending` 或 `Running` 行都会丢失。

Issue #328 增加了一个**可选的、feature-gated 的 SQLite 后端**，
使被追踪的作业能够 survive 重启，并使操作员可以通过内置的 MCP 工具
清理已完成的历史记录。

## 设计概览

```
┌──────────────┐      put/get/list/update_status/delete_older_than      ┌─────────────────┐
│  JobManager  │ ─────────────────────────────────────────────────────▶ │   JobStorage    │
│  (in-proc)   │                                                        │    (trait)      │
└──────────────┘                                                        └─────────────────┘
                                                                                 │
                                                     ┌───────────────────────────┼─────────────────────────┐
                                                     ▼                                                     ▼
                                             InMemoryStorage                                        SqliteStorage
                                             (default, zero-dep)                                    (feature job-persist-sqlite)
```

- `JobStorage` 是 `Send + Sync` 且同步的 — 每次作业转换时直写。
  无后台 flush 任务，无批处理。
- `InMemoryStorage` 是默认值，随每个 wheel 一起发布。语义与 trait
  相同，但显然无法 survive 重启。
- `SqliteStorage` 被 Cargo feature `job-persist-sqlite` 门控，并引入
  `rusqlite`（带 `bundled`），因此不需要系统 SQLite。

## 启用 SQLite 持久化

### 构建

```bash
# 默认构建 — 仅内存，不编译 SQLite 代码
vx just dev

# opt-in — 添加 SqliteStorage 和 rusqlite 依赖
cargo build --workspace --features job-persist-sqlite,python-bindings,ext-module
```

### 配置

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

config = McpHttpConfig(port=8765)
config.job_storage_path = "/var/lib/dcc-mcp/jobs.sqlite3"

registry = ToolRegistry()
# register tools...
server = McpHttpServer(registry, config)
handle = server.start()
```

如果设置了 `job_storage_path` 但 wheel 是**在没有**
`job-persist-sqlite` 的情况下构建的，`server.start()` 会快速失败并返回
描述性错误，而不是静默回退到内存存储。

## 启动恢复

当 `JobManager` 以存储后端启动时，它会扫描状态为 `Pending` 或
`Running` 的行 — 这些是上一个进程退出时正在运行中的作业。
每一行都会被重写为新的终止状态 `JobStatus::Interrupted`，
`error = "server restart"` 并带有新的 `updated_at`，同时发出一个
`$/dcc.jobUpdated` 通知（如果 `JobNotifier` 已连接）。重新连接后
订阅的客户端因此会看到一个干净的终止转换，而不是一个悬空的
`Running` 作业。

恢复失败会以 `error` 级别记录日志，并且**不会**中止启动 —
进程内映射简单地以空开始，进程继续服务新请求。

## `jobs.cleanup` 内置工具

一个符合 SEP-986 规范的内置 MCP 工具，用于修剪终止作业：

```jsonc
// tools/call
{
  "name": "jobs.cleanup",
  "arguments": { "older_than_hours": 24 }  // 默认值: 24
}
// → { "removed": <count>, "older_than_hours": 24 }
```

- Annotations: `destructive_hint: true`, `idempotent_hint: true`,
  `read_only_hint: false`。
- 只有终止状态（`Completed`、`Failed`、`Cancelled`、`Interrupted`）
  才有资格被移除；无论年龄多大，`Pending` 和 `Running` 行永远不会被移除。
- 对任一已配置的后端都有效 — 内存或 SQLite。

## 存储 Schema

```sql
CREATE TABLE IF NOT EXISTS jobs (
    job_id        TEXT PRIMARY KEY,
    parent_job_id TEXT,
    tool          TEXT NOT NULL,
    status        TEXT NOT NULL,
    progress_json TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    error         TEXT,
    result_json   TEXT
);
CREATE INDEX IF NOT EXISTS jobs_status_idx  ON jobs(status);
CREATE INDEX IF NOT EXISTS jobs_parent_idx  ON jobs(parent_job_id);
CREATE INDEX IF NOT EXISTS jobs_updated_idx ON jobs(updated_at);
```

时间戳以 RFC 3339 UTC 字符串存储。进度和结果 payload 是 JSON
序列化的 — 即使内部 `Job` 字段演进，schema 也保持稳定。

## 运维指南

- **备份**: SQLite 文件是单个路径；标准文件级快照即可。
  跨版本没有 WAL 模式承诺 — 将其视为持久缓存，而非记录系统。
- **增长**: 按计划调用 `jobs.cleanup`（cron、k8s CronJob 或从编排
  agent）。默认 24 小时窗口对大多数交互式使用来说足够。
- **迁移**: 目前没有跨版本迁移。如果未来版本中 schema 改变，
  删除文件让 `JobManager` 重新创建它 — 该文件旨在 survive 重启，
  而非升级。
- **并发**: `SqliteStorage` 通过连接上的 `parking_lot::Mutex` 串行化。
  对于每个 DCC 的服务器来说足够好；不要将多个 `McpHttpServer` 实例
  指向同一个文件。

## 相关 issues

- #316 — 带有 `Pending`/`Running`/`Completed` 状态的异步作业执行
- #318 — `JobManager` 核心
- #326 — `$/dcc.jobUpdated` 通知
- #371 — `jobs.get_status` 工具
- **#328** — 本文档
