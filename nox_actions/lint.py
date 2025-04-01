# Import third-party modules
import nox


def lint(session: nox.Session) -> None:
    session.install("isort", "ruff")
    session.run("isort", "--check-only", "dcc_mcp_core", "tests", "nox_actions")
    session.run("ruff", "check", "dcc_mcp_core", "tests", "nox_actions")


def lint_fix(session: nox.Session) -> None:
    session.install("isort", "ruff", "pre-commit", "autoflake")
    session.run("ruff", "format", "dcc_mcp_core", "tests", "nox_actions")
    session.run("ruff", "check", "--fix", "--unsafe-fixes", success_codes=[0, 1])
    session.run("isort", "dcc_mcp_core", "tests", "nox_actions")
    session.run(
        "autoflake",
        "--in-place",
        "--recursive",
        "--remove-all-unused-imports",
        "--remove-unused-variables",
        "--expand-star-imports",
        "--exclude",
        "__init__.py",
        "dcc_mcp_core",
        "tests",
        "nox_actions",
    )
