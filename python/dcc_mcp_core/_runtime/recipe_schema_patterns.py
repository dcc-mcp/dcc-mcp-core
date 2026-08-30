"""Regex safety policy for dependency-free recipe schema validation."""

from __future__ import annotations

import re


def pattern_is_safe(pattern: str) -> bool:
    """Accept only patterns whose backtracking structure is provably linear.

    Python's regular-expression engine has no portable interruption API on
    Python 3.7. A flat textual blacklist is insufficient (for example,
    ``((ab)*)*$`` hides a nested quantifier behind two groups), so this
    scanner tracks the structure of every group and quantifier. Any
    quantifier applied to a quantified or alternated atom, and every
    backreference, is rejected before instance text reaches ``re.search``.
    """
    # Each frame records whether its group already contains a quantifier or
    # an alternation. The preceding atom carries the same metadata when a
    # closing parenthesis makes the group quantifiable.
    frames: list[tuple[bool, bool, int]] = []
    frame_has_quantifier = False
    frame_has_alternation = False
    atom_present = False
    atom_has_quantifier = False
    atom_has_alternation = False
    quantifier_pending = False
    in_character_class = False
    character_class_has_item = False
    character_class_may_negate = False
    escaped = False
    index = 0

    def mark_quantifier() -> bool:
        nonlocal frame_has_quantifier
        nonlocal atom_has_quantifier
        nonlocal quantifier_pending
        if not atom_present or atom_has_quantifier or atom_has_alternation:
            return False
        frame_has_quantifier = True
        atom_has_quantifier = True
        quantifier_pending = True
        return True

    while index < len(pattern):
        character = pattern[index]
        if escaped:
            # Numeric and named backreferences are non-regular and can
            # force unbounded backtracking. Reject all digit escapes to
            # keep the accepted grammar explicit and conservative.
            if character.isdigit():
                return False
            escaped = False
            if in_character_class:
                character_class_has_item = True
                character_class_may_negate = False
            atom_present = True
            atom_has_quantifier = False
            atom_has_alternation = False
            quantifier_pending = False
            index += 1
            continue
        if character == "\\":
            escaped = True
            index += 1
            continue
        if in_character_class:
            if character == "^" and character_class_may_negate:
                character_class_may_negate = False
                index += 1
                continue
            if character == "]" and character_class_has_item:
                in_character_class = False
            else:
                character_class_has_item = True
                character_class_may_negate = False
            atom_present = True
            atom_has_quantifier = False
            atom_has_alternation = False
            quantifier_pending = False
            index += 1
            continue
        if character == "[":
            in_character_class = True
            character_class_has_item = False
            character_class_may_negate = True
            atom_present = True
            atom_has_quantifier = False
            atom_has_alternation = False
            quantifier_pending = False
            index += 1
            continue
        if character == "(":
            # Named backreferences use ``(?P=name)``; named captures are
            # regular, but rejecting the whole construct avoids ambiguity
            # in this deliberately small safety grammar.
            if pattern.startswith("(?P=", index):
                return False
            frames.append((frame_has_quantifier, frame_has_alternation, index))
            frame_has_quantifier = False
            frame_has_alternation = False
            atom_present = False
            atom_has_quantifier = False
            atom_has_alternation = False
            quantifier_pending = False
            index += 1
            continue
        if character == "|":
            frame_has_alternation = True
            atom_present = False
            atom_has_quantifier = False
            atom_has_alternation = False
            quantifier_pending = False
            index += 1
            continue
        if character == ")":
            if not frames:
                return False
            group_has_quantifier = frame_has_quantifier
            group_has_alternation = frame_has_alternation
            _, _, group_start = frames[-1]
            frame_has_quantifier, frame_has_alternation, _ = frames.pop()
            # Preserve nested structure in the enclosing group. Without
            # this propagation, ``((a|b)c)+`` would hide its alternation
            # behind an inner pair of parentheses.
            frame_has_quantifier = frame_has_quantifier or group_has_quantifier
            frame_has_alternation = frame_has_alternation or group_has_alternation
            atom_present = True
            atom_has_quantifier = group_has_quantifier
            atom_has_alternation = group_has_alternation and not _linear_alternation_group(
                pattern[group_start + 1 : index]
            )
            quantifier_pending = False
            index += 1
            continue
        if character in "*+?":
            # A question mark immediately after another quantifier is its
            # lazy suffix, not a second quantifier.
            if character == "?" and quantifier_pending:
                quantifier_pending = False
                index += 1
                continue
            if atom_present:
                if not mark_quantifier():
                    return False
            else:
                # Group prefixes such as ``?:`` and ``?=`` are syntax,
                # not quantifiers; ``re.compile`` validates them.
                atom_present = True
                atom_has_quantifier = False
                atom_has_alternation = False
                quantifier_pending = False
            index += 1
            continue
        if character == "{" and atom_present:
            end = _quantifier_end(pattern, index)
            if end > index:
                if not mark_quantifier():
                    return False
                quantifier_pending = False
                index = end
                continue
        # Ordinary literals, anchors, and group-prefix punctuation are
        # single regular atoms.
        atom_present = True
        atom_has_quantifier = False
        atom_has_alternation = False
        quantifier_pending = False
        index += 1
    # Draft ``pattern`` uses search semantics and therefore does not require
    # anchors. The structural scan above and adjacent-quantifier check below
    # keep the accepted subset linear on Python's backtracking engine.
    # Unanchored quantified groups/classes can trigger expensive search
    # retries at every offset. Keep Draft search semantics for literals and
    # simple quantified atoms, while conservatively rejecting this higher-risk
    # subset before invoking ``re.search``.
    unanchored_complex = not pattern.startswith("^") and (
        re.search(r"[)\]](?:[*+?]|\{(?:\d+(?:,\d*)?|,\d*)\})", pattern) is not None
        or re.search(r"\([^)]*[+*?{][^)]*\)", pattern) is not None
    )
    return (
        not escaped
        and not in_character_class
        and not frames
        and not unanchored_complex
        and not _has_adjacent_quantifiers(pattern)
    )


class _ConsumerSet:
    """Conservative set of code points consumed by one regex boundary."""

    _ASCII_LIMIT = 128
    _ALL_ASCII = (1 << _ASCII_LIMIT) - 1

    def __init__(
        self,
        ascii_mask: int = 0,
        non_ascii_literals: set[int] | None = None,
        *,
        non_ascii_unknown: bool = False,
    ) -> None:
        self.ascii_mask = ascii_mask
        self.non_ascii_literals = set() if non_ascii_literals is None else set(non_ascii_literals)
        self.non_ascii_unknown = non_ascii_unknown

    @classmethod
    def any_character(cls) -> _ConsumerSet:
        """Return the conservative universe of Python ``str`` characters."""
        return cls(cls._ALL_ASCII, non_ascii_unknown=True)

    @classmethod
    def literal(cls, codepoint: int, *, ignore_case: bool = False) -> _ConsumerSet:
        """Return one literal code point, or an unknown fold when needed."""
        if ignore_case:
            return cls.any_character()
        if codepoint < cls._ASCII_LIMIT:
            return cls(1 << codepoint)
        return cls(non_ascii_literals={codepoint})

    def copy(self) -> _ConsumerSet:
        """Return an independent copy."""
        return _ConsumerSet(
            self.ascii_mask,
            self.non_ascii_literals,
            non_ascii_unknown=self.non_ascii_unknown,
        )

    def update(self, other: _ConsumerSet) -> None:
        """Union another conservative consumer set into this one."""
        self.ascii_mask |= other.ascii_mask
        self.non_ascii_literals.update(other.non_ascii_literals)
        self.non_ascii_unknown = self.non_ascii_unknown or other.non_ascii_unknown

    def add_range(self, start: int, end: int, *, ignore_case: bool = False) -> None:
        """Add a class range without expanding an unbounded Unicode interval."""
        if ignore_case:
            self.update(self.any_character())
            return
        if start > end:
            self.update(self.any_character())
            return
        ascii_start = max(start, 0)
        ascii_end = min(end, self._ASCII_LIMIT - 1)
        if ascii_start <= ascii_end:
            width = ascii_end - ascii_start + 1
            self.ascii_mask |= ((1 << width) - 1) << ascii_start
        if end >= self._ASCII_LIMIT:
            if start == end:
                self.non_ascii_literals.add(start)
            else:
                self.non_ascii_unknown = True

    def is_empty(self) -> bool:
        """Return whether the summary contains no possible consumer."""
        return not self.ascii_mask and not self.non_ascii_literals and not self.non_ascii_unknown

    def overlaps(self, other: _ConsumerSet) -> bool:
        """Return whether the two summaries may consume a common code point."""
        if self.is_empty() or other.is_empty():
            return False
        if self.ascii_mask & other.ascii_mask:
            return True
        if self.non_ascii_literals.intersection(other.non_ascii_literals):
            return True
        self_has_non_ascii = self.non_ascii_unknown or bool(self.non_ascii_literals)
        other_has_non_ascii = other.non_ascii_unknown or bool(other.non_ascii_literals)
        return (self.non_ascii_unknown and other_has_non_ascii) or (other.non_ascii_unknown and self_has_non_ascii)

    def single_codepoint(self) -> int | None:
        """Return the sole exact code point, if the summary proves one."""
        if self.non_ascii_unknown:
            return None
        ascii_count = bin(self.ascii_mask).count("1")
        if ascii_count + len(self.non_ascii_literals) != 1:
            return None
        if ascii_count:
            return self.ascii_mask.bit_length() - 1
        return next(iter(self.non_ascii_literals))


class _BoundaryFrame:
    """Track consuming and quantified edges for one sequence or group."""

    def __init__(self, *, zero_width: bool = False) -> None:
        self.zero_width = zero_width
        self.first_consumers = _ConsumerSet()
        self.last_consumers = _ConsumerSet()
        self.leading_quantified = _ConsumerSet()
        self.trailing_quantified = _ConsumerSet()
        self.nullable = False
        self.branch_first = _ConsumerSet()
        self.branch_last = _ConsumerSet()
        self.branch_leading_quantified = _ConsumerSet()
        self.branch_trailing_quantified = _ConsumerSet()
        self.branch_nullable = True

    def add_atom(
        self,
        first_consumers: _ConsumerSet,
        last_consumers: _ConsumerSet,
        *,
        nullable: bool,
        leading_quantified: _ConsumerSet,
        trailing_quantified: _ConsumerSet,
    ) -> bool:
        """Add one atom and report an ambiguous quantified boundary."""
        if self.branch_trailing_quantified.overlaps(leading_quantified):
            return True
        prefix_nullable = self.branch_nullable
        if prefix_nullable:
            self.branch_first.update(first_consumers)
            self.branch_leading_quantified.update(leading_quantified)
        if nullable:
            self.branch_last.update(last_consumers)
            self.branch_trailing_quantified.update(trailing_quantified)
        else:
            self.branch_last = last_consumers.copy()
            self.branch_trailing_quantified = trailing_quantified.copy()
        self.branch_nullable = prefix_nullable and nullable
        return False

    def end_branch(self) -> None:
        """Merge the current alternative into the group's edge summary."""
        self.first_consumers.update(self.branch_first)
        self.last_consumers.update(self.branch_last)
        self.leading_quantified.update(self.branch_leading_quantified)
        self.trailing_quantified.update(self.branch_trailing_quantified)
        self.nullable = self.nullable or self.branch_nullable
        self.branch_first = _ConsumerSet()
        self.branch_last = _ConsumerSet()
        self.branch_leading_quantified = _ConsumerSet()
        self.branch_trailing_quantified = _ConsumerSet()
        self.branch_nullable = True

    def finish(self) -> tuple[_ConsumerSet, _ConsumerSet, bool, _ConsumerSet, _ConsumerSet]:
        """Return the group's consuming and quantified edge summary."""
        self.end_branch()
        if self.zero_width:
            return _ConsumerSet(), _ConsumerSet(), True, _ConsumerSet(), _ConsumerSet()
        return (
            self.first_consumers,
            self.last_consumers,
            self.nullable,
            self.leading_quantified,
            self.trailing_quantified,
        )


def _has_adjacent_quantifiers(pattern: str) -> bool:
    """Detect adjacent quantified atoms in one linear structural pass."""
    frames = [_BoundaryFrame()]
    ignore_case = _pattern_may_ignore_case(pattern)
    index = 0
    while index < len(pattern):
        character = pattern[index]
        if character == "(":
            content_start, zero_width = _group_content_start(pattern, index + 1)
            frames.append(_BoundaryFrame(zero_width=zero_width))
            index = content_start
            continue
        if character == ")":
            if len(frames) == 1:
                return False
            first_consumers, last_consumers, nullable, leading_quantified, trailing_quantified = frames.pop().finish()
            atom_end = index + 1
            quantifier_end = _quantifier_end(pattern, atom_end)
            if quantifier_end > atom_end:
                nullable = nullable or _quantifier_allows_zero(pattern, atom_end)
                leading_quantified = first_consumers.copy()
                trailing_quantified = last_consumers.copy()
            if frames[-1].add_atom(
                first_consumers,
                last_consumers,
                nullable=nullable,
                leading_quantified=leading_quantified,
                trailing_quantified=trailing_quantified,
            ):
                return True
            index = quantifier_end
            continue
        if character == "|":
            frames[-1].end_branch()
            index += 1
            continue
        if character in "^$":
            if frames[-1].add_atom(
                _ConsumerSet(),
                _ConsumerSet(),
                nullable=True,
                leading_quantified=_ConsumerSet(),
                trailing_quantified=_ConsumerSet(),
            ):
                return True
            index += 1
            continue

        if character == "\\":
            atom_end, consumers, zero_width = _escaped_consumer(pattern, index, ignore_case=ignore_case)
        elif character == "[":
            atom_end, consumers = _character_class_consumer(pattern, index, ignore_case=ignore_case)
            zero_width = False
        elif character == ".":
            atom_end = index + 1
            consumers = _ConsumerSet.any_character()
            zero_width = False
        else:
            atom_end = index + 1
            consumers = _ConsumerSet.literal(ord(character), ignore_case=ignore_case)
            zero_width = False

        quantifier_end = _quantifier_end(pattern, atom_end)
        quantified = quantifier_end > atom_end
        nullable = zero_width or (quantified and _quantifier_allows_zero(pattern, atom_end))
        quantified_consumers = consumers.copy() if quantified else _ConsumerSet()
        if frames[-1].add_atom(
            consumers,
            consumers,
            nullable=nullable,
            leading_quantified=quantified_consumers,
            trailing_quantified=quantified_consumers,
        ):
            return True
        index = quantifier_end
    return False


def _pattern_may_ignore_case(pattern: str) -> bool:
    """Return whether any inline flag enables case-insensitive matching."""
    escaped = False
    in_class = False
    class_has_item = False
    class_may_negate = False
    index = 0
    while index < len(pattern):
        character = pattern[index]
        if escaped:
            escaped = False
            if in_class:
                class_has_item = True
                class_may_negate = False
            index += 1
            continue
        if character == "\\":
            escaped = True
            index += 1
            continue
        if in_class:
            if character == "^" and class_may_negate:
                class_may_negate = False
            elif character == "]" and class_has_item:
                in_class = False
            else:
                class_has_item = True
                class_may_negate = False
            index += 1
            continue
        if character == "[":
            in_class = True
            class_has_item = False
            class_may_negate = True
            index += 1
            continue
        if pattern.startswith("(?", index):
            cursor = index + 2
            while cursor < len(pattern) and pattern[cursor] in "aiLmsux-":
                if pattern[cursor] == "i" and "-" not in pattern[index + 2 : cursor]:
                    return True
                cursor += 1
        index += 1
    return False


_ASCII_DIGIT_MASK = ((1 << 10) - 1) << ord("0")
_ASCII_SPACE_MASK = sum(1 << ord(character) for character in " \t\n\r\f\v")
_ASCII_WORD_MASK = _ASCII_DIGIT_MASK | (((1 << 26) - 1) << ord("A")) | (((1 << 26) - 1) << ord("a")) | (1 << ord("_"))


def _escaped_consumer(
    pattern: str,
    index: int,
    *,
    ignore_case: bool,
    in_class: bool = False,
) -> tuple[int, _ConsumerSet, bool]:
    """Parse one escape into its conservative consumer and width."""
    if index + 1 >= len(pattern):
        return len(pattern), _ConsumerSet.any_character(), False
    escaped = pattern[index + 1]
    end = index + 2
    if not in_class and escaped in "AbBZ":
        return end, _ConsumerSet(), True
    if in_class and escaped == "b":
        return end, _ConsumerSet.literal(8, ignore_case=ignore_case), False
    category_masks = {"d": _ASCII_DIGIT_MASK, "s": _ASCII_SPACE_MASK, "w": _ASCII_WORD_MASK}
    if escaped in category_masks:
        return end, _ConsumerSet(category_masks[escaped], non_ascii_unknown=True), False
    if escaped in "DSW":
        return end, _ConsumerSet.any_character(), False
    simple_escapes = {"a": 7, "f": 12, "n": 10, "r": 13, "t": 9, "v": 11}
    if escaped in simple_escapes:
        return end, _ConsumerSet.literal(simple_escapes[escaped], ignore_case=ignore_case), False
    widths = {"x": 2, "u": 4, "U": 8}
    if escaped in widths:
        digits_end = end + widths[escaped]
        try:
            codepoint = int(pattern[end:digits_end], 16)
        except ValueError:
            return min(digits_end, len(pattern)), _ConsumerSet.any_character(), False
        return digits_end, _ConsumerSet.literal(codepoint, ignore_case=ignore_case), False
    if escaped == "N" and end < len(pattern) and pattern[end] == "{":
        name_end = pattern.find("}", end + 1)
        return (name_end + 1 if name_end >= 0 else len(pattern)), _ConsumerSet.any_character(), False
    if escaped.isalnum():
        return end, _ConsumerSet.any_character(), False
    return end, _ConsumerSet.literal(ord(escaped), ignore_case=ignore_case), False


def _character_class_consumer(
    pattern: str,
    index: int,
    *,
    ignore_case: bool,
) -> tuple[int, _ConsumerSet]:
    """Parse a class, including a literal leading ``]``, into consumers."""
    cursor = index + 1
    negated = cursor < len(pattern) and pattern[cursor] == "^"
    if negated:
        cursor += 1
    consumers = _ConsumerSet()
    if cursor < len(pattern) and pattern[cursor] == "]":
        consumers.update(_ConsumerSet.literal(ord("]"), ignore_case=ignore_case))
        cursor += 1
    while cursor < len(pattern):
        if pattern[cursor] == "]":
            return cursor + 1, _ConsumerSet.any_character() if negated else consumers
        item_end, item_consumers = _class_element(pattern, cursor, ignore_case=ignore_case)
        range_start = item_consumers.single_codepoint()
        if (
            item_end < len(pattern)
            and pattern[item_end] == "-"
            and item_end + 1 < len(pattern)
            and pattern[item_end + 1] != "]"
        ):
            range_end_offset, range_consumers = _class_element(
                pattern,
                item_end + 1,
                ignore_case=ignore_case,
            )
            range_end = range_consumers.single_codepoint()
            if range_start is None or range_end is None:
                consumers.update(_ConsumerSet.any_character())
            else:
                consumers.add_range(range_start, range_end, ignore_case=ignore_case)
            cursor = range_end_offset
            continue
        consumers.update(item_consumers)
        cursor = item_end
    return len(pattern), _ConsumerSet.any_character()


def _class_element(
    pattern: str,
    index: int,
    *,
    ignore_case: bool,
) -> tuple[int, _ConsumerSet]:
    """Parse one character-class element."""
    if pattern[index] == "\\":
        end, consumers, _ = _escaped_consumer(pattern, index, ignore_case=ignore_case, in_class=True)
        return end, consumers
    return index + 1, _ConsumerSet.literal(ord(pattern[index]), ignore_case=ignore_case)


def _group_content_start(pattern: str, start: int) -> tuple[int, bool]:
    """Skip zero-width group prefix syntax and return the content offset."""
    for prefix in ("?:", "?=", "?!", "?<=", "?<!", "?>"):
        if pattern.startswith(prefix, start):
            return start + len(prefix), prefix in {"?=", "?!", "?<=", "?<!"}
    if pattern.startswith("?P<", start):
        name_end = pattern.find(">", start + 3)
        return (name_end + 1, False) if name_end >= 0 else (start, False)
    if pattern.startswith("?#", start):
        comment_end = pattern.find(")", start + 2)
        return (comment_end, True) if comment_end >= 0 else (start, True)
    if start < len(pattern) and pattern[start] == "?":
        cursor = start + 1
        while cursor < len(pattern) and pattern[cursor] in "aiLmsux-":
            cursor += 1
        if cursor < len(pattern) and pattern[cursor] == ":":
            return cursor + 1, False
        if cursor < len(pattern) and pattern[cursor] == ")":
            return cursor, True
    return start, False


def _quantifier_end(pattern: str, atom_end: int) -> int:
    """Return the first offset after an atom's optional quantifier."""
    if atom_end >= len(pattern):
        return atom_end
    character = pattern[atom_end]
    if character in "*+?":
        end = atom_end + 1
    elif character == "{":
        end = atom_end + 1
        minimum_start = end
        while end < len(pattern) and pattern[end].isdigit():
            end += 1
        has_minimum = end > minimum_start
        if end < len(pattern) and pattern[end] == ",":
            end += 1
            maximum_start = end
            while end < len(pattern) and pattern[end].isdigit():
                end += 1
            has_maximum = end > maximum_start
            if not has_minimum and not has_maximum and (end >= len(pattern) or pattern[end] != "}"):
                return atom_end
        elif not has_minimum:
            return atom_end
        if end >= len(pattern) or pattern[end] != "}":
            return atom_end
        end += 1
    else:
        return atom_end
    if end < len(pattern) and pattern[end] == "?":
        end += 1
    return end


def _quantifier_allows_zero(pattern: str, atom_end: int) -> bool:
    """Return whether the quantifier at ``atom_end`` permits zero matches."""
    character = pattern[atom_end]
    if character in "*?":
        return True
    if character == "+":
        return False
    minimum_end = atom_end + 1
    while minimum_end < len(pattern) and pattern[minimum_end].isdigit():
        minimum_end += 1
    minimum = pattern[atom_end + 1 : minimum_end]
    return not minimum or int(minimum) == 0


def _linear_alternation_group(body: str) -> bool:
    """Return whether a group is a disjoint fixed-literal alternation."""
    for prefix in ("?:", "?=", "?!", "?<=", "?<!", "?>"):
        if body.startswith(prefix):
            body = body[len(prefix) :]
            break
    branches: list[str] = []
    start = 0
    escaped = False
    depth = 0
    in_class = False
    for index, character in enumerate(body):
        if escaped:
            escaped = False
            continue
        if character == "\\":
            escaped = True
            continue
        if in_class:
            if character == "]":
                in_class = False
            continue
        if character == "[":
            in_class = True
        elif character == "(":
            depth += 1
        elif character == ")" and depth:
            depth -= 1
        elif character == "|" and depth == 0:
            branches.append(body[start:index])
            start = index + 1
    if in_class or escaped or depth:
        return False
    branches.append(body[start:])
    if len(branches) < 2:
        return False
    tokens: list[str] = []
    for branch in branches:
        if not branch or any(character in "\\[](){}*+?^$|." for character in branch):
            return False
        tokens.append(branch[0])
    return len(set(tokens)) == len(tokens)
