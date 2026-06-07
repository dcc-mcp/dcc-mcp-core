"""Consolidated: the canonical Qt dispatcher source lives at python/dcc_mcp_core/qt_dispatcher.py.

The Rust crate ``dcc-mcp-host-rpc`` embeds the canonical source
directly via::

    include_str!("../../../python/dcc_mcp_core/qt_dispatcher.py")

in ``src/qtserver.rs:DISPATCHER_PY``.

This file is kept as a path-only reference.  Any code that reads it
will get this note, **not** the real dispatcher implementation.  For
the real implementation, see ``python/dcc_mcp_core/qt_dispatcher.py``.

To verify that the embedded source matches the canonical source,
run the tests in ``tests/test_dcc_qt_dispatcher_py.py`` — they
validate the canonical source directly.
"""
