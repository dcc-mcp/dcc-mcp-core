"""Prove a Python 3.7 Core wheel runs without site-packages or typing backports."""

from __future__ import annotations

import argparse
from email.parser import Parser
import importlib
import math
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import typing
import zipfile

try:
    from .archive_payload_policy import archive_member_errors
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from archive_payload_policy import archive_member_errors


class _BlockTypingExtensions:
    """Reject accidental backport imports inside the isolated child."""

    @staticmethod
    def find_spec(fullname, path=None, target=None):
        if fullname == "typing_extensions":
            raise ModuleNotFoundError("typing_extensions is intentionally absent")
        return None


def _is_default_requirement(requirement: str) -> bool:
    marker = requirement.partition(";")[2]
    extra_equality = re.search(r"\bextra\s*==\s*(['\"])[^'\"]+\1", marker, re.IGNORECASE)
    return extra_equality is None or re.search(r"\bor\b", marker, re.IGNORECASE) is not None


def _single_wheel(raw: str) -> Path:
    path = Path(raw)
    matches = sorted(path.parent.glob(path.name))
    if len(matches) != 1:
        raise RuntimeError(f"expected one wheel matching {raw!r}, found {len(matches)}")
    return matches[0].resolve()


def _check_wheel_archive(wheel: Path, root: Path) -> None:
    with zipfile.ZipFile(str(wheel)) as archive:
        names = archive.namelist()
        payload_errors = archive_member_errors(archive.infolist())
        if payload_errors:
            raise RuntimeError("; ".join(payload_errors))
        metadata_names = [name for name in names if name.endswith(".dist-info/METADATA")]
        if len(metadata_names) != 1:
            raise RuntimeError("wheel must contain exactly one METADATA file")
        metadata = Parser().parsestr(archive.read(metadata_names[0]).decode("utf-8"))
        requirements = metadata.get_all("Requires-Dist") or []
        if any(
            re.sub(r"[-_.]+", "-", str(requirement).lower()).startswith("typing-extensions")
            and _is_default_requirement(str(requirement))
            for requirement in requirements
        ):
            raise RuntimeError("wheel metadata contains typing-extensions")
        archive.extractall(str(root))


def _run_isolated_child(root: Path) -> None:
    command = [
        sys.executable,
        "-I",
        "-S",
        str(Path(__file__).resolve()),
        "--isolated-root",
        str(root),
    ]
    subprocess.run(command, check=True)


def _verify_protocol(Protocol, runtime_checkable) -> None:
    @runtime_checkable
    class _SizedRunner(Protocol):
        name: str

        def __len__(self) -> int: ...

        def run(self) -> int: ...

    class _DescriptorRunner:
        @property
        def name(self):
            raise AssertionError("property must not execute")

        def __len__(self):
            return 1

        def run(self):
            return 1

    class _DynamicRunner:
        def __getattr__(self, name):
            raise AssertionError("__getattr__ must not execute")

        def __len__(self):
            return 1

        def run(self):
            return 1

    class _ConcreteRunner(_SizedRunner):
        name = "runner"

        def __len__(self):
            return 1

        def run(self):
            return 1

    if not isinstance(_DescriptorRunner(), _SizedRunner):
        raise RuntimeError("static protocol check rejected a valid descriptor-backed object")
    if isinstance(_DynamicRunner(), _SizedRunner):
        raise RuntimeError("static protocol check accepted a dynamic-only object")
    for protocol in (Protocol, _SizedRunner):
        try:
            protocol()
        except TypeError:
            pass
        else:
            raise RuntimeError("fallback protocol declaration was instantiable")
    if getattr(_ConcreteRunner, "_is_protocol", True) or _ConcreteRunner().run() != 1:
        raise RuntimeError("fallback protocol misclassified a concrete subclass")
    try:
        runtime_checkable(_ConcreteRunner)
    except TypeError:
        pass
    else:
        raise RuntimeError("fallback runtime_checkable accepted a concrete subclass")
    try:
        issubclass(_DescriptorRunner, _SizedRunner)
    except TypeError:
        pass
    else:
        raise RuntimeError("unsupported fallback issubclass semantics did not fail closed")


def _verify_isolated_root(root: Path) -> None:
    if sys.version_info[:2] != (3, 7):
        raise RuntimeError("isolated zero-backport smoke requires CPython 3.7")
    if any("site-packages" in entry.lower() for entry in sys.path):
        raise RuntimeError("isolated smoke unexpectedly includes site-packages")
    sys.path.insert(0, str(root))
    sys.meta_path.insert(0, _BlockTypingExtensions())

    from dcc_mcp_core._typing import Literal
    from dcc_mcp_core._typing import Protocol
    from dcc_mcp_core._typing import runtime_checkable
    from dcc_mcp_core.batch import EvalContext
    from dcc_mcp_core.schema import derive_schema

    _verify_protocol(Protocol, runtime_checkable)
    alias = Literal["fbx", "usd", 1, True, None, -0.5]
    if alias.__origin__ is not Literal or alias.__args__ != ("fbx", "usd", 1, True, None, -0.5):
        raise RuntimeError("fallback Literal lost its origin or arguments")

    class _Options:
        pass

    _Options.__annotations__ = {"format": Literal["fbx", "usd"]}

    if typing.get_type_hints(_Options)["format"].__args__ != ("fbx", "usd"):
        raise RuntimeError("fallback Literal does not round-trip through get_type_hints")
    if derive_schema(Literal["fbx", "usd"]) != {"enum": ["fbx", "usd"], "type": "string"}:
        raise RuntimeError("fallback Literal schema derivation failed")
    for value in (b"x", object(), math.nan, math.inf):
        try:
            Literal[value]
        except TypeError:
            pass
        else:
            raise RuntimeError("fallback Literal accepted an unsupported value")
    sandbox_builtins = EvalContext(None)._make_builtins()
    if sandbox_builtins["__import__"]("typing").Literal != "Literal":
        raise RuntimeError("batch annotation sandbox omitted the local Literal")

    callers = (
        "dcc_mcp_core._server._inprocess_contracts",
        "dcc_mcp_core._server.callable_dispatcher",
        "dcc_mcp_core._server.host_pump",
        "dcc_mcp_core.agent_memory",
        "dcc_mcp_core.cancellation",
        "dcc_mcp_core.host._protocols",
        "dcc_mcp_core.skill_index._protocol",
        "dcc_mcp_core.skill_index._vector",
        "dcc_mcp_core.vector_embedder",
    )
    for module_name in callers:
        importlib.import_module(module_name)
    if "typing_extensions" in sys.modules:
        raise RuntimeError("isolated runtime imported typing_extensions")


def main(argv=None) -> int:
    """Run the archive parent or isolated runtime child."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--wheel")
    parser.add_argument("--isolated-root")
    args = parser.parse_args(argv)
    try:
        if args.isolated_root:
            _verify_isolated_root(Path(args.isolated_root))
        elif args.wheel:
            wheel = _single_wheel(args.wheel)
            with tempfile.TemporaryDirectory() as temp_dir:
                root = Path(temp_dir)
                _check_wheel_archive(wheel, root)
                _run_isolated_child(root)
        else:
            raise RuntimeError("--wheel or --isolated-root is required")
    except Exception as exc:
        sys.stderr.write(f"zero-typing-extensions-smoke: {exc}\n")
        return 1
    sys.stdout.write("zero-typing-extensions-smoke: OK\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
