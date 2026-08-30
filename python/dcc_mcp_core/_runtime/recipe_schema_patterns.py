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


def _has_adjacent_quantifiers(pattern: str) -> bool:
    """Detect adjacent quantified atoms that can overlap or explode."""
    previous_quantified: str | None = None
    previous_group_quantified = False
    index = 0
    while index < len(pattern):
        character = pattern[index]
        if character == "(":
            # Treat a complete group as one atom. The main structural scanner
            # validates its contents; this pass additionally catches quantified
            # groups placed next to one another, which otherwise reset state at
            # each parenthesis and allow exponential alternation backtracking.
            depth = 1
            cursor = index + 1
            escaped = False
            in_class = False
            while cursor < len(pattern) and depth:
                current = pattern[cursor]
                if escaped:
                    escaped = False
                elif current == "\\":
                    escaped = True
                elif in_class:
                    if current == "]":
                        in_class = False
                elif current == "[":
                    in_class = True
                elif current == "(":
                    depth += 1
                elif current == ")":
                    depth -= 1
                cursor += 1
            if depth or in_class:
                return False
            quantifier_end = cursor
            if quantifier_end < len(pattern) and pattern[quantifier_end] in "*+?":
                quantifier_end += 1
            elif quantifier_end < len(pattern) and pattern[quantifier_end] == "{":
                end = pattern.find("}", quantifier_end + 1)
                if end >= 0:
                    quantifier_end = end + 1
            quantified = quantifier_end > cursor
            if quantified and (previous_group_quantified or previous_quantified is not None):
                return True
            previous_group_quantified = quantified
            previous_quantified = None
            index = quantifier_end
            continue
        if character == "\\":
            index += 2
            previous_quantified = None
            previous_group_quantified = False
            continue
        if character == "[":
            end = index + 1
            while end < len(pattern) and pattern[end] != "]":
                end += 2 if pattern[end] == "\\" else 1
            atom_end = min(end + 1, len(pattern))
            quantifier_end = atom_end
            if quantifier_end < len(pattern) and pattern[quantifier_end] in "*+?":
                quantifier_end += 1
            elif quantifier_end < len(pattern) and pattern[quantifier_end] == "{":
                close = pattern.find("}", quantifier_end + 1)
                if close >= 0:
                    quantifier_end = close + 1
            quantified = quantifier_end > atom_end
            if quantified and previous_quantified is not None:
                return True
            previous_quantified = "<class>" if quantified else None
            previous_group_quantified = False
            index = quantifier_end
            continue
        if character in "^$|()":
            previous_quantified = None
            previous_group_quantified = False
            index += 1
            continue
        token = character
        quantifier_end = index + 1
        if quantifier_end < len(pattern) and pattern[quantifier_end] in "*+?":
            quantifier_end += 1
        elif quantifier_end < len(pattern) and pattern[quantifier_end] == "{":
            end = pattern.find("}", quantifier_end + 1)
            if end >= 0:
                quantifier_end = end + 1
        if quantifier_end > index + 1:
            if previous_group_quantified or (previous_quantified is not None and previous_quantified == token):
                return True
            previous_quantified = token
            previous_group_quantified = False
            index = quantifier_end
        else:
            previous_quantified = None
            previous_group_quantified = False
            index += 1
    return False


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
