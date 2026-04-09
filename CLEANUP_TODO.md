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
| `dcc-mcp-skills/src/catalog.rs` | 1239 | **Run #96**: 519 impl + 232 script exec + 120 Python bindings + 366 tests = single-concern; execution logic (`execute_script`, `resolve_tool_script`) tightly coupled to catalog; **no split needed** | ✅ No action |
| `dcc-mcp-models/src/skill_metadata.rs` | 1145 | **Run #96 new**: `ToolDeclaration`(79L) + `ToolDeclaration` Python bindings(541L) + `SkillMetadata`+Python bindings(258L) + tests(267L). PyO3 requires all getters/setters inline — split would require complex cfg(feature) cross-module refs. **No split needed** | ✅ No action |
| `dcc-mcp-http/src/handler.rs` | 845 | HTTP MCP handler; large but single-concern (all request handling) | Low |
| `dcc-mcp-usd/src/types.rs` | 847 | USD type definitions; likely needs splitting into `types/primitives.rs`, `types/geometry.rs`, etc. | Medium |
| `dcc-mcp-protocols/src/adapters.rs` | 741 | DCC adapter traits; multiple adapter impls could be split | Medium |
| `dcc-mcp-actions/src/pipeline/python.rs` | 738 | Python pipeline bindings; could split middleware types out | Low |
| `dcc-mcp-transport/src/python/channel.rs` | 717 | Python bindings for FramedChannel; complex but single-concern | Low |
| `dcc-mcp-transport/src/transport/mod.rs` | 692 | TransportManager; large but coherent | Low |
| `dcc-mcp-protocols/src/adapters_python.rs` | 684 | Python bindings for DCC adapters | Low |
| `dcc-mcp-skills/src/resolver.rs` | 683 | Skill dependency resolver; could split resolution strategies | Low |
| `dcc-mcp-transport/src/python/manager.rs` | 665 | Python bindings for TransportManager; complex but single-concern | Low |
| `dcc-mcp-actions/src/registry/mod.rs` | 654 | ActionRegistry; borderline, watch for growth | Low |

- **Next action**: Evaluate `dcc-mcp-usd/src/types.rs` (847 lines) for splitting — highest multi-concern risk
- **Priority**: Low-Medium

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
