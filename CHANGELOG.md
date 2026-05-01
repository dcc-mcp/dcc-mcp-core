## v0.10.0 (2026-03-28)

### Feat

- replace pre-commit with vx prek and add justfile
- add Skills system for zero-code script registration as MCP tools

### Fix

- resolve lint errors in test files (isort, ruff format, D106/F841)
- add cross-platform shell support to justfile
- resolve isort issues and migrate CI to vx

## [0.14.21](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.20...v0.14.21) (2026-05-01)


### Features

* **project:** add active_tool_groups and created_at fields ([#576](https://github.com/loonghao/dcc-mcp-core/issues/576)) ([b3f793c](https://github.com/loonghao/dcc-mcp-core/commit/b3f793c5c01adb470ac71895990dd9a800ad2c34))
* **project:** add register_project_tools with 4 MCP tools ([#576](https://github.com/loonghao/dcc-mcp-core/issues/576)) ([76fe945](https://github.com/loonghao/dcc-mcp-core/commit/76fe945441058224c7e19a9c46c53d28c6c9e8ee))
* **project:** integrate CheckpointStore as DccProject.checkpoints ([#576](https://github.com/loonghao/dcc-mcp-core/issues/576)) ([b535aba](https://github.com/loonghao/dcc-mcp-core/commit/b535aba639e4bc9012aa06ebd07c13bfae23321f))


### Documentation

* **project:** add project-persistence guide (EN + ZH) ([#576](https://github.com/loonghao/dcc-mcp-core/issues/576)) ([f85967f](https://github.com/loonghao/dcc-mcp-core/commit/f85967f497ffa9ab492986a76e226612a651fb5f))
* update outdated crate references and version numbers ([103e6b5](https://github.com/loonghao/dcc-mcp-core/commit/103e6b53987b575cb715ffbb69023ffcdb02a063))

## [0.14.20](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.19...v0.14.20) (2026-05-01)


### Features

* add adapter context policy helpers ([cb54c13](https://github.com/loonghao/dcc-mcp-core/commit/cb54c136cd6042b1f73c325e8af7f7d8133fd5b5))
* add adaptive pump policy ([226c87e](https://github.com/loonghao/dcc-mcp-core/commit/226c87eae8823f55ef91d80d3f547d4c85b871ad))
* add bridge resilience strategies ([11bc11c](https://github.com/loonghao/dcc-mcp-core/commit/11bc11cec5bee91b21de645a25c2fc172fe56206))
* add deferred tool result polling ([30272b6](https://github.com/loonghao/dcc-mcp-core/commit/30272b65896874d1a92642f6282822426919ef01))
* add embedded dispatcher bootstrap ([61e24a5](https://github.com/loonghao/dcc-mcp-core/commit/61e24a5209b4290564513257b107683903f56a7d))
* add gateway instance pooling leases ([69d34d7](https://github.com/loonghao/dcc-mcp-core/commit/69d34d7920ddef8e4732b190f1d6a67c0adf01e5))
* add host execution bridge ([db9d1f4](https://github.com/loonghao/dcc-mcp-core/commit/db9d1f435afa5286813bf3ed2c24e89310e363bb))
* add project state persistence ([51c88eb](https://github.com/loonghao/dcc-mcp-core/commit/51c88eb2a027e6d44fd83b45bf5fdeed0e84c1f9))
* add Rez context bundle skill examples ([b92c92c](https://github.com/loonghao/dcc-mcp-core/commit/b92c92cc76691889758de6936aeb0965f977559d))
* add script execution envelopes ([c9874f0](https://github.com/loonghao/dcc-mcp-core/commit/c9874f08b112bb424c623ec288040231572f0235))
* add structured recipe packs ([48fb309](https://github.com/loonghao/dcc-mcp-core/commit/48fb309c39426ce8a68764d95a5fb2a4fe5f9fe0))
* add weak DCC execution guardrails ([403a1f9](https://github.com/loonghao/dcc-mcp-core/commit/403a1f9ee91d0598fa965233b6bf7e4161f52aef))
* expose gateway instance diagnostics ([b79c047](https://github.com/loonghao/dcc-mcp-core/commit/b79c047bb83c143b34c5e46173d00d7d7a2a3cd0))
* mark tools with fallback input schemas in _meta ([3a75431](https://github.com/loonghao/dcc-mcp-core/commit/3a75431227b67d1332bf0883d54a90d2bea2cab3))
* pass in-process execution metadata ([675a913](https://github.com/loonghao/dcc-mcp-core/commit/675a913d5b9acecfeea4b5618827b85836adac6c))


### Bug Fixes

* allow bare gateway tools for single instance ([2bec3e1](https://github.com/loonghao/dcc-mcp-core/commit/2bec3e1bd1d6194a3209cce6772d3fb0acae914e)), closes [#583](https://github.com/loonghao/dcc-mcp-core/issues/583)
* expose health on MCP instance servers ([c52c635](https://github.com/loonghao/dcc-mcp-core/commit/c52c635fb48c74608a74833a40bda459b7953fa8))
* flatten gateway skill aggregation results ([131f93f](https://github.com/loonghao/dcc-mcp-core/commit/131f93f0f1652a00487dea0f08dd83fefc7f7480)), closes [#582](https://github.com/loonghao/dcc-mcp-core/issues/582)
* keep Python tests compatible with pagination ([#646](https://github.com/loonghao/dcc-mcp-core/issues/646)) ([560f98f](https://github.com/loonghao/dcc-mcp-core/commit/560f98fef0da75066527bc5f345be0d661c842d9))
* keep script execution capture compatible with py38 ([9ac118c](https://github.com/loonghao/dcc-mcp-core/commit/9ac118cd4f3e91f9673f86fc3847ca396e41ffc0))
* normalize script execution parameters ([0f293d2](https://github.com/loonghao/dcc-mcp-core/commit/0f293d29a432ec1ca395d631089a610215644b9b)), closes [#591](https://github.com/loonghao/dcc-mcp-core/issues/591)
* propagate tool result error flag ([b5da89e](https://github.com/loonghao/dcc-mcp-core/commit/b5da89e33a955f76d1e01e68242b18237ebb95c4))
* require executor for main-affined skills ([51a8e5d](https://github.com/loonghao/dcc-mcp-core/commit/51a8e5dc514f76be4914af88768810bca25fc8c2))
* require MCP health before gateway fanout ([a7f7a7a](https://github.com/loonghao/dcc-mcp-core/commit/a7f7a7a2696fc8a244b5b3fbe6db7a33afa2f5bc))
* surface in-process skill errors as structured envelopes ([f13c782](https://github.com/loonghao/dcc-mcp-core/commit/f13c7824f9e7c0f922301a75f4c6295ebe2b4246))
* tighten JSON-RPC request boundary handling ([cc5b81f](https://github.com/loonghao/dcc-mcp-core/commit/cc5b81f73f40675b3503bffb151a8f6e930859a1))


### Documentation

* add AI agent entry points (CLAUDE.md, GEMINI.md, COPILOT.md) ([ea49dab](https://github.com/loonghao/dcc-mcp-core/commit/ea49dab2a9bd4d01d3f01213430ec8062be0f08a))
* add CODEBUDDY.md for CodeBuddy AI agent support ([b08c980](https://github.com/loonghao/dcc-mcp-core/commit/b08c980f4a5b6b741ed09ca2d95df0a7b823cd83))
* migrate skills to v0.15+ sibling-file format, add constants re-exports ([6b071b2](https://github.com/loonghao/dcc-mcp-core/commit/6b071b29733d69a0f7546a8402ce48f060a53398))
* optimize AI agent onboarding, skill discoverability, and tool design guidance ([#647](https://github.com/loonghao/dcc-mcp-core/issues/647)) ([fc2cb88](https://github.com/loonghao/dcc-mcp-core/commit/fc2cb883f645e18fd36c75ce75ff5bbed25be806))
* update AGENTS.md to reference AI agent entry points ([8f3916f](https://github.com/loonghao/dcc-mcp-core/commit/8f3916fb7e2dd1686cdc2774466c320d55b2c0a0))
* update version in llms.txt to 0.14.19 ([3c4ad99](https://github.com/loonghao/dcc-mcp-core/commit/3c4ad9971f2fc80d98cbce710efba4c0c3e9ee1d))

## [0.14.19](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.18...v0.14.19) (2026-04-29)


### Features

* **build:** optimize Windows build speed with sccache and LLD linker ([4c56606](https://github.com/loonghao/dcc-mcp-core/commit/4c5660664ec278fdd6360ceedaccb82d6688ecd9))
* **http:** JobRecoveryPolicy contract for McpHttpConfig ([#567](https://github.com/loonghao/dcc-mcp-core/issues/567)) ([3612aa6](https://github.com/loonghao/dcc-mcp-core/commit/3612aa60e29f67d5d765ddd04e77a7391410e434))
* Prometheus metrics endpoint for gateway observability ([#559](https://github.com/loonghao/dcc-mcp-core/issues/559)) ([730dd8b](https://github.com/loonghao/dcc-mcp-core/commit/730dd8b7d48476708441f3e3d782697624187473))
* **workflow:** persistent idempotency cache via SqliteIdempotencyStore ([#566](https://github.com/loonghao/dcc-mcp-core/issues/566)) ([c494182](https://github.com/loonghao/dcc-mcp-core/commit/c49418235d4a21c59f17a3411c8ff17f4767ca19))
* **workflow:** workflows.resume MCP tool + executor.resume() ([#565](https://github.com/loonghao/dcc-mcp-core/issues/565)) ([8c37f11](https://github.com/loonghao/dcc-mcp-core/commit/8c37f11d3311ad82c8e545f383313c14ae06af66))


### Bug Fixes

* **build:** make sccache opt-in via shell env (regression from 4c56606) ([05af2f2](https://github.com/loonghao/dcc-mcp-core/commit/05af2f217bf584dff23d7869e3147b9d0176b915))
* gateway reliability, security, and logging defaults ([#551](https://github.com/loonghao/dcc-mcp-core/issues/551), [#552](https://github.com/loonghao/dcc-mcp-core/issues/552), [#553](https://github.com/loonghao/dcc-mcp-core/issues/553), [#554](https://github.com/loonghao/dcc-mcp-core/issues/554), [#555](https://github.com/loonghao/dcc-mcp-core/issues/555), [#556](https://github.com/loonghao/dcc-mcp-core/issues/556), [#557](https://github.com/loonghao/dcc-mcp-core/issues/557), [#558](https://github.com/loonghao/dcc-mcp-core/issues/558)) ([#560](https://github.com/loonghao/dcc-mcp-core/issues/560)) ([7749079](https://github.com/loonghao/dcc-mcp-core/commit/7749079e9b092271fce2475193890024610d516d))
* **gateway,skills:** three-tier election + stale-aware list_dcc_instances + strict scan ([#568](https://github.com/loonghao/dcc-mcp-core/issues/568)) ([282eafe](https://github.com/loonghao/dcc-mcp-core/commit/282eafe8c4caac08796c26f590cfcf2e27c3d500))
* **skills:** pin Python child stdio to UTF-8 across platforms ([bc8f5cb](https://github.com/loonghao/dcc-mcp-core/commit/bc8f5cbb9dcc30385397f740a35c909c2ba7be4c))
* **skills:** use ptrace TracerPid detection to skip real-exec tests under tarpaulin ([#570](https://github.com/loonghao/dcc-mcp-core/issues/570)) ([9339462](https://github.com/loonghao/dcc-mcp-core/commit/93394621755f5504a9620898508043ec10e24fb3))
* **transport:** drop exclusive heartbeat lock that dropped concurrent writes ([459492c](https://github.com/loonghao/dcc-mcp-core/commit/459492cd95fe9747b522a2d53e540987c378d2f3))


### Documentation

* **agents:** document gateway reliability, security, logging defaults, and Prometheus metrics ([#551](https://github.com/loonghao/dcc-mcp-core/issues/551)-[#559](https://github.com/loonghao/dcc-mcp-core/issues/559)) ([b8a5e57](https://github.com/loonghao/dcc-mcp-core/commit/b8a5e57fc9e6ecf03e9eae79a30ae4b692da8ff3))
* fix VitePress mustache + markdownlint dash style in workflows.md ([3dd78e1](https://github.com/loonghao/dcc-mcp-core/commit/3dd78e16e69e02f150a579193049aaf65c67c474))
* **skills:** layered architecture guide for complex skills ([#575](https://github.com/loonghao/dcc-mcp-core/issues/575)) ([949e1e7](https://github.com/loonghao/dcc-mcp-core/commit/949e1e768c391b8f89e87122ca1eac771de20e18))

## [0.14.18](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.17...v0.14.18) (2026-04-29)


### Features

* **tunnel:** WS frontend, /tunnels admin endpoint, agent reconnect ([#504](https://github.com/loonghao/dcc-mcp-core/issues/504)) ([2a142c8](https://github.com/loonghao/dcc-mcp-core/commit/2a142c8bcf4c19dd1a29af76f70e455cabdfb01e))

## [0.14.17](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.16...v0.14.17) (2026-04-29)


### Features

* **cancellation:** add check_dcc_cancelled + JobHandle ([#522](https://github.com/loonghao/dcc-mcp-core/issues/522)) ([cdf33b4](https://github.com/loonghao/dcc-mcp-core/commit/cdf33b40808219058263cd2b2b8192bfc8b486a4))
* **http:** migrate PyMcpHttpConfig to #[derive(PyWrapper)] ([#528](https://github.com/loonghao/dcc-mcp-core/issues/528) M3.2) ([05b8fb5](https://github.com/loonghao/dcc-mcp-core/commit/05b8fb5f400e6af7507e5a6f4df73516622bb913))
* **pybridge-derive:** add get(to_string) field mode ([#528](https://github.com/loonghao/dcc-mcp-core/issues/528) M3.1) ([a171bbd](https://github.com/loonghao/dcc-mcp-core/commit/a171bbd0b110b214ab21f96a664f370b300ce1e3))
* **pybridge-derive:** full codegen for #[derive(PyWrapper)] ([#528](https://github.com/loonghao/dcc-mcp-core/issues/528) M2) ([3b94abe](https://github.com/loonghao/dcc-mcp-core/commit/3b94abea73a4ab0c1de2549fec1186691cf40f1d))
* **pybridge:** scaffold dcc-mcp-pybridge-derive proc-macro crate ([781804b](https://github.com/loonghao/dcc-mcp-core/commit/781804b65767f75df3139f3b2916b1f806854576)), closes [#528](https://github.com/loonghao/dcc-mcp-core/issues/528)
* **server-base:** callable-payload dispatch protocols + reference impl ([#520](https://github.com/loonghao/dcc-mcp-core/issues/520)) ([9774093](https://github.com/loonghao/dcc-mcp-core/commit/977409336a82fbb1bbde069646b037083cadf41a))
* **server-base:** MinimalModeConfig declarative progressive loading ([#525](https://github.com/loonghao/dcc-mcp-core/issues/525)) ([b3c5c39](https://github.com/loonghao/dcc-mcp-core/commit/b3c5c3992790e41d130cc969622b02059d13ec63))
* **server-base:** register_inprocess_executor + BaseDccCallableDispatcher ([#521](https://github.com/loonghao/dcc-mcp-core/issues/521)) ([0f93bac](https://github.com/loonghao/dcc-mcp-core/commit/0f93baceb398be0694aca01c36b07f402b826906))
* **skills:** public is_gui_executable + correct_python_executable ([#524](https://github.com/loonghao/dcc-mcp-core/issues/524)) ([ef313b0](https://github.com/loonghao/dcc-mcp-core/commit/ef313b09a3dca8df0a3e11079857adc02de942ca))
* **transport:** add FileRegistry::read_alive auto-eviction ([#523](https://github.com/loonghao/dcc-mcp-core/issues/523)) ([1f49122](https://github.com/loonghao/dcc-mcp-core/commit/1f49122d7863d32b94c236d9d0f81eb0bd91b2c1))
* **tunnel:** control + data plane + e2e MVP for relay ([#504](https://github.com/loonghao/dcc-mcp-core/issues/504)) ([ed096c6](https://github.com/loonghao/dcc-mcp-core/commit/ed096c694ebbf96f0d5f1bb9f145f3f91fbe4874))
* **tunnel:** scaffold dcc-mcp-tunnel-{protocol,relay,agent} crates ([#504](https://github.com/loonghao/dcc-mcp-core/issues/504) PR 1/5) ([2f6ec41](https://github.com/loonghao/dcc-mcp-core/commit/2f6ec413f6f2cbb25efe04ab448060e97679c2d8))


### Bug Fixes

* **callable-dispatcher:** py3.7 compatibility for Protocol/runtime_checkable/Literal ([7af122c](https://github.com/loonghao/dcc-mcp-core/commit/7af122c1e5e7b7857e1bae54edafc6fe96fb4f4e))
* **cancellation:** py3.7 compatibility for Protocol/runtime_checkable ([5fc65b8](https://github.com/loonghao/dcc-mcp-core/commit/5fc65b84e85a34de81a80733082818a4eea608c6)), closes [#522](https://github.com/loonghao/dcc-mcp-core/issues/522)
* **inprocess-executor:** py3.7 compatibility for Protocol/runtime_checkable ([ef6abdd](https://github.com/loonghao/dcc-mcp-core/commit/ef6abdd522d9efe08f4a6233ae8bf92fef47b19d))
* **release:** unblock 0.14.17 - jsonwebtoken security upgrade + flaky test ([4e825a8](https://github.com/loonghao/dcc-mcp-core/commit/4e825a81b44122e9b4cfd0b6de77bf3c04aade12))
* **skills:** preserve on-disk casing in locate_sibling for case-insensitive FS ([58dcb36](https://github.com/loonghao/dcc-mcp-core/commit/58dcb36dd7b20e015e136428cae5cfd7ae4be3d5))


### Code Refactoring

* **models:** migrate SkillMetadata to #[derive(PyWrapper)] ([#528](https://github.com/loonghao/dcc-mcp-core/issues/528) M3.3) ([adbf3c0](https://github.com/loonghao/dcc-mcp-core/commit/adbf3c06231160e5c4aeee9bcc3ec802e6edff00))
* **models:** migrate ToolDeclaration + SkillGroup to #[derive(PyWrapper)] ([#528](https://github.com/loonghao/dcc-mcp-core/issues/528) M3.4) ([167152c](https://github.com/loonghao/dcc-mcp-core/commit/167152cc7420e1fa8427943ce179c45cbe72d1e4))


### Documentation

* **agent:** API reference + guide pages for [#520](https://github.com/loonghao/dcc-mcp-core/issues/520)-[#525](https://github.com/loonghao/dcc-mcp-core/issues/525) host integration ([eb4b3b8](https://github.com/loonghao/dcc-mcp-core/commit/eb4b3b8c949b607b30f3a4b7d404da6bdb1044b5))
* AGENTS.md decision-table now points at the working APIs (RelayServer::start, run_once, auth::issue). llms.txt gains the same entries. New docs/guide/tunnel-relay.md (+ docs/zh/guide/tunnel-relay.md) covers architecture, minimal example, wire format, JWT scoping, eviction, and the MVP-vs-follow-up matrix. ([ed096c6](https://github.com/loonghao/dcc-mcp-core/commit/ed096c694ebbf96f0d5f1bb9f145f3f91fbe4874))
* refresh agent-facing docs after EPIC [#495](https://github.com/loonghao/dcc-mcp-core/issues/495) ([f15f494](https://github.com/loonghao/dcc-mcp-core/commit/f15f4941919f8383098c0fd1d6037296b17161e0))
* **tests:** note e2e test files are CI-active, not to be --ignored ([9cb2ba6](https://github.com/loonghao/dcc-mcp-core/commit/9cb2ba67a8efda32fefa5e94ac6bd6446881bb9d)), closes [#526](https://github.com/loonghao/dcc-mcp-core/issues/526)

## [0.14.16](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.15...v0.14.16) (2026-04-28)


### Bug Fixes

* **http,process:** add missing module files for audit refactor ([06f4313](https://github.com/loonghao/dcc-mcp-core/commit/06f431321922345ee2bc4f12c146ee407a504449))


### Code Refactoring

* **actions:** extract VersionMatcher + ValidationStrategy traits ([#493](https://github.com/loonghao/dcc-mcp-core/issues/493)) ([cc2824c](https://github.com/loonghao/dcc-mcp-core/commit/cc2824c67406aed322af93a08dd04de61cad9f51))
* **core:** extract Registry&lt;V&gt; trait + share contract test ([#489](https://github.com/loonghao/dcc-mcp-core/issues/489)) ([406f43d](https://github.com/loonghao/dcc-mcp-core/commit/406f43dace61c42c3152fe806377f0a25dd8ce58))
* **dcc-mcp-http:** introduce NotificationBuilder for JSON-RPC envelopes ([d4b1948](https://github.com/loonghao/dcc-mcp-core/commit/d4b1948d4314d51b23cfbafda69a204953605347))
* **dcc-mcp-skills:** reorganize validator_*.rs and watcher_*.rs into directory modules ([140e389](https://github.com/loonghao/dcc-mcp-core/commit/140e389c22b25966ac7363298d7b6c13842b6b6b)), closes [#482](https://github.com/loonghao/dcc-mcp-core/issues/482) [#483](https://github.com/loonghao/dcc-mcp-core/issues/483)
* **http,process,transport:** apply audit findings ([36276f7](https://github.com/loonghao/dcc-mcp-core/commit/36276f707f833a3a579707c7fb959994bce764d0))
* **http:** introduce MethodHandler trait + extensible MethodRouter ([#492](https://github.com/loonghao/dcc-mcp-core/issues/492)) ([413d9a9](https://github.com/loonghao/dcc-mcp-core/commit/413d9a98c4b04b5bfb7090cdac28fd48c727b0f7))
* introduce DccName newtype + typed scanner entry point ([9182554](https://github.com/loonghao/dcc-mcp-core/commit/91825542ee51c6faa6b87bb5b49dd9a4da2526f0))
* introduce shared DccMcpError + From impls for HttpError, ProcessError ([417c026](https://github.com/loonghao/dcc-mcp-core/commit/417c026047d99a0f9163797a675b153c4f278964))
* **pyo3:** add wrapper helpers + drift-detection test ([#490](https://github.com/loonghao/dcc-mcp-core/issues/490)) ([cb5cc79](https://github.com/loonghao/dcc-mcp-core/commit/cb5cc79de10218b62180b089e026d56fb0b70c56))
* **python:** extract shared register_tools() helper ([59b41e5](https://github.com/loonghao/dcc-mcp-core/commit/59b41e5be267e7d5c28d3db590ba83cda5d6d275))
* **python:** introduce typed ToolResult envelope + constants module ([819e8c4](https://github.com/loonghao/dcc-mcp-core/commit/819e8c4c15d810e1c4184b3e63381ada5a1e0491))
* **server:** decompose DccServerBase into focused collaborators ([24ec80d](https://github.com/loonghao/dcc-mcp-core/commit/24ec80d405edbdfe2865c54f17e0a4af92a5c799))
* **workspace:** consolidate per-crate pyclass impls under src/python/ ([22e197e](https://github.com/loonghao/dcc-mcp-core/commit/22e197ea8f3f49e7aa3182d1a92203cf65f47e27)), closes [#501](https://github.com/loonghao/dcc-mcp-core/issues/501) [#495](https://github.com/loonghao/dcc-mcp-core/issues/495)
* **workspace:** extract dcc-mcp-logging crate from dcc-mcp-utils ([a3882a2](https://github.com/loonghao/dcc-mcp-core/commit/a3882a2e59299c589b9cb564675ba3f19aa9db14))
* **workspace:** extract dcc-mcp-paths and delete dcc-mcp-utils ([a6bc5fc](https://github.com/loonghao/dcc-mcp-core/commit/a6bc5fcf88f9c672eaf206f6fb473be477b909b3))
* **workspace:** extract dcc-mcp-pybridge crate from dcc-mcp-utils ([dc3b844](https://github.com/loonghao/dcc-mcp-core/commit/dc3b844a9d853f24f729113e976d384d6b7762e5))
* **workspace:** migrate skill-domain code from dcc-mcp-utils to dcc-mcp-skills ([f66c63a](https://github.com/loonghao/dcc-mcp-core/commit/f66c63a705b22e1f9cff38bc4351e181e8568fae))


### Documentation

* **agents:** forbid AI-attribution footers in PRs and commits ([222c479](https://github.com/loonghao/dcc-mcp-core/commit/222c479d4b046b26accf7b596bd1e6bf167a69cb))
* consolidate per-LLM agent rules into AGENTS.md + agents-reference.md ([fdedd52](https://github.com/loonghao/dcc-mcp-core/commit/fdedd5219907bd3b759a6fc5d92dbbcebdfb2e35))
* fix MD049 emphasis style in agents-reference.md ([b881d6d](https://github.com/loonghao/dcc-mcp-core/commit/b881d6dc48abaf550c122bb2c630388a6a84662b))

## [0.14.15](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.14...v0.14.15) (2026-04-27)


### Bug Fixes

* **ci:** extend cargo-clippy conflict workaround to Linux runners ([101df5f](https://github.com/loonghao/dcc-mcp-core/commit/101df5f2449eefe373ccbb5b66111521cd32b1a8))
* **ci:** remove pre-installed cargo-clippy on macOS before toolchain setup ([3bec807](https://github.com/loonghao/dcc-mcp-core/commit/3bec807a9cb5dc7fb27f0e3b97d7f559f1891524))
* **deps:** update rust dependencies ([77eecb2](https://github.com/loonghao/dcc-mcp-core/commit/77eecb24ec9d52859165b63d2672c0f489714b09))
* **http:** replace into_py() with direct tuple arg for PyO3 0.28 compat ([6276862](https://github.com/loonghao/dcc-mcp-core/commit/62768623e4705c6cf90239d61116d710c9f8cc20))
* **models:** add missing recipes_file/introspection_file to SkillMetadata Python constructor ([4f100e9](https://github.com/loonghao/dcc-mcp-core/commit/4f100e9493002318e7f19b3ea1f6d67b3f1be279))
* resolve issues [#464](https://github.com/loonghao/dcc-mcp-core/issues/464) [#465](https://github.com/loonghao/dcc-mcp-core/issues/465) [#466](https://github.com/loonghao/dcc-mcp-core/issues/466) [#467](https://github.com/loonghao/dcc-mcp-core/issues/467) ([b7ad70d](https://github.com/loonghao/dcc-mcp-core/commit/b7ad70d0bdec33afae951091de885851c8f8e5d4))
* **skills:** update SkillCatalog.set_in_process_executor in python.rs to use RwLock API ([16a59bf](https://github.com/loonghao/dcc-mcp-core/commit/16a59bf1d9e7f7956df6206903b2a89639a1e198))
* **workspace-hack:** remove invalid rand/rand_core features removed in 0.10, regenerate with hakari ([a396a9b](https://github.com/loonghao/dcc-mcp-core/commit/a396a9baa4850250af8bb5da767086427a14e2d0))


### Documentation

* sync AI-facing docs with latest API surface ([e611c93](https://github.com/loonghao/dcc-mcp-core/commit/e611c9383425afb31600744ee907bd32d806a2dc))

## [0.14.14](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.13...v0.14.14) (2026-04-26)


### Features

* implement dynamic tool registration ([#462](https://github.com/loonghao/dcc-mcp-core/issues/462)) and output:// resource ([#461](https://github.com/loonghao/dcc-mcp-core/issues/461)) ([835c156](https://github.com/loonghao/dcc-mcp-core/commit/835c156286a7c91012688ee6940e8439ef6bb9cd))
* **skills:** add accumulated evolved skills discovery and persistence ([f8a449e](https://github.com/loonghao/dcc-mcp-core/commit/f8a449e696fe8d27cd9a663b0a7f128482c16b81))


### Bug Fixes

* **ci:** drop --locked from cargo-tarpaulin install to allow rustc 1.90 compatible deps ([c46a98f](https://github.com/loonghao/dcc-mcp-core/commit/c46a98f4b2329a6eb2e44118b0c3df61c9576cd1))
* **ci:** pin cargo-tarpaulin to ~0.31 to avoid cargo-platform 0.3.3 rustc&gt;=1.91 constraint ([2e3049b](https://github.com/loonghao/dcc-mcp-core/commit/2e3049b7c98fb108374cfb80cc0941c252f356b1))
* **ci:** use nightly toolchain for rust-coverage job to satisfy cargo-tarpaulin rustc&gt;=1.91 requirement ([bd958cf](https://github.com/loonghao/dcc-mcp-core/commit/bd958cf3eace03e3ff017d1a6333b65534011e41))
* **ci:** use taiki-e/install-action for cargo-tarpaulin ([db78f20](https://github.com/loonghao/dcc-mcp-core/commit/db78f20d37f9c6a5431258e3f63a0d71a8d122ca))
* commit Cargo.lock and use exact versions in workspace-hack ([913f62c](https://github.com/loonghao/dcc-mcp-core/commit/913f62ce2f1f8d5117ebb5ab7672b69021301067))
* exclude pyo3 from workspace-hack to fix stubgen build ([df5e34e](https://github.com/loonghao/dcc-mcp-core/commit/df5e34e71338fe4ee1a4e3bd985be12c9c55ae3f))
* **gateway:** improve 'unknown tool' error message for internal action name format ([699dbb1](https://github.com/loonghao/dcc-mcp-core/commit/699dbb1c9c26ecf5e866ca17e8f3d64a6300a17d))
* **gateway:** SSE 30s disconnect, tool call cancellation, log noise, and layer metadata ([ee49c87](https://github.com/loonghao/dcc-mcp-core/commit/ee49c874692d04cfc29c3209c8e782bb79d874ac))
* **tests:** add register_tool/deregister_tool/list_dynamic_tools to Python test core-tool sets ([37001ac](https://github.com/loonghao/dcc-mcp-core/commit/37001acefef7f7ff3a9d0edca55778e205c47de8))
* **tests:** update backend_timeout_ms assertions from 10s to 120s default ([ccda28c](https://github.com/loonghao/dcc-mcp-core/commit/ccda28c2cd4407bcb1d3fabcb9ad394adc3f2311))
* **tests:** update tool counts and is_core list for 3 new dynamic-tool methods ([#462](https://github.com/loonghao/dcc-mcp-core/issues/462)) ([04f891a](https://github.com/loonghao/dcc-mcp-core/commit/04f891a52b0487ba7654c4509635965347b87986))


### Performance Improvements

* add workspace-hack via cargo-hakari and optimize build speed ([e307411](https://github.com/loonghao/dcc-mcp-core/commit/e307411767beedc59013850305e30e909459ae16))
* optimize build speed with workspace-hack and macOS fixes ([1e7a4e9](https://github.com/loonghao/dcc-mcp-core/commit/1e7a4e96d6238a6e3afda9a9aff3069eea540df3))
* reduce dev build time via debug=1, test consolidation, and axum http2 removal ([0102bb6](https://github.com/loonghao/dcc-mcp-core/commit/0102bb603a985de2fbc91915922a867ef7309848))

## [0.14.13](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.12...v0.14.13) (2026-04-25)


### Features

* **http:** connection-scoped cache for multi-turn tool call optimization ([#438](https://github.com/loonghao/dcc-mcp-core/issues/438)) ([e5f35b7](https://github.com/loonghao/dcc-mcp-core/commit/e5f35b71829bd26b1cd053c306fe7e7a4817d4cc))


### Bug Fixes

* **ci:** skip stubgen on Python 3.7 builds, mirror release flow in PR CI ([24867a8](https://github.com/loonghao/dcc-mcp-core/commit/24867a8b02ac6861f9ee6cb46553e50d0f8fb015))

## [0.14.12](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.11...v0.14.12) (2026-04-25)


### Features

* add metadata.dcc-mcp.recipes sibling-file + recipes__list/get tools ([#428](https://github.com/loonghao/dcc-mcp-core/issues/428)) ([#447](https://github.com/loonghao/dcc-mcp-core/issues/447)) ([8a06497](https://github.com/loonghao/dcc-mcp-core/commit/8a064974c48108ae50f10eb0776b47dd6ea30341))
* add Rust-powered json_dumps/json_loads and replace stdlib json in library code ([4c64e7e](https://github.com/loonghao/dcc-mcp-core/commit/4c64e7e257839886b7a61efda09d9486d3f003cf))
* add Rust-powered yaml_loads/yaml_dumps, eliminate PyYAML dependency ([230d5a3](https://github.com/loonghao/dcc-mcp-core/commit/230d5a3d2c5c5ca12498e20badeb8cf60c5c7e4a))
* **checkpoint:** add checkpoint/resume helpers for long-running tool executions ([#436](https://github.com/loonghao/dcc-mcp-core/issues/436)) ([10a30f7](https://github.com/loonghao/dcc-mcp-core/commit/10a30f758613a37df9adf8f6b3564bb03010e0c7))
* **http:** agent rationale capture and dcc_feedback__report tool ([#433](https://github.com/loonghao/dcc-mcp-core/issues/433), [#434](https://github.com/loonghao/dcc-mcp-core/issues/434)) ([28e2182](https://github.com/loonghao/dcc-mcp-core/commit/28e2182d8a46f09f8c434f6ab2279e06cac7abb4))
* **http:** docs:// MCP resources for agent-facing format specs ([#435](https://github.com/loonghao/dcc-mcp-core/issues/435)) ([#446](https://github.com/loonghao/dcc-mcp-core/issues/446)) ([f4a2b6e](https://github.com/loonghao/dcc-mcp-core/commit/f4a2b6e1556ff53ed199911a8adba985bd94a3ae))
* **introspect:** add dcc_introspect__* built-in tools for runtime namespace discovery ([#426](https://github.com/loonghao/dcc-mcp-core/issues/426)) ([d65d5e3](https://github.com/loonghao/dcc-mcp-core/commit/d65d5e311679d169dd396bf1801f416de3b2fffc))
* **skill:** add skill_error_with_trace helper for agent self-heal ([#427](https://github.com/loonghao/dcc-mcp-core/issues/427)) ([a4798ce](https://github.com/loonghao/dcc-mcp-core/commit/a4798ceac1d2e24e5de761fe3984a4a29bd75cac))
* **skills:** YAML declarative workflow definitions with task/step semantics ([#439](https://github.com/loonghao/dcc-mcp-core/issues/439)) ([#450](https://github.com/loonghao/dcc-mcp-core/issues/450)) ([a856e0c](https://github.com/loonghao/dcc-mcp-core/commit/a856e0cc5df84800c1201c246bd6091bec024640))


### Documentation

* add missing API docs, fix dead links, add docs-check to pre-push ([#452](https://github.com/loonghao/dcc-mcp-core/issues/452)) ([fb5d20e](https://github.com/loonghao/dcc-mcp-core/commit/fb5d20e8fd0b07c89fe70afc6cc076cc91b4cb8a))
* **agents:** audit AGENTS.md per Augment Code study findings ([cb702a8](https://github.com/loonghao/dcc-mcp-core/commit/cb702a8167194af3da6297d9e47b5e8cd48a5c4c)), closes [#437](https://github.com/loonghao/dcc-mcp-core/issues/437)
* **llms:** add set_in_process_executor to Quick Decision Guide ([1349601](https://github.com/loonghao/dcc-mcp-core/commit/134960171b89c7e3e52ae8d929afa0820ce48e9a)), closes [#421](https://github.com/loonghao/dcc-mcp-core/issues/421)
* **skills:** RFC thin-harness skill authoring pattern ([#425](https://github.com/loonghao/dcc-mcp-core/issues/425)) ([a70848b](https://github.com/loonghao/dcc-mcp-core/commit/a70848ba26523399178be7671982aa87fd0d2014))

## [0.14.11](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.10...v0.14.11) (2026-04-25)


### Features

* 100% stub coverage, remove find_skills, fix CI tests ([e5d7a7d](https://github.com/loonghao/dcc-mcp-core/commit/e5d7a7dc37b8bc3ce61cf5d7704533965869fcf1))
* expand pyo3-stub-gen annotations to all crates ([33777fc](https://github.com/loonghao/dcc-mcp-core/commit/33777fc2c32b285e2d34a1b4b2c20ee5a79c2a9a))
* integrate stubgen into wheel builds, add EventBus method stubs ([5495a4b](https://github.com/loonghao/dcc-mcp-core/commit/5495a4b312f4b92556d1f9d7413804c1e5c669fa))


### Bug Fixes

* **gateway:** replace SSE total timeout with per-chunk idle timeout ([5b9bae7](https://github.com/loonghao/dcc-mcp-core/commit/5b9bae7f85e11cdbbade8c358b148baa715a3f83))
* update search_skills test signatures and gate test compilation in pre-commit ([7499932](https://github.com/loonghao/dcc-mcp-core/commit/7499932ab1a6fba34eb845fe07ac60cea6f48e8e))


### Code Refactoring

* inline rank_skills into search_skills ([c2f3e11](https://github.com/loonghao/dcc-mcp-core/commit/c2f3e11b8b3a23ae4ed8bd400b8c3b75fec105e8))


### Documentation

* remove .codex from default skill search path examples ([eb856be](https://github.com/loonghao/dcc-mcp-core/commit/eb856bee23272824e5d2ebcaf554d2f3dbd59c19))

## [0.14.10](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.9...v0.14.10) (2026-04-24)


### Bug Fixes

* **gateway:** prevent self-loop SSE subscription + pre-subscribe registry sweep ([#419](https://github.com/loonghao/dcc-mcp-core/issues/419)) ([d376056](https://github.com/loonghao/dcc-mcp-core/commit/d37605632f142f3c33a904e2644af1422a942839))


### Code Refactoring

* **build:** centralise wheel feature list in justfile ([e1e26b5](https://github.com/loonghao/dcc-mcp-core/commit/e1e26b5f65b77958f6ae72e76156d343afd1228b))


### Documentation

* document remote-server extensions from issues [#404](https://github.com/loonghao/dcc-mcp-core/issues/404)-[#411](https://github.com/loonghao/dcc-mcp-core/issues/411) ([59c8622](https://github.com/loonghao/dcc-mcp-core/commit/59c86223ca53b5a026d6745c55655d7c18851642))
* mark vitepress sidebar version for release-please auto-bump ([13597e1](https://github.com/loonghao/dcc-mcp-core/commit/13597e1b2fa8cbbc828952aad6e8850d15b31dec))
* **readme:** rewrite README for v0.14+ APIs and fix formatting drift ([29cb0e8](https://github.com/loonghao/dcc-mcp-core/commit/29cb0e8003b274e2dc441796c1af9c30fcb30796))

## [0.14.9](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.8...v0.14.9) (2026-04-24)


### Features

* implement issues [#404](https://github.com/loonghao/dcc-mcp-core/issues/404)-[#411](https://github.com/loonghao/dcc-mcp-core/issues/411) remote server, batch dispatch, elicitation, OAuth, MCP Apps, plugin manifest, code orchestration ([66288af](https://github.com/loonghao/dcc-mcp-core/commit/66288af926db945bf73974ae506a6637586cd3bf))


### Bug Fixes

* **gateway:** prune dead SSE backends, flush logs in real-time, isolate multi-instance log files ([#402](https://github.com/loonghao/dcc-mcp-core/issues/402)) ([e5dfdc5](https://github.com/loonghao/dcc-mcp-core/commit/e5dfdc52eaa722cdf2d2cb426b0bbbed5e9851af))
* **http:** make PyMcpHttpServer and PyServerHandle fields pub(crate) for cross-module access ([332e57f](https://github.com/loonghao/dcc-mcp-core/commit/332e57fb80016d69b79080101842f9d3e8b8e3c8))
* **refactor:** restore missing type definitions and module exports after modularization ([fb12c08](https://github.com/loonghao/dcc-mcp-core/commit/fb12c08757819ad81cd429109e4f495ae1add88a))
* **tests:** expose internal symbols for test compilation after modularization ([92c316b](https://github.com/loonghao/dcc-mcp-core/commit/92c316b2142d4fdf620f0be76aa311660dee2ed9))
* **tests:** expose internal symbols under cfg(test) for test compilation after modularization ([07d5867](https://github.com/loonghao/dcc-mcp-core/commit/07d58675f14b853457edd570f898b6deb935fc90))
* **test:** update file_name_prefix assertion to allow PID suffix (issue [#402](https://github.com/loonghao/dcc-mcp-core/issues/402)) ([be804f9](https://github.com/loonghao/dcc-mcp-core/commit/be804f97d09ea9fb73d985a853d1b2a510e9a059))
* **workflow:** make executor methods pub(crate) for cross-module visibility ([ef45408](https://github.com/loonghao/dcc-mcp-core/commit/ef45408f5e5aaaf8b6751bc891b25a75dd9cba45))


### Code Refactoring

* modularize oversized files into single-responsibility modules ([f989bcf](https://github.com/loonghao/dcc-mcp-core/commit/f989bcf5854074003eff97fa9c8a4767fc5d382a))
* **protocols,http:** split protocol models and gateway test fixtures ([#416](https://github.com/loonghao/dcc-mcp-core/issues/416)) ([b1c29da](https://github.com/loonghao/dcc-mcp-core/commit/b1c29dadb69b53303fec2734f72f1688cbf0afb1))


### Documentation

* fix VitePress dead links and complete Chinese sidebar ([94d03ea](https://github.com/loonghao/dcc-mcp-core/commit/94d03eadb16f698096c5c3d524a21e2c5c2f7225))

## [0.14.8](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.7...v0.14.8) (2026-04-23)


### Documentation

* update outdated workflow docs and translate Chinese placeholders ([#400](https://github.com/loonghao/dcc-mcp-core/issues/400)) ([867824b](https://github.com/loonghao/dcc-mcp-core/commit/867824bf81c9f04a93e8a8048278e76b34633346))

## [0.14.7](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.6...v0.14.7) (2026-04-23)


### Bug Fixes

* **server:** probe job-persist-sqlite feature before setting job_storage_path ([373acf7](https://github.com/loonghao/dcc-mcp-core/commit/373acf71bdad79a7cdcf5d85649a78eb5c69e279))
* **tests:** resolve DccServerBase mock and skill tag assertions ([#397](https://github.com/loonghao/dcc-mcp-core/issues/397)) ([f8ba089](https://github.com/loonghao/dcc-mcp-core/commit/f8ba089d51c941c5a8ee0e92edec3355087d5d53))

## [0.14.6](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.5...v0.14.6) (2026-04-22)


### Features

* **skills:** add in-process DCC execution + observability defaults + error_report skill ([1d4f6b1](https://github.com/loonghao/dcc-mcp-core/commit/1d4f6b1d783fc11058bf146f8f406403cf97abcb))
* **skills:** introduce layered skill architecture with explicit routing ([cd656cd](https://github.com/loonghao/dcc-mcp-core/commit/cd656cd916ae9c3525c624754ba684e04032d3b7))


### Bug Fixes

* **deps:** update rust dependencies ([9aaa77e](https://github.com/loonghao/dcc-mcp-core/commit/9aaa77e0d0ebcefea3ca01413e243545c25e019d))
* **gateway:** support multi-document DCCs in live metadata updates ([6f6efd4](https://github.com/loonghao/dcc-mcp-core/commit/6f6efd469ab5527e6617b1ccea2a5631ef8d3441))
* **gateway:** sync live scene/version to FileRegistry on every heartbeat ([3a83988](https://github.com/loonghao/dcc-mcp-core/commit/3a83988ca4764e0ea95f0856780c08b08c3eaa1e))

## [0.14.5](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.4...v0.14.5) (2026-04-22)


### Features

* **skills:** add SkillValidator for structured SKILL.md linting ([b658515](https://github.com/loonghao/dcc-mcp-core/commit/b658515723cfd3fcc8884ca24e62b13d9dadec56))

## [0.14.4](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.3...v0.14.4) (2026-04-22)


### Bug Fixes

* **ci:** resolve merge conflicts, add missing pyo3 imports, fix markdown lint ([f27acf0](https://github.com/loonghao/dcc-mcp-core/commit/f27acf01401548d234843aa634bd90e254b9ece1))
* **docs:** escape bare {{ }} in scheduler.md (VitePress Vue parse error) ([815c9b8](https://github.com/loonghao/dcc-mcp-core/commit/815c9b8a7d766deac417264f743e8ec8aad1da26))
* **docs:** escape bare {{ }} in workflows.md to prevent VitePress Vue parse error ([96dc2a7](https://github.com/loonghao/dcc-mcp-core/commit/96dc2a71c657bbfa5638fbfcc7ddd214f702d571))
* **docs:** properly escape {{ }} in VitePress markdown ([89f7256](https://github.com/loonghao/dcc-mcp-core/commit/89f725628044507e4dfb1f3f6e52d5631162af06))
* **lint:** allow &lt;code&gt; inline HTML in markdown for VitePress v-pre escape ([94f88d3](https://github.com/loonghao/dcc-mcp-core/commit/94f88d3bd57d443279765ba20228a0a6bf10d2b9))
* **python:** add fallback for __version__ when _core module lacks it ([6c479a1](https://github.com/loonghao/dcc-mcp-core/commit/6c479a1f868e26ed334912658efaac4fc13c8ab6))


### Code Refactoring

* split oversized files into modular components ([485cab4](https://github.com/loonghao/dcc-mcp-core/commit/485cab4664db1c0280425004895eb85d992c1dfe))

## [0.14.3](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.2...v0.14.3) (2026-04-22)


### Features

* **actions,skills:** add execution and timeout_hint_secs to Action and SKILL.md ([#317](https://github.com/loonghao/dcc-mcp-core/issues/317)) ([#337](https://github.com/loonghao/dcc-mcp-core/issues/337)) ([a29e914](https://github.com/loonghao/dcc-mcp-core/commit/a29e914613143b773f5d3fa7c7cd6027b038dbd7))
* **actions:** optional SQLite JobStorage backend ([#328](https://github.com/loonghao/dcc-mcp-core/issues/328)) ([#377](https://github.com/loonghao/dcc-mcp-core/issues/377)) ([55d85f6](https://github.com/loonghao/dcc-mcp-core/commit/55d85f68fa82293f9dc977bb98f7ed398de3c15c))
* **core:** add Workflow primitive skeleton (WorkflowSpec + WorkflowJob) ([#358](https://github.com/loonghao/dcc-mcp-core/issues/358)) ([08fdba3](https://github.com/loonghao/dcc-mcp-core/commit/08fdba3ca7757e612c3df738ef3dd86e6a6a8481))
* **core:** artefact handoff via FileRef resources ([#349](https://github.com/loonghao/dcc-mcp-core/issues/349)) ([#374](https://github.com/loonghao/dcc-mcp-core/issues/374)) ([fde6096](https://github.com/loonghao/dcc-mcp-core/commit/fde6096d28ce54efe452213c758eb8fe74920d49))
* **core:** scheduler for cron + webhook-triggered workflows ([#352](https://github.com/loonghao/dcc-mcp-core/issues/352)) ([#383](https://github.com/loonghao/dcc-mcp-core/issues/383)) ([54cc915](https://github.com/loonghao/dcc-mcp-core/commit/54cc91506455892985f9392c67c82cf12dd367f9))
* **gateway:** async dispatch timeout + opt-in wait-for-terminal response ([#321](https://github.com/loonghao/dcc-mcp-core/issues/321)) ([#381](https://github.com/loonghao/dcc-mcp-core/issues/381)) ([a12114c](https://github.com/loonghao/dcc-mcp-core/commit/a12114c10a77ee42ccede01e6bfb538bb842ad05))
* **gateway:** batch JSON-RPC, session correlation, and cancellation forwarding ([#313](https://github.com/loonghao/dcc-mcp-core/issues/313)) ([fcb1f45](https://github.com/loonghao/dcc-mcp-core/commit/fcb1f45a4789aab883a993d359a39fa9a58013a6))
* **gateway:** JobRoute cache with backend correlation, TTL, and cap ([#322](https://github.com/loonghao/dcc-mcp-core/issues/322)) ([#384](https://github.com/loonghao/dcc-mcp-core/issues/384)) ([39e9f40](https://github.com/loonghao/dcc-mcp-core/commit/39e9f4048a3aa70ba9c247fbeca209ff21b07347))
* **gateway:** multiplex backend SSE notifications to client sessions ([#320](https://github.com/loonghao/dcc-mcp-core/issues/320)) ([#375](https://github.com/loonghao/dcc-mcp-core/issues/375)) ([5cdacf8](https://github.com/loonghao/dcc-mcp-core/commit/5cdacf822b187e40fbf4fd94b8a30bc787240e6a))
* **http:** add JobManager for async job tracking ([#316](https://github.com/loonghao/dcc-mcp-core/issues/316)) ([86dc5dc](https://github.com/loonghao/dcc-mcp-core/commit/86dc5dcfe0703dc7954c448173d88aefec1c41c7))
* **http:** add jobs.get_status built-in tool for job polling ([#319](https://github.com/loonghao/dcc-mcp-core/issues/319)) ([#371](https://github.com/loonghao/dcc-mcp-core/issues/371)) ([f60b777](https://github.com/loonghao/dcc-mcp-core/commit/f60b7772a05ae9a3ff4d096368329ecdf4099062))
* **http:** add MCP prompts primitive with sibling-file + workflow-derived sources ([#351](https://github.com/loonghao/dcc-mcp-core/issues/351), [#355](https://github.com/loonghao/dcc-mcp-core/issues/355)) ([#373](https://github.com/loonghao/dcc-mcp-core/issues/373)) ([8a9fc6a](https://github.com/loonghao/dcc-mcp-core/commit/8a9fc6a90be234a8bf0024142d779fcbc58d4934))
* **http:** async job dispatch path in handle_tools_call ([#318](https://github.com/loonghao/dcc-mcp-core/issues/318)) ([#362](https://github.com/loonghao/dcc-mcp-core/issues/362)) ([e9bc876](https://github.com/loonghao/dcc-mcp-core/commit/e9bc8765992632c2f2e020879c51a2c135c141c5))
* **http:** job lifecycle notifications on progress + $/dcc.jobUpdated channels ([#326](https://github.com/loonghao/dcc-mcp-core/issues/326)) ([#366](https://github.com/loonghao/dcc-mcp-core/issues/366)) ([e100e3e](https://github.com/loonghao/dcc-mcp-core/commit/e100e3eea89223d9b4f691b9a5539a3b21f0bf86))
* **http:** make gateway backend timeout configurable ([#314](https://github.com/loonghao/dcc-mcp-core/issues/314)) ([#334](https://github.com/loonghao/dcc-mcp-core/issues/334)) ([1fb2880](https://github.com/loonghao/dcc-mcp-core/commit/1fb2880a931090706e256edd0129beded8156168))
* **http:** Resources primitive for live DCC state ([#350](https://github.com/loonghao/dcc-mcp-core/issues/350)) ([#360](https://github.com/loonghao/dcc-mcp-core/issues/360)) ([415a3b0](https://github.com/loonghao/dcc-mcp-core/commit/415a3b0264249c6bfe4a673d21961c83d356f198))
* **http:** rewrite built-in tool descriptions with 3-layer behavioral structure ([#341](https://github.com/loonghao/dcc-mcp-core/issues/341)) ([#368](https://github.com/loonghao/dcc-mcp-core/issues/368)) ([6e0200f](https://github.com/loonghao/dcc-mcp-core/commit/6e0200f1ab837c8ad270f9356235bae53f855889))
* **skill:** add check_cancelled() cooperative cancellation API ([#329](https://github.com/loonghao/dcc-mcp-core/issues/329)) ([#338](https://github.com/loonghao/dcc-mcp-core/issues/338)) ([ca8e79b](https://github.com/loonghao/dcc-mcp-core/commit/ca8e79bb0f8f4ce3edff7687e2840cd958cc2ba7))
* **skills:** accept agentskills.io-compliant metadata.dcc-mcp.* keys ([#357](https://github.com/loonghao/dcc-mcp-core/issues/357)) ([2233a7b](https://github.com/loonghao/dcc-mcp-core/commit/2233a7b65001de540297dfccff7fc95fd92c1b9b)), closes [#356](https://github.com/loonghao/dcc-mcp-core/issues/356)
* **skills:** BM25-lite scoring with field weights + sibling-file expansion ([#343](https://github.com/loonghao/dcc-mcp-core/issues/343)) ([#369](https://github.com/loonghao/dcc-mcp-core/issues/369)) ([67d6a45](https://github.com/loonghao/dcc-mcp-core/commit/67d6a459c0d19c1734d79d57ac02bce52405d229))
* **skills:** capability declaration + typed workspace path handshake ([#354](https://github.com/loonghao/dcc-mcp-core/issues/354)) ([#376](https://github.com/loonghao/dcc-mcp-core/issues/376)) ([ace5328](https://github.com/loonghao/dcc-mcp-core/commit/ace5328c532e626780588d5db4b6c74003259694))
* **skills:** surface ToolAnnotations from tools.yaml to MCP tools/list ([#344](https://github.com/loonghao/dcc-mcp-core/issues/344)) ([#363](https://github.com/loonghao/dcc-mcp-core/issues/363)) ([2b870a6](https://github.com/loonghao/dcc-mcp-core/commit/2b870a6e7fe1148e097b7da0c1384b7427230f2c))
* **skills:** unify find_skills and search_skills into one discovery tool ([#340](https://github.com/loonghao/dcc-mcp-core/issues/340)) ([#370](https://github.com/loonghao/dcc-mcp-core/issues/370)) ([f73c52b](https://github.com/loonghao/dcc-mcp-core/commit/f73c52bbdf2417ee257410373760aa93b6375ae7))
* **skills:** wire next-tools from tools.yaml to _meta on CallToolResult ([#342](https://github.com/loonghao/dcc-mcp-core/issues/342)) ([#365](https://github.com/loonghao/dcc-mcp-core/issues/365)) ([1006529](https://github.com/loonghao/dcc-mcp-core/commit/1006529b4f3644bce7b1eed5ee00311bdf939dcd))
* **telemetry,http:** Prometheus /metrics exporter ([#331](https://github.com/loonghao/dcc-mcp-core/issues/331)) ([#364](https://github.com/loonghao/dcc-mcp-core/issues/364)) ([595adec](https://github.com/loonghao/dcc-mcp-core/commit/595adec273f580bada2fdda2dd8cca9fc5b1e6f6))
* **telemetry:** Prometheus /metrics exporter for dcc-mcp-core ([#331](https://github.com/loonghao/dcc-mcp-core/issues/331)) ([#367](https://github.com/loonghao/dcc-mcp-core/issues/367)) ([171b529](https://github.com/loonghao/dcc-mcp-core/commit/171b529065299d7c0a0459f21d109bcd68caddfa))
* **workflow:** full WorkflowExecutor — Tool/Remote/Foreach/Parallel/Approve/Branch ([#348](https://github.com/loonghao/dcc-mcp-core/issues/348)) ([#382](https://github.com/loonghao/dcc-mcp-core/issues/382)) ([8142ade](https://github.com/loonghao/dcc-mcp-core/commit/8142adeca991d7eaf29ce1dcfa1152fbd84e53e5))
* **workflow:** step-level retry, timeout, and idempotency policies ([#353](https://github.com/loonghao/dcc-mcp-core/issues/353)) ([#372](https://github.com/loonghao/dcc-mcp-core/issues/372)) ([a4fd10b](https://github.com/loonghao/dcc-mcp-core/commit/a4fd10becbe593b2c1619a6f8ca7d4afce6b4342))


### Bug Fixes

* **deps:** update rust crate rusqlite to 0.39 ([e90e1de](https://github.com/loonghao/dcc-mcp-core/commit/e90e1deb8da28ccd7fb205a8fc047e524b80cc17))
* **http,process:** honour main-thread affinity in async dispatch ([#332](https://github.com/loonghao/dcc-mcp-core/issues/332)) ([#378](https://github.com/loonghao/dcc-mcp-core/issues/378)) ([b372a57](https://github.com/loonghao/dcc-mcp-core/commit/b372a57d7d508700bdcc485acfc9235835314baf))
* **skills:** accept nested metadata.dcc-mcp.* form in SKILL.md loader ([53f4f4f](https://github.com/loonghao/dcc-mcp-core/commit/53f4f4fd9b1d4cfa0aa0e9c42d6d79d51e72fdca))
* **workflow:** use derive(Default) for BackoffKind to satisfy clippy ([#380](https://github.com/loonghao/dcc-mcp-core/issues/380)) ([5bf37dc](https://github.com/loonghao/dcc-mcp-core/commit/5bf37dc961641d57cba81e04acfd4ceeea7ee8c4))


### Documentation

* docs/api/http.md, docs/api/skills.md, AGENTS.md, CLAUDE.md. ([f73c52b](https://github.com/loonghao/dcc-mcp-core/commit/f73c52bbdf2417ee257410373760aa93b6375ae7))
* docs/guide/capabilities.md ([ace5328](https://github.com/loonghao/dcc-mcp-core/commit/ace5328c532e626780588d5db4b6c74003259694))
* document DCC main-thread affinity and long-running job patterns ([#315](https://github.com/loonghao/dcc-mcp-core/issues/315)) ([e8cc3cb](https://github.com/loonghao/dcc-mcp-core/commit/e8cc3cbffa148df5cb26b2ed14511776945b744e))
* document file logging, bare tool names, and missing HTTP config properties ([25720ef](https://github.com/loonghao/dcc-mcp-core/commit/25720ef60ea3da4e4393f5c2df6755b6b44dcaa6))
* new `docs/guide/gateway.md` + AGENTS.md pointer. ([5cdacf8](https://github.com/loonghao/dcc-mcp-core/commit/5cdacf822b187e40fbf4fd94b8a30bc787240e6a))
* production deployment guide with Docker, systemd, k8s, HA ([#330](https://github.com/loonghao/dcc-mcp-core/issues/330), [#327](https://github.com/loonghao/dcc-mcp-core/issues/327)) ([#339](https://github.com/loonghao/dcc-mcp-core/issues/339)) ([cf64cd2](https://github.com/loonghao/dcc-mcp-core/commit/cf64cd21130a7977dfef7fd4b4e9a46997179052))
* **skills:** promote sibling-file pattern from [#356](https://github.com/loonghao/dcc-mcp-core/issues/356) to a repo-wide design rule ([#361](https://github.com/loonghao/dcc-mcp-core/issues/361)) ([208258f](https://github.com/loonghao/dcc-mcp-core/commit/208258fd3bea90b5147e070ab6b30bddfb1c99b6))

## [0.14.2](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.1...v0.14.2) (2026-04-21)


### Documentation

* update all documentation for v0.14 DccLink transport API ([#311](https://github.com/loonghao/dcc-mcp-core/issues/311)) ([50a8050](https://github.com/loonghao/dcc-mcp-core/commit/50a80500c749753c17423369f6f72f35040ea098))

## [0.14.1](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.0...v0.14.1) (2026-04-20)


### Features

* **http/gateway:** bare tool names per instance ([#307](https://github.com/loonghao/dcc-mcp-core/issues/307)) ([#309](https://github.com/loonghao/dcc-mcp-core/issues/309)) ([dd091bf](https://github.com/loonghao/dcc-mcp-core/commit/dd091bf1493822e415f5c761799534b791d93f31))
* **logging:** rolling file logger for Rust + Python ([#308](https://github.com/loonghao/dcc-mcp-core/issues/308)) ([6efe208](https://github.com/loonghao/dcc-mcp-core/commit/6efe2082c41b3c35f4199f03f7e1d3f9b20c8923))

## [0.14.0](https://github.com/loonghao/dcc-mcp-core/compare/v0.13.6...v0.14.0) (2026-04-20)


### ⚠ BREAKING CHANGES

* **http,transport:** removes TransportManager, FramedChannel, FramedIo, IpcListener (Python class), ListenerHandle, RoutingStrategy, ConnectionPool, InstanceRouter, CircuitBreaker, MessageEnvelope, Request/Response/Notification/Ping/Pong/ShutdownMessage, connect_ipc, and the encode_request / encode_response / encode_notify / decode_envelope functions. Use IpcChannelAdapter, GracefulIpcChannelAdapter, SocketServerAdapter, DccLinkFrame, and FileRegistry/ServiceEntry instead.

### Bug Fixes

* **http,transport:** gateway lifecycle + remove legacy transport stack ([4549237](https://github.com/loonghao/dcc-mcp-core/commit/454923787a4325e92c92431e0e2c813f4fa035ed)), closes [#303](https://github.com/loonghao/dcc-mcp-core/issues/303) [#251](https://github.com/loonghao/dcc-mcp-core/issues/251)
* **tests:** use tmp_path for sandbox allow_paths tests ([69ca84b](https://github.com/loonghao/dcc-mcp-core/commit/69ca84ba078e9c85fc312babc167d25f1d0e5afd))

## [0.13.6](https://github.com/loonghao/dcc-mcp-core/compare/v0.13.5...v0.13.6) (2026-04-20)


### Features

* **process,transport:** MainThreadPump dispatcher + EventStream MCP bridge ([#283](https://github.com/loonghao/dcc-mcp-core/issues/283)) ([23eb553](https://github.com/loonghao/dcc-mcp-core/commit/23eb553f8caf61008837eba533e3c00e5ee1872a))
* **transport:** add criterion benchmarks for IPC round-trip performance ([d186faf](https://github.com/loonghao/dcc-mcp-core/commit/d186faf5a3093f189d9957b3303f50c5ea224f25))
* **transport:** add fault-injection tests for IPC and TCP transport ([#296](https://github.com/loonghao/dcc-mcp-core/issues/296)) ([c4adb2e](https://github.com/loonghao/dcc-mcp-core/commit/c4adb2e62bbef338a6c57577458e94d70753ec61)), closes [#289](https://github.com/loonghao/dcc-mcp-core/issues/289)
* **transport:** add Python bindings for DccLink adapters ([de2eed7](https://github.com/loonghao/dcc-mcp-core/commit/de2eed7e0086f267e2d11d672265098b664b0731)), closes [#287](https://github.com/loonghao/dcc-mcp-core/issues/287)
* **transport:** add reentrancy-safe dispatch to GracefulIpcChannelAdapter ([f651b1a](https://github.com/loonghao/dcc-mcp-core/commit/f651b1a79ccc67b0ff6c13fbb4a81c493de473bd)), closes [#285](https://github.com/loonghao/dcc-mcp-core/issues/285)


### Bug Fixes

* **shm:** shorten segment names for macOS POSIX shm compatibility ([#298](https://github.com/loonghao/dcc-mcp-core/issues/298)) ([791f8f8](https://github.com/loonghao/dcc-mcp-core/commit/791f8f8385e0f5b920c9e92e197e15295adf1613))
* **shm:** use short_id() for all segment names to fix macOS CI ([d20283a](https://github.com/loonghao/dcc-mcp-core/commit/d20283a038ae692e2fc15153cab97f91ab7d0f5c)), closes [#294](https://github.com/loonghao/dcc-mcp-core/issues/294) [#295](https://github.com/loonghao/dcc-mcp-core/issues/295)
* **tests:** add missing uuid import to transport manager tests ([#300](https://github.com/loonghao/dcc-mcp-core/issues/300)) ([b2bcbe3](https://github.com/loonghao/dcc-mcp-core/commit/b2bcbe3d518543c17bdbca2c6d96cde97bbb1442))
* **tests:** update Python tests for short ID format ([f06467c](https://github.com/loonghao/dcc-mcp-core/commit/f06467c33e294865de375b8d9f68ff79afb009c9))


### Code Refactoring

* **shm:** migrate from memmap2 to ipckit SharedMemory with TTL support ([#297](https://github.com/loonghao/dcc-mcp-core/issues/297)) ([5e0d626](https://github.com/loonghao/dcc-mcp-core/commit/5e0d62667012470aa8cba7cdbce2065dbc3f8f79))


### Documentation

* enhance AI-agent documentation with progressive disclosure and tool priority guide ([#301](https://github.com/loonghao/dcc-mcp-core/issues/301)) ([7516f04](https://github.com/loonghao/dcc-mcp-core/commit/7516f045bbe1524dfe343864b54e99800f2f78cd))
* fix outdated references and add missing guide pages ([#302](https://github.com/loonghao/dcc-mcp-core/issues/302)) ([c642195](https://github.com/loonghao/dcc-mcp-core/commit/c6421957c0b174c9889415a99763fd3acb5219ba))

## [0.13.5](https://github.com/loonghao/dcc-mcp-core/compare/v0.13.4...v0.13.5) (2026-04-19)


### Features

* **transport:** add DCC-Link ipckit channel and server adapters ([8a29851](https://github.com/loonghao/dcc-mcp-core/commit/8a298514e6855a16049da66e68e28b1cb1d8e880))
* **transport:** route local ipc through ipckit async sockets ([7d7daae](https://github.com/loonghao/dcc-mcp-core/commit/7d7daae7eae9cf9c0bdb3fe6c7089b3b1e62b387))


### Bug Fixes

* **http,transport:** suppress unused variable warning and clean stale unix sockets ([aeb506e](https://github.com/loonghao/dcc-mcp-core/commit/aeb506eda728f69fce25b48a477327e323ae4856))


### Documentation

* refresh stale terminology for v0.13.4 codebase ([7325bac](https://github.com/loonghao/dcc-mcp-core/commit/7325bacb2d5ade458f8ed2e9f6e063fa6a9c7969))

## [0.13.4](https://github.com/loonghao/dcc-mcp-core/compare/v0.13.3...v0.13.4) (2026-04-19)


### Features

* **http:** add 2025-06-18 elicitation create flow ([fd0bd6e](https://github.com/loonghao/dcc-mcp-core/commit/fd0bd6e1cd807c6ab8dadd86a75fcb96132211a6))
* **http:** add roots cache and list_roots meta tool ([#272](https://github.com/loonghao/dcc-mcp-core/issues/272)) ([e952a1d](https://github.com/loonghao/dcc-mcp-core/commit/e952a1db72711ad0a47d18b67834c5e62969bc0e))
* **http:** implement logging/setLevel and notifications/message streaming ([#271](https://github.com/loonghao/dcc-mcp-core/issues/271)) ([a05f47d](https://github.com/loonghao/dcc-mcp-core/commit/a05f47d0505625f12d6f04e7fbf7aa4adda7bf64))
* **process:** add thread-affinity dispatcher primitives ([#273](https://github.com/loonghao/dcc-mcp-core/issues/273)) ([be6e47a](https://github.com/loonghao/dcc-mcp-core/commit/be6e47ab21804fd1f5d276e99ee237f68ef12b3c))


### Bug Fixes

* **process:** align PyStandaloneDispatcher with actual dispatcher API ([11cfd5b](https://github.com/loonghao/dcc-mcp-core/commit/11cfd5bb40b5381d7a3b733ca493b2519f8f740c))


### Documentation

* enhance AI agent documentation (run [#15](https://github.com/loonghao/dcc-mcp-core/issues/15)) ([#268](https://github.com/loonghao/dcc-mcp-core/issues/268)) ([867de50](https://github.com/loonghao/dcc-mcp-core/commit/867de501df1bccaf16c2d14164fa2406b1394e16))

## [0.13.3](https://github.com/loonghao/dcc-mcp-core/compare/v0.13.2...v0.13.3) (2026-04-18)


### Features

* **adapters:** add JavaScript and TypeScript to ScriptLanguage enum ([acc72a2](https://github.com/loonghao/dcc-mcp-core/commit/acc72a2d25d17e148e80dc222c24776a1d09b013))
* **http/gateway:** proactive skill.name tool namespacing ([#238](https://github.com/loonghao/dcc-mcp-core/issues/238)) ([9caba47](https://github.com/loonghao/dcc-mcp-core/commit/9caba47a9aee8696be28a7509bfe144fa23859f2))
* **http:** drop annotations on __skill__/__group__ stubs in tools/list ([fe629f9](https://github.com/loonghao/dcc-mcp-core/commit/fe629f91461bad7dbdc8ff6f3115dec931eab1f1)), closes [#235](https://github.com/loonghao/dcc-mcp-core/issues/235)
* **http:** negotiate MCP protocol version (2025-06-18 + 2025-03-26) ([94c9638](https://github.com/loonghao/dcc-mcp-core/commit/94c96382581c8e04cc6fcd40f77b9c0cab9414ac)), closes [#239](https://github.com/loonghao/dcc-mcp-core/issues/239)
* **http:** opt-in lazy-actions fast-path ([#254](https://github.com/loonghao/dcc-mcp-core/issues/254)) ([b1e7754](https://github.com/loonghao/dcc-mcp-core/commit/b1e77544b0cb239451f694342931282fc107d1c4))
* **http:** progress notifications and cooperative cancellation ([#240](https://github.com/loonghao/dcc-mcp-core/issues/240), [#241](https://github.com/loonghao/dcc-mcp-core/issues/241)) ([f260754](https://github.com/loonghao/dcc-mcp-core/commit/f2607540cb47a8e1f2f365043c07a86ecff514b2))
* **http:** ResourceLink content for DCC artifacts ([#243](https://github.com/loonghao/dcc-mcp-core/issues/243)) ([5168336](https://github.com/loonghao/dcc-mcp-core/commit/5168336ef5c53fc03cf69c37273d7a92415740ab))
* **http:** structuredContent + outputSchema on MCP 2025-06-18 ([#242](https://github.com/loonghao/dcc-mcp-core/issues/242)) ([e17629a](https://github.com/loonghao/dcc-mcp-core/commit/e17629aa47e27c8688c2631c3c8f3032926d23ea))
* **http:** surface search-hint in skill stubs and apply error envelope ([e4af853](https://github.com/loonghao/dcc-mcp-core/commit/e4af853f4718ede027641c8d3bf16d153df35646))
* **http:** tools/list pagination + delta notification ([#234](https://github.com/loonghao/dcc-mcp-core/issues/234)) ([78879fb](https://github.com/loonghao/dcc-mcp-core/commit/78879fb1bfedfff244f0ef4d009d00b444ea7929))
* **naming:** add SEP-986 tool-name and action-id validators ([3a60242](https://github.com/loonghao/dcc-mcp-core/commit/3a60242e30d5ecce40e9c7cf877c242e5fd518ef))
* **protocols:** add structured error envelope for tools/call failures ([314a37a](https://github.com/loonghao/dcc-mcp-core/commit/314a37abad28f30884396db5b4f2ca5acaed0025)), closes [#237](https://github.com/loonghao/dcc-mcp-core/issues/237)


### Bug Fixes

* **http/gateway:** exclude __gateway__ sentinel from DCC instance listings ([ecf8712](https://github.com/loonghao/dcc-mcp-core/commit/ecf8712d22449a98c25b69c1bdc2a4f6fade64c7))
* **http/gateway:** replace `/` tool-name separator with `.` (SEP-986) ([43ef97b](https://github.com/loonghao/dcc-mcp-core/commit/43ef97b1efaf90d10e44eb60b6cf611755254812)), closes [#261](https://github.com/loonghao/dcc-mcp-core/issues/261)
* **http/gateway:** scope version self-yield to sentinel and heartbeat it ([b120e9c](https://github.com/loonghao/dcc-mcp-core/commit/b120e9cc36b03fdcc3a20692c5693b0ec810ee7a))
* **skills:** fail loud when DCC host Python is unset ([#231](https://github.com/loonghao/dcc-mcp-core/issues/231)) ([09285ea](https://github.com/loonghao/dcc-mcp-core/commit/09285ead1367725c043a6f9423e68df4ecf7334b))
* **transport:** reap ghost registry rows and preserve gateway sentinel ([f10bf30](https://github.com/loonghao/dcc-mcp-core/commit/f10bf306eb2f52e6d981016339a87388da6fee30))


### Documentation

* add next-tools, agentskills.io fields, security & commit guidelines ([#233](https://github.com/loonghao/dcc-mcp-core/issues/233)) ([4102c86](https://github.com/loonghao/dcc-mcp-core/commit/4102c86403895e73ab82d9c2311e96920a94b3db))
* cross-reference integration guide from CLAUDE.md and AGENTS.md ([bd8b1e2](https://github.com/loonghao/dcc-mcp-core/commit/bd8b1e2c20d3094ffaa2370b35f19fe51e0fc425))
* enhance AI agent guidance and fix documentation inconsistencies ([#225](https://github.com/loonghao/dcc-mcp-core/issues/225)) ([b274dd3](https://github.com/loonghao/dcc-mcp-core/commit/b274dd34edb4aa7c216410b860ef23668fc8c4ec))
* **skills:** add DCC integration architecture guide ([d49c6a1](https://github.com/loonghao/dcc-mcp-core/commit/d49c6a199ac9b3a0346a49e3b23043300520dc0f))

## [0.13.2](https://github.com/loonghao/dcc-mcp-core/compare/v0.13.1...v0.13.2) (2026-04-17)


### Features

* window-target capture, instance-bound diagnostics, and tool groups ([#215](https://github.com/loonghao/dcc-mcp-core/issues/215)) ([89079fb](https://github.com/loonghao/dcc-mcp-core/commit/89079fb49a259c484187053cfceba81e7338b812))


### Documentation

* enhance AI agent guidance for v0.13.x with SkillScope, MCP best practices, DccServerBase ([#216](https://github.com/loonghao/dcc-mcp-core/issues/216)) ([13d3f34](https://github.com/loonghao/dcc-mcp-core/commit/13d3f3415433acd5131be408d79d49e54bce3ce9))
* fix action→tool terminology, add DccGatewayElection API, sync ZH capture docs ([#218](https://github.com/loonghao/dcc-mcp-core/issues/218)) ([2bd655d](https://github.com/loonghao/dcc-mcp-core/commit/2bd655d3d8b4fa4502e021e4ae75c5bad70a49ea))

## [0.13.1](https://github.com/loonghao/dcc-mcp-core/compare/v0.13.0...v0.13.1) (2026-04-17)


### Bug Fixes

* align http bindings and refresh adapter docs ([7ab2a43](https://github.com/loonghao/dcc-mcp-core/commit/7ab2a43482e44b24e756b1c0950eec1458a0e7ca))
* **gateway:** aggregate tools from all backends into unified MCP facade ([0ed10b7](https://github.com/loonghao/dcc-mcp-core/commit/0ed10b751f859622997cc0ea3d51510bc36abf11))
* restore _core compatibility aliases ([32b56a1](https://github.com/loonghao/dcc-mcp-core/commit/32b56a1ce3dc75c954b903c9bfb25027481d4799))
* satisfy rust clippy on latest stable ([d444ddf](https://github.com/loonghao/dcc-mcp-core/commit/d444ddfa2cde63f91eebb02b27873d3a672e57ff))
* **test:** update test_entry_to_dict_keys for new ServiceEntry fields ([858fb6b](https://github.com/loonghao/dcc-mcp-core/commit/858fb6b3e7c0a62a0acf91890399505cfb397249))


### Code Refactoring

* auto-derive server_version from package, promote deferred imports to top-level ([3c782cc](https://github.com/loonghao/dcc-mcp-core/commit/3c782cccfd1a59e779b4bd36f05d41201bd4f6c2))
* clean up skill tool terminology ([76ab53e](https://github.com/loonghao/dcc-mcp-core/commit/76ab53e4da87021d0454d949cb3a9b5251de138f))
* code quality, stale API fixes, AGENTS.md as navigation map ([6d1a46d](https://github.com/loonghao/dcc-mcp-core/commit/6d1a46d0deb302708778351e0ba41f96275de797))
* remove legacy action aliases ([ac2f2c6](https://github.com/loonghao/dcc-mcp-core/commit/ac2f2c61fb80db5d056c1f867b864cdecbb7871f))
* rename action APIs to tool APIs ([a71c0d9](https://github.com/loonghao/dcc-mcp-core/commit/a71c0d9d28aa861b2bbca4352e4c3cf425958a2c))

## [0.13.0](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.29...v0.13.0) (2026-04-15)


### ⚠ BREAKING CHANGES

* Complete rewrite from Python+Pydantic to Rust+PyO3+maturin.

### Features

* add dcc-mcp-http crate — MCP Streamable HTTP server (2025-03-26 spec) ([#103](https://github.com/loonghao/dcc-mcp-core/issues/103)) ([6cd7887](https://github.com/loonghao/dcc-mcp-core/commit/6cd788785b535616256ae4b115e072ab4b9b74b6))
* add dcc-mcp-transport crate with async transport layer ([6c77e69](https://github.com/loonghao/dcc-mcp-core/commit/6c77e697f81d300367853f5f6f821e5b37d9aa85))
* Add foundational components and documentation for dcc-mcp-core ([86a1754](https://github.com/loonghao/dcc-mcp-core/commit/86a1754fc3685c7f1c735e58d7506b7a1611788c))
* Add function adapters for Action classes ([6b62c87](https://github.com/loonghao/dcc-mcp-core/commit/6b62c87dfed6174e79b0afe7caf9f927cf4ff19b))
* Add Pydantic extensions and update related modules ([4eb4f80](https://github.com/loonghao/dcc-mcp-core/commit/4eb4f80b646ddd9a0050590be64bfcd37d427591))
* add Python 3.7 support with separate non-abi3 wheel builds ([82208a1](https://github.com/loonghao/dcc-mcp-core/commit/82208a149cb579fab8ec835d7ee32e54c3c8c508))
* add Skills system for zero-code script registration as MCP tools ([cab3c28](https://github.com/loonghao/dcc-mcp-core/commit/cab3c28d111e6fa1d56bde827febc0ebd64769a2))
* add transport Python types, fix docs, drop py3.8 CI ([#69](https://github.com/loonghao/dcc-mcp-core/issues/69)) ([89c70a7](https://github.com/loonghao/dcc-mcp-core/commit/89c70a7c95981d61ea0c4017b6f1672a16efe05d))
* Add various test plugins and utilities for plugin management system ([b3ebe68](https://github.com/loonghao/dcc-mcp-core/commit/b3ebe6876b9cba33673aab51c0eaba6c7514d184))
* **bridge:** WebSocket JSON-RPC 2.0 protocol, DccBridge Python API, and standalone server ([#145](https://github.com/loonghao/dcc-mcp-core/issues/145) [#146](https://github.com/loonghao/dcc-mcp-core/issues/146) [#147](https://github.com/loonghao/dcc-mcp-core/issues/147)) ([b604c94](https://github.com/loonghao/dcc-mcp-core/commit/b604c945274c88cf78ebd8d560d7eca79bf8484c))
* Complete issues [#180](https://github.com/loonghao/dcc-mcp-core/issues/180) and [#179](https://github.com/loonghao/dcc-mcp-core/issues/179) - Gateway improvements ([#183](https://github.com/loonghao/dcc-mcp-core/issues/183)) ([eb739a1](https://github.com/loonghao/dcc-mcp-core/commit/eb739a117135f401c0adda3fc2d78ccc0173485f))
* **core:** add ActionChain for native multi-step operation orchestration ([#142](https://github.com/loonghao/dcc-mcp-core/issues/142)) ([bd01937](https://github.com/loonghao/dcc-mcp-core/commit/bd01937e0202d9446bff89e104bc7d67c18921fc))
* DCC_MCP_{APP}_SKILL_PATHS env var + create_skill_manager factory ([#119](https://github.com/loonghao/dcc-mcp-core/issues/119)) ([8a15a1e](https://github.com/loonghao/dcc-mcp-core/commit/8a15a1effcc4c4e1a5377c26ea5814e2c0189317))
* **dcc-mcp-maya:** register diagnostic IPC actions for dcc-diagnostics skill ([#141](https://github.com/loonghao/dcc-mcp-core/issues/141)) ([8f0d909](https://github.com/loonghao/dcc-mcp-core/commit/8f0d909f85bd72e1cf24ca49ee3af5be0b69dbc2))
* Enhance action management and DCC support ([f019c20](https://github.com/loonghao/dcc-mcp-core/commit/f019c20ebb4bb98ef4f0cd9351e6a11e5df9c99a))
* Enhance action registration and classification ([de765a8](https://github.com/loonghao/dcc-mcp-core/commit/de765a8f79c8662fb675bfa522a0feebaf01ab24))
* Enhance ActionRegistry with DCC-specific features ([5375337](https://github.com/loonghao/dcc-mcp-core/commit/53753376e4b028d76603a9b018a8258261d33f22))
* **examples:** add dcc-diagnostics and workflow skill examples ([67b5b89](https://github.com/loonghao/dcc-mcp-core/commit/67b5b894150a056c59b5c4331cbd2c0c2d07f0eb))
* expose BridgeRegistry Python API (BridgeContext, BridgeRegistry, register_bridge) ([a8c7ec1](https://github.com/loonghao/dcc-mcp-core/commit/a8c7ec11efdede98899b86ecc9da58ba04711c6b))
* **gateway:** add MCP Resources API and SSE push for dynamic instance discovery ([71b9928](https://github.com/loonghao/dcc-mcp-core/commit/71b99281fef82e1bd0d01df71110bffd1b348ffe))
* implement skills system with metadata dir, depends, examples and e2e tests ([5ee9970](https://github.com/loonghao/dcc-mcp-core/commit/5ee997033bd90740e13ee588a96b6303f47634a9))
* Improve imports and module interface in dcc_mcp_core ([d62792c](https://github.com/loonghao/dcc-mcp-core/commit/d62792ccdfbfea06e6c3ed49dd4254a8e2c7dfdb))
* Improve imports and module interface in dcc_mcp_core ([b2605a9](https://github.com/loonghao/dcc-mcp-core/commit/b2605a929506d1c7b9b70adf61b8151139a8a61d))
* **models:** add Rust-backed serialization for ActionResultModel ([63385f1](https://github.com/loonghao/dcc-mcp-core/commit/63385f1d22cb2e13dc8c7edc1159ebb250fbcd83))
* **models:** align SkillMetadata with Anthropic Skills + ClawHub standards ([#114](https://github.com/loonghao/dcc-mcp-core/issues/114)) ([02805d8](https://github.com/loonghao/dcc-mcp-core/commit/02805d8add9b4d626cda4c1310d36f09c0a08357))
* **packaging:** bundle general-purpose skills inside the wheel ([4f2e8f5](https://github.com/loonghao/dcc-mcp-core/commit/4f2e8f5e64ede8c32853497c7ba8514579709441))
* **protocols:** add 4 cross-DCC protocol traits + complete Python bindings ([d683efc](https://github.com/loonghao/dcc-mcp-core/commit/d683efc20828dee9007d6f17b697c662af541a09))
* **protocols:** add BridgeKind + bridge fields to DccCapabilities for non-Python DCCs ([b51ca78](https://github.com/loonghao/dcc-mcp-core/commit/b51ca784bffaa9d0db1f341b0d95b211f49a49a6))
* **python:** add DCC adapter base abstractions (DccServerBase, DccSkillHotReloader, DccGatewayElection, factory) ([#187](https://github.com/loonghao/dcc-mcp-core/issues/187)) ([3da5cf5](https://github.com/loonghao/dcc-mcp-core/commit/3da5cf58ba22eb36ac09f612770d8e6bf7712f92))
* replace pre-commit with vx prek and add justfile ([fd56ac9](https://github.com/loonghao/dcc-mcp-core/commit/fd56ac998d5d117bb204f1302465bc72bd27b63f))
* Restructure imports, remove unused code, and update templates ([64f3f3e](https://github.com/loonghao/dcc-mcp-core/commit/64f3f3e6db7c0bd76cf13e324568f097608b5c46))
* rewrite core in Rust with workspace crates architecture ([3308ee1](https://github.com/loonghao/dcc-mcp-core/commit/3308ee1d7a465cca82d966786ab9ed936dc5ba33))
* RTK-inspired token optimization (-80% consumption) ([#181](https://github.com/loonghao/dcc-mcp-core/issues/181)) ([87f1f1c](https://github.com/loonghao/dcc-mcp-core/commit/87f1f1c4e01f6ecb5ef2f64562c0b770506c1fab))
* **server_base:** add filter_existing param to collect_skill_search_paths ([e6b488b](https://github.com/loonghao/dcc-mcp-core/commit/e6b488b914824b8a2003772997c6eee5bc4f7822)), closes [#197](https://github.com/loonghao/dcc-mcp-core/issues/197)
* **server:** integrated auto-gateway — first-wins port competition, zero extra processes ([#164](https://github.com/loonghao/dcc-mcp-core/issues/164)) ([058e3dd](https://github.com/loonghao/dcc-mcp-core/commit/058e3dd65d2bfda897c7b4fdadf591799e9ebe61))
* **server:** sidecar process management — PID file, WS heartbeat, reconnect timeout, session TTL ([b019191](https://github.com/loonghao/dcc-mcp-core/commit/b0191916a8d06bb06592849b4873766a3b18413c))
* **skill:** add pure-Python skill script helpers + squash auto-improve adapters refactor ([4d342fc](https://github.com/loonghao/dcc-mcp-core/commit/4d342fce23d4bb83b5db84b82869b95506c26205))
* **skills,http:** add explicit deferred tool hints ([38ed73d](https://github.com/loonghao/dcc-mcp-core/commit/38ed73d1119c71c6787afc1c1a70c0fd0a2d6572))
* **skills,http:** on-demand skill discovery with search_skills and lightweight stubs ([#136](https://github.com/loonghao/dcc-mcp-core/issues/136)) ([01c6165](https://github.com/loonghao/dcc-mcp-core/commit/01c6165a8cd1569d9125aa19f8205aa6f7969097))
* **skills:** add SkillCatalog with progressive skill loading and core discovery tools ([#111](https://github.com/loonghao/dcc-mcp-core/issues/111)) ([a708379](https://github.com/loonghao/dcc-mcp-core/commit/a7083794da0054845beb4b87fc23cb37e5b048aa))
* **skills:** on-demand skill discovery meta-tools and progressive loading ([#143](https://github.com/loonghao/dcc-mcp-core/issues/143) [#148](https://github.com/loonghao/dcc-mcp-core/issues/148) [#149](https://github.com/loonghao/dcc-mcp-core/issues/149) [#150](https://github.com/loonghao/dcc-mcp-core/issues/150)) ([dc3c9b4](https://github.com/loonghao/dcc-mcp-core/commit/dc3c9b443852a00182a1fde2746efe605fd160e1))
* **skills:** Skills-First architecture — tools/call executes skill scripts via ActionDispatcher ([#113](https://github.com/loonghao/dcc-mcp-core/issues/113)) ([ae0b12d](https://github.com/loonghao/dcc-mcp-core/commit/ae0b12de9e378994eadf4d62018bab0cce2f4ba8))
* squash auto-improve branch + bump version to 0.12.6 ([9d7e37f](https://github.com/loonghao/dcc-mcp-core/commit/9d7e37fd15808186c855eb3d21d1f39d7a60fd1c))
* squash auto-improve features and fix CI PyPI Trusted Publishing ([#75](https://github.com/loonghao/dcc-mcp-core/issues/75)) ([06b8eee](https://github.com/loonghao/dcc-mcp-core/commit/06b8eee23d3c722364cc942a9e8afd6bb69342d3))
* **transport,gateway:** multi-document support and agent disambiguation ([67bc624](https://github.com/loonghao/dcc-mcp-core/commit/67bc62472333dca67bd2f8b1dda18f4293586aee))
* **transport:** add bind_and_register + find_best_service for zero-config service discovery ([720b6eb](https://github.com/loonghao/dcc-mcp-core/commit/720b6eb880e974ffa8e5b5d2e42db35542eb0f9e))
* **transport:** round-robin multi-instance load balancing + rank_services API ([55e4450](https://github.com/loonghao/dcc-mcp-core/commit/55e4450a28cba5c9d2465928665ee30a4101de6a))
* version-aware gateway election, SkillPolicy/Deps/Scope, MCP cancellation ([b136571](https://github.com/loonghao/dcc-mcp-core/commit/b13657180368b9ba05bdf635002756b01340dc19))


### Bug Fixes

* add __iter__ and to_json() to ActionResultModel for JSON ergonomics ([147c731](https://github.com/loonghao/dcc-mcp-core/commit/147c731e03a912e075ae37e7e91972164276f91c))
* add cross-platform shell support to justfile ([8cc8de1](https://github.com/loonghao/dcc-mcp-core/commit/8cc8de1760aea8a8a28349a913dd334beec35772))
* add Python 3.7 compatibility for importlib.metadata ([db342ff](https://github.com/loonghao/dcc-mcp-core/commit/db342ffa3a14a5fc87d79df798b02357ac099cc6))
* add special handling for Python 3.7 in GitHub Actions workflow ([3cd04f6](https://github.com/loonghao/dcc-mcp-core/commit/3cd04f6ba20a762b9b353cd78eebd5a700a557cf))
* add update_documents to ServiceDiscovery trait, ServiceRegistry and TransportManager ([519d762](https://github.com/loonghao/dcc-mcp-core/commit/519d762d4effe5b7557a6bf1c2d6d965cb12a722))
* **ci:** add python/dcc_mcp_core/__init__.py for maturin python-source ([859dbb7](https://github.com/loonghao/dcc-mcp-core/commit/859dbb798088b39c8ed31faf85551529777e46cc))
* **ci:** add vx install dir to PATH after setup ([9cec3b7](https://github.com/loonghao/dcc-mcp-core/commit/9cec3b7f2f33bc8d610a3855f6afcbc9cbdf1b38))
* **ci:** fix Python 3.7 runner and update actions versions + add tests ([#90](https://github.com/loonghao/dcc-mcp-core/issues/90)) ([8b6157a](https://github.com/loonghao/dcc-mcp-core/commit/8b6157a97685e1fc8dda4ca604bf7527334c283c))
* **ci:** remove duplicate Cache Cargo step in dcc-integration.yml ([5e7d4f8](https://github.com/loonghao/dcc-mcp-core/commit/5e7d4f8a87592e0944425d2c1a731b06460f3d64))
* **ci:** remove duplicate tag-triggered publish in release.yml ([eeb78b4](https://github.com/loonghao/dcc-mcp-core/commit/eeb78b466935f170d69ccb2770b765add87f4428))
* **ci:** remove stale gateway entry from Cargo.toml; fix remaining noqa RUF100 + E711/E712/SIM118 ([a5b3ef9](https://github.com/loonghao/dcc-mcp-core/commit/a5b3ef98ec4cd5a3aae0b2376398675f6e8fea16))
* **ci:** remove stale noqa directives; expose session_ttl_secs in Python binding ([313623e](https://github.com/loonghao/dcc-mcp-core/commit/313623e36f252ca37477d334af3e503839ade1f6))
* **ci:** update dcc-integration.yml to use split test files ([cbedaef](https://github.com/loonghao/dcc-mcp-core/commit/cbedaef9e4114c3f58680e651d5a2158aa5ac475))
* **ci:** use 'just install' (build+pip) instead of 'maturin develop' ([45ea35d](https://github.com/loonghao/dcc-mcp-core/commit/45ea35d8a28ce0274bb03943d73e8a1ec08fa6e7))
* **ci:** use 'vx just' instead of installing just to PATH ([ded7a24](https://github.com/loonghao/dcc-mcp-core/commit/ded7a24b0c23b5f1462f9008d8c1a8eec3e425db))
* **ci:** use ubuntu-22.04 for Python 3.7 and replace setup-just with vx ([c8eda5b](https://github.com/loonghao/dcc-mcp-core/commit/c8eda5b6076d623709a27ed8378c2ed899786422))
* dead link in zh getting-started and release workflow skip issue ([8d709f6](https://github.com/loonghao/dcc-mcp-core/commit/8d709f673b4e3518bfb97f5064cc072cce8fbd84))
* **deps:** update dependency platformdirs to v4 ([59f65da](https://github.com/loonghao/dcc-mcp-core/commit/59f65da4818deb09ea4d6f85488a7963fe1418ec))
* **deps:** update rust dependencies ([a260381](https://github.com/loonghao/dcc-mcp-core/commit/a26038120bb49502f21dbae8d3089990200f3deb))
* drop the write guard before calling flush_to_file(). ([6addfff](https://github.com/loonghao/dcc-mcp-core/commit/6addffffaff49fb8e1bfd1c74f3da0d066cf022a))
* **http,skills:** resolve 5 real performance and correctness issues ([f825b3b](https://github.com/loonghao/dcc-mcp-core/commit/f825b3b88c7aa18a4d5980afeae12f0897b5ac0a))
* improve GitHub Actions workflows for Windows compatibility ([7220901](https://github.com/loonghao/dcc-mcp-core/commit/722090165c89829c24d98d20bcb37ec9ae015a86))
* **process:** fix PyProcessWatcher.start() tokio runtime context bug and add 20 tests for lifecycle API [iteration-done] ([96cc8df](https://github.com/loonghao/dcc-mcp-core/commit/96cc8df98afbe9775c1c6c486100ef0db977ed1d))
* **process:** replace eprintln with tracing::warn in launcher tests ([a1161a2](https://github.com/loonghao/dcc-mcp-core/commit/a1161a2b2f3da6830b182d6f3cd2929c5948269a))
* **protocols,tests:** restore DccCapabilities repr + fix IpcListener platform test ([1596689](https://github.com/loonghao/dcc-mcp-core/commit/159668996551e6c952abfc91c1591ea95d3c65c7))
* **protocols:** add ..Default::default() to DccCapabilities struct literals ([913435f](https://github.com/loonghao/dcc-mcp-core/commit/913435f4eec1da5c6c4aa8e2b091daf5acd081e0))
* remove component from release-please config to use v0.x.x tag format ([3bb0696](https://github.com/loonghao/dcc-mcp-core/commit/3bb06964eb3d73ac8e17605e7fa2fc1d6c9d063d))
* resolve all PyO3 0.23 python-bindings compilation errors ([7180c4e](https://github.com/loonghao/dcc-mcp-core/commit/7180c4e41eb0d367a4d71ef1a394fe9e6a07fd9f))
* resolve CI clippy errors and unify dev toolchain ([0300b0b](https://github.com/loonghao/dcc-mcp-core/commit/0300b0ba68d18b98f11393450bd3e692bddacf6c))
* resolve isort issues and migrate CI to vx ([31ed2a9](https://github.com/loonghao/dcc-mcp-core/commit/31ed2a9669f40b1b490cc8875f38c32e3c09ba52))
* resolve lint errors in test files (isort, ruff format, D106/F841) ([d703c4a](https://github.com/loonghao/dcc-mcp-core/commit/d703c4af92587b90c8165ed56d6e57ee714b8502))
* resolve release-please 'package.version is not tagged' error ([b433c71](https://github.com/loonghao/dcc-mcp-core/commit/b433c71db40c162d5f0694981012bdb9bb95410b))
* **restore:** restore test_adapters_python.py lost after squash — 67 tests for DCC adapter Python bindings ([7b40582](https://github.com/loonghao/dcc-mcp-core/commit/7b40582968e290297e4189a898cc59feac560f00))
* **server_base:** pass port via McpHttpConfig constructor (read-only attribute) ([75ac61c](https://github.com/loonghao/dcc-mcp-core/commit/75ac61c14e74ea052fe79adf436f57f7b8a8a402))
* **server_base:** use discover() API, set dcc_type, read DCC_MCP_REGISTRY_DIR ([#191](https://github.com/loonghao/dcc-mcp-core/issues/191)) ([bd5f0c2](https://github.com/loonghao/dcc-mcp-core/commit/bd5f0c2709335a8ec389209ee33cd805395730ca))
* **skills,models:** fix 3 reported bugs in parse_skill_md, SkillScanner, ActionResultModel ([63ebe7d](https://github.com/loonghao/dcc-mcp-core/commit/63ebe7daebca7877e8728568103d329a0e509037))
* **skills:** fix skill discovery, execution param passing, and script compatibility ([#159](https://github.com/loonghao/dcc-mcp-core/issues/159)) ([7f644da](https://github.com/loonghao/dcc-mcp-core/commit/7f644da01d6a9068e3e542da6dd271e778e1e808))
* **skills:** resolve relative script paths against skill root + configurable Python interpreter ([10224bf](https://github.com/loonghao/dcc-mcp-core/commit/10224bfeaff24c7d2a044de11db89d0852301254))
* **test:** apply ruff auto-fix ([638d1c0](https://github.com/loonghao/dcc-mcp-core/commit/638d1c007bd3df32b5b48731987edc091d511add))
* **tests:** correct 7 failing tests across 3 test files ([af9dae1](https://github.com/loonghao/dcc-mcp-core/commit/af9dae1f77f3c951eae8615e7af394480572b341))
* **tests:** fix platform-specific assertions causing Linux/macOS CI failures ([db2aea2](https://github.com/loonghao/dcc-mcp-core/commit/db2aea2aaaf531ced7b14a68afa0eaf136153a0a))
* **tests:** fix platform-specific assertions on Linux/macOS ([31c9f24](https://github.com/loonghao/dcc-mcp-core/commit/31c9f245327e42453a78fe6036e9b22d396c0ffe))
* **tests:** update axum-test usage for v20 API — TestServer::new() no longer returns Result ([#166](https://github.com/loonghao/dcc-mcp-core/issues/166)) ([892bb57](https://github.com/loonghao/dcc-mcp-core/commit/892bb574216c5b5d2be3f2276f95377b4ca5db4a))
* **tests:** use parent tmpdir in sandbox path test (cross-platform) ([7c8df02](https://github.com/loonghao/dcc-mcp-core/commit/7c8df0240f8d46d9b708e6afb65a41306fa8ec55))
* **tests:** use Path.resolve() for sandbox path test (macOS /tmp symlink) ([277bcdb](https://github.com/loonghao/dcc-mcp-core/commit/277bcdb1912cb387a9edac9cd8845a8faf59301d))
* **tests:** use real tmpdir for is_path_allowed cross-platform test ([3aa28a8](https://github.com/loonghao/dcc-mcp-core/commit/3aa28a84efae19f2af84f3916050ef3b68f734bf))
* **transport:** resolve DashMap deadlock in FileRegistry heartbeat and update_status ([6addfff](https://github.com/loonghao/dcc-mcp-core/commit/6addffffaff49fb8e1bfd1c74f3da0d066cf022a))
* update GitHub Actions workflows for better Python version compatibility ([0e4b2bc](https://github.com/loonghao/dcc-mcp-core/commit/0e4b2bca67e4c9b18690ead283e05d13fb0d8ee7))
* update GitHub Actions workflows to regenerate poetry.lock before install ([752206b](https://github.com/loonghao/dcc-mcp-core/commit/752206b98daf86d455cdcc33374110e81dc301b6))
* update Mermaid diagrams for better GitHub compatibility and visibility ([fc43474](https://github.com/loonghao/dcc-mcp-core/commit/fc43474a2b3c8fea4e44841788a70b0f741d5c77))
* use derive(Default) for ServiceStatus enum ([3b265f6](https://github.com/loonghao/dcc-mcp-core/commit/3b265f6a6c8e24d813b7ded1962b2403bf33bce1))


### Code Refactoring

* consolidate cleanup, docs, tests, and CI improvements ([28d7391](https://github.com/loonghao/dcc-mcp-core/commit/28d73912b0dcd9233b2b0eb6f076702321c0fb5d))
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

* add AI-friendly docs (AGENTS.md, CLAUDE.md, SKILL.md) + modernize READMEs ([2b3c958](https://github.com/loonghao/dcc-mcp-core/commit/2b3c958ca24791aba0482c3be73a48d750769b4b))
* add complete implementation summary ([c364fb2](https://github.com/loonghao/dcc-mcp-core/commit/c364fb20446295864d7c4c9064b2ba849e605531))
* Add comprehensive Sphinx documentation for DCC-MCP-Core ([d49dbaf](https://github.com/loonghao/dcc-mcp-core/commit/d49dbaf463fe2dc56aa3c51284fddb299efdf029))
* add GEMINI.md, enhance AI agent guides with decision tables and integration patterns ([#87](https://github.com/loonghao/dcc-mcp-core/issues/87)) ([42ff2ba](https://github.com/loonghao/dcc-mcp-core/commit/42ff2ba604c118177edbffa665b9d8fdfb31062d))
* **agents:** update AGENTS.md for Skills-First API + add codecov setup ([b5b9eac](https://github.com/loonghao/dcc-mcp-core/commit/b5b9eac9c6595518da9f9ce930709edffb008cad))
* comprehensive feature documentation ([e270f1c](https://github.com/loonghao/dcc-mcp-core/commit/e270f1cd9805f7780bc9ec036caeb5524fb6fce5))
* comprehensive README rewrite + 3 new guides (MCP+Skills, Gateway Election, Scopes/Policies/Deps) ([e385969](https://github.com/loonghao/dcc-mcp-core/commit/e38596937cee001ebf3632ccac2bc2783f06088e))
* document bundled skills and get_bundled_skill_paths() API ([6545e8f](https://github.com/loonghao/dcc-mcp-core/commit/6545e8fb9bf4713f9d03ecf78efed69b77ebb759))
* enhance AI agent guidance for v0.12.29 ([453ca25](https://github.com/loonghao/dcc-mcp-core/commit/453ca25968cad0227de5bb222092112267062cf8))
* enhance AI agent guidance with Bridge, Scene Model, Serialization APIs and MCP 2026 roadmap ([2425cda](https://github.com/loonghao/dcc-mcp-core/commit/2425cda64ed52d19a2d6c434eaee2589089271a7))
* enhance AI agent guidance, fix legacy API refs, update llms.txt ([#85](https://github.com/loonghao/dcc-mcp-core/issues/85)) ([11ee040](https://github.com/loonghao/dcc-mcp-core/commit/11ee0404b2a2ab36d7c432c587600cf441cbdcba))
* fix API accuracy across capture/http/process/usd docs (EN+ZH) ([ded9f24](https://github.com/loonghao/dcc-mcp-core/commit/ded9f242c2d5c8f6b03b2ca6a17dce06156bf68e))
* fix API correctness and enhance AI agent guidance (Run [#4](https://github.com/loonghao/dcc-mcp-core/issues/4)) ([09a8971](https://github.com/loonghao/dcc-mcp-core/commit/09a8971386c2f0bf8d1c1679c43c43058cf136f0))
* fix dead links and update README ([14a358f](https://github.com/loonghao/dcc-mcp-core/commit/14a358fddc6781f60b136575ecd5a09bdd0dee72))
* **http,agents:** document gateway competition API and update agent guide ([583aa6b](https://github.com/loonghao/dcc-mcp-core/commit/583aa6b802224ef1f48c49a527380b484db96fb2))
* migrate from Sphinx to VitePress with i18n support ([1c4ef9c](https://github.com/loonghao/dcc-mcp-core/commit/1c4ef9cd96ef23d9c6d7c32605e4fd785324d5d7))
* refresh all documentation for v0.12.23 ([d53605b](https://github.com/loonghao/dcc-mcp-core/commit/d53605be678ba763bd4aa38d59d23dcef5dcf1b1))
* replace action terminology with skill across docs and type stubs ([0985e5b](https://github.com/loonghao/dcc-mcp-core/commit/0985e5bd58486c1456741d8756ebfd3e134dbb4b))
* update AGENTS.md with new MCP HTTP architecture design ([#107](https://github.com/loonghao/dcc-mcp-core/issues/107)) ([8cec983](https://github.com/loonghao/dcc-mcp-core/commit/8cec9833be8d5f22954f7a6c80b3059f0d708762))
* update AI agent guidance for v0.12.7 — McpHttpServer, FramedChannel.call(), 12 crates ([#106](https://github.com/loonghao/dcc-mcp-core/issues/106)) ([b6b1d37](https://github.com/loonghao/dcc-mcp-core/commit/b6b1d37e815f97eefbc15b5eb752530160e0be4e))
* update AI agent guidance for v0.12.9 — MCP 2025-11-05 draft, 12 crates, DeferredExecutor tips ([#109](https://github.com/loonghao/dcc-mcp-core/issues/109)) ([8ad45bb](https://github.com/loonghao/dcc-mcp-core/commit/8ad45bb682e7507f583b96ee8bf049495ead14b8))

## [0.12.29](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.28...v0.12.29) (2026-04-15)


### Features

* **gateway:** add MCP Resources API and SSE push for dynamic instance discovery ([71b9928](https://github.com/loonghao/dcc-mcp-core/commit/71b99281fef82e1bd0d01df71110bffd1b348ffe))
* **transport,gateway:** multi-document support and agent disambiguation ([67bc624](https://github.com/loonghao/dcc-mcp-core/commit/67bc62472333dca67bd2f8b1dda18f4293586aee))
* version-aware gateway election, SkillPolicy/Deps/Scope, MCP cancellation ([b136571](https://github.com/loonghao/dcc-mcp-core/commit/b13657180368b9ba05bdf635002756b01340dc19))


### Bug Fixes

* add update_documents to ServiceDiscovery trait, ServiceRegistry and TransportManager ([519d762](https://github.com/loonghao/dcc-mcp-core/commit/519d762d4effe5b7557a6bf1c2d6d965cb12a722))
* **http,skills:** resolve 5 real performance and correctness issues ([f825b3b](https://github.com/loonghao/dcc-mcp-core/commit/f825b3b88c7aa18a4d5980afeae12f0897b5ac0a))


### Documentation

* add complete implementation summary ([c364fb2](https://github.com/loonghao/dcc-mcp-core/commit/c364fb20446295864d7c4c9064b2ba849e605531))
* comprehensive feature documentation ([e270f1c](https://github.com/loonghao/dcc-mcp-core/commit/e270f1cd9805f7780bc9ec036caeb5524fb6fce5))
* comprehensive README rewrite + 3 new guides (MCP+Skills, Gateway Election, Scopes/Policies/Deps) ([e385969](https://github.com/loonghao/dcc-mcp-core/commit/e38596937cee001ebf3632ccac2bc2783f06088e))

## [0.12.28](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.27...v0.12.28) (2026-04-15)


### Bug Fixes

* **server_base:** use discover() API, set dcc_type, read DCC_MCP_REGISTRY_DIR ([#191](https://github.com/loonghao/dcc-mcp-core/issues/191)) ([bd5f0c2](https://github.com/loonghao/dcc-mcp-core/commit/bd5f0c2709335a8ec389209ee33cd805395730ca))

## [0.12.27](https://github.com/loonghao/dcc-mcp-core/compare/v0.12.26...v0.12.27) (2026-04-15)


### Bug Fixes

* **server_base:** pass port via McpHttpConfig constructor (read-only attribute) ([75ac61c](https://github.com/loonghao/dcc-mcp-core/commit/75ac61c14e74ea052fe79adf436f57f7b8a8a402))

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
