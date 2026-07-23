"""Typing backports for Python 3.7 (Maya 2022 / Blender 2.83)."""

from __future__ import annotations

try:
    from typing import Literal
    from typing import Protocol
    from typing import runtime_checkable
except ImportError:  # pragma: no cover - Python 3.7 only
    from typing_extensions import get_annotations

    class _ProtocolMeta(type):
        """Minimal structural runtime checks for Python 3.7 protocols."""

        def __instancecheck__(cls, instance):
            if not cls.__dict__.get("_is_runtime_protocol", False):
                raise TypeError("Instance checks require @runtime_checkable")

            members = set()
            callable_members = set()
            for base in cls.__mro__:
                if base in (Protocol, object):
                    continue
                members.update(name for name in get_annotations(base) if not name.startswith("_"))
                for name, value in base.__dict__.items():
                    if name.startswith("_"):
                        continue
                    if callable(value) or isinstance(value, (classmethod, property, staticmethod)):
                        members.add(name)
                    if callable(value) or isinstance(value, (classmethod, staticmethod)):
                        callable_members.add(name)
            return all(
                hasattr(instance, name) and (name not in callable_members or getattr(instance, name) is not None)
                for name in members
            )

    class Protocol(metaclass=_ProtocolMeta):  # type: ignore[no-redef]
        """Structural Protocol subset used by the Python 3.7 runtime."""

        _is_runtime_protocol = False

    def runtime_checkable(cls):  # type: ignore[misc]
        cls._is_runtime_protocol = True
        return cls

    class _LiteralMeta(type):
        """Build type-like aliases accepted by Python 3.7 get_type_hints()."""

        def __getitem__(cls, item):
            args = item if isinstance(item, tuple) else (item,)
            return type(
                "_LiteralAlias",
                (),
                {
                    "__origin__": cls,
                    "__args__": args,
                },
            )

    class Literal(metaclass=_LiteralMeta):  # type: ignore[no-redef]
        """Runtime-introspectable Literal subset for Python 3.7."""


__all__ = ["Literal", "Protocol", "runtime_checkable"]
