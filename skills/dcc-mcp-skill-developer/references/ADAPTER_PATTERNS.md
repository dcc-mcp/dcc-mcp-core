# Adapter Patterns

Use this reference when adding server wiring or deciding how much lifecycle
machinery a DCC adapter needs.

## Composition Root

Start from `DccServerBase` and keep the adapter's server module as a composition
root:

- Build options with `DccServerOptions.from_env(...)`.
- Pass a DCC-specific `builtin_skills_dir`.
- Preserve `dcc_name` / `dcc_type` as data, not hardcoded Maya assumptions.
- Register all tools, resources, prompts, and built-in actions before
  `server.start()`.
- Keep `start_server()` / `stop_server()` singleton helpers thin and boring.

Lean hosts can follow the Blender shape: a small subclass, version probe,
built-in skills path, and progressive loading helpers.

Mature hosts can add Maya-style hardening only when needed:

- Host execution bridge and UI-thread dispatcher.
- Readiness signals for process, dispatcher, and DCC state.
- Capability manifest for agent-visible host affordances.
- Scene, docs, command, and diagnostics resources.
- Hot reload for development skill paths.
- Safe shutdown hooks and quit handling.

## Skill Search Paths

Use a host-specific path first, then the generic path:

- `DCC_MCP_<DCC>_SKILL_PATHS`
- `DCC_MCP_SKILL_PATHS`

For example, Maya uses `DCC_MCP_MAYA_SKILL_PATHS` plus the generic shared path.
Future hosts should follow the same naming convention.

## Dispatcher Shape

Scene-touching host APIs usually need main-thread execution:

- Maya: main-thread dispatcher for `maya.cmds`, UI work, scene mutation, and
  plugin loading.
- Blender: `bpy` calls usually need host-context execution; timers or a host
  loop can drain work.
- 3ds Max: `pymxs` calls should be treated as main-thread work.
- Custom hosts: default to main-thread affinity until the API contract proves a
  call is pure, file-only, or process-local.

Use `affinity: any` only for pure Python, filesystem, validation, or serialization
steps that do not touch the live DCC process.

## When To Add Features

Add adapter features in response to real host needs:

- Add readiness when startup order or UI thread availability affects calls.
- Add resources when agents need stable context without invoking tools.
- Add capability manifests when multiple tools depend on host version or plugin
  availability.
- Add VRS traces when a bug is visible through gateway REST.
- Add live DCC E2E tests when mocks cannot prove host behavior.
