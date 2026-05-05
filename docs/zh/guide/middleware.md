# 网关中间件链

网关支持可插拔的 `BeforeCall` / `AfterCall` 中间件链，应用于每一次 `tools/call` 分发（issue #770）。

## 快速开始

```rust
use dcc_mcp_gateway::gateway::middleware::{
    AuditMiddleware, MiddlewareChain, QuotaMiddleware, RedactionMiddleware,
};
use std::sync::Arc;

let config = GatewayConfig {
    middleware_chain: MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::default()))
        .with_before(Arc::new(QuotaMiddleware::new(100)))  // 每分钟 100 次
        .with_before(Arc::new(RedactionMiddleware::new(["api_key", "token"]))),
    ..GatewayConfig::default()
};
```

## 内置中间件

| 中间件 | 用途 | 失败行为 |
|--------|------|---------|
| `AuditMiddleware` | 每次 `tools/call` 时输出一条 `tracing::info!` 结构化日志，包含方法、工具名、DCC 类型、会话 ID、耗时及结果 | 永不阻断 |
| `QuotaMiddleware::new(N)` | 全局每分钟最多 N 次调用；超出时返回 `MiddlewareError::QuotaExceeded` | 中止调用，返回 429 等效错误 |
| `RedactionMiddleware::new(fields)` | 将匹配字段的参数值替换为 `"[REDACTED]"`，防止日志或转发中泄露敏感信息 | 永不失败 |

## 自定义中间件

实现 `BeforeCallMiddleware` 或 `AfterCallMiddleware` trait：

```rust
use dcc_mcp_gateway::gateway::middleware::{
    AfterCallMiddleware, BeforeCallMiddleware, CallContext, CallResult, MiddlewareError,
};

pub struct MyMiddleware;

#[async_trait::async_trait]
impl BeforeCallMiddleware for MyMiddleware {
    async fn before_call(&self, ctx: &mut CallContext) -> Result<(), MiddlewareError> {
        // 在调用分发前检查或修改 ctx
        tracing::info!(tool = ?ctx.tool_slug, dcc = ?ctx.dcc_type, "自定义前置中间件");
        Ok(())
    }
}

#[async_trait::async_trait]
impl AfterCallMiddleware for MyMiddleware {
    async fn after_call(
        &self,
        ctx: &mut CallContext,
        result: &mut CallResult,
    ) -> Result<(), MiddlewareError> {
        tracing::info!(success = result.success, "自定义后置中间件");
        Ok(())
    }
}
```

注册方式：

```rust
MiddlewareChain::new()
    .with_before(Arc::new(MyMiddleware))
    .with_after(Arc::new(MyMiddleware))
```

## `CallContext` 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `method` | `String` | MCP 方法名（`tools/call`、`tools/list` 等） |
| `tool_slug` | `Option<String>` | 工具名称 |
| `dcc_type` | `Option<String>` | DCC 类型（`maya`、`blender` 等） |
| `session_id` | `Option<String>` | MCP 会话 ID |
| `request_id` | `String` | 每次请求的唯一 ID（与审计日志 `request_id` 对应） |
| `args` | `serde_json::Value` | 工具参数（可变；`RedactionMiddleware` 会修改此字段） |
| `metadata` | `HashMap<String, String>` | 中间件间传递数据的通用容器 |

## 执行顺序

```
请求 → BeforeCall[0] → BeforeCall[1] → ... → 分发 → AfterCall[0] → AfterCall[1] → 响应
```

若任意 `before_call` 返回 `Err`，调用将被中止，后续中间件不再执行。

## 与 Admin UI 集成

`AuditMiddleware` 会填充 `/admin/api/calls` 数据源。同时开启两者可在仪表盘中查看实时调用历史：

```rust
GatewayConfig {
    admin_enabled: true,
    middleware_chain: MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::default())),
    ..GatewayConfig::default()
}
```

## 参见

- [admin-ui.md](admin-ui.md) — 消费审计数据的仪表盘
- [gateway.md](gateway.md) — 完整的网关配置参考
- [observability.md](observability.md) — OTLP 追踪（中间件可通过 `CallContext` 丰富 span 属性）
