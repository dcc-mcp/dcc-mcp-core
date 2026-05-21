"""Lifecycle collaborator for :class:`dcc_mcp_core.server_base.DccServerBase`.

Keeps shutdown-hook and atexit-registration state in one place while preserving
the historical private attribute names on the owner instance for compatibility.
"""

from __future__ import annotations

import logging
from typing import Any
from typing import Callable
import weakref

logger = logging.getLogger(__name__)


class ServerLifecycleController:
    """Owns quit-hook bookkeeping for one ``DccServerBase`` instance."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    def ensure_state(self) -> None:
        owner_dict = self._owner.__dict__
        owner_dict.setdefault("_quit_hooks", [])
        owner_dict.setdefault("_quit_hooks_ran", False)
        owner_dict.setdefault("_atexit_registered", False)

    def prepare_start(
        self,
        *,
        install_atexit_hook: bool,
        stop_from_atexit: Callable[[weakref.ReferenceType[Any]], None],
        atexit_register: Callable[..., Any],
    ) -> None:
        self.ensure_state()
        self._owner._quit_hooks_ran = False
        if install_atexit_hook and not self._owner._atexit_registered:
            atexit_register(stop_from_atexit, weakref.ref(self._owner))
            self._owner._atexit_registered = True

    def register_quit_hook(self, callback: Callable[[], Any]) -> Callable[[], Any]:
        if not callable(callback):
            raise TypeError("quit hook must be callable")
        self.ensure_state()
        self._owner._quit_hooks.append(callback)
        return callback

    def unregister_quit_hook(self, callback: Callable[[], Any]) -> bool:
        self.ensure_state()
        hooks = self._owner._quit_hooks
        for idx in range(len(hooks) - 1, -1, -1):
            if hooks[idx] is callback:
                del hooks[idx]
                return True
        return False

    def run_quit_hooks(self, *, dcc_name: str) -> None:
        self.ensure_state()
        if self._owner._quit_hooks_ran:
            return
        self._owner._quit_hooks_ran = True
        while self._owner._quit_hooks:
            hook = self._owner._quit_hooks.pop()
            try:
                hook()
            except Exception as exc:
                logger.warning("[%s] Quit hook failed: %s", dcc_name, exc, exc_info=True)
