# Auth — API Key 与 OAuth 2.1 / CIMD

> 源码：[`python/dcc_mcp_core/auth.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/auth.py) · Issue [#408](https://github.com/loonghao/dcc-mcp-core/issues/408)
>
> **[English](../../api/auth.md)**

远程 MCP 服务器的声明式认证配置。提供面向工作室/内网的 Bearer-token（API Key）认证，以及面向公网 SaaS 部署的 OAuth 2.1 + [CIMD（Client ID Metadata Documents）](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization#client-id-metadata-documents) 动态客户端注册。

**如何选择**

- **API Key** — 工作室/内网，单一共享密钥，无外部身份提供方。
- **OAuth / CIMD** — 公网 SaaS，按用户身份鉴权，自动客户端注册，可通过 [Managed Agents Vaults](https://www.anthropic.com/news/claude-code-vaults) 刷新 Token。

## 导入

```python
from dcc_mcp_core import (
    ApiKeyConfig,
    OAuthConfig,
    CimdDocument,
    TokenValidationError,
    generate_api_key,
    validate_bearer_token,
)
```

## `ApiKeyConfig`

Bearer Token 认证配置数据类。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `api_key` | `str \| None` | `None` | 直接传入密钥；设置后会覆盖 `env_var` |
| `env_var` | `str` | `"DCC_MCP_API_KEY"` | 调用 `.resolve()` 时读取的环境变量名 |
| `header_name` | `str` | `"Authorization"` | 期望 `Bearer <key>` 形式的 HTTP 头 |

```python
cfg = ApiKeyConfig(env_var="MY_MCP_SECRET")
token = cfg.resolve()   # 字段 → 环境变量 → None
mcp_cfg.api_key = token
```

## `OAuthConfig`

OAuth 2.1 声明式配置。可生成 [`CimdDocument`](#cimddocument)，用于从 `GET /.well-known/oauth-client-metadata` 提供服务。

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `provider_url` | `str` | — | OAuth 服务商基础 URL |
| `client_id` | `str \| None` | `None` | 预注册 client ID；`None` 时走 CIMD 动态注册 |
| `scopes` | `list[str]` | `[]` | 申请的 OAuth scope |
| `client_name` | `str` | `"dcc-mcp-server"` | 授权对话框中显示的名称 |
| `redirect_uri` | `str \| None` | `None` | CIMD 默认回调 URI |

**派生只读属性**

- `authorization_endpoint` → `<provider>/authorize`
- `token_endpoint` → `<provider>/token`
- `well_known_url` → `<provider>/.well-known/oauth-client-metadata`

```python
oauth = OAuthConfig(
    provider_url="https://auth.shotgrid.example.com",
    scopes=["scene:read", "render:write"],
    client_name="Maya MCP Server",
)
doc = oauth.to_cimd_document(redirect_uri="http://localhost:8765/oauth/callback")
```

## `CimdDocument`

[Client ID Metadata Document](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization#client-id-metadata-documents) — 动态客户端注册元数据。调用 `.to_dict()` 序列化为 JSON，通过 `/.well-known/oauth-client-metadata` 提供。

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `client_name` | `str` | — | 授权对话框显示名 |
| `redirect_uris` | `list[str]` | — | 所有可能用到的回调 URI |
| `grant_types` | `list[str]` | `["authorization_code"]` | |
| `response_types` | `list[str]` | `["code"]` | |
| `token_endpoint_auth_method` | `str` | `"none"` | 仅 PKCE 的 public client |
| `scope` | `str \| None` | `None` | 空格分隔 |
| `logo_uri` / `client_uri` / `contacts` | 可选 | — | 授权页装饰 |

## `validate_bearer_token(headers, *, expected_token, header_name="Authorization") -> bool`

纯 Python 工具处理器中可直接使用的 Bearer Token 校验器，防时序攻击。

- `expected_token is None` 时返回 `True`（鉴权关闭，记录警告）。
- 头部等于 `Bearer <expected_token>` 时返回 `True`。
- 头部缺失/方案错误/不匹配时返回 `False`。
- 使用 [`secrets.compare_digest`](https://docs.python.org/3/library/secrets.html#secrets.compare_digest) 做定长比较。
- 请求头名称大小写不敏感。

```python
from dcc_mcp_core import validate_bearer_token

def secure_handler(params, *, request_headers=None):
    if not validate_bearer_token(request_headers or {}, expected_token=os.environ["DCC_MCP_API_KEY"]):
        return {"success": False, "message": "Unauthorized"}
    ...
```

## `generate_api_key(length: int = 32) -> str`

基于 `secrets.token_urlsafe` 的强随机 URL-safe base64 字符串。`length=32` 产出 43 字符。

## 落地现状

目前这些类型是**声明式配置对象**，服务于：(a) Python 工具处理器直接调用 `validate_bearer_token`；(b) `McpHttpConfig.api_key` 字段（Bearer Token 路径，当前已可用）。

Rust 侧对 `/.well-known/oauth-client-metadata` 端点以及 `/mcp` 请求 Bearer 检查的完整支持，跟踪于 issue [#408](https://github.com/loonghao/dcc-mcp-core/issues/408)。API Key 路径立即可用；OAuth 路径在 Rust 层落地后通过 `McpHttpConfig(enable_oauth=True)` 开启。

## 参见

- [远程服务器指南](../guide/remote-server.md)
- [`McpHttpConfig.api_key`](./http.md)
- [MCP 认证规范](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
