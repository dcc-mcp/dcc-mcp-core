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
    from .dependency_marker_policy import is_default_requirement as _is_default_requirement
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from archive_payload_policy import archive_member_errors
    from dependency_marker_policy import is_default_requirement as _is_default_requirement


class _BlockTypingExtensions:
    """Reject accidental backport imports inside the isolated child."""

    @staticmethod
    def find_spec(fullname, path=None, target=None):
        if fullname == "typing_extensions":
            raise ModuleNotFoundError("typing_extensions is intentionally absent")
        return None


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

    class _ChildRunner(_ConcreteRunner):
        pass

    for candidate, target, expected in (
        (object, _ConcreteRunner, False),
        (_ConcreteRunner, _ConcreteRunner, True),
        (_ChildRunner, _ConcreteRunner, True),
        (_ConcreteRunner, _ChildRunner, False),
        (object, _ChildRunner, False),
        (_DescriptorRunner, _ConcreteRunner, False),
    ):
        if isinstance(candidate(), target) is not expected or issubclass(candidate, target) is not expected:
            raise RuntimeError("fallback concrete subclass lost nominal type checks")
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

    concrete = type("Protocol", (_SizedRunner,), {"name": "runner", "__len__": lambda self: 1, "run": lambda self: 1})
    child = type("Protocol", (concrete,), {})
    if concrete._is_protocol or child._is_protocol or concrete().run() != 1:
        raise RuntimeError("fallback class name changed concrete classification")
    if not isinstance(child(), concrete) or not issubclass(child, concrete) or isinstance(object(), concrete):
        raise RuntimeError("fallback named concrete class lost nominal checks")
    for bases in ((_DescriptorRunner, Protocol), (Protocol, _DescriptorRunner)):
        try:
            type("Protocol", bases, {})
        except TypeError:
            pass
        else:
            raise RuntimeError("fallback class name bypassed protocol base restrictions")

    @runtime_checkable
    class _VersionedRunner(Protocol):
        revision = 1

        def run(self) -> int: ...

    class _RevisionDescriptor:
        def __get__(self, instance, owner):
            raise AssertionError("default data descriptor must not execute")

    class _VersionedObject(_DescriptorRunner):
        revision = _RevisionDescriptor()

    if isinstance(_DynamicRunner(), _VersionedRunner) or not isinstance(_VersionedObject(), _VersionedRunner):
        raise RuntimeError("fallback default data member lost its static presence requirement")


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
    _verify_annotation_execution()

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


def _verify_annotation_execution() -> None:
    """Schema-valid annotations must survive materialization and sandbox execution."""
    from dcc_mcp_core.dcc_api_executor import DccApiExecutor
    from dcc_mcp_core.script_execution import normalize_file_backed_script_execution_params

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        executor = DccApiExecutor("3dsmax", dispatcher=None, script_materialization_root=root)
        for module in ("typing", "typing_extensions"):
            for annotation, value, schema_type in (
                ("Annotated[int, 'units']", 7, "integer"),
                ("Literal['fbx', 'usd']", "fbx", "string"),
                ("List[int]", [1, 2], "array"),
            ):
                symbol = annotation.split("[", 1)[0]
                source = f"from {module} import {symbol}\ndef main(value: {annotation}):\n    return value\n"
                prepared = normalize_file_backed_script_execution_params(
                    {"code": source, "params": {"value": value}},
                    dcc_type="3dsmax",
                    instance_id="annotation-contract",
                    session_id="offline",
                    materialization_root=root,
                )
                if prepared.parameters_schema["properties"]["value"]["type"] != schema_type:
                    raise RuntimeError("script lost its parameter schema")
                result = executor.execute_params({"file_path": prepared.file_path, "params": {"value": value}})
                if not result["success"] or result["output"] != value:
                    raise RuntimeError(f"materialized annotation script failed: {result}")
            for body, error in (
                ("return Annotated(int)", "not callable"),
                ("return Annotated.__class__", "reflective"),
                ("import os\n    return value", "sandbox import"),
                ("from typing_extensions import get_type_hints\n    return value", "cannot import name"),
            ):
                source = f"from {module} import Annotated\ndef main(value: Annotated[int, 'units']):\n    {body}\n"
                prepared = normalize_file_backed_script_execution_params(
                    {"code": source, "params": {"value": 7}},
                    dcc_type="3dsmax",
                    instance_id="annotation-contract",
                    session_id="offline",
                    materialization_root=root,
                )
                result = executor.execute_params({"file_path": prepared.file_path, "params": {"value": 7}})
                if result["success"] or error not in result["error"]:
                    raise RuntimeError(f"materialized annotation script weakened sandbox restrictions: {result}")


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
