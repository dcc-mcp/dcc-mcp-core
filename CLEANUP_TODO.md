# CLEANUP_TODO.md

Track non-urgent cleanup items for future runs. Items here are safe to defer
because the current code is functionally correct.

---

## Dependency Upgrades

### `axum-test`: v17 → v20 (dev-dependency only)

- **File**: `crates/dcc-mcp-http/Cargo.toml`
- **Current**: `axum-test = "17"` (locked at 17.3.0)
- **Available**: v20.0.0
- **Risk**: Major version bump — likely has breaking API changes in test helpers
- **Action**: Review axum-test v20 changelog before upgrading; update test code in `crates/dcc-mcp-http/src/tests.rs` accordingly
- **Priority**: Low (tests still pass with v17)

### GitHub Dependabot: 3 moderate vulnerabilities on default branch

- **Location**: `https://github.com/loonghao/dcc-mcp-core/security/dependabot`
- **Scope**: Likely indirect (transitive) dependencies
- **Action**: Review dependabot alerts and update affected crates via `cargo update`
- **Priority**: Medium — should be addressed before next release
- **Run #95 update**: `cargo update` applied 3 patch bumps (fastrand 2.4.0→2.4.1, tokio 1.51.0→1.51.1, toml_edit 0.25.10→0.25.11). Cargo.lock excluded by .gitignore (correct for library). CVE status unchanged — Dependabot alerts are for transitive deps on default branch, not resolvable via patch updates alone.

---

## Structural Observations (Stage 9)

### `dcc-mcp-http/src/protocol.rs` — 21 public items in one file

- ~350 lines with 21 `pub struct/enum` definitions
- Borderline (< 500 lines) but growing fast as MCP protocol evolves
- **Action**: Consider splitting into `protocol/types.rs`, `protocol/request.rs`, `protocol/response.rs` if it grows beyond 500 lines
- **Priority**: Low

### `ControlMessage` enum with `#[allow(dead_code)]`

- **File**: `crates/dcc-mcp-transport/src/channel.rs:77`
- **Decision (Run #93)**: **Keep permanently.** The enum is constructed (Pong/Shutdown variants sent via channel) but never pattern-matched; the `#[allow(dead_code)]` suppressor and the `///` comment on the enum accurately explain the design intent. This is a valid channel-based dispatch pattern, not unused code.
- **Status**: Closed — no further tracking needed.

### `std::sync::Mutex` vs `parking_lot::Mutex` — Migration

- **Status**: ✅ **COMPLETE (Run #94)** — All 5 crates migrated.
- **Affected crates** (all done):
  - ~~`dcc-mcp-transport`~~ ✅ **Run #92: migrated**
  - ~~`dcc-mcp-actions`~~ ✅ **Run #93: migrated**
  - ~~`dcc-mcp-telemetry`~~ ✅ **Run #94: migrated** (`recorder.rs`, 5 expects removed)
  - ~~`dcc-mcp-sandbox`~~ ✅ **Run #94: migrated** (`audit.rs`, 4 expects removed)
  - ~~`dcc-mcp-process`~~ ✅ **Run #94: migrated** (`watcher.rs` + `launcher.rs`, 5 lock-poison handlers removed)

### Large Files (>500 lines) — Structural Evaluation

Files exceeding 500-line threshold (excluding test files), tracked since Run #91:

| File | Lines | Analysis | Priority |
|------|-------|----------|----------|
| `dcc-mcp-protocols/src/adapters/` | **SPLIT ✅** | **Run #106**: Split into adapters/types.rs (297L) + adapters/traits.rs (304L) + adapters/tests.rs (282L) + adapters/mod.rs (17L). All pub paths unchanged. 0 Clippy warnings. | **Done** |
| `dcc-mcp-protocols/src/adapters_python/` | **SPLIT ✅** | **Run #107**: Split adapters_python.rs (1152L) into adapters_python/enums.rs (173L) + adapters_python/data.rs (843L) + adapters_python/scene_node.rs (86L) + adapters_python/mod.rs (28L). All pub paths unchanged. 0 Clippy warnings. | **Done** |
| `dcc-mcp-protocols/src/mock/tests.rs` | **1000+** (41071B) | Test-only code. No action needed. | ✅ No action |
| `dcc-mcp-protocols/src/mock/adapter.rs` | **898** (30292B) | Mock DCC adapter implementation. Large but acceptable for mock helpers. | Low |
| `dcc-mcp-skills/src/catalog.rs` | **1092+** (44753B) | **Run #105**: Still growing. Single-concern; monitor. | ✅ No action |
| `dcc-mcp-models/src/skill_metadata.rs` | **1021** (37654B) | PyO3 inline. No split needed. | ✅ No action |
| `dcc-mcp-transport/src/python/channel.rs` | **717** (29844B) | Python bindings for FramedChannel; complex but single-concern | Low |
| `dcc-mcp-transport/src/python/manager.rs` | **665** (24619B) | Python bindings for TransportManager; single-concern | Low |
| `dcc-mcp-transport/src/framed/tests.rs` | **779** | Test-only. No action needed. | ✅ No action |
| `dcc-mcp-transport/src/transport/mod.rs` | **692** (25233B) | TransportManager; large but coherent | Low |
| `dcc-mcp-actions/src/registry/mod.rs` | **654** | ActionRegistry; borderline, watch for growth | Low |
| `dcc-mcp-actions/src/pipeline/python.rs` | **669** (26210B) | Python bindings for pipeline. Single-concern. | Low |
| `dcc-mcp-skills/src/resolver.rs` | **683** (23920B) | Skill dependency resolver; could split resolution strategies | Low |
| `dcc-mcp-transport/src/pool/mod.rs` | **676** (22509B) | ConnectionPool; borderline, watch for growth | Low |
| `tests/test_http_transport_dcc_deep.py` | **1342** | Test-only (1342L). No action needed — test files are exempt. | ✅ No action |

**Note (Run #103)**: pipeline.rs (was 1166L) successfully split by iteration Agent into `pipeline/` submodules — max file now 669L (python.rs). mock.rs (was 1274L) split into `mock/` subdir. Both structural improvements confirmed ✅

**Note (Run #104)**: shm.md EN+ZH fixed (PySharedBuffer.create(capacity), id property, PyBufferPool(buffer_size), PySceneDataKind enum values). Stages 1–8 all clean. +416 new Python tests (10623 total).

**Note (Run #105)**: 3 unused imports removed in mock/tests.rs (DccHierarchy/DccRenderCapture/DccSceneManager). protocols.md EN+ZH: added 8 missing data type sections (DccInfo/DccCapabilities/DccError/DccErrorCode/ScriptLanguage/ScriptResult/SceneInfo/SceneStatistics) — these were referenced as return types but had no API docs. +235 Python tests (10858 total, +108 from iteration Agent).

**Note (Run #106)**: adapters.rs (1207L) split into adapters/ submodule: types.rs (297L) + traits.rs (304L) + tests.rs (282L) + mod.rs (17L). All `crate::adapters::*` paths unchanged. 0 Clippy warnings. 11089 Python tests pass. Next: evaluate adapters_python.rs (1057L) structural split.

**Note (Run #107)**: adapters_python.rs (1152L) split into adapters_python/ submodule: enums.rs (173L, PyScriptLanguage + PyDccErrorCode) + data.rs (843L, 12 data structs) + scene_node.rs (86L, recursive PySceneNode) + mod.rs (28L). All pub paths unchanged. 0 Clippy warnings. Stages 1–8 all clean. +269 new Python tests from iteration Agent (11419 total). Next: catalog.rs (1092L) monitor growth.

---

## Completed Items

| Run | Item | Status |
|-----|------|--------|
| #89 | Fix VersionedRegistry.remove() doc example (^1.0.0 constraint) | ✅ Fixed |
| #90 | Replace `.unwrap()` with `.expect()` in `handler.rs` and `python.rs` | ✅ Fixed |
| #91 | Replace 3 bare `.unwrap()` with `.expect()` in `ConnectionPool` production paths | ✅ Fixed |
| #92 | Migrate `dcc-mcp-transport` from `std::sync::Mutex` to `parking_lot::Mutex` | ✅ Fixed |
| #93 | Migrate `dcc-mcp-actions` from `std::sync::Mutex` to `parking_lot::Mutex` (5 files) | ✅ Fixed |
| #93 | `ControlMessage` dead_code decision: keep permanently (valid channel-dispatch pattern) | ✅ Closed |
| #94 | Migrate `dcc-mcp-telemetry`, `dcc-mcp-sandbox`, `dcc-mcp-process` from `std::sync::Mutex` to `parking_lot::Mutex` (4 files, 14 lock-poison handlers removed) | ✅ Fixed |
| #94 | `parking_lot` migration — ALL 5 crate targets complete | ✅ Closed |
| #95 | `catalog.rs` 907-line file evaluated — no split needed (498 impl + 120 bindings + 253 tests, well structured) | ✅ Closed |
| #95 | `cargo update` patch bumps: fastrand→2.4.1, tokio→1.51.1, toml_edit→0.25.11 | ✅ Applied |
| #96 | `_core.pyi` stub: add `ToolDeclaration` class, sync `SkillMetadata` (allowed_tools/license/compatibility/tools:list[ToolDeclaration]) | ✅ Fixed |
| #96 | Docs: update SKILL.md examples in README/README_zh/llms.txt/llms-full.txt (tools→allowed-tools) | ✅ Fixed |
| #96 | Merge conflict `test_skills_e2e.py` resolved (use allowed_tools field per new SkillMetadata schema) | ✅ Fixed |
| #96 | `skill_metadata.rs` 1145L evaluated — no split needed (PyO3 bindings require inline getters/setters) | ✅ Closed |
| #96 | `catalog.rs` 1239L re-evaluated — no split needed (expanded tests + script execution logic, single-concern) | ✅ Closed |
| #97 | `_core.pyi` stub: add `McpHttpServer` skill methods (register_handler/has_handler/catalog/discover/load_skill/unload_skill/find_skills/list_skills/get_skill_info/is_loaded/loaded_count) + `create_skill_manager` function stub | ✅ Fixed |
| #97 | Docs: update llms.txt + llms-full.txt — promote `create_skill_manager` Skills-First API in Quick Decision Guide, add Skills-First example section | ✅ Fixed |
| #97 | Tests: add `test_create_skill_manager.py` — 26 tests covering Skills-First factory API (zero coverage before this run) | ✅ Added |
| #98 | `_core.pyi` stub: add `SkillCatalog` class (8 methods) + `SkillSummary` class (8 fields) + `get_app_skill_paths_from_env` function — all 3 public symbols exported from `__init__.py` but missing from stub | ✅ Fixed |
| #98 | Docs: update llms.txt + llms-full.txt — add `SkillCatalog`/`SkillSummary` API reference, `get_app_skill_paths_from_env` doc, DCC-specific env var example (`DCC_MCP_{APP}_SKILL_PATHS`) | ✅ Fixed |
| #98 | Tests: fix `test_create_skill_manager.py::test_app_name_used_in_repr` — false assumption that `app_name` becomes server name; default server name is `dcc-mcp` (APP_NAME const); add `test_default_server_name_in_repr` + `test_custom_server_name_in_repr` | ✅ Fixed |
| #98 | CLEANUP_TODO.md: restored from commit d1d83b5 (lost during origin/main merge — file only lives on auto-improve branch) | ✅ Restored |
| #99 | `_core.pyi` stub: add `McpServerHandle = ServerHandle` alias — `McpServerHandle` was in `__init__.py/__all__` but missing from pyi (1 symbol gap) | ✅ Fixed |
| #99 | Large files table: **ERRONEOUS UPDATE** — Run #99 incorrectly reported 5 files as reduced (768/755/675/669/630) when actual sizes were unchanged (845/847/741/738/684). Corrected in Run #100. | ⚠️ Corrected |
| #100 | Large files table: corrected erroneous Run #99 line counts — all 5 files verified at original sizes; added pool/mod.rs (676L) and manager.rs (665L) to table | ✅ Fixed |
| #100 | All 9 scan stages: clean — 0 Clippy warnings, 0 Ruff warnings, 9516 pytest passed (+167 vs Run #99 from iteration Agent's 293 new tests), `_core`/`__all__`/`_core.pyi` fully synchronized | ✅ Verified |
| #101 | `test_mcp_http_server.py`: replace deprecated `streamablehttp_client` (×2) with `streamable_http_client` — mcp SDK 1.27.0 deprecation; 0 DeprecationWarnings now in `-W error::DeprecationWarning` mode | ✅ Fixed |
| #101 | `.gitignore`: add `ruff_out.txt` + `test_out.txt` — cleanup Agent temp files left untracked | ✅ Fixed |
| #101 | `dcc-mcp-usd/src/types.rs` (847L) structural evaluation: 389L tests + 458L impl across 6 tightly-coupled types. **No split needed.** Closed. | ✅ Closed |
| #102 | Docs: fix `VersionParseError → ValueError` in `docs/api/actions.md` EN+ZH — Python binding maps Rust `VersionParseError` to `PyValueError` | ✅ Fixed |
| #103 | Docs: fix `DccAdapter/DccConnection/DccScriptEngine` incorrect Python import in `docs/guide/protocols.md` EN+ZH — these are Rust traits, not Python importable; replaced with duck-typing note + correct data-type imports only | ✅ Fixed |
| #103 | `pipeline.rs` (1166L) split by iteration Agent into `pipeline/` submodules (max 669L); `mock.rs` (1274L) split into `mock/` subdir — structural improvements confirmed | ✅ Verified |
| #104 | Docs: fix shm.md API errors — PySharedBuffer.create(capacity) not size_bytes, id property not buffer_id(), PyBufferPool(buffer_size), PySceneDataKind enum values (EN+ZH) | ✅ Fixed |
| #105 | Clippy: remove 3 unused trait imports in mock/tests.rs (DccHierarchy/DccRenderCapture/DccSceneManager) — were in import but never used in code | ✅ Fixed |
| #105 | Docs: protocols.md EN+ZH — add 8 missing data type sections: DccInfo/DccCapabilities/DccError/DccErrorCode/ScriptLanguage/ScriptResult/SceneInfo/SceneStatistics (all exported from `__init__.py` but had no API docs) | ✅ Fixed |
