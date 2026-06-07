"""Bootstrap installer for the universal Qt-event-loop dispatcher.

Shipped over a one-shot `commandPort` (or any equivalent one-shot
Python eval surface) so the Rust sidecar can transition a DCC from
"no in-DCC server yet" to "qtserver://host:port reachable" without
requiring the dispatcher Python source to exist on disk.

How the Rust client uses this file
==================================

1. ``include_str!("../../../python/dcc_mcp_core/qt_dispatcher.py")`` — embed
   the canonical ``dcc_mcp_core.qt_dispatcher`` source directly from the
   workspace root (the single source of truth).
2. ``include_str!("../python/dcc_qt_dispatcher_bootstrap.py")`` —
   embed this file's source.
3. Send **one** ``commandPort`` line that wraps both:

   .. code:: python

       __import__('builtins').exec(
           __import__('builtins').compile(
               '_DISPATCHER_SOURCE = ' + repr(dispatcher_src) + chr(10) +
               '_REQUESTED_PORT = 0' + chr(10) +
               bootstrap_src,
               '<dcc-mcp-qt-bootstrap>', 'exec'))

   ``commandPort``'s reply is the ``repr`` of the eval result, which
   is ``None`` (``exec`` returns ``None``). The bootstrap result is
   captured separately in the next round-trip:

4. Send a follow-up line:

   .. code:: python

       __import__('json').dumps(
           __import__('_dcc_qt_dispatcher').start_qt_server(
               port=<requested_port>))

   The reply is a JSON dict-compatible server handle containing ``{"host",
   "port", "url", "qt_binding", "dispatcher_version", "reused"}`` — the
   client parses ``port`` and reconnects to ``qtserver://<host>:<port>`` for
   all future calls.

Why a separate file
===================

Splitting bootstrap (this file) from the dispatcher source
(``dcc_mcp_core.qt_dispatcher``) lets each be unit-tested with stock Python
in CI — the dispatcher's pure-Python parts (``_DispatchRegistry``,
``execute`` semantics) run without Qt; the bootstrap's installer
logic runs without any TCP socket. Both are then re-tested by the
``QtServerClient`` integration tests with a real Qt server in the
loop.

Failure semantics
=================

Bootstrap never raises out of ``commandPort``'s eval — exceptions are
captured into ``_install_result["error"]`` and the dispatcher module
is **not** registered in ``sys.modules`` on failure. The follow-up
``start_qt_server`` call then surfaces the failure as a clean
JSON-line error envelope on the wire, which the Rust client maps
to :class:`HostRpcError::TransportError`.
"""

import sys
import types

_BOOTSTRAP_VERSION = "1"
_MODULE_NAME = "_dcc_qt_dispatcher"


def _install(source):
    """Synthesise ``_dcc_qt_dispatcher`` from ``source`` and register it in ``sys.modules``.

    Returns the module on success or a dict describing the failure
    (kept narrow so the result still serialises to JSON for the Rust
    client).
    """
    existing = sys.modules.get(_MODULE_NAME)
    if existing is not None and getattr(existing, "__dcc_mcp_bootstrap__", None) == _BOOTSTRAP_VERSION:
        return existing
    module = types.ModuleType(_MODULE_NAME)
    module.__file__ = "<dcc-mcp-qt-dispatcher>"
    module.__dcc_mcp_bootstrap__ = _BOOTSTRAP_VERSION
    try:
        compiled = compile(source, module.__file__, "exec")
    except SyntaxError as exc:
        return {
            "ok": False,
            "stage": "compile",
            "error": f"{exc.__class__.__name__}: {exc}",
        }
    try:
        exec(compiled, module.__dict__)
    except Exception as exc:
        return {
            "ok": False,
            "stage": "exec",
            "error": f"{exc.__class__.__name__}: {exc}",
        }
    sys.modules[_MODULE_NAME] = module
    return module


# The Rust client injects ``_DISPATCHER_SOURCE`` (string) and
# ``_REQUESTED_PORT`` (int) into ``globals()`` before exec'ing this
# file. ``__name__`` is left at module default ``__main__`` because
# the Rust caller uses an empty namespace.
_install_result = _install(_DISPATCHER_SOURCE)  # noqa: F821 — injected
