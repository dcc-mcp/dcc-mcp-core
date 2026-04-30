# Example Rez Skill Packages

These packages show how to split DCC MCP skills by production context. They are
tiny on purpose: each package contributes one focused skill, one tool schema,
and a minimal script so pipeline teams can copy the layout into real Rez
packages.

Prefer composing several narrow packages over one huge `studio-skills` package.
The context bundle should load only the tools that match the active project,
task, asset type, and DCC host.

| Package | Context |
|---------|---------|
| `commercial_2d_layers` | Photoshop layered deliverables and motion handoff |
| `commercial_3d_product_render` | Product lookdev and render review |
| `film_animation_blocking` | Maya shot blocking and playblast review |
| `film_fx_cache_review` | Houdini simulation/cache review |
| `game_level_layout` | Game level layout checks |
| `asset_material_authoring` | Material graph and texture export checks |

See `docs/guide/rez-skill-packages.md` and `examples/context-bundles/` for the
matching launch-context manifests.
