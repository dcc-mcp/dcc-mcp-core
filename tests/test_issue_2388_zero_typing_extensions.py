"""Regression contract for the zero-dependency Python 3.7 typing boundary."""

from __future__ import annotations

import ast
import importlib.util
import math
from pathlib import Path
import sys
import typing
import unittest

REPO_ROOT = Path(__file__).resolve().parents[1]
TYPING_MODULE = REPO_ROOT / "python" / "dcc_mcp_core" / "_typing.py"
PYTHON_PACKAGE = REPO_ROOT / "python" / "dcc_mcp_core"


class _BlockTypingExtensions:
    """Fail if the compatibility boundary tries to import the backport."""

    @staticmethod
    def find_spec(fullname, path=None, target=None):
        if fullname == "typing_extensions":
            raise ModuleNotFoundError("typing_extensions is intentionally absent")
        return None


def _load_simulated_python37_typing():
    """Load ``_typing`` with the three Python 3.8 typing APIs hidden."""
    names = ("Literal", "Protocol", "runtime_checkable")
    saved = {name: getattr(typing, name) for name in names if hasattr(typing, name)}
    blocker = _BlockTypingExtensions()
    sys.meta_path.insert(0, blocker)
    try:
        for name in names:
            if hasattr(typing, name):
                delattr(typing, name)
        spec = importlib.util.spec_from_file_location("_issue_2388_typing", TYPING_MODULE)
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    finally:
        sys.meta_path.remove(blocker)
        for name, value in saved.items():
            setattr(typing, name, value)


class TypingCompatibilityTests(unittest.TestCase):
    @unittest.skipIf(sys.version_info < (3, 8), "stdlib typing APIs begin in Python 3.8")
    def test_modern_interpreters_export_stdlib_objects_by_identity(self):
        spec = importlib.util.spec_from_file_location("_issue_2388_modern_typing", TYPING_MODULE)
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)

        self.assertIs(module.Literal, typing.Literal)
        self.assertIs(module.Protocol, typing.Protocol)
        self.assertIs(module.runtime_checkable, typing.runtime_checkable)

    def test_python37_protocol_checks_are_static_and_cover_dunder_members(self):
        compat = _load_simulated_python37_typing()

        @compat.runtime_checkable
        class SizedRunner(compat.Protocol):
            name: str

            def __len__(self) -> int: ...

            def run(self) -> int: ...

        class DescriptorBackedRunner:
            @property
            def name(self):
                raise AssertionError("property must not execute")

            def __len__(self):
                return 1

            def run(self):
                return 1

        class DynamicOnlyRunner:
            def __getattr__(self, name):
                raise AssertionError("__getattr__ must not execute")

            def __len__(self):
                return 1

            def run(self):
                return 1

        class BrokenRunner(DescriptorBackedRunner):
            run = None

        self.assertIsInstance(DescriptorBackedRunner(), SizedRunner)
        self.assertNotIsInstance(DynamicOnlyRunner(), SizedRunner)
        self.assertNotIsInstance(BrokenRunner(), SizedRunner)

    def test_python37_protocol_unsupported_semantics_fail_closed(self):
        compat = _load_simulated_python37_typing()

        class Undecorated(compat.Protocol):
            value: str

        class Plain:
            value = "x"

        with self.assertRaisesRegex(TypeError, "runtime_checkable"):
            isinstance(Plain(), Undecorated)
        with self.assertRaises(TypeError):
            compat.runtime_checkable(Plain)
        with self.assertRaisesRegex(TypeError, "issubclass"):
            issubclass(Plain, compat.runtime_checkable(Undecorated))

    def test_python37_protocol_declarations_reject_instantiation_and_concrete_decorator_misuse(self):
        compat = _load_simulated_python37_typing()

        class DeclaredProtocol(compat.Protocol):
            def run(self) -> int: ...

        class ConcreteRunner(DeclaredProtocol):
            def run(self) -> int:
                return 1

        with self.assertRaisesRegex(TypeError, "Protocols cannot be instantiated"):
            compat.Protocol()
        with self.assertRaisesRegex(TypeError, "Protocols cannot be instantiated"):
            DeclaredProtocol()

        self.assertTrue(DeclaredProtocol._is_protocol)
        self.assertFalse(ConcreteRunner._is_protocol)
        self.assertEqual(ConcreteRunner().run(), 1)
        self.assertIs(compat.runtime_checkable(DeclaredProtocol), DeclaredProtocol)
        with self.assertRaisesRegex(TypeError, "only to protocol classes"):
            compat.runtime_checkable(ConcreteRunner)

    def test_python37_concrete_protocol_descendants_use_nominal_type_checks(self):
        compat = _load_simulated_python37_typing()

        for runtime_enabled in (False, True):

            class RunnerProtocol(compat.Protocol):
                def run(self) -> int: ...

            if runtime_enabled:
                compat.runtime_checkable(RunnerProtocol)

            class ConcreteRunner(RunnerProtocol):
                def run(self) -> int:
                    return 1

            class ChildRunner(ConcreteRunner):
                pass

            class GrandchildRunner(ChildRunner):
                pass

            class SiblingRunner(RunnerProtocol):
                def run(self) -> int:
                    return 1

            class StructuralRunner:
                def run(self) -> int:
                    return 1

            cases = (
                (object, ConcreteRunner, False),
                (ConcreteRunner, ConcreteRunner, True),
                (ChildRunner, ConcreteRunner, True),
                (GrandchildRunner, ConcreteRunner, True),
                (GrandchildRunner, ChildRunner, True),
                (ChildRunner, ChildRunner, True),
                (ConcreteRunner, ChildRunner, False),
                (object, ChildRunner, False),
                (SiblingRunner, ConcreteRunner, False),
                (StructuralRunner, ConcreteRunner, False),
            )
            for candidate, target, expected in cases:
                for check, value in ((isinstance, candidate()), (issubclass, candidate)):
                    with self.subTest(
                        runtime_enabled=runtime_enabled,
                        check=check.__name__,
                        candidate=candidate.__name__,
                        target=target.__name__,
                    ):
                        self.assertIs(check(value, target), expected)

    def test_python37_protocol_declaration_rejects_every_non_protocol_base(self):
        compat = _load_simulated_python37_typing()

        class NonProtocolBase:
            pass

        with self.assertRaisesRegex(TypeError, "non-protocol"):

            class InvalidProtocol(NonProtocolBase, compat.Protocol):
                def run(self) -> int: ...

    def test_python37_class_name_cannot_change_protocol_classification(self):
        compat = _load_simulated_python37_typing()

        class Runner(compat.Protocol):
            def run(self) -> int: ...

        for name in ("Protocol", "ConcreteRunner"):
            with self.subTest(name=name):
                concrete = type(name, (Runner,), {"run": lambda self: 7})
                child = type(name, (concrete,), {})
                self.assertFalse(concrete._is_protocol)
                self.assertFalse(child._is_protocol)
                self.assertEqual(concrete().run(), 7)
                self.assertIsInstance(child(), concrete)
                self.assertTrue(issubclass(child, concrete))
                self.assertNotIsInstance(object(), concrete)
                self.assertFalse(issubclass(object, concrete))
                with self.assertRaises(TypeError):
                    compat.runtime_checkable(concrete)

    def test_python37_named_protocol_declarations_preserve_base_restrictions(self):
        compat = _load_simulated_python37_typing()

        class NonProtocol:
            pass

        for name in ("Protocol", "RunnerProtocol"):
            with self.subTest(name=name):
                declared = type(name, (compat.Protocol,), {"__annotations__": {}, "run": lambda self: None})
                self.assertTrue(declared._is_protocol)
                with self.assertRaises(TypeError):
                    declared()
                with self.assertRaises(TypeError):
                    isinstance(object(), declared)
                self.assertIs(compat.runtime_checkable(declared), declared)
                self.assertNotIsInstance(object(), declared)
                for bases in ((NonProtocol, compat.Protocol), (compat.Protocol, NonProtocol)):
                    with self.subTest(bases=bases), self.assertRaisesRegex(TypeError, "non-protocol"):
                        type(name, bases, {})

    def test_python37_default_data_members_remain_static_requirements(self):
        compat = _load_simulated_python37_typing()

        class Versioned(compat.Protocol):
            # Keep the simulated 3.7 namespace independent of newer lazy annotation descriptors.
            __annotations__ = {}
            revision = 1

        @compat.runtime_checkable
        class Runner(Versioned, compat.Protocol):
            __annotations__ = {}

            def run(self) -> int: ...

        class WithoutRevision:
            def run(self):
                return 7

            def __getattr__(self, name):
                raise AssertionError("dynamic lookup must not execute")

        class WithRevision(WithoutRevision):
            revision = None

        class HostileDescriptor:
            def __get__(self, instance, owner):
                raise AssertionError("descriptor must not execute")

        class DescriptorRevision(WithoutRevision):
            revision = HostileDescriptor()

        class BrokenMethod(WithRevision):
            run = None

        self.assertNotIsInstance(WithoutRevision(), Runner)
        self.assertIsInstance(WithRevision(), Runner)
        self.assertIsInstance(DescriptorRevision(), Runner)
        self.assertNotIsInstance(BrokenMethod(), Runner)
        with self.assertRaisesRegex(TypeError, "issubclass"):
            issubclass(WithRevision, Runner)

    def test_python37_literal_accepts_only_nonempty_json_preserving_scalars(self):
        compat = _load_simulated_python37_typing()

        alias = compat.Literal["fbx", 1, True, None, -0.5]
        self.assertIs(alias.__origin__, compat.Literal)
        self.assertEqual(alias.__args__, ("fbx", 1, True, None, -0.5))

        class CapabilityInt(int):
            def open(self):
                return None

        rejected = (
            (),
            b"fbx",
            ((1, 2),),
            ([1],),
            ({"value": 1},),
            (...,),
            (object(),),
            (CapabilityInt(1),),
            (math.nan,),
            (math.inf,),
        )
        for item in rejected:
            with self.subTest(item=repr(item)), self.assertRaises(TypeError):
                compat.Literal[item]


class PackagingBoundaryTests(unittest.TestCase):
    def test_default_dependencies_do_not_include_typing_extensions(self):
        pyproject = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
        dependency_block = pyproject.split("dependencies = [", 1)[1].split("]", 1)[0]
        self.assertNotIn("typing-extensions", dependency_block.lower())
        self.assertNotIn("typing_extensions", dependency_block.lower())

    def test_production_python_never_imports_typing_extensions(self):
        violations = []
        for path in sorted(PYTHON_PACKAGE.rglob("*.py")):
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
            for node in ast.walk(tree):
                if isinstance(node, ast.Import) and any(alias.name == "typing_extensions" for alias in node.names):
                    violations.append(path.relative_to(REPO_ROOT).as_posix())
                if isinstance(node, ast.ImportFrom) and node.module == "typing_extensions":
                    violations.append(path.relative_to(REPO_ROOT).as_posix())
        self.assertEqual(violations, [])

    def test_production_protocol_literal_callers_use_the_single_local_boundary(self):
        direct_imports = []
        protected_names = {"Literal", "Protocol", "runtime_checkable"}
        for path in sorted(PYTHON_PACKAGE.rglob("*.py")):
            if path == TYPING_MODULE:
                continue
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
            for node in ast.walk(tree):
                if not isinstance(node, ast.ImportFrom):
                    continue
                imported = protected_names.intersection(alias.name for alias in node.names)
                if imported and node.module != "dcc_mcp_core._typing":
                    direct_imports.append((path.relative_to(REPO_ROOT).as_posix(), sorted(imported)))
        self.assertEqual(direct_imports, [])


if __name__ == "__main__":
    unittest.main()
