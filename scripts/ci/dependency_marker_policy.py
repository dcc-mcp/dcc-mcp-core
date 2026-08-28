"""Conservative, stdlib-only proof that a requirement is extra-only.

This is not an environment-marker evaluator. Non-extra comparisons are
unknown (possibly true); only an actual ``extra == nonempty-string`` token
comparison proves false for the default empty extra. Unknown syntax fails
closed. Quoted values never become marker variables or boolean operators.
"""

from __future__ import annotations

import re

_TOKEN = re.compile(r"""\s*(?:('[^'\\\r\n]*'|"[^"\\\r\n]*")|([A-Za-z_][A-Za-z_0-9]*)|(===|==|!=|~=|<=|>=|<|>|\(|\)))""")
_VARIABLES = frozenset(
    (
        "python_version",
        "python_full_version",
        "os_name",
        "sys_platform",
        "platform_release",
        "platform_system",
        "platform_version",
        "platform_machine",
        "platform_python_implementation",
        "implementation_name",
        "implementation_version",
        "extra",
    )
)
_OPERATORS = frozenset(("===", "==", "!=", "~=", "<=", ">=", "<", ">", "in", "not in"))


class _DefaultPossibility:
    def __init__(self, marker: str) -> None:
        self.tokens = []
        position = 0
        while position < len(marker):
            match = _TOKEN.match(marker, position)
            if match is None:
                raise ValueError("unsupported marker token")
            quoted, word, operator = match.groups()
            self.tokens.append(("string", quoted[1:-1]) if quoted is not None else ("token", word or operator))
            position = match.end()
        self.position = 0

    def take(self):
        if self.position == len(self.tokens):
            raise ValueError("incomplete marker")
        token = self.tokens[self.position]
        self.position += 1
        return token

    def accept(self, value: str) -> bool:
        if self.position < len(self.tokens) and self.tokens[self.position] == ("token", value):
            self.position += 1
            return True
        return False

    def expression(self, depth: int = 0) -> bool:
        if depth > 32:
            raise ValueError("marker nesting limit")
        possible = self.conjunction(depth)
        while self.accept("or"):
            right = self.conjunction(depth)
            possible = possible or right
        return possible

    def conjunction(self, depth: int) -> bool:
        possible = self.atom(depth)
        while self.accept("and"):
            right = self.atom(depth)
            possible = possible and right
        return possible

    def operand(self):
        token = self.take()
        if token[0] != "string" and token[1] not in _VARIABLES:
            raise ValueError("unsupported marker variable")
        return token

    def atom(self, depth: int) -> bool:
        if self.accept("("):
            possible = self.expression(depth + 1)
            if not self.accept(")"):
                raise ValueError("unclosed marker group")
            return possible
        left = self.operand()
        kind, operator = self.take()
        if operator == "not" and self.accept("in"):
            operator = "not in"
        if kind != "token" or operator not in _OPERATORS:
            raise ValueError("unsupported marker operator")
        right = self.operand()
        if operator == "==":
            for variable, value in ((left, right), (right, left)):
                if variable == ("token", "extra") and value[0] == "string" and value[1]:
                    return False
        return True


def is_default_requirement(requirement: str) -> bool:
    """Return True unless the complete marker proves default-inactive."""
    marker = requirement.partition(";")[2].strip()
    if not marker or len(marker) > 8192:
        return True
    try:
        parser = _DefaultPossibility(marker)
        possible = parser.expression()
        return possible or parser.position != len(parser.tokens)
    except ValueError:
        return True
