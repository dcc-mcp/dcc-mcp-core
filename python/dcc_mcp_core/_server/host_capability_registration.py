"""Host capability tool registration for :class:`DccServerBase`.

This collaborator owns the adapter provider lifecycle and the standard
``dcc_host_capabilities`` backend tool. ``DccServerBase`` keeps thin
compatibility wrappers and the existing provider-owned state attributes.
"""

from __future__ import annotations

from typing import Any
from typing import Callable

from dcc_mcp_core.capabilities import build_unconfigured_host_capabilities
from dcc_mcp_core.capabilities import register_host_capability_tool


class HostCapabilityRegistration:
    """Register and rebind one server's host capability tool."""

    def __init__(self, server: Any, *, package_version: str) -> None:
        self._server = server
        self._package_version = package_version

    def set_provider(
        self,
        provider: Callable[[], dict[str, Any]] | None,
        *,
        cache: Any | None = None,
        provider_cache_key: Callable[[], tuple[str | None, str, str]] | None = None,
    ) -> None:
        """Store the adapter provider configuration and rebind its handler."""
        self._server._host_capability_provider = provider
        self._server._host_capability_cache = cache
        self._server._host_capability_provider_cache_key = provider_cache_key
        self.register()

    def register(self) -> bool:
        """Register the tool once and bind it to the current provider."""
        provider = self._server._host_capability_provider
        if provider is None:
            provider = self._unconfigured_provider()

        self._server._host_capability_registration = register_host_capability_tool(
            self._server,
            dcc_name=self._server._dcc_name,
            provider=provider,
            cache=self._server._host_capability_cache,
            provider_cache_key=self._server._host_capability_provider_cache_key,
        )
        return self._server._host_capability_registration

    def _unconfigured_provider(self) -> Callable[[], dict[str, Any]]:
        def provider() -> dict[str, Any]:
            return build_unconfigured_host_capabilities(
                dcc_name=self._server._dcc_name,
                host_version=self._server._version_string(),
                adapter_version=str(
                    getattr(
                        getattr(self._server._options, "sidecar", None),
                        "adapter_version",
                        None,
                    )
                    or self._package_version
                ),
            )

        return provider


class HostCapabilityRegistrationMixin:
    """Compatibility facade exposing host capability registration on a server."""

    def _initialize_host_capability_registration(self, package_version: str) -> None:
        self._host_capability_package_version = package_version
        self.register_host_capability_tool()

    def _register_host_capabilities(self) -> HostCapabilityRegistration:
        controller = self.__dict__.get("_host_capability_registration_controller")
        if controller is None:
            controller = HostCapabilityRegistration(
                self,
                package_version=getattr(self, "_host_capability_package_version", "0.0.0-dev"),
            )
            self._host_capability_registration_controller = controller
        return controller

    def set_host_capability_provider(
        self,
        provider: Callable[[], dict[str, Any]] | None,
        *,
        cache: Any | None = None,
        provider_cache_key: Callable[[], tuple[str | None, str, str]] | None = None,
    ) -> None:
        """Set the adapter provider and immediately rebind its MCP handler."""
        self._register_host_capabilities().set_provider(
            provider,
            cache=cache,
            provider_cache_key=provider_cache_key,
        )

    def register_host_capability_tool(self) -> bool:
        """Register or rebind the standard host capability MCP tool."""
        return self._register_host_capabilities().register()
