# Testing And Release

Use the smallest test that proves the adapter contract, then add one live or
HTTP-level smoke when behavior crosses process boundaries.

## Test Layers

| Layer | What to prove |
|---|---|
| Unit | option resolution, server construction, env vars, skill path collection |
| Dispatcher | main-affinity calls run on the host dispatcher and return envelopes |
| Skill lifecycle | `search_skills` -> `load_skill` -> typed tool -> `unload_skill` |
| REST/MCP | direct `/mcp` or `/v1/*` search, describe, load, and call |
| Gateway | multi-instance routing, policy, compact responses, debug traces |
| Live DCC | one host smoke that creates/queries/cleans up real scene state |
| Packaging | wheel or plugin archive installs into the target host runtime |

## Validation Commands

Prefer repository-native commands. For Python projects, prefer `vx uv` when it
is available in the environment, then fall back to direct Python only when the
wrapper is unavailable or hides behavior you need to inspect.

Typical gates:

```bash
python -m ruff check src tests
python -m ruff format --check src tests
python -m pytest
```

For Rust/PyO3 core changes, run the workspace's `just` or `cargo` gates that
match the touched crates.

For `dcc-mcp-core` toolchain or dependency refreshes, prefer vx-managed Cargo so
local runs match CI:

```bash
vx cargo update
vx cargo tree -d
vx cargo build --workspace --all-targets --timings
```

## Live-DCC Smoke Shape

Every adapter should eventually provide one documented smoke:

1. Start the DCC host in the supported mode.
2. Start or load the adapter.
3. Discover one skill.
4. Load it.
5. Call one safe typed tool.
6. Verify host-visible state.
7. Stop the adapter and ensure registry rows are gone.

If CI cannot run the real DCC, keep the mock HTTP test in CI and document the
manual live smoke command in the adapter repository.

## PR Notes

PR descriptions should include:

- short summary of runtime or skill behavior changed;
- validation commands, without machine-specific paths;
- any live-DCC gap that remains;
- linked core issues for deferred shared APIs.
