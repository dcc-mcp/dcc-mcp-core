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
