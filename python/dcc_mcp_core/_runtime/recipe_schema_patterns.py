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
            if character == "]":
                in_character_class = False
            atom_present = True
            atom_has_quantifier = False
            atom_has_alternation = False
            quantifier_pending = False
            index += 1
            continue
        if character == "[":
            in_character_class = True
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
            end = index + 1
            while end < len(pattern) and pattern[end].isdigit():
                end += 1
            if end > index + 1:
                if end < len(pattern) and pattern[end] == ",":
                    end += 1
                    while end < len(pattern) and pattern[end].isdigit():
                        end += 1
                if end < len(pattern) and pattern[end] == "}":
                    if not mark_quantifier():
                        return False
                    index = end + 1
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
        re.search(r"[)\]](?:[*+?]|\{\d)", pattern) is not None
        or re.search(r"\([^)]*[+*?{][^)]*\)", pattern) is not None
    )
    return (
        not escaped
        and not in_character_class
        and not frames
        and not unanchored_complex
        and not _has_adjacent_quantifiers(pattern)
    )


class _BoundaryFrame:
    """Track quantified boundaries for one regex sequence or group."""

    def __init__(self, *, zero_width: bool = False) -> None:
        self.zero_width = zero_width
        self.first_tokens: set[str] = set()
        self.last_tokens: set[str] = set()
        self.nullable = False
        self.branch_first: set[str] = set()
        self.branch_last: set[str] = set()
        self.branch_nullable = True

    def add_atom(self, first_tokens: set[str], last_tokens: set[str], *, nullable: bool) -> bool:
        """Add one atom and report whether its leading boundary overlaps."""
        if _quantifier_boundaries_overlap(self.branch_last, first_tokens):
            return True
        if self.branch_nullable:
            self.branch_first.update(first_tokens)
        if nullable:
            self.branch_last.update(last_tokens)
        else:
            self.branch_last = set(last_tokens)
        self.branch_nullable = self.branch_nullable and nullable
        return False

    def end_branch(self) -> None:
        """Merge the current alternative into the group's edge summary."""
        self.first_tokens.update(self.branch_first)
        self.last_tokens.update(self.branch_last)
        self.nullable = self.nullable or self.branch_nullable
        self.branch_first = set()
        self.branch_last = set()
        self.branch_nullable = True

    def finish(self) -> tuple[set[str], set[str], bool]:
        """Return all quantified tokens exposed by the group's outer edges."""
        self.end_branch()
        if self.zero_width:
            return set(), set(), True
        return self.first_tokens, self.last_tokens, self.nullable


def _has_adjacent_quantifiers(pattern: str) -> bool:
    """Detect adjacent quantified atoms in one linear structural pass."""
    frames = [_BoundaryFrame()]
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
            first_tokens, last_tokens, nullable = frames.pop().finish()
            atom_end = index + 1
            quantifier_end = _quantifier_end(pattern, atom_end)
            if quantifier_end > atom_end:
                first_tokens = {"<group>"}
                last_tokens = first_tokens
                nullable = nullable or _quantifier_allows_zero(pattern, atom_end)
            if frames[-1].add_atom(first_tokens, last_tokens, nullable=nullable):
                return True
            index = quantifier_end
            continue
        if character == "|":
            frames[-1].end_branch()
            index += 1
            continue
        if character in "^$":
            index += 1
            continue

        token = character
        if character == "\\":
            atom_end = min(index + 2, len(pattern))
            token = pattern[index:atom_end]
        elif character == "[":
            atom_end = index + 1
            escaped = False
            while atom_end < len(pattern):
                current = pattern[atom_end]
                if escaped:
                    escaped = False
                elif current == "\\":
                    escaped = True
                elif current == "]":
                    atom_end += 1
                    break
                atom_end += 1
            token = "<class>"
        else:
            atom_end = index + 1

        quantifier_end = _quantifier_end(pattern, atom_end)
        quantified_tokens = {token} if quantifier_end > atom_end else set()
        nullable = quantifier_end > atom_end and _quantifier_allows_zero(pattern, atom_end)
        if frames[-1].add_atom(quantified_tokens, quantified_tokens, nullable=nullable):
            return True
        index = quantifier_end
    return False


def _quantifier_boundaries_overlap(previous: set[str], current: set[str]) -> bool:
    """Return whether adjacent quantified boundaries can consume the same text."""
    if not previous or not current:
        return False
    if "<group>" in current:
        return True
    if "<class>" in current:
        return True
    if "<group>" in previous:
        return True
    return bool(previous.intersection(current))


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
        while end < len(pattern) and pattern[end].isdigit():
            end += 1
        if end == atom_end + 1:
            return atom_end
        if end < len(pattern) and pattern[end] == ",":
            end += 1
            while end < len(pattern) and pattern[end].isdigit():
                end += 1
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
    return int(pattern[atom_end + 1 : minimum_end]) == 0


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
