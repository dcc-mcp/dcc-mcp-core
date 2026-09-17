"""Declaration-lint pairing rule for the behavior verification contract.

Part 1 of core#2269's proposal: *every write verb ships with a read-only state
export*. A skill whose write tools mutate DCC state without any way to read
that state back makes silent successes undetectable — the evaluation's
central negative result.

This module implements the pairing rule as a dependency-free function over a
``tools.yaml`` tool table. A write verb is considered *paired* when any of
these hold:

1. It declares ``next-tools.on-success`` pointing at a read-only tool
   (the media skill already does this with ``media__probe``).
2. It declares an explicit ``readback`` (or ``state_export``) field naming a
   read-only tool.
3. Its name shares a domain token with a read-only tool in the same table
   (for example ``set_keyframes`` pairs with ``get_keyframes``).

Common verb tokens (``set``, ``create``, ``apply``, ...) are excluded from the
domain match so that ``set_transform`` and ``get_transform`` pair on
``transform`` rather than never pairing on ``set``/``get``.
"""

from __future__ import annotations

from dataclasses import dataclass
import re
from typing import Any
from typing import Mapping
from typing import Sequence

_CODE_UNPAIRED_WRITE_VERB = "unpaired_write_verb"

# Verbs that describe the mutation itself rather than the state domain. They
# carry no pairing signal and are dropped from domain-token matching.
_VERB_STOPWORDS = frozenset(
    {
        "add",
        "apply",
        "assign",
        "bake",
        "bind",
        "build",
        "convert",
        "create",
        "delete",
        "edit",
        "execute",
        "export",
        "extract",
        "generate",
        "import",
        "install",
        "launch",
        "make",
        "modify",
        "move",
        "remove",
        "render",
        "run",
        "save",
        "set",
        "start",
        "stop",
        "transcode",
        "update",
        "write",
    }
)

_TOKEN_RE = re.compile(r"[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[0-9]+")


@dataclass(frozen=True)
class DeclarationFinding:
    """One declaration-lint violation."""

    tool: str
    code: str
    reason: str

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-safe representation of this finding."""
        return {"tool": self.tool, "code": self.code, "reason": self.reason}


def _tool_name(tool: Mapping[str, Any]) -> str | None:
    name = tool.get("name")
    return name if isinstance(name, str) and name else None


def _is_read_only(tool: Mapping[str, Any]) -> bool:
    return bool(tool.get("read_only"))


def _tokens(name: str) -> set[str]:
    return {token.lower() for token in _TOKEN_RE.findall(name) if token}


def _domain_tokens(name: str) -> set[str]:
    tokens = _tokens(name) - _VERB_STOPWORDS
    # A name made entirely of stopwords (e.g. ``set``) still needs some signal;
    # fall back to the raw tokens so the tool is not silently skipped.
    return tokens or _tokens(name)


def _next_success_targets(tool: Mapping[str, Any]) -> list[str]:
    next_tools = tool.get("next-tools")
    if not isinstance(next_tools, Mapping):
        return []
    on_success = next_tools.get("on-success")
    if not isinstance(on_success, list):
        return []
    return [item for item in on_success if isinstance(item, str)]


def _matches_read_only(reference: str, read_only_names: set[str]) -> bool:
    for name in read_only_names:
        if reference == name:
            return True
        for separator in ("__", ".", ":", "/"):
            if reference.endswith(separator + name):
                return True
    return False


def _explicit_readback(tool: Mapping[str, Any], read_only_names: set[str]) -> bool:
    for field in ("readback", "state_export", "readback_tool"):
        value = tool.get(field)
        if isinstance(value, str) and _matches_read_only(value, read_only_names):
            return True
    return False


def _paired_by_next_success(tool: Mapping[str, Any], read_only_names: set[str]) -> bool:
    return any(_matches_read_only(target, read_only_names) for target in _next_success_targets(tool))


def _paired_by_domain(tool_name: str, read_only_names: set[str]) -> bool:
    write_domain = _domain_tokens(tool_name)
    return any(write_domain & _domain_tokens(read_name) for read_name in read_only_names)


def find_unpaired_write_verbs(tools: Sequence[Mapping[str, Any]]) -> list[DeclarationFinding]:
    """Return one :class:`DeclarationFinding` per unpaired write verb.

    A write verb is any tool whose ``read_only`` is falsy (absent tools are
    treated as writes, matching the conservative default: a verb that might
    mutate state must prove it has a readback).

    Parameters
    ----------
    tools:
        The tool table from a ``tools.yaml`` declaration (each entry a mapping
        with at least ``name`` and, for the pairing rules, ``read_only`` /
        ``next-tools`` / ``readback``).

    Returns
    -------
    List[DeclarationFinding]
        Findings in table order; empty when every write verb is paired.

    """
    read_only_names = {name for tool in tools if _is_read_only(tool) for name in (_tool_name(tool),) if name}
    findings: list[DeclarationFinding] = []
    for tool in tools:
        name = _tool_name(tool)
        if name is None or _is_read_only(tool):
            continue
        if _paired_by_next_success(tool, read_only_names) or _explicit_readback(tool, read_only_names):
            continue
        if _paired_by_domain(name, read_only_names):
            continue
        findings.append(
            DeclarationFinding(
                tool=name,
                code=_CODE_UNPAIRED_WRITE_VERB,
                reason=(
                    f"write verb {name} has no paired read-only state export; add a read_only tool in "
                    "the same domain, point next-tools.on-success at one, or declare readback"
                ),
            )
        )
    return findings


def lint_tool_table(tools: Sequence[Mapping[str, Any]], source: str = "") -> list[DeclarationFinding]:
    """Alias for :func:`find_unpaired_write_verbs` with an optional *source* label.

    *source* is retained for a future diagnostic format and is currently
    informational only; it does not change the returned findings.
    """
    return find_unpaired_write_verbs(tools)


__all__ = [
    "DeclarationFinding",
    "find_unpaired_write_verbs",
    "lint_tool_table",
]
