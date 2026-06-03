# 每个 DCC 的 REST 技能 API

> Issue 参考：[#658](https://github.com/dcc-mcp/dcc-mcp-core/issues/658) ·
> [#660](https://github.com/dcc-mcp/dcc-mcp-core/issues/660) ·
> 总揽 [#657](https://github.com/dcc-mcp/dcc-mcp-core/issues/657)

MCP 网关**并非**唯一能让外部调用 DCC 技能的方式。每个嵌入 DCC 进程的 `McpHttpServer` 也可以将已发现的技能暴露为一个小型、带版本的 REST 接口，挂载在 `/v1/*` 路径下。网关随后**索引并路由**到这些每个 DCC 的服务，而不是将每个后端 Action 单独发布为 MCP 工具。

## 为什么这样设计

| 仅使用网关暴露的问题 | REST 接口如何解决 |
|---------------------|-----------------|
| `tools/list` 随 `实例数 × Action 数` 线性增长 | REST 接口有界——仅 9 条固定路由 |
| MCP 工具名必须符合 `^[A-Za-z0-9_-]{1,64}$` | REST slug 使用 `<dcc>.<skill>.<action>` |
| 非 MCP Agent 调用 DCC 代码需要额外适配器 | 直接 `POST /v1/call` 加 JSON 请求体 |
| "技能未加载"没有结构化错误类 | `ServiceErrorKind::SkillNotLoaded`（kebab-case） |

## 路由

| 方法 | 路径 | 用途 |
|------|------|------|
| GET  | `/v1/healthz` | 存活检查 |
| GET  | `/v1/readyz`  | 三状态就绪检查（进程/分发器/DCC） |
| GET  | `/v1/openapi.json` | utoipa 生成的 OpenAPI 3.x 契约 |
| GET  | `/v1/skills`  | 已加载的技能/Action 列表 |
| POST | `/v1/search`  | 紧凑的关键词/tag/dcc/scope 搜索 |
| POST | `/v1/describe` | 获取某个 slug 的 Schema + 注解 |
| GET  | `/v1/tools/{slug}` | describe 的别名 |
| POST | `/v1/call`    | 按 slug 调用一个工具 |
| GET  | `/v1/context` | 当前 DCC 场景/文档快照 |

## SOLID 分层

```text
SkillRestRouter   ← axum 薄适配层
       │
SkillRestService  ← 纯业务逻辑，无 axum 依赖
   │  │  │  │
   ▼  ▼  ▼  ▼
SkillCatalogSource  ToolInvoker  AuthGate  AuditSink
   (trait)          (trait)      (trait)    (trait)
```

每个协作者都是一个 **trait**，适配器（Maya/Blender/Houdini）可以替换自己的实现，而无需修改路由器。默认接线：

- `SkillCatalog`（`dcc-mcp-skills`）作为目录来源
- `ToolDispatcher`（`dcc-mcp-actions`）负责调用
- `AllowLocalhostGate` 负责认证（仅允许回环地址）
- `NoopAuditSink` 负责审计

## Token 效率

`/v1/search` 的命中结果**故意省略** `input_schema`。回归测试断言每个序列化后的 `SkillListEntry` 不超过 `SEARCH_HIT_BUDGET_BYTES`（当前为 512 字节），Agent 每轮可以翻页数百个能力，不会消耗过多上下文。Schema 通过 `POST /v1/describe` 的 `include_schema: true`（默认值）按需获取。

## 企业级控制（#660）

- **版本化路径** — `/v1/*` 是稳定契约
- **结构化错误** — 单一信封 `{kind, message, hint, request_id, candidates?}`，`kind` 为 kebab-case（`unknown-slug`、`ambiguous`、`skill-not-loaded`、`invalid-params`、`unauthorized`、`bad-request`、`affinity-violation`、`not-ready`、`host-busy`、`backend-error`、`internal`）
- **认证门** — 可插拔 `AuthGate`。默认 `AllowLocalhostGate` 拒绝非回环地址。通过安装 `BearerTokenGate::new(vec![token])` 并将监听器绑定到非回环接口来启用远程调用
- **审计 Sink** — 每次调用发出一个 `AuditEvent`（`{request_id, at, slug, route, subject, outcome, duration_ms}`）
- **三状态就绪** — `进程 / 分发器 / DCC`。在所有三项均为绿色之前，`/v1/call` 返回 `503 not-ready`
- **OpenAPI** — 由 `utoipa` 从请求/响应类型的 `ToSchema` derive 自动生成，无需手动维护 JSON

## 接线示例

```rust
use std::sync::Arc;
use axum::Router;
use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
use dcc_mcp_skill_rest::{
    AllowLocalhostGate, BearerTokenGate, NoopAuditSink, SkillRestConfig,
    SkillRestService, StaticReadiness, build_skill_rest_router,
};
use dcc_mcp_skills::SkillCatalog;

fn build_dcc_app(
    registry: Arc<ToolRegistry>,
    dispatcher: Arc<ToolDispatcher>,
) -> Router {
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));
    let service = SkillRestService::from_catalog_and_dispatcher(catalog, dispatcher);
    let cfg = SkillRestConfig::new(service)
        .with_auth(Arc::new(AllowLocalhostGate::new()))
        .with_audit(Arc::new(NoopAuditSink))
        .with_readiness(Arc::new(StaticReadiness::fully_ready()));
    build_skill_rest_router(cfg)
}
```

## 调用模式

```bash
# 1. 搜索紧凑命中
curl -s localhost:8765/v1/search -d '{"query":"sphere"}' -H 'content-type: application/json'

# 2. 获取某个 slug 的 schema
curl -s localhost:8765/v1/describe \
  -d '{"tool_slug":"maya.spheres.create_sphere","include_schema":true}' \
  -H 'content-type: application/json'

# 3. 调用
curl -s localhost:8765/v1/call \
  -d '{"tool_slug":"maya.spheres.create_sphere","params":{"radius":1.5}}' \
  -H 'content-type: application/json'
```
