"""Sidecar bootstrap injected into Maya at ``commandPort`` connect time.

The Rust ``CommandPortClient`` (see ``commandport.rs``) embeds this
file's source at build time via ``include_str!`` and ships it as the
**first** payload sent over a freshly opened ``commandPort``. Maya
evaluates the wrapping ``exec(compile(<src>, ...))`` on its main
thread and the body below runs synchronously inside Maya's
interpreter.

What it does
============

Installs ``dcc_mcp_maya._sidecar`` as a **virtual module** that
re-exports the dispatcher already shipped at
``dcc_mcp_maya.sidecar._dispatcher``. The wire frame the sidecar
binary subsequently sends per ``tools/call`` is::

    __import__('dcc_mcp_maya._sidecar', fromlist=['dispatch']).dispatch(
        {"action": ..., "args": ..., "request_id": ...}
    )

so the only thing this bootstrap needs to guarantee is that
``dcc_mcp_maya._sidecar.dispatch`` resolves to the correct callable
**before** the first such wire frame arrives.

Why a virtual module instead of a static ``.py`` file
=====================================================

* **Wire-format authority belongs to the binary.** The Rust sidecar
  knows the exact entry-point name it will call. By installing that
  name dynamically, a Maya-side install that lacks the leaf shim file
  cannot break dispatch — sidecar mode "just works" as long as the
  dispatcher proper is importable.

* **Version skew is impossible by construction.** The bootstrap's
  ``_BOOTSTRAP_VERSION`` is checked against ``__dcc_mcp_bootstrap__``
  on the installed module. A reconnect from a newer sidecar binary
  overwrites the older install; a reconnect from the same version is
  a true no-op.

* **No file-layout coupling.** Adapter authors are free to refactor
  the dispatcher inside ``dcc_mcp_maya.sidecar`` without breaking
  third-party wire clients — the bootstrap is the only consumer of
  the dispatcher's import path.

Failure semantics
=================

Bootstrap **never raises** out of Maya's eval. The function returns
silently in two benign cases:

* ``dcc_mcp_maya`` is not installed (no sidecar mode available).
* The module is already installed at this exact bootstrap version
  (re-entrant connect).

When ``dcc_mcp_maya`` is present but
``dcc_mcp_maya.sidecar._dispatcher`` is not importable, bootstrap
installs a fallback virtual module. The first real ``dispatch()``
call then returns a structured ``sidecar-dispatcher-unavailable``
envelope so the gateway sees an MCP-shaped error rather than a
transport-error from Maya's eval.
"""

import json
import sys
import types

_BOOTSTRAP_VERSION = "1"
_MODULE_NAME = "dcc_mcp_maya._sidecar"
_PARENT_NAME = "dcc_mcp_maya"


def _payload_field(payload, key):
    if isinstance(payload, dict):
        value = payload.get(key)
        if value is None:
            return None
        return str(value)
    return None


def _failure_envelope(reason, payload):
    return {
        "success": False,
        "error": "sidecar-dispatcher-unavailable",
        "message": (
            "dcc_mcp_maya.sidecar._dispatcher is not importable; "
            "install or upgrade dcc-mcp-maya with sidecar dispatcher support."
        ),
        "context": {
            "kind": "sidecar_dispatcher_unavailable",
            "reason": reason,
            "action": _payload_field(payload, "action"),
            "request_id": _payload_field(payload, "request_id"),
        },
    }


def _make_fallback_dispatch(reason):
    def dispatch(payload):
        return json.dumps(_failure_envelope(reason, payload), sort_keys=True)

    def dispatch_payload(payload, **_kwargs):
        return _failure_envelope(reason, payload)

    return dispatch, dispatch_payload


def _install():
    existing = sys.modules.get(_MODULE_NAME)
    if existing is not None and getattr(existing, "__dcc_mcp_bootstrap__", None) == _BOOTSTRAP_VERSION:
        return

    try:
        from dcc_mcp_maya.sidecar._dispatcher import dispatch
        from dcc_mcp_maya.sidecar._dispatcher import dispatch_payload
    except ImportError as exc:
        dispatch, dispatch_payload = _make_fallback_dispatch(str(exc))

    parent = sys.modules.get(_PARENT_NAME)
    if parent is None:
        return

    module = types.ModuleType(_MODULE_NAME)
    module.__file__ = "<dcc-mcp-sidecar-bootstrap>"
    module.__dcc_mcp_bootstrap__ = _BOOTSTRAP_VERSION
    module.dispatch = dispatch
    module.dispatch_payload = dispatch_payload

    sys.modules[_MODULE_NAME] = module
    parent._sidecar = module


_install()
