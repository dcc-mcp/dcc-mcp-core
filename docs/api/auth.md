# Auth — API Key and OAuth 2.1 / CIMD

> Source: [`python/dcc_mcp_core/auth.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/auth.py) · Issue [#408](https://github.com/loonghao/dcc-mcp-core/issues/408)
>
> **[中文版](../zh/api/auth.md)**

Declarative authentication configuration for remote MCP servers. Provides
Bearer-token ("API key") auth for studio environments and OAuth 2.1 +
[CIMD (Client ID Metadata Documents)](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization#client-id-metadata-documents)
for production cloud deployments.

**When to use**

- **API Key** — studio / internal network, single shared secret, no identity provider.
- **OAuth / CIMD** — public SaaS, per-user identity, automatic client registration, token refresh via [Managed Agents Vaults](https://www.anthropic.com/news/claude-code-vaults).

## Imports

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

Configuration dataclass for Bearer-token auth.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `api_key` | `str \| None` | `None` | Literal token — overrides `env_var` when set |
| `env_var` | `str` | `"DCC_MCP_API_KEY"` | Env var read at `.resolve()` time |
| `header_name` | `str` | `"Authorization"` | HTTP header (`Bearer <key>` expected) |

```python
cfg = ApiKeyConfig(env_var="MY_MCP_SECRET")
token = cfg.resolve()   # field → env var → None
mcp_cfg.api_key = token
```

## `OAuthConfig`

Declarative OAuth 2.1 configuration. Produces a [`CimdDocument`](#cimddocument)
suitable for serving from `GET /.well-known/oauth-client-metadata`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `provider_url` | `str` | — | Base URL of the OAuth provider |
| `client_id` | `str \| None` | `None` | Pre-registered client ID (leave `None` for CIMD dynamic registration) |
| `scopes` | `list[str]` | `[]` | Requested OAuth scopes |
| `client_name` | `str` | `"dcc-mcp-server"` | Human-readable server name shown in auth dialog |
| `redirect_uri` | `str \| None` | `None` | Default redirect URI for CIMD |

**Derived URLs** (read-only properties):

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

[Client ID Metadata Document](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization#client-id-metadata-documents)
for dynamic client registration. Serialise with `.to_dict()` and serve JSON
from `/.well-known/oauth-client-metadata`.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `client_name` | `str` | — | Shown in consent dialog |
| `redirect_uris` | `list[str]` | — | Must include every callback used |
| `grant_types` | `list[str]` | `["authorization_code"]` | |
| `response_types` | `list[str]` | `["code"]` | |
| `token_endpoint_auth_method` | `str` | `"none"` | PKCE-only public client |
| `scope` | `str \| None` | `None` | Space-separated |
| `logo_uri`, `client_uri`, `contacts` | optional | — | Cosmetics for consent screen |

## `validate_bearer_token(headers, *, expected_token, header_name="Authorization") -> bool`

Constant-time Bearer-token check for use inside pure-Python tool handlers.

- Returns `True` when `expected_token is None` (auth disabled, logs a warning).
- Returns `True` when the header equals `Bearer <expected_token>`.
- Returns `False` on missing header, wrong scheme, or mismatch.
- Uses [`secrets.compare_digest`](https://docs.python.org/3/library/secrets.html#secrets.compare_digest) to prevent timing attacks.
- Case-insensitive header lookup.

```python
from dcc_mcp_core import validate_bearer_token

def secure_handler(params, *, request_headers=None):
    if not validate_bearer_token(request_headers or {}, expected_token=os.environ["DCC_MCP_API_KEY"]):
        return {"success": False, "message": "Unauthorized"}
    ...
```

## `generate_api_key(length: int = 32) -> str`

Cryptographically secure URL-safe base64 token (`secrets.token_urlsafe`).
`length=32` produces a 43-character string.

```python
from dcc_mcp_core import generate_api_key
token = generate_api_key()            # "xZ3qB2..." — use as DCC_MCP_API_KEY
```

## Integration path

Today these types are **declarative configuration objects** consumed either
by (a) Python tool handlers calling `validate_bearer_token` directly, or
(b) the `McpHttpConfig.api_key` field (Bearer-token path, supported now).

Full Rust-side enforcement of the `/.well-known/oauth-client-metadata`
endpoint and the `/mcp` Bearer check is tracked in issue
[#408](https://github.com/loonghao/dcc-mcp-core/issues/408). API keys work
today; OAuth is opt-in via `McpHttpConfig(enable_oauth=True)` once the
Rust layer lands.

## See also

- [Remote Server guide](../guide/remote-server.md) — end-to-end deployment
- [`McpHttpConfig.api_key`](./http.md) — how the server consumes the token
- [MCP authorization spec](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
