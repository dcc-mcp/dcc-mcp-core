# VX + just new-project toolchain template

A small, copy-ready toolchain standard for **new DCC-MCP projects and adapters**.
Copy this folder into a new repository root, or cherry-pick the files into an
existing one. It is the reference implementation behind the "VX + just by
default" guidance in `CONTRIBUTING.md` and
[New Adapter Onboarding](../../docs/guide/new-adapter-onboarding.md).

## The standard (five rules)

1. **Fixed tool versions, never floating.** `vx.toml` declares exact tool
   versions and a committed `vx.lock` pins the resolved version + per-platform
   download URL for every tool. A `latest` constraint lets a stale cache or a
   silent upgrade change what every developer and CI job runs without a
   reviewable diff. Refresh the lock with `vx lock`; never hand-edit it.
2. **One entry point, shared by local and CI.** Every task lives in the
   `justfile`. Local developers and CI both run `vx just ci`. There is no
   separate CI-only command set to drift out of sync.
3. **Everything runs through `vx`.** Recipes call `vx <tool>` so the resolved
   version is the pinned one — never a bare tool from `PATH`. No silent fallback
   to a bare tool is allowed to manufacture a green result.
4. **Fail closed on required tooling.** The action ref is pinned to a reviewed
   release commit *and* the `version` input is explicit (it defaults to `latest`
   and ignores the ref). `cache: false` avoids a stale cached binary skipping
   install. A dedicated guard asserts `vx.lock` is present, because
   `vx lock --check` exits 0 when the file is missing.
5. **Prove execution, don't trust setup.** `doctor` and `verify.py` spawn real
   `vx` child processes; a broken pin, a wrong binary version, or a bare-tool
   shim makes them exit non-zero. A green "Setup vx" step is not evidence.

## Files in this template

| File | Purpose |
|------|---------|
| `vx.toml` | Declarative, fixed tool versions (`[tools]`); no `[scripts]` |
| `vx.lock` | Committed, generated lock with per-platform download URLs |
| `justfile` | Single local/CI entry point (`ci`), all recipes via `vx` |
| `verify.py` | Stdlib-only verification harness (real `vx` child processes) |
| `ci.yml.example` | Cross-platform Windows/macOS/Linux CI (copy to `.github/workflows/ci.yml`) |
| `vx-lock-guard.yml.example` | Optional standalone fail-closed lock guard |
| `aftereffects-ci.yml.example` | Worked AE adapter migration (design sketch, not applied) |

`.yml.example` files use a non-workflow extension on purpose: they are templates
to copy, not live workflows to be discovered by the adopting repo until you
deliberately rename them. This kept them out of `actionlint`/`check-yaml` while
still being fully lint-clean (validated with `actionlint` before shipping).

## Adopt in a new project

```bash
# 1. Copy the four core files into the new repo root.
cp vx.toml vx.lock justfile verify.py /path/to/new-repo/

# 2. Copy the CI example and drop the .example suffix.
mkdir -p /path/to/new-repo/.github/workflows
cp ci.yml.example /path/to/new-repo/.github/workflows/ci.yml

# 3. Local verification (identical recipe set to CI).
cd /path/to/new-repo
vx just ci          # lock-check -> doctor -> verify

# 4. Once you have source + tests, extend `ci` in the justfile with
#    fmt-check / lint / test, keeping `ci` as the single entry point.
```

`vx.toml` pins a day-to-day dev interpreter (Python 3.12 here). Projects that
must support a floor/latest Python matrix express it in CI via `vx python@3.9`,
`vx python@3.11`, `vx python@3.12` command-line overrides (see
`aftereffects-ci.yml.example`), not by adding a second drifting tool list.

## Verification harness (`verify.py`)

`verify.py` is dependency-free (stdlib only) and isolated: it never reads or
writes the developer's `HOME`, `PATH`, or Windows registry. It runs real `vx`
child processes against an isolated, throw-away `VX_HOME`/`HOME` and prints a
PASS/FAIL/SKIP table, exiting non-zero if any check fails.

| Check | Evidence |
|-------|----------|
| `vx_binary_executes` | pinned vx binary runs; asserts its version (`--expect-vx` / `VX_EXPECT_VERSION`) |
| `pinned_tool_version` | `just` resolves to the version pinned in `vx.toml`, not a bare `PATH` tool |
| `lock_consistent` | committed `vx.lock` passes `vx lock --check` |
| `lock_drift_fails_closed` | editing `vx.toml` without re-locking returns non-zero + a stderr diagnostic |
| `missing_lock_trap_documented` | confirms `vx lock --check` exits 0 on a MISSING lock → guard must assert presence |
| `cold_warm_cache` | a fresh `VX_HOME` installs the tool; a warm run reuses it |
| `version_override_isolation` | `vx just@1.57 --version` wins over the `1.58` pin (shim/version isolation) |
| `whitespace_unicode_path` | resolution + lock check work from a path with a space and non-ASCII characters |

Every subprocess is bounded by a hard timeout (cancel evidence), and all state
lives in `TemporaryDirectory` fixtures that are removed on exit (cleanup
evidence). Native DCC GUI/license runs are intentionally **NOT_RUN** here.

Run it directly or from CI:

```bash
python verify.py                         # local
VX_EXPECT_VERSION=0.9.34 python verify.py   # assert the pinned vx binary
```

## Supported runtimes

The pinned tool versions in `vx.toml` are the day-to-day dev toolchain. A
project on the Python 3.7 floor (Maya 2022 / Blender 2.83) keeps its py37
CI/wheel/syntax gates; VX command-line overrides (`vx python@3.7`,
`vx python@3.12`) express the floor/latest matrix in CI without a second tool
list. This template does not remove or weaken any existing runtime gate.

## Incremental migration

Existing projects migrate one at a time; this is not a blanket rewrite. The AE
worked example (`aftereffects-ci.yml.example`) shows how to migrate one existing
adapter's non-native contract/test job while preserving floor/latest
compatibility and build/`twine` release-security gates. Apply it only after this
Core pilot passes review.
