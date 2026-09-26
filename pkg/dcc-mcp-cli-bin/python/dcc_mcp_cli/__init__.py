"""PyPI wrapper that installs and runs the ``dcc-mcp-cli`` binary.

``pip install dcc-mcp-cli`` (or ``uvx dcc-mcp-cli``) puts a ``dcc-mcp-cli``
command on ``PATH``. The wheel carries the compressed release archive of the
Rust binary for the installing platform and unpacks it into the environment's
scripts directory on first use, so the executing process is the real binary.

Most users never import this package. Programmatic callers can use
:func:`binary_path` to spawn the binary without searching ``PATH``::

    import subprocess
    from dcc_mcp_cli import binary_path

    subprocess.run([str(binary_path()), "--version"])
"""

from __future__ import annotations

from ._bootstrap import PACKAGE_MANAGER_MARKER
from ._bootstrap import PayloadError
from ._bootstrap import binary_path
from ._bootstrap import main

__all__ = [
    "PACKAGE_MANAGER_MARKER",
    "PayloadError",
    "__version__",
    "binary_path",
    "main",
]

__version__ = "0.20.36"  # x-release-please-version
