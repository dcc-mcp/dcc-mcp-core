"""Authentication helpers for dcc-mcp-core MCP servers (issue #408).

Provides declarative types and helpers for:

1. **API key auth** — Bearer token validation for studio environments.
2. **CIMD (Client ID Metadata Documents)** — OAuth 2.1 automatic client
   registration via a ``/.well-known/oauth-client-metadata`` endpoint.
3. **Auth middleware** — ``validate_bearer_token`` for use inside tool
   handlers or as a pre-dispatch hook.

MCP spec reference (2025-11-25):
<https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization>

Note:
----
The Rust-level ``McpHttpServer`` support for serving the CIMD well-known
endpoint and enforcing Bearer token auth on ``/mcp`` is planned (issue #408).
These types are **declarative configuration objects** that feed the Rust layer
once it is implemented.  Until then, :func:`validate_bearer_token` can be
called manually from Python tool handlers.

Usage
-----
::

    from dcc_mcp_core.auth import OAuthConfig, ApiKeyConfig, validate_bearer_token

    # API key auth (simplest — no OAuth needed)
    import os
    cfg = McpHttpConfig(port=8765)
    cfg.api_key = os.environ.get("DCC_MCP_API_KEY")

    # OAuth 2.1 + CIMD (recommended for production)
    oauth_cfg = OAuthConfig(
        provider_url="https://auth.shotgrid.example.com",
        client_id="dcc-mcp-client",
        scopes=["scene:read", "render:write"],
        client_name="DCC MCP Server",
    )
    cimd_doc = oauth_cfg.to_cimd_document(redirect_uri="http://localhost:8765/oauth/callback")

    # Manual token validation in a tool handler
    def my_secure_tool(params, *, request_headers=None):
        if not validate_bearer_token(
            request_headers or {},
            expected_token=os.environ.get("DCC_MCP_API_KEY"),
        ):
            return {"success": False, "message": "Unauthorized"}
        ...

"""

from __future__ import annotations

import dataclasses
import logging
import os
import secrets
from typing import Any

logger = logging.getLogger(__name__)

__all__ = [
    "ApiKeyConfig",
    "CimdDocument",
    "OAuthConfig",
    "TokenValidationError",
    "generate_api_key",
    "validate_bearer_token",
]


class TokenValidationError(Exception):
    """Raised when a Bearer token fails validation."""


@dataclasses.dataclass
class ApiKeyConfig:
    """Configuration for API-key (Bearer token) auth.

    Args:
        api_key: The expected Bearer token.  ``None`` disables auth
            (development mode — logs a warning on every request).
        env_var: If ``api_key`` is ``None`` and this env var is set, the
            env var value is used as the key at runtime.  Default
            ``"DCC_MCP_API_KEY"``.
        header_name: HTTP header to read from.  Default
            ``"Authorization"`` (reads ``Bearer <key>``).

    Example::

        cfg = ApiKeyConfig(env_var="MY_MCP_SECRET")
        # In practice: set McpHttpConfig.api_key = cfg.resolve()

    """

    api_key: str | None = None
    env_var: str = "DCC_MCP_API_KEY"
    header_name: str = "Authorization"

    def resolve(self) -> str | None:
        """Return the effective API key (field value → env var → None)."""
        if self.api_key:
            return self.api_key
        return os.environ.get(self.env_var)


@dataclasses.dataclass
class CimdDocument:
    """CIMD (Client ID Metadata Document) for OAuth 2.1 client registration.

    Serialise to JSON with :meth:`to_dict` and serve from
    ``GET /.well-known/oauth-client-metadata``.

    Spec: https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization#client-id-metadata-documents
    """

    client_name: str
    redirect_uris: list[str]
    grant_types: list[str] = dataclasses.field(default_factory=lambda: ["authorization_code"])
    response_types: list[str] = dataclasses.field(default_factory=lambda: ["code"])
    token_endpoint_auth_method: str = "none"
    scope: str | None = None
    logo_uri: str | None = None
    client_uri: str | None = None
    contacts: list[str] = dataclasses.field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        """Return the document as a JSON-serialisable dict."""
        doc: dict[str, Any] = {
            "client_name": self.client_name,
            "redirect_uris": self.redirect_uris,
            "grant_types": self.grant_types,
            "response_types": self.response_types,
            "token_endpoint_auth_method": self.token_endpoint_auth_method,
        }
        if self.scope:
            doc["scope"] = self.scope
        if self.logo_uri:
            doc["logo_uri"] = self.logo_uri
        if self.client_uri:
            doc["client_uri"] = self.client_uri
        if self.contacts:
            doc["contacts"] = self.contacts
        return doc


@dataclasses.dataclass
class OAuthConfig:
    """OAuth 2.1 configuration for a remote MCP server.

    Args:
        provider_url: Base URL of the OAuth 2.1 identity provider.
        client_id: Pre-registered client ID (for confidential clients).
            Leave ``None`` for CIMD-based dynamic registration.
        scopes: List of requested OAuth scopes.
        client_name: Human-readable server name shown in the auth dialog.
        redirect_uri: Default redirect URI for the CIMD document.

    Example::

        cfg = OAuthConfig(
            provider_url="https://auth.shotgrid.example.com",
            scopes=["scene:read", "render:write"],
            client_name="DCC MCP Server",
        )
        doc = cfg.to_cimd_document(redirect_uri="http://localhost:8765/oauth/callback")

    """

    provider_url: str
    client_id: str | None = None
    scopes: list[str] = dataclasses.field(default_factory=list)
    client_name: str = "dcc-mcp-server"
    redirect_uri: str | None = None

    @property
    def authorization_endpoint(self) -> str:
        """Derived authorization endpoint URL."""
        base = self.provider_url.rstrip("/")
        return f"{base}/authorize"

    @property
    def token_endpoint(self) -> str:
        """Derived token endpoint URL."""
        base = self.provider_url.rstrip("/")
        return f"{base}/token"

    @property
    def well_known_url(self) -> str:
        """CIMD well-known metadata URL for this server's client registration."""
        base = self.provider_url.rstrip("/")
        return f"{base}/.well-known/oauth-client-metadata"

    def to_cimd_document(
        self,
        redirect_uri: str | None = None,
    ) -> CimdDocument:
        """Build a :class:`CimdDocument` suitable for CIMD registration.

        Args:
            redirect_uri: Override redirect URI (defaults to
                ``self.redirect_uri``).

        Returns:
            :class:`CimdDocument` ready to be JSON-serialised and served
            from ``GET /.well-known/oauth-client-metadata``.

        """
        uri = redirect_uri or self.redirect_uri or "http://localhost:8765/oauth/callback"
        return CimdDocument(
            client_name=self.client_name,
            redirect_uris=[uri],
            scope=" ".join(self.scopes) if self.scopes else None,
        )


# ---------------------------------------------------------------------------
# Token validation helpers
# ---------------------------------------------------------------------------


def validate_bearer_token(
    headers: dict[str, str],
    *,
    expected_token: str | None,
    header_name: str = "Authorization",
) -> bool:
    """Validate a Bearer token from HTTP request headers.

    Returns ``True`` when:

    - ``expected_token`` is ``None`` (auth disabled; logs a warning).
    - The ``Authorization`` header value equals ``Bearer <expected_token>``
      (constant-time comparison to prevent timing attacks).

    Returns ``False`` when:

    - The ``Authorization`` header is missing.
    - The value does not start with ``"Bearer "``.
    - The token does not match ``expected_token``.

    Args:
        headers: HTTP request headers dict (case-insensitive lookup
            attempted automatically).
        expected_token: The valid API key / Bearer token.  ``None``
            disables auth.
        header_name: Header name to check.  Default ``"Authorization"``.

    Raises:
        TokenValidationError: Never raised — all failures return ``False``
            to avoid leaking validation state in exceptions.

    Example::

        ok = validate_bearer_token(
            {"Authorization": "Bearer secret123"},
            expected_token="secret123",
        )
        assert ok is True

    """
    if expected_token is None:
        logger.warning(
            "validate_bearer_token: expected_token is None — auth disabled (dev mode). "
            "Set DCC_MCP_API_KEY or configure OAuthConfig for production."
        )
        return True

    raw = _get_header(headers, header_name)
    if raw is None:
        return False

    if not raw.startswith("Bearer "):
        return False

    provided = raw[len("Bearer ") :]
    return secrets.compare_digest(provided.encode(), expected_token.encode())


def generate_api_key(length: int = 32) -> str:
    """Generate a cryptographically secure API key.

    Args:
        length: Number of random bytes (output is URL-safe base64, so the
            string will be longer than ``length`` chars).  Default ``32``
            yields a 43-character key.

    Returns:
        URL-safe base64-encoded random string.

    Example::

        key = generate_api_key()
        # "xZ3qB2…" — use as DCC_MCP_API_KEY

    """
    return secrets.token_urlsafe(length)


def _get_header(headers: dict[str, str], name: str) -> str | None:
    """Case-insensitive header lookup."""
    for key, value in headers.items():
        if key.lower() == name.lower():
            return value
    return None
