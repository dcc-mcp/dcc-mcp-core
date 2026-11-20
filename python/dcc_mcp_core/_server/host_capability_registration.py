"""Host capability tool registration for :class:`DccServerBase`.

This collaborator owns the adapter provider lifecycle and the standard
``dcc_host_capabilities`` backend tool. ``DccServerBase`` keeps thin
compatibility wrappers and the existing provider-owned state attributes.
"""

from __future__ import annotations

from typing import Any
from typing import Callable

from dcc_mcp_core.capabilities import CapabilityProbeRunner
from dcc_mcp_core.capabilities import build_unconfigured_host_capabilities
from dcc_mcp_core.host.capabilities import HostCapabilityCache
from dcc_mcp_core.host.capabilities import register_capability_tools


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
        probe_runner: CapabilityProbeRunner | None = None,
        probe_timeout_secs: float = 5.0,
    ) -> None:
        """Store the adapter provider configuration and rebind its handler."""
        self._server._host_capability_provider = provider
        self._server._host_capability_cache = cache
        self._server._host_capability_provider_cache_key = provider_cache_key
        if probe_runner is not None and not hasattr(probe_runner, "run_all"):
            probe_runner = CapabilityProbeRunner(
                probe_runner,
                timeout_secs=probe_timeout_secs,
            )
        self._server._host_capability_probe_runner = probe_runner
        self.register()

    def register(self) -> bool:
        """Register the tool once and bind it to the current provider."""
        provider = self._server._host_capability_provider
        if provider is None:
            provider = self._unconfigured_provider()

        provider_cache_key = self._server._host_capability_provider_cache_key or self._default_cache_key

        self._server._host_capability_registration = register_capability_tools(
            self._server,
            dcc_name=self._server._dcc_name,
            provider=provider,
            cache=self._server._host_capability_cache,
            provider_cache_key=provider_cache_key,
            probe_runner=getattr(self._server, "_host_capability_probe_runner", None),
        )
        return self._server._host_capability_registration

    def _default_cache_key(self) -> tuple[str | None, str, str]:
        sidecar = getattr(getattr(self._server, "_options", None), "sidecar", None)
        adapter_version = str(getattr(sidecar, "adapter_version", None) or self._package_version)
        return (self._server._dcc_name, str(self._server._version_string()), adapter_version)

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
        probe_runner: CapabilityProbeRunner | None = None,
        probe_timeout_secs: float = 5.0,
    ) -> None:
        """Set the adapter provider and immediately rebind its MCP handler."""
        self._register_host_capabilities().set_provider(
            provider,
            cache=cache,
            provider_cache_key=provider_cache_key,
            probe_runner=probe_runner,
            probe_timeout_secs=probe_timeout_secs,
        )

    def register_host_capability_tool(self) -> bool:
        """Register or rebind the standard host capability MCP tools."""
        return self._register_host_capabilities().register()

    def invalidate_host_capability_cache(self) -> None:
        """Discard cached probe records; the next call re-reads all probes."""
        cache = getattr(self, "_host_capability_cache", None)
        if cache is not None:
            provider_cache_key = getattr(self, "_host_capability_provider_cache_key", None)
            if not callable(provider_cache_key):
                controller = self._register_host_capabilities()
                provider_cache_key = controller._default_cache_key
            try:
                cache.invalidate(HostCapabilityCache.cache_key(*provider_cache_key()))
            except Exception:
                cache.invalidate()
