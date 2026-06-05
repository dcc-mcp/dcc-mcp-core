# Sentry 错误监控

网关和服务端二进制文件内置了 Sentry SDK 集成，用于实时错误监控。
SDK 会自动捕获 Rust panics、显式错误事件以及选定的 span breadcrumbs。

## 启用的

设置 `DCC_MCP_SENTRY_DSN` 环境变量为你的 Sentry 项目 DSN。
SDK 在服务器启动时初始化，并自动捕获 panics。

```bash
DCC_MCP_SENTRY_DSN="https://<key>@o<org>.ingest.sentry.io/<project>" \
  dcc-mcp-server --app maya
```

## 配置

| 变量                            | 默认值              | 描述                              |
|---------------------------------|---------------------|-----------------------------------|
| `DCC_MCP_SENTRY_DSN`            | （禁用）            | Sentry 项目 DSN                   |
| `DCC_MCP_SENTRY_ENVIRONMENT`    | `production`        | 用于 source-map 过滤的环境标签    |
| `DCC_MCP_SENTRY_RELEASE`        | crate 版本          | 发布标识符（提交关联）            |
| `DCC_MCP_SENTRY_SAMPLE_RATE`    | `1.0`               | 错误事件采样率（`0.0`–`1.0`）    |

当 `DCC_MCP_SENTRY_DSN` 不存在时，SDK 完全跳过初始化，因此零配置部署
没有任何开销。

## 会捕获什么

| 事件类型                | 自动？    | 说明                             |
|-------------------------|-----------|----------------------------------|
| Rust panics             | ✅ 是     | 由 Sentry panic hook 捕获        |
| 显式 `sentry::capture_error` | 手动 | 在 catch 块中使用                |
| 显式 `sentry::capture_message` | 手动 | 用于业务逻辑告警                 |
| Span breadcrumbs        | ✅ 是     | 当 OTLP 追踪也启用时             |
| Webhook 投递失败        | ✅ 是     | 当配置了 webhooks 时             |

## Python 端的错误报告

Python 适配器可以通过 Rust 桥接将异常转发到 Sentry。
使用 `dcc_mcp_core.telemetry` 辅助函数：

```python
from dcc_mcp_core import capture_exception

try:
    # … 技能执行 …
    pass
except Exception as exc:
    capture_exception(exc)
```

## 禁用 Sentry

Sentry crate 默认被编译进二进制文件。要将其从二进制文件中排除：

```bash
# 不使用默认特性构建，然后仅选择需要的特性
cargo build --no-default-features -p dcc-mcp-server
```

当 `DCC_MCP_SENTRY_DSN` 不存在时，SDK 跳过初始化，因此编译进来的 crate
除非配置了 DSN，否则增加的大小可以忽略不计。

## E2E 测试

真实数据摄入的 Sentry E2E 测试由 `sentry_e2e` 特性门控，需要有效的
`DCC_MCP_SENTRY_DSN` 环境变量：

```bash
cargo test --features sentry_e2e --test sentry_e2e
```

这些测试会向你配置的 Sentry 项目发送事件，并端到端验证摄入管道。
除非显式设置 `sentry_e2e` 标志，否则 CI 中会排除这些测试。

## 参见

- [gateway.md](gateway.md) — 网关配置，包括 webhooks 和可观测性
- [observability.md](observability.md) — 指标、OTLP 追踪、Prometheus
- [production-deployment.md](production-deployment.md) — 生产部署检查清单
