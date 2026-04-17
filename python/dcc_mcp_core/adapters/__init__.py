"""DCC-adapter base classes for non-traditional hosts.

This sub-package collects standalone, pure-Python adapter templates that do
**not** inherit from :class:`dcc_mcp_core.DccServerBase`.  They are intended
for hosts whose capability profile is narrower than a full DCC (Maya, Blender,
…) — most notably WebView / browser-based tool panels.

Adapters here are deliberately small, opinionated, and designed to be
sub-classed by integration projects (e.g. *AuroraView*).
"""

from __future__ import annotations

# Local folder imports
from dcc_mcp_core.adapters.webview import CAPABILITY_KEYS
from dcc_mcp_core.adapters.webview import WEBVIEW_DEFAULT_CAPABILITIES
from dcc_mcp_core.adapters.webview import WebViewAdapter
from dcc_mcp_core.adapters.webview import WebViewContext

__all__ = [
    "CAPABILITY_KEYS",
    "WEBVIEW_DEFAULT_CAPABILITIES",
    "WebViewAdapter",
    "WebViewContext",
]
