"""Bounded zero-dependency typing compatibility for Python 3.7."""

from __future__ import annotations

import inspect
import math

try:
    from typing import Literal
    from typing import Protocol
    from typing import runtime_checkable
except ImportError:  # pragma: no cover - Python 3.7 only
    _MISSING = object()
    _PROTOCOL_ROOT = None
    _PROTOCOL_INTERNALS = frozenset(
        {
            "__annotations__",
            "__class__",
            "__dict__",
            "__doc__",
            "__module__",
            "__new__",
            "__orig_bases__",
            "__slots__",
            "__subclasshook__",
            "__weakref__",
            "_is_protocol",
            "_is_runtime_protocol",
        }
    )

    def _protocol_members(protocol):
        members = set()
        callable_members = set()
        for base in protocol.__mro__:
            if not getattr(base, "_is_protocol", False) or base is _PROTOCOL_ROOT:
                continue
            members.update(inspect.getattr_static(base, "__annotations__", ()))
            for name, value in base.__dict__.items():
                if name in _PROTOCOL_INTERNALS or name.startswith("_abc_"):
                    continue
                members.add(name)
                if callable(value) or isinstance(value, (classmethod, staticmethod)):
                    callable_members.add(name)
        return members, callable_members

    class _ProtocolMeta(type):
        """Minimal, static structural checks for Core's Python 3.7 protocols."""

        def __new__(mcls, name, bases, namespace):
            bootstrapping = _PROTOCOL_ROOT is None
            declares_protocol = bootstrapping or any(base is _PROTOCOL_ROOT for base in bases)
            if not bootstrapping and declares_protocol:
                invalid_bases = [
                    base
                    for base in bases
                    if base is not _PROTOCOL_ROOT and not base.__dict__.get("_is_protocol", False)
                ]
                if invalid_bases:
                    raise TypeError("Protocols can only inherit from other protocols, got non-protocol base")
            cls = super().__new__(mcls, name, bases, namespace)
            cls._is_protocol = declares_protocol
            cls._is_runtime_protocol = False
            return cls

        def __call__(cls, *args, **kwargs):
            if cls.__dict__.get("_is_protocol", False):
                raise TypeError("Protocols cannot be instantiated")
            return super().__call__(*args, **kwargs)

        def __instancecheck__(cls, instance):
            if not cls.__dict__.get("_is_protocol", False):
                return super().__instancecheck__(instance)
            if not cls.__dict__.get("_is_runtime_protocol", False):
                raise TypeError("Instance checks require @runtime_checkable")
            members, callable_members = _protocol_members(cls)
            for name in members:
                value = inspect.getattr_static(instance, name, _MISSING)
                if value is _MISSING:
                    return False
                if name in callable_members and not (callable(value) or isinstance(value, (classmethod, staticmethod))):
                    return False
            return True

        def __subclasscheck__(cls, subclass):
            if not cls.__dict__.get("_is_protocol", False):
                return super().__subclasscheck__(subclass)
            raise TypeError("issubclass() is not supported by the Python 3.7 Protocol subset")

    class Protocol(metaclass=_ProtocolMeta):  # type: ignore[no-redef]
        """Structural Protocol subset required by Core on Python 3.7."""

    _PROTOCOL_ROOT = Protocol

    def runtime_checkable(cls):  # type: ignore[misc]
        """Enable bounded runtime instance checks for a fallback Protocol."""
        if not isinstance(cls, _ProtocolMeta) or not cls.__dict__.get("_is_protocol", False):
            raise TypeError("@runtime_checkable can be applied only to protocol classes")
        cls._is_runtime_protocol = True
        return cls

    class _LiteralMeta(type):
        """Build aliases for non-empty JSON-preserving scalar literals."""

        def __getitem__(cls, item):
            args = item if isinstance(item, tuple) else (item,)
            if not args:
                raise TypeError("Literal requires at least one value")
            for value in args:
                if value is None:
                    continue
                if type(value) not in (bool, float, int, str):
                    raise TypeError("Literal values must be JSON-preserving scalars")
                if type(value) is float and not math.isfinite(value):
                    raise TypeError("Literal values must be JSON-preserving scalars")
            return type(
                "_LiteralAlias",
                (),
                {
                    "__origin__": cls,
                    "__args__": args,
                },
            )

    class Literal(metaclass=_LiteralMeta):  # type: ignore[no-redef]
        """Runtime-introspectable Literal subset required by Core on Python 3.7."""


__all__ = ["Literal", "Protocol", "runtime_checkable"]
