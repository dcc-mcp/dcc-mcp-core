## v0.10.0 (2026-03-28)

### Feat

- replace pre-commit with vx prek and add justfile
- add Skills system for zero-code script registration as MCP tools

### Fix

- resolve lint errors in test files (isort, ruff format, D106/F841)
- add cross-platform shell support to justfile
- resolve isort issues and migrate CI to vx

## [0.12.26](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.25...v0.12.26) (2026-04-15)


### Features

* **python:** add DCC adapter base abstractions (DccServerBase, DccSkillHotReloader, DccGatewayElection, factory) ([#187](https://github.com/loonghao/dcc-mcp-core/issues/187)) ([3da5cf5](https://github.com/loonghao/dcc-mcp-core/commit/3da5cf58ba22eb36ac09f612770d8e6bf7712f92))

## [0.12.25](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.24...v0.12.25) (2026-04-15)


### Features

* expose BridgeRegistry Python API (BridgeContext, BridgeRegistry, register_bridge) ([a8c7ec1](https://github.com/loonghao/dcc-mcp-core/commit/a8c7ec11efdede98899b86ecc9da58ba04711c6b))

## [0.12.24](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.23...v0.12.24) (2026-04-14)


### Features

* Complete issues [#180](https://github.com/loonghao/dcc-mcp-core/issues/180) and [#179](https://github.com/loonghao/dcc-mcp-core/issues/179) - Gateway improvements ([#183](https://github.com/loonghao/dcc-mcp-core/issues/183)) ([eb739a1](https://github.com/loonghao/dcc-mcp-core/commit/eb739a117135f401c0adda3fc2d78ccc0173485f))
* RTK-inspired token optimization (-80% consumption) ([#181](https://github.com/loonghao/dcc-mcp-core/issues/181)) ([87f1f1c](https://github.com/loonghao/dcc-mcp-core/commit/87f1f1c4e01f6ecb5ef2f64562c0b770506c1fab))


### Documentation

* refresh all documentation for v0.12.23 ([d53605b](https://github.com/loonghao/dcc-mcp-core/commit/d53605be678ba763bd4aa38d59d23dcef5dcf1b1))

## [0.12.23](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.22...v0.12.23) (2026-04-14)


### Documentation

* **http,agents:** document gateway competition API and update agent guide ([583aa6b](https://github.com/loonghao/dcc-mcp-core/commit/583aa6b802224ef1f48c49a527380b484db96fb2))

## [0.12.22](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.21...v0.12.22) (2026-04-14)


### Features

* **skills,http:** add explicit deferred tool hints ([38ed73d](https://github.com/loonghao/dcc-mcp-core/commit/38ed73d1119c71c6787afc1c1a70c0fd0a2d6572))

## [0.12.21](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.20...v0.12.21) (2026-04-14)


### Bug Fixes

* **ci:** add vx install dir to PATH after setup ([9cec3b7](https://github.com/loonghao/dcc-mcp-core/commit/9cec3b7f2f33bc8d610a3855f6afcbc9cbdf1b38))
* **ci:** remove duplicate Cache Cargo step in dcc-integration.yml ([5e7d4f8](https://github.com/loonghao/dcc-mcp-core/commit/5e7d4f8a87592e0944425d2c1a731b06460f3d64))
* **ci:** use 'vx just' instead of installing just to PATH ([ded7a24](https://github.com/loonghao/dcc-mcp-core/commit/ded7a24b0c23b5f1462f9008d8c1a8eec3e425db))
* **ci:** use ubuntu-22.04 for Python 3.7 and replace setup-just with vx ([c8eda5b](https://github.com/loonghao/dcc-mcp-core/commit/c8eda5b6076d623709a27ed8378c2ed899786422))

## [0.12.20](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.19...v0.12.20) (2026-04-13)


### Features

* **server:** integrated auto-gateway — first-wins port competition, zero extra processes ([#164](https://github.com/loonghao/dcc-mcp-core/issues/164)) ([058e3dd](https://github.com/loonghao/dcc-mcp-core/commit/058e3dd65d2bfda897c7b4fdadf591799e9ebe61))
* **server:** sidecar process management — PID file, WS heartbeat, reconnect timeout, session TTL ([b019191](https://github.com/loonghao/dcc-mcp-core/commit/b0191916a8d06bb06592849b4873766a3b18413c))


### Bug Fixes

* add __iter__ and to_json() to ActionResultModel for JSON ergonomics ([147c731](https://github.com/loonghao/dcc-mcp-core/commit/147c731e03a912e075ae37e7e91972164276f91c))
* **ci:** remove stale gateway entry from Cargo.toml; fix remaining noqa RUF100 + E711/E712/SIM118 ([a5b3ef9](https://github.com/loonghao/dcc-mcp-core/commit/a5b3ef98ec4cd5a3aae0b2376398675f6e8fea16))
* **ci:** remove stale noqa directives; expose session_ttl_secs in Python binding ([313623e](https://github.com/loonghao/dcc-mcp-core/commit/313623e36f252ca37477d334af3e503839ade1f6))
* **skills:** fix skill discovery, execution param passing, and script compatibility ([#159](https://github.com/loonghao/dcc-mcp-core/issues/159)) ([7f644da](https://github.com/loonghao/dcc-mcp-core/commit/7f644da01d6a9068e3e542da6dd271e778e1e808))
* **tests:** update axum-test usage for v20 API — TestServer::new() no longer returns Result ([#166](https://github.com/loonghao/dcc-mcp-core/issues/166)) ([892bb57](https://github.com/loonghao/dcc-mcp-core/commit/892bb574216c5b5d2be3f2276f95377b4ca5db4a))

## [0.12.19](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.18...v0.12.19) (2026-04-13)


### Features

* **bridge:** WebSocket JSON-RPC 2.0 protocol, DccBridge Python API, and standalone server ([#145](https://github.com/loonghao/dcc-mcp-core/issues/145) [#146](https://github.com/loonghao/dcc-mcp-core/issues/146) [#147](https://github.com/loonghao/dcc-mcp-core/issues/147)) ([b604c94](https://github.com/loonghao/dcc-mcp-core/commit/b604c945274c88cf78ebd8d560d7eca79bf8484c))
* **core:** add ActionChain for native multi-step operation orchestration ([#142](https://github.com/loonghao/dcc-mcp-core/issues/142)) ([bd01937](https://github.com/loonghao/dcc-mcp-core/commit/bd01937e0202d9446bff89e104bc7d67c18921fc))
* **dcc-mcp-maya:** register diagnostic IPC actions for dcc-diagnostics skill ([#141](https://github.com/loonghao/dcc-mcp-core/issues/141)) ([8f0d909](https://github.com/loonghao/dcc-mcp-core/commit/8f0d909f85bd72e1cf24ca49ee3af5be0b69dbc2))
* **examples:** add dcc-diagnostics and workflow skill examples ([67b5b89](https://github.com/loonghao/dcc-mcp-core/commit/67b5b894150a056c59b5c4331cbd2c0c2d07f0eb))
* **packaging:** bundle general-purpose skills inside the wheel ([4f2e8f5](https://github.com/loonghao/dcc-mcp-core/commit/4f2e8f5e64ede8c32853497c7ba8514579709441))
* **skills:** on-demand skill discovery meta-tools and progressive loading ([#143](https://github.com/loonghao/dcc-mcp-core/issues/143) [#148](https://github.com/loonghao/dcc-mcp-core/issues/148) [#149](https://github.com/loonghao/dcc-mcp-core/issues/149) [#150](https://github.com/loonghao/dcc-mcp-core/issues/150)) ([dc3c9b4](https://github.com/loonghao/dcc-mcp-core/commit/dc3c9b443852a00182a1fde2746efe605fd160e1))


### Bug Fixes

* **test:** apply ruff auto-fix ([638d1c0](https://github.com/loonghao/dcc-mcp-core/commit/638d1c007bd3df32b5b48731987edc091d511add))


### Documentation

* document bundled skills and get_bundled_skill_paths() API ([6545e8f](https://github.com/loonghao/dcc-mcp-core/commit/6545e8fb9bf4713f9d03ecf78efed69b77ebb759))

## [0.12.18](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.17...v0.12.18) (2026-04-12)


### Bug Fixes

* **skills,models:** fix 3 reported bugs in parse_skill_md, SkillScanner, ActionResultModel ([63ebe7d](https://github.com/loonghao/dcc-mcp-core/commit/63ebe7daebca7877e8728568103d329a0e509037))
* **skills:** resolve relative script paths against skill root + configurable Python interpreter ([10224bf](https://github.com/loonghao/dcc-mcp-core/commit/10224bfeaff24c7d2a044de11db89d0852301254))

## [0.12.17](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.16...v0.12.17) (2026-04-11)


### Features

* **skills,http:** on-demand skill discovery with search_skills and lightweight stubs ([#136](https://github.com/loonghao/dcc-mcp-core/issues/136)) ([01c6165](https://github.com/loonghao/dcc-mcp-core/commit/01c6165a8cd1569d9125aa19f8205aa6f7969097))

## [0.12.16](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.15...v0.12.16) (2026-04-11)


### Bug Fixes

* **tests:** use parent tmpdir in sandbox path test (cross-platform) ([7c8df02](https://github.com/loonghao/dcc-mcp-core/commit/7c8df0240f8d46d9b708e6afb65a41306fa8ec55))

## [0.12.15](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.14...v0.12.15) (2026-04-11)


### Features

* **models:** add Rust-backed serialization for ActionResultModel ([63385f1](https://github.com/loonghao/dcc-mcp-core/commit/63385f1d22cb2e13dc8c7edc1159ebb250fbcd83))
* **protocols:** add BridgeKind + bridge fields to DccCapabilities for non-Python DCCs ([b51ca78](https://github.com/loonghao/dcc-mcp-core/commit/b51ca784bffaa9d0db1f341b0d95b211f49a49a6))
* **skill:** add pure-Python skill script helpers + squash auto-improve adapters refactor ([4d342fc](https://github.com/loonghao/dcc-mcp-core/commit/4d342fce23d4bb83b5db84b82869b95506c26205))


### Bug Fixes

* **protocols:** add ..Default::default() to DccCapabilities struct literals ([913435f](https://github.com/loonghao/dcc-mcp-core/commit/913435f4eec1da5c6c4aa8e2b091daf5acd081e0))
* **tests:** correct 7 failing tests across 3 test files ([af9dae1](https://github.com/loonghao/dcc-mcp-core/commit/af9dae1f77f3c951eae8615e7af394480572b341))
* **tests:** use Path.resolve() for sandbox path test (macOS /tmp symlink) ([277bcdb](https://github.com/loonghao/dcc-mcp-core/commit/277bcdb1912cb387a9edac9cd8845a8faf59301d))

## [0.12.14](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.13...v0.12.14) (2026-04-11)


### Bug Fixes

* **tests:** fix platform-specific assertions on Linux/macOS ([31c9f24](https://github.com/loonghao/dcc-mcp-core/commit/31c9f245327e42453a78fe6036e9b22d396c0ffe))
* **tests:** use real tmpdir for is_path_allowed cross-platform test ([3aa28a8](https://github.com/loonghao/dcc-mcp-core/commit/3aa28a84efae19f2af84f3916050ef3b68f734bf))

## [0.12.13](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.12...v0.12.13) (2026-04-10)


### Features

* **protocols:** add 4 cross-DCC protocol traits + complete Python bindings ([d683efc](https://github.com/loonghao/dcc-mcp-core/commit/d683efc20828dee9007d6f17b697c662af541a09))


### Bug Fixes

* **protocols,tests:** restore DccCapabilities repr + fix IpcListener platform test ([1596689](https://github.com/loonghao/dcc-mcp-core/commit/159668996551e6c952abfc91c1591ea95d3c65c7))
* **tests:** fix platform-specific assertions causing Linux/macOS CI failures ([db2aea2](https://github.com/loonghao/dcc-mcp-core/commit/db2aea2aaaf531ced7b14a68afa0eaf136153a0a))


### Documentation

* fix API accuracy across capture/http/process/usd docs (EN+ZH) ([ded9f24](https://github.com/loonghao/dcc-mcp-core/commit/ded9f242c2d5c8f6b03b2ca6a17dce06156bf68e))
* replace action terminology with skill across docs and type stubs ([0985e5b](https://github.com/loonghao/dcc-mcp-core/commit/0985e5bd58486c1456741d8756ebfd3e134dbb4b))

## [0.12.12](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.11...v0.12.12) (2026-04-09)


### Features

* DCC_MCP_{APP}_SKILL_PATHS env var + create_skill_manager factory ([#119](https://github.com/loonghao/dcc-mcp-core/issues/119)) ([8a15a1e](https://github.com/loonghao/dcc-mcp-core/commit/8a15a1effcc4c4e1a5377c26ea5814e2c0189317))


### Documentation

* **agents:** update AGENTS.md for Skills-First API + add codecov setup ([b5b9eac](https://github.com/loonghao/dcc-mcp-core/commit/b5b9eac9c6595518da9f9ce930709edffb008cad))

## [0.12.11](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.10...v0.12.11) (2026-04-09)


### Features

* **models:** align SkillMetadata with Anthropic Skills + ClawHub standards ([#114](https://github.com/loonghao/dcc-mcp-core/issues/114)) ([02805d8](https://github.com/loonghao/dcc-mcp-core/commit/02805d8add9b4d626cda4c1310d36f09c0a08357))
* **skills:** Skills-First architecture — tools/call executes skill scripts via ActionDispatcher ([#113](https://github.com/loonghao/dcc-mcp-core/issues/113)) ([ae0b12d](https://github.com/loonghao/dcc-mcp-core/commit/ae0b12de9e378994eadf4d62018bab0cce2f4ba8))

## [0.12.10](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.9...v0.12.10) (2026-04-09)


### Features

* **skills:** add SkillCatalog with progressive skill loading and core discovery tools ([#111](https://github.com/loonghao/dcc-mcp-core/issues/111)) ([a708379](https://github.com/loonghao/dcc-mcp-core/commit/a7083794da0054845beb4b87fc23cb37e5b048aa))


### Documentation

* update AI agent guidance for v0.12.9 — MCP 2025-11-05 draft, 12 crates, DeferredExecutor tips ([#109](https://github.com/loonghao/dcc-mcp-core/issues/109)) ([8ad45bb](https://github.com/loonghao/dcc-mcp-core/commit/8ad45bb682e7507f583b96ee8bf049495ead14b8))

## [0.12.9](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.8...v0.12.9) (2026-04-08)


### Documentation

* update AGENTS.md with new MCP HTTP architecture design ([#107](https://github.com/loonghao/dcc-mcp-core/issues/107)) ([8cec983](https://github.com/loonghao/dcc-mcp-core/commit/8cec9833be8d5f22954f7a6c80b3059f0d708762))

## [0.12.8](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.7...v0.12.8) (2026-04-07)


### Features

* add dcc-mcp-http crate — MCP Streamable HTTP server (2025-03-26 spec) ([#103](https://github.com/loonghao/dcc-mcp-core/issues/103)) ([6cd7887](https://github.com/loonghao/dcc-mcp-core/commit/6cd788785b535616256ae4b115e072ab4b9b74b6))


### Documentation

* update AI agent guidance for v0.12.7 — McpHttpServer, FramedChannel.call(), 12 crates ([#106](https://github.com/loonghao/dcc-mcp-core/issues/106)) ([b6b1d37](https://github.com/loonghao/dcc-mcp-core/commit/b6b1d37e815f97eefbc15b5eb752530160e0be4e))

## [0.12.7](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.6...v0.12.7) (2026-04-06)


### Bug Fixes

* **ci:** fix Python 3.7 runner and update actions versions + add tests ([#90](https://github.com/loonghao/dcc-mcp-core/issues/90)) ([8b6157a](https://github.com/loonghao/dcc-mcp-core/commit/8b6157a97685e1fc8dda4ca604bf7527334c283c))


### Documentation

* fix API correctness and enhance AI agent guidance (Run [#4](https://github.com/loonghao/dcc-mcp-core/issues/4)) ([09a8971](https://github.com/loonghao/dcc-mcp-core/commit/09a8971386c2f0bf8d1c1679c43c43058cf136f0))

## [0.12.6](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.5...v0.12.6) (2026-04-06)


### Features

* squash auto-improve branch + bump version to 0.12.6 ([9d7e37f](https://github.com/loonghao/dcc-mcp-core/commit/9d7e37fd15808186c855eb3d21d1f39d7a60fd1c))
* **transport:** add bind_and_register + find_best_service for zero-config service discovery ([720b6eb](https://github.com/loonghao/dcc-mcp-core/commit/720b6eb880e974ffa8e5b5d2e42db35542eb0f9e))
* **transport:** round-robin multi-instance load balancing + rank_services API ([55e4450](https://github.com/loonghao/dcc-mcp-core/commit/55e4450a28cba5c9d2465928665ee30a4101de6a))


### Bug Fixes

* **ci:** fix Python 3.7 runner and update actions versions + add tests ([#90](https://github.com/loonghao/dcc-mcp-core/issues/90)) ([8b6157a](https://github.com/loonghao/dcc-mcp-core/commit/8b6157a97685e1fc8dda4ca604bf7527334c283c))
* **ci:** remove duplicate tag-triggered publish in release.yml ([eeb78b4](https://github.com/loonghao/dcc-mcp-core/commit/eeb78b466935f170d69ccb2770b765add87f4428))
* **ci:** update dcc-integration.yml to use split test files ([cbedaef](https://github.com/loonghao/dcc-mcp-core/commit/cbedaef9e4114c3f58680e651d5a2158aa5ac475))
* **process:** fix PyProcessWatcher.start() tokio runtime context bug and add 20 tests for lifecycle API [iteration-done] ([96cc8df](https://github.com/loonghao/dcc-mcp-core/commit/96cc8df98afbe9775c1c6c486100ef0db977ed1d))
* **process:** replace eprintln with tracing::warn in launcher tests ([a1161a2](https://github.com/loonghao/dcc-mcp-core/commit/a1161a2b2f3da6830b182d6f3cd2929c5948269a))
* **restore:** restore test_adapters_python.py lost after squash — 67 tests for DCC adapter Python bindings ([7b40582](https://github.com/loonghao/dcc-mcp-core/commit/7b40582968e290297e4189a898cc59feac560f00))


### Documentation

* add GEMINI.md, enhance AI agent guides with decision tables and integration patterns ([#87](https://github.com/loonghao/dcc-mcp-core/issues/87)) ([42ff2ba](https://github.com/loonghao/dcc-mcp-core/commit/42ff2ba604c118177edbffa665b9d8fdfb31062d))

## [0.12.5](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.4...v0.12.5) (2026-04-05)


### Documentation

* enhance AI agent guidance, fix legacy API refs, update llms.txt ([#85](https://github.com/loonghao/dcc-mcp-core/issues/85)) ([11ee040](https://github.com/loonghao/dcc-mcp-core/commit/11ee0404b2a2ab36d7c432c587600cf441cbdcba))

## [0.12.4](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.3...v0.12.4) (2026-04-05)


### Features

* add Python 3.7 support with separate non-abi3 wheel builds ([82208a1](https://github.com/loonghao/dcc-mcp-core/commit/82208a149cb579fab8ec835d7ee32e54c3c8c508))


### Documentation

* add AI-friendly docs (AGENTS.md, CLAUDE.md, SKILL.md) + modernize READMEs ([2b3c958](https://github.com/loonghao/dcc-mcp-core/commit/2b3c958ca24791aba0482c3be73a48d750769b4b))

## [0.12.3](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.2...v0.12.3) (2026-04-05)


### Features

* squash auto-improve features and fix CI PyPI Trusted Publishing ([#75](https://github.com/loonghao/dcc-mcp-core/issues/75)) ([06b8eee](https://github.com/loonghao/dcc-mcp-core/commit/06b8eee23d3c722364cc942a9e8afd6bb69342d3))


### Bug Fixes

* **deps:** update rust dependencies ([a260381](https://github.com/loonghao/dcc-mcp-core/commit/a26038120bb49502f21dbae8d3089990200f3deb))

## [0.12.2](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.1...v0.12.2) (2026-04-03)


### Bug Fixes

* dead link in zh getting-started and release workflow skip issue ([8d709f6](https://github.com/loonghao/dcc-mcp-core/commit/8d709f673b4e3518bfb97f5064cc072cce8fbd84))

## [0.12.1](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.0...v0.12.1) (2026-04-02)


### Features

* add transport Python types, fix docs, drop py3.8 CI ([#69](https://github.com/loonghao/dcc-mcp-core/issues/69)) ([89c70a7](https://github.com/loonghao/dcc-mcp-core/commit/89c70a7c95981d61ea0c4017b6f1672a16efe05d))


### Code Refactoring

* consolidate cleanup, docs, tests, and CI improvements ([28d7391](https://github.com/loonghao/dcc-mcp-core/commit/28d73912b0dcd9233b2b0eb6f076702321c0fb5d))

## [0.12.0](https://github.com/loonghao/dcc-mcp-core/compare/v0.11.0...v0.12.0) (2026-03-30)


### ⚠ BREAKING CHANGES

* Complete rewrite from Python+Pydantic to Rust+PyO3+maturin.

### Features

* Add foundational components and documentation for dcc-mcp-core ([86a1754](https://github.com/loonghao/dcc-mcp-core/commit/86a1754fc3685c7f1c735e58d7506b7a1611788c))
* Add function adapters for Action classes ([6b62c87](https://github.com/loonghao/dcc-mcp-core/commit/6b62c87dfed6174e79b0afe7caf9f927cf4ff19b))
* Add Pydantic extensions and update related modules ([4eb4f80](https://github.com/loonghao/dcc-mcp-core/commit/4eb4f80b646ddd9a0050590be64bfcd37d427591))
* add Skills system for zero-code script registration as MCP tools ([cab3c28](https://github.com/loonghao/dcc-mcp-core/commit/cab3c28d111e6fa1d56bde827febc0ebd64769a2))
* Add various test plugins and utilities for plugin management system ([b3ebe68](https://github.com/loonghao/dcc-mcp-core/commit/b3ebe6876b9cba33673aab51c0eaba6c7514d184))
* Enhance action management and DCC support ([f019c20](https://github.com/loonghao/dcc-mcp-core/commit/f019c20ebb4bb98ef4f0cd9351e6a11e5df9c99a))
* Enhance action registration and classification ([de765a8](https://github.com/loonghao/dcc-mcp-core/commit/de765a8f79c8662fb675bfa522a0feebaf01ab24))
* Enhance ActionRegistry with DCC-specific features ([5375337](https://github.com/loonghao/dcc-mcp-core/commit/53753376e4b028d76603a9b018a8258261d33f22))
* implement skills system with metadata dir, depends, examples and e2e tests ([5ee9970](https://github.com/loonghao/dcc-mcp-core/commit/5ee997033bd90740e13ee588a96b6303f47634a9))
* Improve imports and module interface in dcc_mcp_core ([d62792c](https://github.com/loonghao/dcc-mcp-core/commit/d62792ccdfbfea06e6c3ed49dd4254a8e2c7dfdb))
* Improve imports and module interface in dcc_mcp_core ([b2605a9](https://github.com/loonghao/dcc-mcp-core/commit/b2605a929506d1c7b9b70adf61b8151139a8a61d))
* replace pre-commit with vx prek and add justfile ([fd56ac9](https://github.com/loonghao/dcc-mcp-core/commit/fd56ac998d5d117bb204f1302465bc72bd27b63f))
* Restructure imports, remove unused code, and update templates ([64f3f3e](https://github.com/loonghao/dcc-mcp-core/commit/64f3f3e6db7c0bd76cf13e324568f097608b5c46))
* rewrite core in Rust with workspace crates architecture ([3308ee1](https://github.com/loonghao/dcc-mcp-core/commit/3308ee1d7a465cca82d966786ab9ed936dc5ba33))


### Bug Fixes

* add cross-platform shell support to justfile ([8cc8de1](https://github.com/loonghao/dcc-mcp-core/commit/8cc8de1760aea8a8a28349a913dd334beec35772))
* add Python 3.7 compatibility for importlib.metadata ([db342ff](https://github.com/loonghao/dcc-mcp-core/commit/db342ffa3a14a5fc87d79df798b02357ac099cc6))
* add special handling for Python 3.7 in GitHub Actions workflow ([3cd04f6](https://github.com/loonghao/dcc-mcp-core/commit/3cd04f6ba20a762b9b353cd78eebd5a700a557cf))
* **ci:** add python/dcc_mcp_core/__init__.py for maturin python-source ([859dbb7](https://github.com/loonghao/dcc-mcp-core/commit/859dbb798088b39c8ed31faf85551529777e46cc))
* **ci:** use 'just install' (build+pip) instead of 'maturin develop' ([45ea35d](https://github.com/loonghao/dcc-mcp-core/commit/45ea35d8a28ce0274bb03943d73e8a1ec08fa6e7))
* **deps:** update dependency platformdirs to v4 ([59f65da](https://github.com/loonghao/dcc-mcp-core/commit/59f65da4818deb09ea4d6f85488a7963fe1418ec))
* improve GitHub Actions workflows for Windows compatibility ([7220901](https://github.com/loonghao/dcc-mcp-core/commit/722090165c89829c24d98d20bcb37ec9ae015a86))
* remove component from release-please config to use v0.x.x tag format ([3bb0696](https://github.com/loonghao/dcc-mcp-core/commit/3bb06964eb3d73ac8e17605e7fa2fc1d6c9d063d))
* resolve all PyO3 0.23 python-bindings compilation errors ([7180c4e](https://github.com/loonghao/dcc-mcp-core/commit/7180c4e41eb0d367a4d71ef1a394fe9e6a07fd9f))
* resolve CI clippy errors and unify dev toolchain ([0300b0b](https://github.com/loonghao/dcc-mcp-core/commit/0300b0ba68d18b98f11393450bd3e692bddacf6c))
* resolve isort issues and migrate CI to vx ([31ed2a9](https://github.com/loonghao/dcc-mcp-core/commit/31ed2a9669f40b1b490cc8875f38c32e3c09ba52))
* resolve lint errors in test files (isort, ruff format, D106/F841) ([d703c4a](https://github.com/loonghao/dcc-mcp-core/commit/d703c4af92587b90c8165ed56d6e57ee714b8502))
* resolve release-please 'package.version is not tagged' error ([b433c71](https://github.com/loonghao/dcc-mcp-core/commit/b433c71db40c162d5f0694981012bdb9bb95410b))
* update GitHub Actions workflows for better Python version compatibility ([0e4b2bc](https://github.com/loonghao/dcc-mcp-core/commit/0e4b2bca67e4c9b18690ead283e05d13fb0d8ee7))
* update GitHub Actions workflows to regenerate poetry.lock before install ([752206b](https://github.com/loonghao/dcc-mcp-core/commit/752206b98daf86d455cdcc33374110e81dc301b6))
* update Mermaid diagrams for better GitHub compatibility and visibility ([fc43474](https://github.com/loonghao/dcc-mcp-core/commit/fc43474a2b3c8fea4e44841788a70b0f741d5c77))


### Code Refactoring

* **dcc_mcp_core:** Replace plugin manager with action manager across the codebase ([18ea75b](https://github.com/loonghao/dcc-mcp-core/commit/18ea75bb1d7507f5ecbcb390a7ee5a949cc7a94f))
* Enhance ActionRegistry and add hooks ([e300109](https://github.com/loonghao/dcc-mcp-core/commit/e300109f3d2a1da1e3dbc45d1b025c469cab0417))
* Enhance error handling and parameter management ([f58bba1](https://github.com/loonghao/dcc-mcp-core/commit/f58bba15f9c9717afa2eccab962533d95d82d2f1))
* Improve action manager and registry implementation ([8e9bc65](https://github.com/loonghao/dcc-mcp-core/commit/8e9bc652cf6fb59ddc3059022769895a475a7d6e))
* Improve action path handling and code clarity ([8e42439](https://github.com/loonghao/dcc-mcp-core/commit/8e424397a4d45b4437dc5038f90fb6f58f7411bb))
* Improve code quality and functionality in multiple modules ([4f8212f](https://github.com/loonghao/dcc-mcp-core/commit/4f8212f90cb256e78f817b9ed7107d6160aded67))
* Improve dependency injector module ([5fc76d6](https://github.com/loonghao/dcc-mcp-core/commit/5fc76d61ee77deba3cdb97cc509f35bb5310340d))
* Improve imports and replace hardcoded constants ([d69ff22](https://github.com/loonghao/dcc-mcp-core/commit/d69ff22f467155cf0e6483b0b6b66a196fc54afa))
* Improve test methods and update comments ([8931253](https://github.com/loonghao/dcc-mcp-core/commit/89312534646bda1d4c64c1cf8b683d8480a94919))
* Optimize imports and update workflows ([4279cf7](https://github.com/loonghao/dcc-mcp-core/commit/4279cf74f9c2d29e6a445f8bd400ccec2a268244))
* Refactor action manager and adapters ([1367826](https://github.com/loonghao/dcc-mcp-core/commit/1367826267cdcc885ea9fa0bdcce1e4e720656af))
* Refactor build and repository setup ([bf8acdf](https://github.com/loonghao/dcc-mcp-core/commit/bf8acdf146c533fcf5b5081c87625b226b123030))
* remove legacy Python code and fix tag format to v0.x.0 ([88b54ee](https://github.com/loonghao/dcc-mcp-core/commit/88b54ee3f5b0ef8112a199cd62cd3d4eb75b24ae))
* Remove unused modules and add path conversion functions ([fcdabb8](https://github.com/loonghao/dcc-mcp-core/commit/fcdabb8708c8e388965a6802ecacf3f48668dcf4))
* Standardize string quotation usage across the codebase ([90728f7](https://github.com/loonghao/dcc-mcp-core/commit/90728f739b178a3a46dbeeb9359f6aac4bcfe0a6))
* Update platformdirs handling in config paths ([b7d7ba9](https://github.com/loonghao/dcc-mcp-core/commit/b7d7ba9e0cb082e9f4dfef00d5e13b4a50ecfe75))


### Documentation

* Add comprehensive Sphinx documentation for DCC-MCP-Core ([d49dbaf](https://github.com/loonghao/dcc-mcp-core/commit/d49dbaf463fe2dc56aa3c51284fddb299efdf029))
* migrate from Sphinx to VitePress with i18n support ([1c4ef9c](https://github.com/loonghao/dcc-mcp-core/commit/1c4ef9cd96ef23d9c6d7c32605e4fd785324d5d7))

## [0.11.0](https://github.com/loonghao/dcc-mcp-core/compare/dcc-mcp-core-v0.10.0...dcc-mcp-core-v0.11.0) (2026-03-29)


### ⚠ BREAKING CHANGES

* Complete rewrite from Python+Pydantic to Rust+PyO3+maturin.

### Features

* Add foundational components and documentation for dcc-mcp-core ([86a1754](https://github.com/loonghao/dcc-mcp-core/commit/86a1754fc3685c7f1c735e58d7506b7a1611788c))
* Add function adapters for Action classes ([6b62c87](https://github.com/loonghao/dcc-mcp-core/commit/6b62c87dfed6174e79b0afe7caf9f927cf4ff19b))
* Add Pydantic extensions and update related modules ([4eb4f80](https://github.com/loonghao/dcc-mcp-core/commit/4eb4f80b646ddd9a0050590be64bfcd37d427591))
* add Skills system for zero-code script registration as MCP tools ([cab3c28](https://github.com/loonghao/dcc-mcp-core/commit/cab3c28d111e6fa1d56bde827febc0ebd64769a2))
* Add various test plugins and utilities for plugin management system ([b3ebe68](https://github.com/loonghao/dcc-mcp-core/commit/b3ebe6876b9cba33673aab51c0eaba6c7514d184))
* Enhance action management and DCC support ([f019c20](https://github.com/loonghao/dcc-mcp-core/commit/f019c20ebb4bb98ef4f0cd9351e6a11e5df9c99a))
* Enhance action registration and classification ([de765a8](https://github.com/loonghao/dcc-mcp-core/commit/de765a8f79c8662fb675bfa522a0feebaf01ab24))
* Enhance ActionRegistry with DCC-specific features ([5375337](https://github.com/loonghao/dcc-mcp-core/commit/53753376e4b028d76603a9b018a8258261d33f22))
* implement skills system with metadata dir, depends, examples and e2e tests ([5ee9970](https://github.com/loonghao/dcc-mcp-core/commit/5ee997033bd90740e13ee588a96b6303f47634a9))
* Improve imports and module interface in dcc_mcp_core ([d62792c](https://github.com/loonghao/dcc-mcp-core/commit/d62792ccdfbfea06e6c3ed49dd4254a8e2c7dfdb))
* Improve imports and module interface in dcc_mcp_core ([b2605a9](https://github.com/loonghao/dcc-mcp-core/commit/b2605a929506d1c7b9b70adf61b8151139a8a61d))
* replace pre-commit with vx prek and add justfile ([fd56ac9](https://github.com/loonghao/dcc-mcp-core/commit/fd56ac998d5d117bb204f1302465bc72bd27b63f))
* Restructure imports, remove unused code, and update templates ([64f3f3e](https://github.com/loonghao/dcc-mcp-core/commit/64f3f3e6db7c0bd76cf13e324568f097608b5c46))
* rewrite core in Rust with workspace crates architecture ([3308ee1](https://github.com/loonghao/dcc-mcp-core/commit/3308ee1d7a465cca82d966786ab9ed936dc5ba33))


### Bug Fixes

* add cross-platform shell support to justfile ([8cc8de1](https://github.com/loonghao/dcc-mcp-core/commit/8cc8de1760aea8a8a28349a913dd334beec35772))
* add Python 3.7 compatibility for importlib.metadata ([db342ff](https://github.com/loonghao/dcc-mcp-core/commit/db342ffa3a14a5fc87d79df798b02357ac099cc6))
* add special handling for Python 3.7 in GitHub Actions workflow ([3cd04f6](https://github.com/loonghao/dcc-mcp-core/commit/3cd04f6ba20a762b9b353cd78eebd5a700a557cf))
* **ci:** add python/dcc_mcp_core/__init__.py for maturin python-source ([859dbb7](https://github.com/loonghao/dcc-mcp-core/commit/859dbb798088b39c8ed31faf85551529777e46cc))
* **ci:** use 'just install' (build+pip) instead of 'maturin develop' ([45ea35d](https://github.com/loonghao/dcc-mcp-core/commit/45ea35d8a28ce0274bb03943d73e8a1ec08fa6e7))
* **deps:** update dependency platformdirs to v4 ([59f65da](https://github.com/loonghao/dcc-mcp-core/commit/59f65da4818deb09ea4d6f85488a7963fe1418ec))
* improve GitHub Actions workflows for Windows compatibility ([7220901](https://github.com/loonghao/dcc-mcp-core/commit/722090165c89829c24d98d20bcb37ec9ae015a86))
* resolve all PyO3 0.23 python-bindings compilation errors ([7180c4e](https://github.com/loonghao/dcc-mcp-core/commit/7180c4e41eb0d367a4d71ef1a394fe9e6a07fd9f))
* resolve CI clippy errors and unify dev toolchain ([0300b0b](https://github.com/loonghao/dcc-mcp-core/commit/0300b0ba68d18b98f11393450bd3e692bddacf6c))
* resolve isort issues and migrate CI to vx ([31ed2a9](https://github.com/loonghao/dcc-mcp-core/commit/31ed2a9669f40b1b490cc8875f38c32e3c09ba52))
* resolve lint errors in test files (isort, ruff format, D106/F841) ([d703c4a](https://github.com/loonghao/dcc-mcp-core/commit/d703c4af92587b90c8165ed56d6e57ee714b8502))
* resolve release-please 'package.version is not tagged' error ([b433c71](https://github.com/loonghao/dcc-mcp-core/commit/b433c71db40c162d5f0694981012bdb9bb95410b))
* update GitHub Actions workflows for better Python version compatibility ([0e4b2bc](https://github.com/loonghao/dcc-mcp-core/commit/0e4b2bca67e4c9b18690ead283e05d13fb0d8ee7))
* update GitHub Actions workflows to regenerate poetry.lock before install ([752206b](https://github.com/loonghao/dcc-mcp-core/commit/752206b98daf86d455cdcc33374110e81dc301b6))
* update Mermaid diagrams for better GitHub compatibility and visibility ([fc43474](https://github.com/loonghao/dcc-mcp-core/commit/fc43474a2b3c8fea4e44841788a70b0f741d5c77))


### Code Refactoring

* **dcc_mcp_core:** Replace plugin manager with action manager across the codebase ([18ea75b](https://github.com/loonghao/dcc-mcp-core/commit/18ea75bb1d7507f5ecbcb390a7ee5a949cc7a94f))
* Enhance ActionRegistry and add hooks ([e300109](https://github.com/loonghao/dcc-mcp-core/commit/e300109f3d2a1da1e3dbc45d1b025c469cab0417))
* Enhance error handling and parameter management ([f58bba1](https://github.com/loonghao/dcc-mcp-core/commit/f58bba15f9c9717afa2eccab962533d95d82d2f1))
* Improve action manager and registry implementation ([8e9bc65](https://github.com/loonghao/dcc-mcp-core/commit/8e9bc652cf6fb59ddc3059022769895a475a7d6e))
* Improve action path handling and code clarity ([8e42439](https://github.com/loonghao/dcc-mcp-core/commit/8e424397a4d45b4437dc5038f90fb6f58f7411bb))
* Improve code quality and functionality in multiple modules ([4f8212f](https://github.com/loonghao/dcc-mcp-core/commit/4f8212f90cb256e78f817b9ed7107d6160aded67))
* Improve dependency injector module ([5fc76d6](https://github.com/loonghao/dcc-mcp-core/commit/5fc76d61ee77deba3cdb97cc509f35bb5310340d))
* Improve imports and replace hardcoded constants ([d69ff22](https://github.com/loonghao/dcc-mcp-core/commit/d69ff22f467155cf0e6483b0b6b66a196fc54afa))
* Improve test methods and update comments ([8931253](https://github.com/loonghao/dcc-mcp-core/commit/89312534646bda1d4c64c1cf8b683d8480a94919))
* Optimize imports and update workflows ([4279cf7](https://github.com/loonghao/dcc-mcp-core/commit/4279cf74f9c2d29e6a445f8bd400ccec2a268244))
* Refactor action manager and adapters ([1367826](https://github.com/loonghao/dcc-mcp-core/commit/1367826267cdcc885ea9fa0bdcce1e4e720656af))
* Refactor build and repository setup ([bf8acdf](https://github.com/loonghao/dcc-mcp-core/commit/bf8acdf146c533fcf5b5081c87625b226b123030))
* remove legacy Python code and fix tag format to v0.x.0 ([88b54ee](https://github.com/loonghao/dcc-mcp-core/commit/88b54ee3f5b0ef8112a199cd62cd3d4eb75b24ae))
* Remove unused modules and add path conversion functions ([fcdabb8](https://github.com/loonghao/dcc-mcp-core/commit/fcdabb8708c8e388965a6802ecacf3f48668dcf4))
* Standardize string quotation usage across the codebase ([90728f7](https://github.com/loonghao/dcc-mcp-core/commit/90728f739b178a3a46dbeeb9359f6aac4bcfe0a6))
* Update platformdirs handling in config paths ([b7d7ba9](https://github.com/loonghao/dcc-mcp-core/commit/b7d7ba9e0cb082e9f4dfef00d5e13b4a50ecfe75))


### Documentation

* Add comprehensive Sphinx documentation for DCC-MCP-Core ([d49dbaf](https://github.com/loonghao/dcc-mcp-core/commit/d49dbaf463fe2dc56aa3c51284fddb299efdf029))
* migrate from Sphinx to VitePress with i18n support ([1c4ef9c](https://github.com/loonghao/dcc-mcp-core/commit/1c4ef9cd96ef23d9c6d7c32605e4fd785324d5d7))

## v0.9.0 (2026-03-24)

### Feat

- Add function adapters for Action classes

### Refactor

- Improve action manager and registry implementation
- Improve dependency injector module
- Refactor action manager and adapters

## v0.8.0 (2025-04-07)

### Feat

- Enhance action registration and classification

### Refactor

- Enhance ActionRegistry and add hooks

## v0.7.0 (2025-04-05)

### Feat

- Add Pydantic extensions and update related modules

### Refactor

- Improve test methods and update comments
- Enhance error handling and parameter management

## v0.6.0 (2025-04-03)

### Feat

- Enhance action management and DCC support

## v0.5.0 (2025-04-01)

### Feat

- Enhance ActionRegistry with DCC-specific features

## v0.4.0 (2025-03-27)

### Feat

- Restructure imports, remove unused code, and update templates

### Refactor

- Remove unused modules and add path conversion functions

## v0.3.1 (2025-03-24)

### Refactor

- Standardize string quotation usage across the codebase
- Improve action path handling and code clarity

## v0.3.0 (2025-03-23)

### Feat

- Improve imports and module interface in dcc_mcp_core
- Improve imports and module interface in dcc_mcp_core

### Refactor

- **dcc_mcp_core**: Replace plugin manager with action manager across the codebase Update references from plugin manager to action manager in imports, documentation, and tests.

## v0.2.0 (2025-03-19)

### Feat

- Add various test plugins and utilities for plugin management system

### Refactor

- Improve imports and replace hardcoded constants
- Improve code quality and functionality in multiple modules
- Update platformdirs handling in config paths

## v0.1.0 (2025-03-19)

### Feat

- Add foundational components and documentation for dcc-mcp-core

### Fix

- add Python 3.7 compatibility for importlib.metadata
- add special handling for Python 3.7 in GitHub Actions workflow
- improve GitHub Actions workflows for Windows compatibility
- update GitHub Actions workflows for better Python version compatibility
- update GitHub Actions workflows to regenerate poetry.lock before install
- update Mermaid diagrams for better GitHub compatibility and visibility

### Refactor

- Optimize imports and update workflows
- Refactor build and repository setup
