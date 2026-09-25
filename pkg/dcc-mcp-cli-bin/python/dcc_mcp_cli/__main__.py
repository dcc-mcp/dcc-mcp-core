"""Allow ``python -m dcc_mcp_cli`` to run the bundled CLI."""

from __future__ import annotations

import sys

from ._bootstrap import main

if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
