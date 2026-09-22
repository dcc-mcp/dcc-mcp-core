# Skills benchmark dataset

Seed corpus for [`crates/dcc-mcp-skills-bench`](../../crates/dcc-mcp-skills-bench).

## What is in here

`seeds.json` — the real skills harvested from this repository, parsed with the
production loader (`dcc_mcp_skills::parse_skill_md`) so each entry carries the
same fields the ranker sees at runtime: SKILL.md frontmatter plus the sibling
`tools.yaml` tool declarations.

Seeds are harvested from these workspace-relative roots:

| root | why |
|---|---|
| `skills/` | skills this repository ships |
| `examples/skills/` | worked examples, including edge cases |
| `python/dcc_mcp_core/skills/` | bundled Python-side skills |
| `tests/fixtures/skills/` | loader edge cases, useful as ranking stress cases |

The corpus is *not* only these seeds. They are the real part; the benchmark
fills the rest with the deterministic synthetic generator (seed 42) to reach
the 300 and 1000 scales. That split is deliberate — a catalogue of entirely
synthetic skills measures how well the scorer separates `maya-skill-00001`
from `blender-skill-00002`, which the DCC prefix alone can do.

## Regenerating

```bash
cargo run -p dcc-mcp-skills-bench --bin skills-bench -- regenerate-seeds
```

Regenerate whenever skills are added or their frontmatter changes. There is a
test (`seeds::tests::committed_snapshot_matches_a_live_harvest`) that fails
when the snapshot is stale, so a changed skill cannot silently drift out of
the benchmark.

## Versioning

The dataset and the bench crate evolve in the same commit — there is no pinned
core version and no external repository to keep in sync. `schema` in
`seeds.json` carries `CORPUS_SCHEMA_VERSION`; a snapshot whose schema does not
match the generator is treated as absent and regenerated rather than used.

## When this moves out

The issue that created this benchmark set a split condition: extract the
dataset into its own repository once it exceeds 10 MB, or once a real cross-adapter
device matrix is needed. The bench crate stays in `dcc-mcp-core` either way —
it has to, because it drives `dcc-mcp-gateway-search` through its Rust API
rather than over HTTP.
