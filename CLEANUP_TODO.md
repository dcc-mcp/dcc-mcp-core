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
| `dcc-mcp-protocols/src/adapters.rs` | **1207** | **Run #103**: Measured at 1207L (prev record 1331 was from an early version; iteration Agent's refactor of mock.rs removed some code). Split plan (core vs cross-DCC traits) still valid but not blocking. | **Medium** — evaluate next run |
| `dcc-mcp-protocols/src/adapters_python.rs` | **1057** | **Run #103**: 1057L. Contains 15 `#[pyclass]`/`#[pymethods]` blocks, one per trait. PyO3 inline rules apply. Evaluate after adapters.rs split. | **Low** — after adapters.rs split |
| `dcc-mcp-protocols/src/mock/tests.rs` | **1058** | **Run #103**: Test-only code (mock adapter tests). No action needed. | ✅ No action |
| `dcc-mcp-protocols/src/mock/adapter.rs` | **772** | **Run #103**: Mock DCC adapter implementation. Large but acceptable for mock helpers. | Low |
| `dcc-mcp-skills/src/catalog.rs` | **1092** | **Run #103**: ~1092L. **Run #96** evaluation: single-concern, no split needed. Monitor growth. | ✅ No action |
| `dcc-mcp-models/src/skill_metadata.rs` | **1021** | **Run #97**: PyO3 requires all getters/setters inline. **No split needed** | ✅ No action |
| `dcc-mcp-http/src/handler.rs` | 768 | HTTP MCP handler; large but single-concern | Low |
| `dcc-mcp-usd/src/types.rs` | 755 | **Run #101**: Evaluated — 389L tests + 458L impl, no split needed. ✅ Closed | ✅ No action |
| `dcc-mcp-actions/src/pipeline/python.rs` | **669** | **Run #103**: Pipeline already split to subdir by iteration Agent. `python.rs` is the Python bindings file for the pipeline module. Single-concern. | Low |
| `dcc-mcp-transport/src/python/channel.rs` | **650** | Python bindings for FramedChannel; complex but single-concern | Low |
| `dcc-mcp-transport/src/transport/mod.rs` | **614** | TransportManager; large but coherent | Low |
| `dcc-mcp-transport/src/pool/mod.rs` | **610** | ConnectionPool; borderline, watch for growth | Low |
| `dcc-mcp-actions/src/registry/mod.rs` | **606** | ActionRegistry; borderline, watch for growth | Low |
| `dcc-mcp-skills/src/resolver.rs` | **601** | Skill dependency resolver; could split resolution strategies | Low |
| `dcc-mcp-transport/src/python/manager.rs` | **617** | Python bindings for TransportManager; complex but single-concern | Low |

**Note (Run #103)**: pipeline.rs (was 1166L) successfully split by iteration Agent into `pipeline/` submodules — max file now 669L (python.rs). mock.rs (was 1274L) split into `mock/` subdir. Both structural improvements confirmed ✅

**Next action (Run #103+)**: Evaluate `adapters.rs` (1207L) split — Core traits vs Cross-DCC Protocol traits boundary is documented in file header. Medium risk due to adapters_python.rs use paths.

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
