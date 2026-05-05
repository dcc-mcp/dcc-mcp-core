# OpenAPI → MCP 挂载助手

通过单个配置块，将任意现有 REST API 暴露为网关中的 MCP 工具（issue #773）。

## 快速开始

```rust
gateway_builder.mount_openapi(
    OpenApiMount::from_url("https://api.example.com/openapi.json")
        .base_url("https://api.example.com")
        .auth(AuthConfig::bearer("$MY_API_TOKEN"))
        .tool_prefix("example"),
)
```

这会为每个 OpenAPI 操作生成一个 MCP 工具。工具名遵循 `{prefix}__{operationId}` 格式；若 `operationId` 缺失，则使用 `{prefix}__{method}_{path_sanitized}`。

## 工作原理

1. **解析 spec** — `OpenApiMount` 获取并解析 OpenAPI 3.x JSON spec
2. **生成工具** — 每个操作生成一个 MCP 工具；`inputSchema` 由 `requestBody` + `parameters` 构建
3. **HTTP 转发** — 调用 `tools/call` 时，将路径/查询/请求体参数映射并转发给后端，同时注入认证 Header

## `OpenApiMount` 构建器

```rust
OpenApiMount::from_url("https://api.example.com/openapi.json")
    // 必填：后端基础 URL
    .base_url("https://api.example.com")
    // 可选：认证转发
    .auth(AuthConfig::bearer("$MY_API_TOKEN"))
    // 可选：所有生成工具名的前缀（避免命名冲突）
    .tool_prefix("example")
```

## 认证配置

| 方法 | 说明 |
|------|------|
| `AuthConfig::bearer("$TOKEN")` | `Authorization: Bearer <token>` — `$ENV_VAR` 引用在调用时解析 |
| `AuthConfig::api_key("X-Api-Key", "$KEY")` | 自定义 Header 注入 |
| `AuthConfig::basic("$USER", "$PASS")` | HTTP Basic 认证（base64 编码） |

环境变量引用（`$VAR_NAME`）在调用时而非挂载时解析。若变量未设置，则省略该 Header。

## 生成的工具 Schema 示例

对于带 JSON 请求体的 `POST /pets` 操作：

```json
{
  "name": "example__createPet",
  "description": "创建新宠物",
  "inputSchema": {
    "type": "object",
    "properties": {
      "name": { "type": "string" },
      "species": { "type": "string" }
    },
    "required": ["name"]
  }
}
```

## 参数映射

| OpenAPI 位置 | 映射方式 |
|-------------|---------|
| `path` 参数（`/pets/{petId}`） | 替换 URL 路径中的占位符 |
| `query` 参数 | 作为 `?key=value` 追加到 URL |
| `requestBody`（JSON） | 序列化为 JSON 请求体 |

## 错误处理

后端返回 HTTP 4xx/5xx 响应时，映射为 `CallError::BackendError { status, body }`。

## 网关注册

```rust
impl GatewayBuilder {
    pub fn mount_openapi(mut self, mount: OpenApiMount) -> Self { ... }
}

// 挂载多个 API
gateway_builder
    .mount_openapi(OpenApiMount::from_url("...").tool_prefix("api1"))
    .mount_openapi(OpenApiMount::from_url("...").tool_prefix("api2"))
```

## 限制

- 仅支持 OpenAPI 3.x JSON spec（YAML 可通过字符串转换）
- `$ref` 解析仅限于单层内联引用
- 尚不支持文件上传（`multipart/form-data`）
- WebSocket / 流式操作不作为 MCP 工具暴露

## 参见

- [gateway.md](gateway.md) — 完整的网关配置参考
- [middleware.md](middleware.md) — 通过 `BeforeCallMiddleware` 添加认证转发策略
- [rest-api-surface.md](rest-api-surface.md) — 每个 DCC 的 REST 技能 API
