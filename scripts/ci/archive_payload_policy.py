"""Shared fail-closed policy for Python distribution archive members."""

from __future__ import annotations

import re
import stat
import unicodedata

_METADATA_SUFFIXES = (".data", ".dist-info", ".egg-info")
_WINDOWS_RESERVED = frozenset(
    {"aux", "con", "nul", "prn"} | {f"com{index}" for index in range(1, 10)} | {f"lpt{index}" for index in range(1, 10)}
)


def _member_name(member) -> str:
    if isinstance(member, str):
        return member
    filename = getattr(member, "filename", None)
    if isinstance(filename, str):
        return filename
    name = getattr(member, "name", None)
    if isinstance(name, str):
        return name
    raise ValueError(f"unsafe archive member {member!r}")


def _unsafe_component(component: str) -> bool:
    if component.endswith((".", " ")) or any(character in '<>:"|?*' for character in component):
        return True
    if any(ord(character) < 32 or ord(character) == 127 for character in component):
        return True
    stem = component.split(".", 1)[0].casefold()
    return stem in _WINDOWS_RESERVED


def normalize_archive_member(name: str) -> str:
    """Return a platform-independent relative archive path or reject it."""
    if not isinstance(name, str) or not name or "\x00" in name:
        raise ValueError(f"unsafe archive member {name!r}")
    try:
        portable = unicodedata.normalize("NFKC", name.replace("\\", "/"))
    except UnicodeError as exc:
        raise ValueError(f"unsafe archive member {name!r}") from exc
    if portable.startswith("/") or "//" in portable or re.match(r"^[A-Za-z]:", portable):
        raise ValueError(f"unsafe archive member {name!r}")
    parts = []
    for part in portable.split("/"):
        if part in ("", "."):
            continue
        if part == ".." or _unsafe_component(part):
            raise ValueError(f"unsafe archive member {name!r}")
        parts.append(part)
    if not parts:
        raise ValueError(f"unsafe archive member {name!r}")
    return "/".join(parts)


def _project_name(value: str) -> str:
    return re.sub(r"[-_.]+", "-", unicodedata.normalize("NFKC", value).casefold())


def _is_typing_extensions_component(component: str) -> bool:
    folded = component.casefold()
    if folded.endswith((".py", ".pyc")):
        module = folded.rsplit(".", 1)[0]
        if folded.endswith(".pyc"):
            # CPython caches retain the module name before the cache/opt tag.
            module = re.sub(r"\.cpython-[0-9]+(?:\.opt-[a-z0-9]+)?$", "", module)
        if _project_name(module) == "typing-extensions":
            return True
    if _project_name(folded) == "typing-extensions":
        return True
    for suffix in _METADATA_SUFFIXES:
        if not folded.endswith(suffix):
            continue
        distribution = _project_name(folded[: -len(suffix)])
        if distribution == "typing-extensions" or distribution.startswith("typing-extensions-"):
            return True
    return False


def _contains_typing_extensions_alias(name: str) -> bool:
    try:
        portable = unicodedata.normalize("NFKC", name.replace("\\", "/"))
    except UnicodeError:
        return False
    components = []
    for part in portable.split("/"):
        components.extend(part.split(":"))
    return any(_is_typing_extensions_component(part.rstrip(". ")) for part in components if part)


def _member_type_error(member, name: str) -> str | None:
    external_attr = getattr(member, "external_attr", None)
    if isinstance(external_attr, int):
        mode = (external_attr >> 16) & 0xFFFF
        if mode and stat.S_ISLNK(mode):
            return f"archive contains forbidden symlink member {name!r}"
        file_type = stat.S_IFMT(mode)
        if file_type not in (0, stat.S_IFREG, stat.S_IFDIR):
            return f"archive contains forbidden special member {name!r}"

    is_symbolic_link = getattr(member, "issym", None)
    is_hard_link = getattr(member, "islnk", None)
    if callable(is_symbolic_link) and callable(is_hard_link) and (is_symbolic_link() or is_hard_link()):
        target = getattr(member, "linkname", "")
        try:
            normalize_archive_member(target)
        except ValueError:
            return f"archive contains forbidden link member {name!r} with unsafe target {target!r}"
        return f"archive contains forbidden link member {name!r}"
    is_file = getattr(member, "isfile", None)
    is_directory = getattr(member, "isdir", None)
    if callable(is_file) and callable(is_directory) and not (is_file() or is_directory()):
        return f"archive contains forbidden special member {name!r}"
    return None


def archive_member_errors(members) -> list[str]:
    """Reject unsafe, linked, conflicting, duplicate, or backport members."""
    errors = []
    seen = {}
    directories = {}
    for member in members:
        name = None
        try:
            name = _member_name(member)
            normalized = normalize_archive_member(name)
        except ValueError as exc:
            errors.append(str(exc))
            if isinstance(name, str) and _contains_typing_extensions_alias(name):
                errors.append(f"archive contains forbidden typing_extensions payload {name!r}")
            continue
        member_type_error = _member_type_error(member, name)
        if member_type_error is not None:
            errors.append(member_type_error)
        portable_key = normalized.casefold()
        previous = seen.get(portable_key)
        if previous is not None:
            errors.append(f"archive contains duplicate portable member paths {previous!r} and {name!r}")
        else:
            seen[portable_key] = name
        is_directory = getattr(member, "is_dir", None) or getattr(member, "isdir", None)
        member_is_directory = is_directory() if callable(is_directory) else name.endswith(("/", "\\"))
        parts = portable_key.split("/")
        for depth in range(1, len(parts) + 1):
            path = "/".join(parts[:depth])
            # Every ancestor is an implied directory. An explicit directory
            # may confirm it later, but a regular file can never occupy it.
            requires_directory = depth < len(parts) or member_is_directory
            if path in directories and directories[path] != requires_directory:
                errors.append(f"archive contains file/directory conflict at {path!r} from member {name!r}")
            else:
                directories[path] = requires_directory
        if _contains_typing_extensions_alias(normalized):
            errors.append(f"archive contains forbidden typing_extensions payload {name!r}")
    return sorted(set(errors))
