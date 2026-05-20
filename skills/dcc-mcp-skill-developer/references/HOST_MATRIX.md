# Host Matrix

Use this when porting patterns between existing adapters or planning a new one.

| Host family | Current signal | Default stance | Borrow this pattern |
|-------------|----------------|----------------|---------------------|
| Maya | Mature adapter with rich skills, dispatcher, readiness, resources, safe session, and stage taxonomy. | Scene and plugin operations are `affinity: main`. Pure file work can be `any`. | Strict tools.yaml metadata, stage taxonomy, cancellation, diagnostics chains, VRS for REST regressions. |
| Blender | Lean adapter around `DccServerBase`, `bpy` version probe, built-in skills path, and progressive loading helpers. | Treat `bpy` calls as host-context work. Make affinity explicit even where older skills omitted it. | Start new adapters from this small server shape. |
| 3ds Max | Adapter exists, but current skills are closer to legacy documentation than modern nested SKILL.md packages. | Treat `pymxs` work as `affinity: main`. | Migrate domain logic into `SKILL.md` + `tools.yaml` + `scripts/` with schemas and tests. |
| Houdini | Python host with node graph and file/cache workflows. | Main-thread for `hou` scene mutation; `any` for file validation and serialization. | Blend Blender's lean scaffold with Maya-style stage taxonomy for scene, authoring, interchange, and pipeline. |
| Photoshop / ZBrush / Unity | May need bridges, external processes, or non-Python APIs. | Keep host transport separated from skill contracts. | Use typed tools over bridge calls; expose diagnostics and resources for host state. |
| Custom studio host | Unknown capability and lifecycle. | Preserve unknown DCC names; do not reject at core boundaries unless a boundary requires a known host. | Parameterize names, paths, examples, and tests across at least two host families. |

## Migration Heuristics

- If a repository has rich domain scripts but no modern `tools.yaml`, migrate the
  contract first, then improve implementation internals.
- If a skill uses prose-only instructions for callable actions, split durable
  actions into typed tools.
- If a tool currently calls raw eval or script execution, ask whether the user
  intent is common enough to deserve a schema.
- If one host needs special lifecycle behavior, keep it adapter-local until a
  second host proves the abstraction belongs in core.

## Future Adapter Defaults

Start with:

- `DccServerBase`.
- Built-in skills directory.
- `search_skills` / `load_skill` progressive loading.
- Host-specific environment path plus `DCC_MCP_SKILL_PATHS`.
- Explicit execution and affinity metadata.
- Mockable scripts with lazy host imports.
- One live or executable E2E path where host behavior matters.

Add richer Maya-style systems only after a failing workflow names the need.
