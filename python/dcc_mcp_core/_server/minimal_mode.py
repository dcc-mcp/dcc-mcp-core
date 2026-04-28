"""Declarative progressive skill loading for DCC adapters (issue #525).

Every embedded DCC needs progressive skill loading at startup so the
initial ``tools/list`` payload stays small. Maya hard-codes this in
``server.py``; Houdini, Unreal, Blender etc. would each re-implement
the same pattern with only the skill names changing.

This module exposes :class:`MinimalModeConfig` — a small frozen
descriptor — so DCC adapters supply *only the data*, and
:meth:`DccServerBase.register_builtin_actions` does the rest.
"""

# Import built-in modules
from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import logging
import os
from typing import TYPE_CHECKING
from typing import Any
from typing import Mapping

if TYPE_CHECKING:
    pass

logger = logging.getLogger(__name__)

__all__ = [
    "MinimalModeConfig",
    "resolve_default_tools",
    "resolve_minimal_disabled",
]


@dataclass(frozen=True)
class MinimalModeConfig:
    """Declarative descriptor for minimal-mode skill loading.

    Attributes:
        skills: Skills to fully load at startup. Order is preserved.
        deactivate_groups: Per-skill list of tool groups to leave
            inactive after the skill is loaded. Mapping is
            ``{skill_name: (group_a, group_b, …)}``. Skills not in
            ``skills`` are ignored even if they appear here.
        env_var_minimal: Name of the env var that disables minimal
            mode when set to a falsy literal (``"0"``, ``"false"``,
            ``"no"``, ``""``). When unset, minimal mode is **on**.
        env_var_default_tools: Name of the env var that overrides
            ``skills`` with an explicit comma- or whitespace-separated
            list of skill names; takes precedence over
            ``env_var_minimal``.

    """

    skills: tuple[str, ...]
    deactivate_groups: Mapping[str, tuple[str, ...]] = field(default_factory=dict)
    env_var_minimal: str = "DCC_MCP_MINIMAL"
    env_var_default_tools: str = "DCC_MCP_DEFAULT_TOOLS"


_FALSY = frozenset({"", "0", "false", "no", "off"})


def resolve_minimal_disabled(env_var: str, environ: Mapping[str, str] | None = None) -> bool:
    """Return ``True`` when ``env_var`` resolves to a falsy literal.

    A *falsy* env-var value (``"0"``, ``"false"``, ``"no"``, ``"off"``,
    empty string) means the user has **disabled** minimal mode and
    wants every discovered skill loaded. Comparison is
    case-insensitive. Unset variable → minimal mode stays enabled
    (returns ``False``).
    """
    env = environ if environ is not None else os.environ
    value = env.get(env_var)
    if value is None:
        return False
    return value.strip().lower() in _FALSY


def resolve_default_tools(
    env_var: str,
    environ: Mapping[str, str] | None = None,
) -> tuple[str, ...] | None:
    """Parse the ``env_var_default_tools`` override.

    Returns the explicit list of skill names supplied via the env var,
    or ``None`` when the env var is unset / empty after normalisation.
    Accepts both comma- and whitespace-separated values; trims each
    token; deduplicates while preserving the first occurrence.
    """
    env = environ if environ is not None else os.environ
    raw = env.get(env_var)
    if raw is None:
        return None
    # Normalise: split on commas and whitespace, drop empty tokens.
    tokens: list[str] = []
    seen: set[str] = set()
    for piece in raw.replace(",", " ").split():
        token = piece.strip()
        if token and token not in seen:
            seen.add(token)
            tokens.append(token)
    if not tokens:
        return None
    return tuple(tokens)


def apply_minimal_mode(
    server: Any,
    config: MinimalModeConfig,
    *,
    environ: Mapping[str, str] | None = None,
    dcc_name: str = "",
) -> int:
    """Apply *config* against *server* (a :class:`DccServerBase` instance).

    Decision order:

    1. If ``config.env_var_default_tools`` resolves to a non-empty
       list → load exactly those skills, ignore ``deactivate_groups``.
    2. Else if ``config.env_var_minimal`` resolves to a falsy literal
       → load every discovered skill, ignore ``deactivate_groups``.
    3. Else → load ``config.skills`` and apply ``deactivate_groups``.

    Returns the number of skills successfully loaded.
    """
    explicit = resolve_default_tools(config.env_var_default_tools, environ)
    if explicit is not None:
        logger.info(
            "[%s] Minimal mode overridden by %s: loading %d skill(s)",
            dcc_name,
            config.env_var_default_tools,
            len(explicit),
        )
        return _load_named(server, explicit, dcc_name=dcc_name)

    if resolve_minimal_disabled(config.env_var_minimal, environ):
        logger.info(
            "[%s] Minimal mode disabled by %s; loading all discovered skills",
            dcc_name,
            config.env_var_minimal,
        )
        try:
            discovered = [s.name for s in server.list_skills()]
        except Exception as exc:
            logger.debug("[%s] list_skills() failed during minimal_mode: %s", dcc_name, exc)
            discovered = []
        return _load_named(server, tuple(discovered), dcc_name=dcc_name)

    loaded = _load_named(server, config.skills, dcc_name=dcc_name)
    catalog = getattr(server, "catalog", None)
    if catalog is None:
        logger.debug("[%s] No catalog handle; skipping group deactivation", dcc_name)
        return loaded
    for skill_name, groups in config.deactivate_groups.items():
        if skill_name not in config.skills:
            continue
        for group in groups:
            try:
                catalog.deactivate_group(group)
            except Exception as exc:
                logger.debug(
                    "[%s] deactivate_group(%r) for skill %r failed: %s",
                    dcc_name,
                    group,
                    skill_name,
                    exc,
                )
    return loaded


def _load_named(server: Any, names: tuple[str, ...], *, dcc_name: str) -> int:
    """Load each skill by name; return the count of successful loads."""
    loaded = 0
    for name in names:
        try:
            server.load_skill(name)
            loaded += 1
        except Exception as exc:
            logger.warning("[%s] load_skill(%r) failed: %s", dcc_name, name, exc)
    return loaded
