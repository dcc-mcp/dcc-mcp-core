# B3 owned pixels game input

The owner may explicitly add `game_navigation` and `relative_mouse` to
`UiControlRuntimeOptions.allowed_actions`. The default grants remain empty.
This route requires an integrity-pinned Windows Native runtime advertising the
exact `dcc-cua.game-navigation.b3.v1` capability, profile `game_b3.v1`, and the
requested pixels action scopes. Existing scopes do not require this capability.
Native must independently verify the same capability in its actual Host hello
before opening a task. Core preserves a typed startup `unsupported` rejection;
it never infers compatibility from a version range or error message.

After a fresh snapshot, a combined action is one `ui_control__act` request:

```json
{
  "action": "game_navigation",
  "intent": "game_navigation",
  "keys": ["W+LeftShift"],
  "duration_ms": 250,
  "dx": 32,
  "dy": -8,
  "snapshot_id": "the-current-snapshot-id"
}
```

This requires both immutable action grants and both keyboard and raw-coordinate
policy permissions. Core emits one canonical raw-input foreground action. Native
owns the bounded DOWN, relative MOVE while keys remain down, and UP transaction.
Core never splits it into two calls or creates a cross-call key lease.

The eleven canonical keys are `W,A,S,D,SPACE,LSHIFT,LCTRL,Z,E,F,M`. The frontend
splits `+`, trims ASCII text, uppercases, then maps only `SHIFT`/`LEFTSHIFT` to
`LSHIFT` and `CTRL`/`CONTROL`/`LEFTCTRL`/`LEFTCONTROL` to `LCTRL`. Duplicates after
normalization, empty or unknown tokens, and more than four keys are rejected.
The wire accepts canonical individual keys only. Legacy canvas vocabulary is
unchanged; this profile adds no arrow keys or `SPACEBAR` alias.

`duration_ms` is a strict integer from 0 to 500. Omission emits 0, meaning a
same-call tap. Explicit null, booleans and floats are invalid. Optional `dx` and
`dy` must appear together as integers from -256 to 256, with a nonzero pair.
Standalone `relative_mouse` requires `intent=game_navigation` and the pair; it
accepts no keys, button, duration, path or absolute coordinates. Deltas undergo
no screenshot or DPI transform. Unknown action fields are rejected, including
a caller-supplied profile. Request arguments cannot widen owner grants.

Each call consumes its observation. Native results retain `effect=unverifiable`
and require state verification; a successful transport acknowledgement is not
proof of gameplay movement, posture, camera change or physical key release.
Failed game delivery isolates the owned task, without replay or a release claim.
Native owns deadline propagation, queue ownership through worker cleanup and
physical release evidence.

The shared fixture and descriptor are preserved byte-for-byte in
`tests/fixtures/cua_game_input`. Tests cover the canonical wire, frontend aliases,
strict negotiation, policy/grant denial and the real Python Skill bridge against
an in-memory MCP process. These tests do not launch a Host or prove live input.
An independently frozen Core wheel and matching Native binary still require
exact artifact pairing and real target-bound application E2E acceptance.
