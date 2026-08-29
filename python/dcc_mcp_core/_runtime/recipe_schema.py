"""Dependency-free Draft 2020-12 validator for published recipe schemas."""

from __future__ import annotations

from decimal import Decimal
from decimal import InvalidOperation
import json
import math
import re
from typing import Any
from typing import ClassVar


def _matches_json_type(value: Any, expected: Any) -> bool:
    expected_types = expected if isinstance(expected, list) else [expected]
    for item in expected_types:
        if item == "string" and isinstance(value, str):
            return True
        if item == "number" and isinstance(value, (int, float)) and not isinstance(value, bool):
            return isinstance(value, int) or math.isfinite(value)
        if item == "integer" and isinstance(value, (int, float)) and not isinstance(value, bool):
            return isinstance(value, int) or (math.isfinite(value) and value.is_integer())
        if item == "boolean" and isinstance(value, bool):
            return True
        if item == "array" and isinstance(value, list):
            return True
        if item == "object" and isinstance(value, dict):
            return True
        if item == "null" and value is None:
            return True
    return False


class _RecipeSchemaValidator:
    """Dependency-free Draft 2020-12 assertion validator for recipe inputs."""

    _TYPES: ClassVar[set[str]] = {"null", "boolean", "object", "array", "number", "integer", "string"}
    _MAX_DEPTH: ClassVar[int] = 128
    _MAX_NODES: ClassVar[int] = 10000
    _MAX_CONTAINER_ITEMS: ClassVar[int] = 10000
    _MAX_SCHEMA_DEPTH: ClassVar[int] = 128
    _MAX_SCHEMA_NODES: ClassVar[int] = 10000
    _MAX_SCHEMA_SIZE: ClassVar[int] = 1_000_000
    _MAX_INSTANCE_SIZE: ClassVar[int] = 1_000_000

    def __init__(self, schema: Any) -> None:
        self.schema = schema
        try:
            if len(json.dumps(schema, ensure_ascii=False).encode("utf-8")) > self._MAX_SCHEMA_SIZE:
                raise ValueError("schema size budget exceeded")
        except (TypeError, ValueError, OverflowError) as exc:
            raise ValueError("schema is not JSON data") from exc
        self._check_schema(schema, "$")

    def validate(self, instance: Any) -> list[str]:
        try:
            instance_size = len(json.dumps(instance, ensure_ascii=False).encode("utf-8"))
        except (TypeError, ValueError, OverflowError, RecursionError) as exc:
            raise ValueError("instance is not JSON data") from exc
        if instance_size > self._MAX_INSTANCE_SIZE:
            raise ValueError("instance size budget exceeded")
        errors: list[str] = []
        self._nodes = 0
        self._annotations = {"properties": {}, "items": {}}
        self._validate(instance, self.schema, "$", errors, set(), 0)
        return errors

    def _clone_annotations(self) -> dict[str, dict[str, set[Any]]]:
        return {
            kind: {path: set(values) for path, values in paths.items()} for kind, paths in self._annotations.items()
        }

    def _merge_annotations(self, source: dict[str, dict[str, set[Any]]]) -> None:
        for kind, paths in source.items():
            target = self._annotations[kind]
            for path, values in paths.items():
                target.setdefault(path, set()).update(values)

    @classmethod
    def _check_schema(cls, schema: Any, path: str, depth: int = 0, state: dict[str, int] | None = None) -> None:
        if state is None:
            state = {"nodes": 0}
        state["nodes"] += 1
        if depth > cls._MAX_SCHEMA_DEPTH or state["nodes"] > cls._MAX_SCHEMA_NODES:
            raise ValueError("schema resource budget exceeded")
        if isinstance(schema, bool):
            return
        if not isinstance(schema, dict):
            raise ValueError(path)
        if "$ref" in schema and not isinstance(schema["$ref"], str):
            raise ValueError(path)
        if "type" in schema:
            typ = schema["type"]
            values = typ if isinstance(typ, list) else [typ]
            if not values or any(item not in cls._TYPES for item in values) or len(set(values)) != len(values):
                raise ValueError(path)
        if "required" in schema:
            required = schema["required"]
            if (
                not isinstance(required, list)
                or any(not isinstance(item, str) for item in required)
                or len(set(required)) != len(required)
            ):
                raise ValueError(path)
        for key in ("properties", "patternProperties", "$defs", "definitions", "dependentSchemas"):
            if key in schema and not isinstance(schema[key], dict):
                raise ValueError(path)
        for key in (
            "additionalProperties",
            "unevaluatedProperties",
            "propertyNames",
            "contains",
            "items",
            "unevaluatedItems",
            "not",
            "if",
            "then",
            "else",
        ):
            if key in schema and not isinstance(schema[key], (dict, bool)):
                raise ValueError(path)
        for key in ("prefixItems", "allOf", "anyOf", "oneOf"):
            if key in schema:
                value = schema[key]
                if not isinstance(value, list) or (key in ("allOf", "anyOf", "oneOf") and not value):
                    raise ValueError(path)
                for index, item in enumerate(value):
                    cls._check_schema(item, f"{path}.{key}[{index}]", depth + 1, state)
        for key in (
            "minProperties",
            "maxProperties",
            "minItems",
            "maxItems",
            "minLength",
            "maxLength",
            "minContains",
            "maxContains",
        ):
            if key in schema and (not isinstance(schema[key], int) or isinstance(schema[key], bool) or schema[key] < 0):
                raise ValueError(path)
        for key in ("minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum", "multipleOf"):
            if key in schema and (
                not isinstance(schema[key], (int, float))
                or isinstance(schema[key], bool)
                or (isinstance(schema[key], float) and not math.isfinite(schema[key]))
            ):
                raise ValueError(path)
        if "multipleOf" in schema and schema["multipleOf"] <= 0:
            raise ValueError(path)
        if "pattern" in schema:
            if not isinstance(schema["pattern"], str):
                raise ValueError(path)
            re.compile(schema["pattern"])
        if "uniqueItems" in schema and not isinstance(schema["uniqueItems"], bool):
            raise ValueError(path)
        if "enum" in schema and (not isinstance(schema["enum"], list) or not schema["enum"]):
            raise ValueError(path)
        if "dependentRequired" in schema:
            deps = schema["dependentRequired"]
            if not isinstance(deps, dict) or any(
                not isinstance(name, str)
                or not isinstance(values, list)
                or any(not isinstance(item, str) for item in values)
                or len(set(values)) != len(values)
                for name, values in deps.items()
            ):
                raise ValueError(path)
        for key in ("properties", "patternProperties", "$defs", "definitions", "dependentSchemas"):
            for name, child in (schema.get(key) or {}).items():
                if not isinstance(name, str):
                    raise ValueError(path)
                cls._check_schema(child, f"{path}.{key}.{name}", depth + 1, state)
        if "additionalProperties" in schema:
            cls._check_schema(schema["additionalProperties"], f"{path}.additionalProperties", depth + 1, state)
        if "unevaluatedProperties" in schema:
            cls._check_schema(schema["unevaluatedProperties"], f"{path}.unevaluatedProperties", depth + 1, state)
        for key in ("propertyNames", "contains", "items", "unevaluatedItems", "not", "if", "then", "else"):
            if key in schema:
                cls._check_schema(schema[key], f"{path}.{key}", depth + 1, state)
        if "const" in schema:
            try:
                json.dumps(schema["const"])
            except (TypeError, ValueError) as exc:
                raise ValueError(path) from exc

    def _resolve_ref(self, ref: str) -> Any:
        if ref == "#":
            return self.schema
        if not ref.startswith("#/"):
            raise ValueError(ref)
        value: Any = self.schema
        for component in ref[2:].split("/"):
            component = component.replace("~1", "/").replace("~0", "~")
            if not isinstance(value, dict) or component not in value:
                raise ValueError(ref)
            value = value[component]
        return value

    @staticmethod
    def _type_name(value: Any) -> str:
        if value is None:
            return "null"
        if isinstance(value, bool):
            return "bool"
        if isinstance(value, int):
            return "int"
        if isinstance(value, float):
            return "float"
        if isinstance(value, str):
            return "str"
        if isinstance(value, list):
            return "array"
        if isinstance(value, dict):
            return "object"
        return type(value).__name__

    @staticmethod
    def _json_equal(left: Any, right: Any) -> bool:
        """Compare JSON values using JSON number (1 == 1.0) semantics."""
        if isinstance(left, bool) or isinstance(right, bool):
            return type(left) is type(right) and left == right
        if isinstance(left, (int, float)) and isinstance(right, (int, float)):
            return left == right
        if isinstance(left, list) and isinstance(right, list):
            return len(left) == len(right) and all(
                _RecipeSchemaValidator._json_equal(a, b) for a, b in zip(left, right)
            )
        if isinstance(left, dict) and isinstance(right, dict):
            return set(left) == set(right) and all(_RecipeSchemaValidator._json_equal(left[k], right[k]) for k in left)
        return type(left) is type(right) and left == right

    @staticmethod
    def _is_multiple_of(value: Any, divisor: Any) -> bool:
        try:
            if isinstance(value, int) and isinstance(divisor, int):
                return value % divisor == 0
            left = Decimal(str(value))
            right = Decimal(str(divisor))
            if not left.is_finite() or not right.is_finite() or right == 0:
                raise ValueError("multipleOf is not representable")
            left_sign, left_digits, left_exponent = left.as_tuple()
            right_sign, right_digits, right_exponent = right.as_tuple()
            left_coefficient = (-1 if left_sign else 1) * int("".join(map(str, left_digits)) or "0")
            right_coefficient = (-1 if right_sign else 1) * int("".join(map(str, right_digits)) or "0")
            exponent_delta = left_exponent - right_exponent
            if abs(exponent_delta) > 4096:
                raise ValueError("multipleOf is not representable")
            if exponent_delta >= 0:
                return (left_coefficient * (10**exponent_delta)) % right_coefficient == 0
            return left_coefficient % (right_coefficient * (10 ** (-exponent_delta))) == 0
        except (InvalidOperation, OverflowError, ValueError) as exc:
            raise ValueError("multipleOf is not representable") from exc

    @staticmethod
    def _matches(value: Any, expected: Any) -> bool:
        return _matches_json_type(value, expected)

    def _validate(
        self,
        value: Any,
        schema: Any,
        path: str,
        errors: list[str],
        resolving: set[Any],
        depth: int,
    ) -> bool:
        self._annotations["properties"].setdefault(path, set())
        self._annotations["items"].setdefault(path, set())
        self._nodes += 1
        if self._nodes > self._MAX_NODES:
            raise ValueError("schema node budget exceeded")
        if depth > self._MAX_DEPTH:
            raise ValueError("instance depth exceeded")
        if isinstance(schema, bool):
            if not schema:
                errors.append(f"{path}: Schema rejected this value")
                return False
            return True
        if not isinstance(schema, dict):
            raise ValueError(path)
        ref = schema.get("$ref")
        valid = True
        if ref is not None:
            resolution_key = (ref, path)
            if resolution_key in resolving:
                raise ValueError("recursive schema resolution exceeded")
            resolving.add(resolution_key)
            try:
                if not self._validate(value, self._resolve_ref(ref), path, errors, resolving, depth):
                    valid = False
            finally:
                resolving.remove(resolution_key)
        if "type" in schema and not self._matches(value, schema["type"]):
            if path.startswith("$.") and path.count(".") == 1 and "[" not in path and isinstance(schema["type"], str):
                name = path[2:]
                errors.append(f"Input '{name}' expected {schema['type']}, got {self._type_name(value)}")
            else:
                errors.append(f"{path}: Expected type {schema['type']}, got {self._type_name(value)}")
            return False
        if "enum" in schema and not any(self._json_equal(value, candidate) for candidate in schema["enum"]):
            errors.append(f"{path}: Value is not one of the allowed options")
            valid = False
        if "const" in schema and not self._json_equal(value, schema["const"]):
            errors.append(f"{path}: Value does not match the required constant")
            valid = False

        for _index, child in enumerate(schema.get("allOf", ())):
            base_annotations = self._annotations
            self._annotations = self._clone_annotations()
            branch_annotations = self._annotations
            branch_valid = self._validate(value, child, path, errors, resolving, depth)
            self._annotations = base_annotations
            if branch_valid:
                self._merge_annotations(branch_annotations)
            else:
                valid = False
        for key, mode, message in (("anyOf", "any", "anyOf"), ("oneOf", "one", "oneOf")):
            if key in schema:
                matches = 0
                base_annotations = self._annotations
                successful_annotations: list[dict[str, dict[str, set[Any]]]] = []
                for child in schema[key]:
                    branch_errors: list[str] = []
                    self._annotations = self._clone_annotations()
                    branch_annotations = self._annotations
                    if self._validate(value, child, path, branch_errors, resolving, depth):
                        matches += 1
                        successful_annotations.append(branch_annotations)
                    self._annotations = base_annotations
                needed = matches >= 1 if mode == "any" else matches == 1
                if needed:
                    for branch_annotations in successful_annotations:
                        self._merge_annotations(branch_annotations)
                if not needed:
                    errors.append(f"{path}: Value must satisfy {message}")
                    valid = False
        if "not" in schema:
            base_annotations = self._annotations
            self._annotations = self._clone_annotations()
            not_valid = self._validate(value, schema["not"], path, [], resolving, depth)
            self._annotations = base_annotations
            if not_valid:
                errors.append(f"{path}: Value must not satisfy the schema")
                valid = False
        if "if" in schema:
            condition: list[str] = []
            base_annotations = self._annotations
            self._annotations = self._clone_annotations()
            condition_valid = self._validate(value, schema["if"], path, condition, resolving, depth)
            condition_annotations = self._annotations
            self._annotations = base_annotations
            if condition_valid:
                self._merge_annotations(condition_annotations)
                if "then" in schema and not self._validate(value, schema["then"], path, errors, resolving, depth):
                    valid = False
            elif "else" in schema and not self._validate(value, schema["else"], path, errors, resolving, depth):
                valid = False

        if isinstance(value, (int, float)) and not isinstance(value, bool):
            if "minimum" in schema and value < schema["minimum"]:
                errors.append(f"{path}: Number is below the minimum")
                valid = False
            if "maximum" in schema and value > schema["maximum"]:
                errors.append(f"{path}: Number exceeds the maximum")
                valid = False
            if "exclusiveMinimum" in schema and value <= schema["exclusiveMinimum"]:
                errors.append(f"{path}: Number is below the exclusive minimum")
                valid = False
            if "exclusiveMaximum" in schema and value >= schema["exclusiveMaximum"]:
                errors.append(f"{path}: Number exceeds the exclusive maximum")
                valid = False
            if "multipleOf" in schema and not self._is_multiple_of(value, schema["multipleOf"]):
                errors.append(f"{path}: Number is not a multiple of the required value")
                valid = False
        if isinstance(value, str):
            length = len(value)
            if "minLength" in schema and length < schema["minLength"]:
                errors.append(f"{path}: String is shorter than the minimum length")
                valid = False
            if "maxLength" in schema and length > schema["maxLength"]:
                errors.append(f"{path}: String exceeds the maximum length")
                valid = False
            if "pattern" in schema and re.search(schema["pattern"], value) is None:
                errors.append(f"{path}: String does not match the required pattern")
                valid = False
        if isinstance(value, list):
            if len(value) > self._MAX_CONTAINER_ITEMS:
                raise ValueError("instance container budget exceeded")
            if "minItems" in schema and len(value) < schema["minItems"]:
                errors.append(f"{path}: Array has fewer than the minimum number of items")
                valid = False
            if "maxItems" in schema and len(value) > schema["maxItems"]:
                errors.append(f"{path}: Array exceeds the maximum number of items")
                valid = False
            if schema.get("uniqueItems"):
                for i, item in enumerate(value):
                    if any(self._json_equal(item, prior) for prior in value[:i]):
                        errors.append(f"{path}[{i}]: Array items must be unique")
                        valid = False
                        break
            for index, child in enumerate(schema.get("prefixItems", ())):
                if index < len(value) and not self._validate(
                    value[index], child, f"{path}[{index}]", errors, resolving, depth + 1
                ):
                    valid = False
            evaluated_items = self._annotations["items"].setdefault(path, set())
            evaluated_items.update(range(min(len(value), len(schema.get("prefixItems", ())))))
            if "items" in schema:
                start = len(schema.get("prefixItems", ()))
                for index in range(start, len(value)):
                    evaluated_items.add(index)
                    if not self._validate(
                        value[index], schema["items"], f"{path}[{index}]", errors, resolving, depth + 1
                    ):
                        valid = False
            if "contains" in schema:
                matches = 0
                for index, item in enumerate(value):
                    base_annotations = self._annotations
                    self._annotations = self._clone_annotations()
                    branch_annotations = self._annotations
                    item_valid = self._validate(item, schema["contains"], f"{path}[{index}]", [], resolving, depth + 1)
                    self._annotations = base_annotations
                    if item_valid:
                        matches += 1
                        evaluated_items.add(index)
                        self._merge_annotations(branch_annotations)
                minimum = schema.get("minContains", 1)
                maximum = schema.get("maxContains")
                if matches < minimum or (maximum is not None and matches > maximum):
                    errors.append(f"{path}: Array does not contain the required item count")
                    valid = False
            if "unevaluatedItems" in schema:
                for index, item in enumerate(value):
                    if index in evaluated_items:
                        continue
                    assertion = schema["unevaluatedItems"]
                    if assertion is False:
                        errors.append(f"{path}[{index}]: Unevaluated items are not allowed")
                        valid = False
                    elif assertion is not True and not self._validate(
                        item, assertion, f"{path}[{index}]", errors, resolving, depth + 1
                    ):
                        valid = False
        if isinstance(value, dict):
            if len(value) > self._MAX_CONTAINER_ITEMS:
                raise ValueError("instance container budget exceeded")
            required = schema.get("required", ())
            for name in required:
                if name not in value:
                    errors.append(
                        f"Missing required input: {name}"
                        if path == "$"
                        else f"{path}: Missing required property '{name}'"
                    )
                    valid = False
            if "minProperties" in schema and len(value) < schema["minProperties"]:
                errors.append(f"{path}: Object has fewer than the minimum number of properties")
                valid = False
            if "maxProperties" in schema and len(value) > schema["maxProperties"]:
                errors.append(f"{path}: Object exceeds the maximum number of properties")
                valid = False
            properties = schema.get("properties", {})
            patterns = schema.get("patternProperties", {})
            evaluated = self._annotations["properties"].setdefault(path, set())
            for name, child in properties.items():
                if name in value:
                    evaluated.add(name)
                    if not self._validate(value[name], child, f"{path}.{name}", errors, resolving, depth + 1):
                        valid = False
            for name, item in value.items():
                matched = False
                for pattern, child in patterns.items():
                    if re.search(pattern, name):
                        matched = True
                        evaluated.add(name)
                        if not self._validate(item, child, f"{path}.{name}", errors, resolving, depth + 1):
                            valid = False
                if name not in properties and not matched:
                    additional = schema.get("additionalProperties", True)
                    if "additionalProperties" in schema:
                        evaluated.add(name)
                    if additional is False:
                        errors.append(f"{path}.{name}: Additional properties are not allowed")
                        valid = False
                    elif additional is not True and not self._validate(
                        item, additional, f"{path}.{name}", errors, resolving, depth + 1
                    ):
                        valid = False
            if "dependentRequired" in schema:
                for name, dependencies in schema["dependentRequired"].items():
                    if name in value:
                        for dependency in dependencies:
                            if dependency not in value:
                                errors.append(f"{path}: Property '{dependency}' is required when '{name}' is present")
                                valid = False
            if "dependentSchemas" in schema:
                for name, child in schema["dependentSchemas"].items():
                    if name in value and not self._validate(value, child, path, errors, resolving, depth):
                        valid = False
            if "unevaluatedProperties" in schema:
                assertion = schema["unevaluatedProperties"]
                for name, item in value.items():
                    if name in evaluated:
                        continue
                    if assertion is False:
                        errors.append(f"{path}.{name}: Unevaluated properties are not allowed")
                        valid = False
                    elif assertion is not True and not self._validate(
                        item, assertion, f"{path}.{name}", errors, resolving, depth + 1
                    ):
                        valid = False
            if "propertyNames" in schema:
                for name in value:
                    if not self._validate(name, schema["propertyNames"], f"{path}.{name}", errors, resolving, depth):
                        valid = False
        return valid
