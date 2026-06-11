# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.18.20](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.19...v0.18.20) (2026-06-11)


### Features

* add server release bundles ([2139b01](https://github.com/dcc-mcp/dcc-mcp-core/commit/2139b01a5fa7c920df593092b2b85d27c368c087))


### Bug Fixes

* improve gateway sidecar diagnostics ([6045d19](https://github.com/dcc-mcp/dcc-mcp-core/commit/6045d19eadc06bcb309027c2002098d6b829b996))

## [0.18.19](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.18...v0.18.19) (2026-06-11)


### Bug Fixes

* resolve packaged gateway binary ([039c062](https://github.com/dcc-mcp/dcc-mcp-core/commit/039c062bc036543f0ac8be899ddeb5db7b84190a))

## [0.18.18](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.17...v0.18.18) (2026-06-11)


### Features

* **marketplace:** add add-repo command for direct GitHub install (PIP-1377) ([e892e48](https://github.com/dcc-mcp/dcc-mcp-core/commit/e892e480b0059a62268d6a6dac610b7a372b7c0c))


### Bug Fixes

* clippy warnings for Rust 1.96 — needless_borrows_for_generic_args and let_and_return ([30a39ad](https://github.com/dcc-mcp/dcc-mcp-core/commit/30a39ad984c451b66bbe61f0d683c7cbc5e3370f))


### Documentation

* document auto-update, --restart, install --execute, gateway-ensure crate, and marketplace slide-out ([232479c](https://github.com/dcc-mcp/dcc-mcp-core/commit/232479c28eda613ea51e13cb1e17ae8f0a11f387))

## [0.18.17](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.16...v0.18.17) (2026-06-10)


### Features

* **dcc-mcp-updater, gateway, cli, server:** add gateway-controlled auto-update mechanism ([4d10fd1](https://github.com/dcc-mcp/dcc-mcp-core/commit/4d10fd19273fcac1a6507eae9fccd2f3da2762e3))


### Bug Fixes

* **ci, media:** add py314 coverage and portable ffmpeg flags ([b40dfda](https://github.com/dcc-mcp/dcc-mcp-core/commit/b40dfda22c8d1cf50b359d77351f76717de83fe2))
* **ci:** explicitly install lightningcss native module after npm ci ([a906380](https://github.com/dcc-mcp/dcc-mcp-core/commit/a906380d307e2aae792e540d70dccd86737436f2))
* **cli, fmt, clippy:** fix lint errors and apply cargo fmt ([5609cbf](https://github.com/dcc-mcp/dcc-mcp-core/commit/5609cbf046825469549a78e8aa8d4e8592d9aef3))
* **gateway:** align daemon keepalive liveness ([eb1f92d](https://github.com/dcc-mcp/dcc-mcp-core/commit/eb1f92d3c1c7dbd97201825cd434ae096ff30448))
* **updater, gateway, cli:** address PR review issues ([a9b0279](https://github.com/dcc-mcp/dcc-mcp-core/commit/a9b02798bc198d31fc36a7ba065d2440952a506d))

## [0.18.16](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.15...v0.18.16) (2026-06-10)


### Features

* add --restart flag to dcc-mcp-server gateway daemon ([bd94731](https://github.com/dcc-mcp/dcc-mcp-core/commit/bd947310b07089b8b4c145efe8c0950765fe50ad))
* **admin-ui:** Phase 1 panel consolidation — Discover/Overview/Traces sub-tabs ([9ec9d75](https://github.com/dcc-mcp/dcc-mcp-core/commit/9ec9d75689306c9b30f1bface1cdf4807ff102c8))
* **admin:** slide-out detail panel for installed marketplace packages ([bc3863f](https://github.com/dcc-mcp/dcc-mcp-core/commit/bc3863f610211227170de60c8c0a527908db765c))
* **cli:** implement install --execute with consent gating and rollback ([c60d2f0](https://github.com/dcc-mcp/dcc-mcp-core/commit/c60d2f0d4188adcfaf260e3252299f51ea289b2f))
* **cli:** implement install --execute with InstallStepAction, consent gating, and rollback ([e30db34](https://github.com/dcc-mcp/dcc-mcp-core/commit/e30db3484b8fbd311d175530589bf4eebfef8339))
* **cli:** implement three-layer gateway binary discovery ([9a0e600](https://github.com/dcc-mcp/dcc-mcp-core/commit/9a0e600e0b668f034c84204f68d7b7c8b805efbf))
* **core:** add exception protection and watchdog for gateway guardian ([134e783](https://github.com/dcc-mcp/dcc-mcp-core/commit/134e783654ce45590319b40f43c764082a192434))
* **core:** add watchdog for Rust gateway guardian in sidecar ([6646eb8](https://github.com/dcc-mcp/dcc-mcp-core/commit/6646eb877dbf0171981d78ab814770690e43edb1))
* extract shared gateway ensure crate ([efe247f](https://github.com/dcc-mcp/dcc-mcp-core/commit/efe247f6bcdd2d06993d99ead3e06d7e39f61c2f))
* reduce gateway guardian watchdog interval from 60s to 15s with immediate retry ([9db3af7](https://github.com/dcc-mcp/dcc-mcp-core/commit/9db3af76d5b3ef9c6e8ed6ceaf8f977f3c6a09da))
* **skills:** integrate gateway ensure into dcc-cli-gateway skill ([d8e0c3b](https://github.com/dcc-mcp/dcc-mcp-core/commit/d8e0c3b7ff448b13657c4f3dfbc3a0125101776b))


### Bug Fixes

* **admin-ui:** resolve TypeScript errors in panel consolidation ([1d8095d](https://github.com/dcc-mcp/dcc-mcp-core/commit/1d8095d03cc90a94949b1b006411a04b9f7cd2fd))
* apply cargo fmt and ruff format for CI formatting checks ([706bc06](https://github.com/dcc-mcp/dcc-mcp-core/commit/706bc06780151659a1864cdf2ff61a4cc782f52a))
* cargo fmt formatting for gateway_discovery.rs ([ad02358](https://github.com/dcc-mcp/dcc-mcp-core/commit/ad02358467ef2c7f334c26cf96f88e75d1e7ef23))
* cargo fmt formatting for install.rs ([106a686](https://github.com/dcc-mcp/dcc-mcp-core/commit/106a6868ea2c60f447af59f74ec3badd7da38384))
* **ci:** pin build-wheel windows runner to windows-2022 ([66bcb33](https://github.com/dcc-mcp/dcc-mcp-core/commit/66bcb33455668b260bb204b7b9a9d695dc8699dd))
* **ci:** use any() in release asset pattern matching to fix false timeout ([bfc05d8](https://github.com/dcc-mcp/dcc-mcp-core/commit/bfc05d86e5597b1ebdc4dff9364802b37f78973f))
* collapse nested if-let in rollback_all (clippy::collapsible_if) ([a184acc](https://github.com/dcc-mcp/dcc-mcp-core/commit/a184acc7eb2641b4a264c3d44e5836621a21fac9))
* **gateway-ensure:** collapse nested if to fix clippy collapsible_if ([cdf8961](https://github.com/dcc-mcp/dcc-mcp-core/commit/cdf8961ef176fecdcd90b1ea159243c212296777))
* **gateway:** restructure _LaunchLock.acquire to reduce TOCTOU windows ([45f83e4](https://github.com/dcc-mcp/dcc-mcp-core/commit/45f83e430d1e17e085d4c7090ce615d275467b82))
* increase probe wait time in flaky test to avoid macOS CI timing ([2949828](https://github.com/dcc-mcp/dcc-mcp-core/commit/294982875760afaa486e3e1c96ad3c3d8fe37e05))
* lint errors in Maya E2E tests (D403 docstring caps + F841 unused var) ([77bb21c](https://github.com/dcc-mcp/dcc-mcp-core/commit/77bb21c34551aceb1e04133080aecd8b8b1c7dd1))
* **p0-3:** add Python-side version-aware gateway takeover ([37a2cf3](https://github.com/dcc-mcp/dcc-mcp-core/commit/37a2cf3cb51e6783df47d8ee0d56be404816925a))
* **p0-3:** resolve merge conflict and ruff format ([9895e24](https://github.com/dcc-mcp/dcc-mcp-core/commit/9895e24e40cf22d53eed6768c5969417c43bf199))
* pivot install --execute to marketplace.json data source ([16b8d69](https://github.com/dcc-mcp/dcc-mcp-core/commit/16b8d697868dddec0006af87a92bbbfe7d4bd01c))
* prevent silent embedded-fallback after gateway daemon ensure fails ([e01d06d](https://github.com/dcc-mcp/dcc-mcp-core/commit/e01d06d33562df2e14776eac6c06e396bf74fc7f))
* remove trailing blank line to satisfy cargo fmt ([de967d7](https://github.com/dcc-mcp/dcc-mcp-core/commit/de967d76fda7d6e15360390cbf2cff5a752a1e73))
* remove trailing semicolon in AlreadyExists branch to return Result ([0104f35](https://github.com/dcc-mcp/dcc-mcp-core/commit/0104f35d5937c4f3e2fa167e888194f676d9a1ca))
* replace unwrap_or_else closure with block in sidecar launcher ([2ffe396](https://github.com/dcc-mcp/dcc-mcp-core/commit/2ffe396d7941f9720671d309ad73b54124d9fc5d))
* replace unwrap_or_else with unwrap_or in gateway_launch_lock_stale_after ([ba4024d](https://github.com/dcc-mcp/dcc-mcp-core/commit/ba4024d0e7d00d7d0e7af864ec90e744f0b7af1b))
* resolve clippy warnings — unneeded return and unnecessary closure ([e3d367b](https://github.com/dcc-mcp/dcc-mcp-core/commit/e3d367baed2ff8e2f1afc982b82a80fae8a145a6))
* resolve E402 lint errors by moving imports to top of file ([76ae900](https://github.com/dcc-mcp/dcc-mcp-core/commit/76ae900d38d72b4e4700bcfc9f90751b900546d8))
* restore return keyword in lock-acquire double-check block ([dba8811](https://github.com/dcc-mcp/dcc-mcp-core/commit/dba8811ee75f763e80096821de11ca501a742611))
* restore return keywords for early health-check returns ([04a1c0d](https://github.com/dcc-mcp/dcc-mcp-core/commit/04a1c0d6b7f8a59ed935f23fd3aea9562c9e7518))
* restore version to 0.18.15 (x-release-please-version) ([e50eb6f](https://github.com/dcc-mcp/dcc-mcp-core/commit/e50eb6f89e86d9ca055cb87f05963c4356a5d6b7))
* ruff format on Maya E2E tests ([6807f85](https://github.com/dcc-mcp/dcc-mcp-core/commit/6807f85e877a4696736bf6fd727a55f091ad2f8e))
* rustfmt, rename push_arg, remove restart_spawn_fresh, add pre-push fmt ([941621d](https://github.com/dcc-mcp/dcc-mcp-core/commit/941621d21292807af74ef10bcfcc2b4a402e4bd8))
* set DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES=1 in ScopedMarketplaceInstallRoot ([6f83f56](https://github.com/dcc-mcp/dcc-mcp-core/commit/6f83f5649aea9dfac76416d14f56affdfd0d686d))
* **sidecar:** abort old guardian before replacing in watchdog restart ([010d198](https://github.com/dcc-mcp/dcc-mcp-core/commit/010d198362c4413753bbc49bb581aa534403cb45))
* **skills:** eliminate env var race condition in resolve_registry_dcc_type tests ([8803a86](https://github.com/dcc-mcp/dcc-mcp-core/commit/8803a86f962df09c45b3900f4c491bf46e13174e))
* **skills:** eliminate env var race condition in resolve_registry_dcc_type tests ([598361d](https://github.com/dcc-mcp/dcc-mcp-core/commit/598361dd188cd9f3befe75ccb160873e531c7393))
* **transport:** add dual-format SystemTime deserializer and defensive parse fallback ([68cafc2](https://github.com/dcc-mcp/dcc-mcp-core/commit/68cafc2fb453a958e7c2ce9caedb4ad5b4753c47))
* unify concurrent gateway ensure timeout and lock-loser wait logic ([bd7504f](https://github.com/dcc-mcp/dcc-mcp-core/commit/bd7504f0d8e032b126243cd6460ac83d498ceadc))
* update bundled catalog test to use photoshop (infra-only, no install metadata) ([a6c5117](https://github.com/dcc-mcp/dcc-mcp-core/commit/a6c51174122540aec3f42ab3799020e52960ef79))


### Documentation

* add gateway ensure lifecycle commands, marketplace source management, and AGENTS marketplace skill references ([07de3e1](https://github.com/dcc-mcp/dcc-mcp-core/commit/07de3e1fda867df39f2900dc04ba45b655b22f95))

## [0.18.15](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.14...v0.18.15) (2026-06-08)


### Bug Fixes

* **ci:** increase release asset verification retries to 20 and add diagnostic logging ([ed91306](https://github.com/dcc-mcp/dcc-mcp-core/commit/ed913068b0e2102afaf56faed0387407f08379c3))

## [0.18.14](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.13...v0.18.14) (2026-06-08)


### Features

* **core:** extract shared registration phase pipeline from Maya adapter ([99bb2ef](https://github.com/dcc-mcp/dcc-mcp-core/commit/99bb2ef62134ecd0246e383794853c67a80f71c5))


### Bug Fixes

* resolve ruff lint/format errors in _registration.py ([74dac34](https://github.com/dcc-mcp/dcc-mcp-core/commit/74dac3450fe3e7b9ea7c9037a7dec02d717023db))

## [0.18.13](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.12...v0.18.13) (2026-06-08)


### Features

* **admin-ui:** add marketplace source management, update flow, force install, and structured error display ([83d3b5f](https://github.com/dcc-mcp/dcc-mcp-core/commit/83d3b5f1acb8a385fa354a7dda686abdfbea90db))


### Bug Fixes

* **admin-ui:** replace isLoading with isPending for mutation result ([80ac6cf](https://github.com/dcc-mcp/dcc-mcp-core/commit/80ac6cf93dd0b769417aed9ff14f8d621229fdee))
* **ci:** skip npm ci on macOS rust-check — use prebuilt admin UI artifact ([4670ea1](https://github.com/dcc-mcp/dcc-mcp-core/commit/4670ea10d09ad6692370dd994d158b769b6e49b1))
* **ci:** upload build-wheels artifacts directly to GitHub Release ([ed6fe5b](https://github.com/dcc-mcp/dcc-mcp-core/commit/ed6fe5b26d69adeb8b6b022c1e7c42e8d8ba68e9))
* **release:** upload dcc_mcp_core wheels to GitHub Release per-platform ([39e1662](https://github.com/dcc-mcp/dcc-mcp-core/commit/39e16629c380c080b9ca77906abcf0bd87db7f0b))


### Code Refactoring

* **core:** extract shared _lazy.py helper for lazy-import scaffolding ([3df68c3](https://github.com/dcc-mcp/dcc-mcp-core/commit/3df68c3e680fa8eeaa0d2a5c8a5d8a54d6fc85d6))
* **core:** split DccServerBase into 4 seam controllers (PIP-688) ([1e470ff](https://github.com/dcc-mcp/dcc-mcp-core/commit/1e470ff1d926c949f69aa07706f911ba1b2dded9))

## [0.18.12](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.11...v0.18.12) (2026-06-08)


### Features

* **admin-ui:** add Panel type extension, URL alias infrastructure, and tab query readers ([fd18d89](https://github.com/dcc-mcp/dcc-mcp-core/commit/fd18d89ac88984d2cd8a2e71f2298b929eb77d25))
* **catalog:** extend install schema with pip package support ([04a3f27](https://github.com/dcc-mcp/dcc-mcp-core/commit/04a3f27d4222053d7d88800de5b5215281dc44d4))
* **cli:** add `gateway ensure|start|stop|status` lifecycle commands ([ee342bc](https://github.com/dcc-mcp/dcc-mcp-core/commit/ee342bc48085f641da7e3b095cbeb525aa2bfe12))
* **skills:** add marketplace-publish-extension skill package ([6d9894f](https://github.com/dcc-mcp/dcc-mcp-core/commit/6d9894fff675111889965ac672d280974036451d))
* **skills:** add marketplace-publish-extension skill package ([278918d](https://github.com/dcc-mcp/dcc-mcp-core/commit/278918da8a2d31e3ca9372df291ee1250637df75))
* **skills:** add marketplace-publish-extension skill package ([a63b4f4](https://github.com/dcc-mcp/dcc-mcp-core/commit/a63b4f4cf4a679725014b856e9d0c013b89801c5))
* **skills:** formalize marketplace-create-extension and marketplace-publish-extension to skills/ ([a9633d7](https://github.com/dcc-mcp/dcc-mcp-core/commit/a9633d7066c255bdbc6295894378df887ef62cb7))
* **skills:** formalize marketplace-create-extension and marketplace-publish-extension to skills/ ([f096d21](https://github.com/dcc-mcp/dcc-mcp-core/commit/f096d210876d7812391f91369cebdcca90104047))
* **skills:** formalize marketplace-create-extension and marketplace-publish-extension to skills/ ([cb13d83](https://github.com/dcc-mcp/dcc-mcp-core/commit/cb13d83e48f2f37453472b436878896c4d8b426f))


### Bug Fixes

* cargo fmt import ordering in capability/index.rs ([e4b3c9e](https://github.com/dcc-mcp/dcc-mcp-core/commit/e4b3c9e32c80c185bbd1022aa71d4c957893f437))
* **ci:** remove continue-on-error from reusable workflow job in release.yml ([fd00c37](https://github.com/dcc-mcp/dcc-mcp-core/commit/fd00c37cfd7742522181a7542959362c5492cdef))
* **test:** set is_gateway=True in crash_recovery_loop on_promote callback ([4545577](https://github.com/dcc-mcp/dcc-mcp-core/commit/4545577c3a10426dcb8933e00d3e78416e739ae2))


### Code Refactoring

* **capability:** consolidate dual compute_fingerprint into gateway-core ([467f0f7](https://github.com/dcc-mcp/dcc-mcp-core/commit/467f0f749d673b71367b91729ee1c7db300d2ca3))


### Documentation

* add Marketplace Panel and Admin Workflow sections to docs ([a165134](https://github.com/dcc-mcp/dcc-mcp-core/commit/a165134b0863559848772096e38c43b141586f93))
* align CLI references to CLI-first narrative with Python fallback positioning ([11b3d68](https://github.com/dcc-mcp/dcc-mcp-core/commit/11b3d688ac2defcfa74300eb6f5d70c07762b35c))
* **gateway:** add Maya rendering intent routing table to agent-workflows guide ([d41a736](https://github.com/dcc-mcp/dcc-mcp-core/commit/d41a736b4e826c3d8ad079ca3ecd0df6d35fbdc6))

## [0.18.11](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.10...v0.18.11) (2026-06-07)


### Features

* add marketplace-create-extension and formalize publish-extension skills ([#1564](https://github.com/dcc-mcp/dcc-mcp-core/issues/1564)) ([e5816b9](https://github.com/dcc-mcp/dcc-mcp-core/commit/e5816b9455cd1ef49eccfd6195594473c16e67c1))

## [0.18.10](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.9...v0.18.10) (2026-06-07)


### Features

* **admin-ui:** add marketplace detail modal and platform module ([262bf4d](https://github.com/dcc-mcp/dcc-mcp-core/commit/262bf4de5f9d27ada5faf9f294aa9385ccf8da83))
* **admin-ui:** add marketplace Playwright E2E test specs ([347389a](https://github.com/dcc-mcp/dcc-mcp-core/commit/347389aeb1d4a6655eb74cec6818b8c6f6498505))
* **catalog:** add dcc-mcp-maya-mgear entry ([676b1d8](https://github.com/dcc-mcp/dcc-mcp-core/commit/676b1d8c59a42756896eac9e6df94d1d55db1620))
* **gateway:** add dcc_types[] + tags_any OR filters to search ([b778a9e](https://github.com/dcc-mcp/dcc-mcp-core/commit/b778a9e42c199f05da2e782cc25567faa1ca6297))
* **skills:** add marketplace-publish-extension skill package ([500a010](https://github.com/dcc-mcp/dcc-mcp-core/commit/500a010cf637ea7adbb5fdc1a2e5271553b8fdb9))


### Bug Fixes

* deduplicate Qt dispatcher Python implementation ([d203895](https://github.com/dcc-mcp/dcc-mcp-core/commit/d2038954c31d7ef47bc0ddfb8f1e7e68f0896166))
* resolve TypeScript and markdown lint issues ([cffa316](https://github.com/dcc-mcp/dcc-mcp-core/commit/cffa316bba0c58ea22e07b2280a7cc422dcbb08e))
* sort imports in clawhub_sync.py (ruff I001) ([5f62525](https://github.com/dcc-mcp/dcc-mcp-core/commit/5f62525c23e2db7d976152c86bb831111da39b8a))
* tolerate versioned ClawHub duplicate error messages ([a44f7c2](https://github.com/dcc-mcp/dcc-mcp-core/commit/a44f7c2edc1018a4eb0e84697ab26687983705a6))
* tolerate versioned ClawHub duplicate errors ([b3483ad](https://github.com/dcc-mcp/dcc-mcp-core/commit/b3483adee9394a6ab1d988a78c4917434eb23b7d))

## [0.18.9](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.8...v0.18.9) (2026-06-07)


### Features

* extract shared dcc-mcp-marketplace crate ([#1537](https://github.com/dcc-mcp/dcc-mcp-core/issues/1537)) ([b6c3560](https://github.com/dcc-mcp/dcc-mcp-core/commit/b6c3560e982b1debd7002b980a2e4087785eb4d1))


### Bug Fixes

* disable gateway election for job persistence probe server ([56a5bf2](https://github.com/dcc-mcp/dcc-mcp-core/commit/56a5bf28f4ccccfff54d9cebbb85e32a2a10c34f))


### Documentation

* **adr:** add ADR-010 MCP 2026-07-28 dual-protocol migration strategy ([c7a61c2](https://github.com/dcc-mcp/dcc-mcp-core/commit/c7a61c251fb77fcb7db25a08ceccc8822d3bc112))
* **skills-creator:** correct tags_any/dcc_types as planned fields, not current ([966286c](https://github.com/dcc-mcp/dcc-mcp-core/commit/966286ca5c579e10a24cd56965ae5c7e07986d8b))

## [0.18.8](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.7...v0.18.8) (2026-06-07)


### Bug Fixes

* **ci:** add npm ci retry in build-wheel action for transient network errors ([cd020e2](https://github.com/dcc-mcp/dcc-mcp-core/commit/cd020e2e9b084594a2c088793cf6647855ccbc51))
* **gateway:** add abort_and_wait to GatewayHandle to prevent SIGABRT on shutdown ([5e152f8](https://github.com/dcc-mcp/dcc-mcp-core/commit/5e152f8a28a709a583a88c8e8cdd6ce09ff21dbb))
* **gateway:** add abort_and_wait to GatewayHandle to prevent SIGABRT on shutdown ([99eb553](https://github.com/dcc-mcp/dcc-mcp-core/commit/99eb553cf6aae0b87062fcd27d816a45a9e678de))

## [0.18.7](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.6...v0.18.7) (2026-06-06)


### Bug Fixes

* increase SSE notification budget for prompts watcher on slow CI runners ([#1542](https://github.com/dcc-mcp/dcc-mcp-core/issues/1542)) ([6f34229](https://github.com/dcc-mcp/dcc-mcp-core/commit/6f3422959ea9d5ab6fce69af36d236858788f04e))
* make --source exclusive and add icon to marketplace schema ([#1541](https://github.com/dcc-mcp/dcc-mcp-core/issues/1541)) ([8fb01f2](https://github.com/dcc-mcp/dcc-mcp-core/commit/8fb01f262e9ba722275ea5c6c30fd967b869e5ba))

## [0.18.6](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.5...v0.18.6) (2026-06-06)


### Features

* **marketplace:** CLI read-only commands with schema validation and multi-source merge ([beeea34](https://github.com/dcc-mcp/dcc-mcp-core/commit/beeea34e7a19dde49a4a0e20290a30ac99e3f5e6))


### Bug Fixes

* add icon field to CatalogEntry for compatibility with feat/marketplace-icon ([336a0fc](https://github.com/dcc-mcp/dcc-mcp-core/commit/336a0fc68dc25b25d1f548287e431aeb931b56e8))
* **ci:** isolate workflow_dispatch from push concurrency in release workflow ([792a1f7](https://github.com/dcc-mcp/dcc-mcp-core/commit/792a1f752a149e3db7bf3c56eee6d92cd50060e8))
* **ci:** scope cancel-in-progress to push events only in release concurrency ([588455f](https://github.com/dcc-mcp/dcc-mcp-core/commit/588455f4105148c77a7aeefb05e4e84e63333451))
* **ci:** set cancel-in-progress to true in release workflow concurrency ([a273a25](https://github.com/dcc-mcp/dcc-mcp-core/commit/a273a250294ebac7d2b545d4c79a25c6e20d2100))

## [0.18.5](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.4...v0.18.5) (2026-06-06)


### Bug Fixes

* **ci:** remove pull_request_target workflow for Sentry E2E on fork PRs ([#1535](https://github.com/dcc-mcp/dcc-mcp-core/issues/1535)) ([fa44899](https://github.com/dcc-mcp/dcc-mcp-core/commit/fa44899caaa6401bf7364449390549375d05b27c))

## [0.18.4](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.3...v0.18.4) (2026-06-06)


### Features

* **marketplace:** add icon field to CatalogEntry and admin UI cards ([3b40e89](https://github.com/dcc-mcp/dcc-mcp-core/commit/3b40e89016a55bb6b742fd28cad2290df8ff3e26))


### Bug Fixes

* **ci:** add pull_request_target workflow for Sentry E2E on fork PRs ([0db35c7](https://github.com/dcc-mcp/dcc-mcp-core/commit/0db35c799690fe8dee4c08e468b19449c4a82735))
* **marketplace:** cargo fmt ([03a3646](https://github.com/dcc-mcp/dcc-mcp-core/commit/03a36464d1fd7943c1aaeb7d7a8dafdd805cf8c1))
* **marketplace:** compact icon CSS to stay under 3000-line file-size gate ([2ad39b9](https://github.com/dcc-mcp/dcc-mcp-core/commit/2ad39b9517f21e5674e8f643d350112893229c94))
* **marketplace:** extract CSS to feature directory to stay under file-size gate ([fc645aa](https://github.com/dcc-mcp/dcc-mcp-core/commit/fc645aa50a3493810b662df0ffd22a4173e1efd5))
* **marketplace:** remove blank lines to pass 3000-line file-size gate ([ee1222c](https://github.com/dcc-mcp/dcc-mcp-core/commit/ee1222cb33dae8a414d94c4f8dd5ffa77fd5da92))
* **marketplace:** switch to letter fallback when icon image fails to load ([6d896bd](https://github.com/dcc-mcp/dcc-mcp-core/commit/6d896bdb295a0fbf5d311acd3e0315b07125ddfd))

## [0.18.3](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.2...v0.18.3) (2026-06-06)


### Features

* **admin-ui:** add integrations panel ([6e493a0](https://github.com/dcc-mcp/dcc-mcp-core/commit/6e493a0a1aca59d55b817aa6ff7957f39baf85a7))
* **gateway:** optimize discovery workflow ([880cc63](https://github.com/dcc-mcp/dcc-mcp-core/commit/880cc632422f586fc974f15b98b1cb8fd702ca83))
* **models:** add call_examples to describe responses ([ee534c4](https://github.com/dcc-mcp/dcc-mcp-core/commit/ee534c41d12c1d0667f3b7250af47a20f2014b68))


### Documentation

* add Admin Integrations configuration docs ([ee5bc18](https://github.com/dcc-mcp/dcc-mcp-core/commit/ee5bc18acffbbcc2bdb05ac20b3c5ed768629e7f))
* add FPT to compatibility matrix, mark pending adapter tags ([898ba2c](https://github.com/dcc-mcp/dcc-mcp-core/commit/898ba2c31deec2fb43c1ba74db1a4f270f933685))
* **skills-creator:** document gateway-facing tag taxonomy ([1755018](https://github.com/dcc-mcp/dcc-mcp-core/commit/1755018eaa5ee9e386d652d9df6ff3b35d7b8736))

## [0.18.2](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.1...v0.18.2) (2026-06-05)


### Bug Fixes

* **ci:** install Windows Python 3.7 for release wheels ([#1520](https://github.com/dcc-mcp/dcc-mcp-core/issues/1520)) ([a9e8ddd](https://github.com/dcc-mcp/dcc-mcp-core/commit/a9e8dddee03fbb6a5a2c739145fc6dcd3658f8f8))

## [0.18.1](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.18.0...v0.18.1) (2026-06-05)


### Bug Fixes

* remove openssl from server binary graph ([169283c](https://github.com/dcc-mcp/dcc-mcp-core/commit/169283c4160c59cf35e677f3b45fc3a9a1c82786))

## [0.18.0](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.17.56...v0.18.0) (2026-06-05)


### Features

* **admin-ui:** add Marketplace panel with per-DCC install/uninstall ([9bff7e5](https://github.com/dcc-mcp/dcc-mcp-core/commit/9bff7e5b44b8c6788d5e4e5e8a23de9e9fb93013))
* **daemon:** refactor gateway guardian to use launch_detached, add build_gateway_daemon_command ([#1509](https://github.com/dcc-mcp/dcc-mcp-core/issues/1509)) ([3a14199](https://github.com/dcc-mcp/dcc-mcp-core/commit/3a141992f83ef37006c0acc73bf146b53085444d))
* **gateway:** add _meta passthrough to adapter skill params + marketplace admin API endpoints ([6dca6ad](https://github.com/dcc-mcp/dcc-mcp-core/commit/6dca6ad71f8f875f2463a0960c23790f331bb1ee))
* **gateway:** passthrough bounded request meta ([bfbbd0a](https://github.com/dcc-mcp/dcc-mcp-core/commit/bfbbd0a241042779d30d6328f5c3faf3ee0bf0b9))
* **server:** add Sentry real-ingest E2E tests and CI job ([e63b4cc](https://github.com/dcc-mcp/dcc-mcp-core/commit/e63b4ccede62d6f1709e210d55e73b10ddedc2a8))
* **server:** Sentry error monitoring and webhook analytics documentation ([1010bb8](https://github.com/dcc-mcp/dcc-mcp-core/commit/1010bb84782247d38a4592375a7fe6527528e08b))


### Bug Fixes

* **gateway:** harden meta passthrough for CI ([8e063d2](https://github.com/dcc-mcp/dcc-mcp-core/commit/8e063d2590f9f5559b044dde8bda3ba21b8b2a3c))


### Documentation

* add adapter release train onboarding docs ([197d124](https://github.com/dcc-mcp/dcc-mcp-core/commit/197d1240efcdf390f4ce79aabf310624ab59286a))
* add Autodesk Product Help catalog connector ([c320fac](https://github.com/dcc-mcp/dcc-mcp-core/commit/c320facd0ebeb42543a334c5fd9c3d69db194d45))
* add marketplace, analytics dashboard, and sentry monitoring docs ([b006143](https://github.com/dcc-mcp/dcc-mcp-core/commit/b006143ad70798525e4886ffa35c07020bfb4318))
* **release:** document 0.18.0 minor rationale ([5428b7b](https://github.com/dcc-mcp/dcc-mcp-core/commit/5428b7bb69635c72df5e3ab63fa4e872cddb8cff))

## [0.17.56](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.17.55...v0.17.56) (2026-06-04)


### Features

* **marketplace:** install zip skill packages ([0e0ab4b](https://github.com/dcc-mcp/dcc-mcp-core/commit/0e0ab4b9a70673b0b5b95675df094ca4c5c9faaf))

## [0.17.55](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.17.54...v0.17.55) (2026-06-04)


### Features

* **cli:** add marketplace catalog discovery ([fb38554](https://github.com/dcc-mcp/dcc-mcp-core/commit/fb385540fbbd3b380810e6c89ecd9f0b1108975c))
* **cli:** install marketplace skill packages ([221cd36](https://github.com/dcc-mcp/dcc-mcp-core/commit/221cd3645e0d392d87185abf9fe8a93212492bad))
* **gateway:** add daemon mode support for gateway server and Python API ([2145b91](https://github.com/dcc-mcp/dcc-mcp-core/commit/2145b9144daf43ba8e3b3698ac0b57229e150863))
* **marketplace:** add update/outdated commands and migrate gateway catalog ([3f46148](https://github.com/dcc-mcp/dcc-mcp-core/commit/3f461489ff328368147bb88e75b7fc5e2ea17ee0))


### Bug Fixes

* **cli:** harden marketplace package installs ([2c7e839](https://github.com/dcc-mcp/dcc-mcp-core/commit/2c7e8397d8e8614ee4980cf9fd34c5afbc8a3930))
* **daemon:** address review feedback for PIP-513 daemon mode ([9580852](https://github.com/dcc-mcp/dcc-mcp-core/commit/95808525e0f93c3953efcb53a838c65ed4ad9f48))
* **daemon:** harden gateway detach semantics ([156ba2d](https://github.com/dcc-mcp/dcc-mcp-core/commit/156ba2d7e66f5805a30e29a499d04200cb6a8fce))
* **marketplace:** update packages from latest catalog refs ([7c2a7c4](https://github.com/dcc-mcp/dcc-mcp-core/commit/7c2a7c4af37cf1ef1995fc12e2e2b6827bd103b8))


### Documentation

* add built-in skills, Qt inspector, capability graph, dispatch readiness documentation ([ece42cf](https://github.com/dcc-mcp/dcc-mcp-core/commit/ece42cf2a4c9603b599e50637b7d4c785a114075))
* update cli-reference.md with outdated/update commands ([3f46148](https://github.com/dcc-mcp/dcc-mcp-core/commit/3f461489ff328368147bb88e75b7fc5e2ea17ee0))

## [0.17.54](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.17.53...v0.17.54) (2026-06-04)


### Features

* **admin:** add P1 analytics dashboard with daily aggregation API ([38ea196](https://github.com/dcc-mcp/dcc-mcp-core/commit/38ea19649a35e87496e1c6be17a3ed397d77ecf2))
* **gateway:** add idle lifecycle policy ([d425305](https://github.com/dcc-mcp/dcc-mcp-core/commit/d4253059448cd241797f7bee845eba26a03e9758))
* **gateway:** add status tracking and handle to Rust gateway guardian ([65d7dc4](https://github.com/dcc-mcp/dcc-mcp-core/commit/65d7dc45136102a4e160925470ded271d160bb28))
* **gateway:** publish guardian watchdog status to sidecar registry metadata ([afccb4d](https://github.com/dcc-mcp/dcc-mcp-core/commit/afccb4dcb8297798053edf4e653136c667d1679f))
* **gateway:** wire gateway ensure into Python DccServerBase default startup ([72f1ed6](https://github.com/dcc-mcp/dcc-mcp-core/commit/72f1ed64e5d7dba90e03cf1ee0d392a86223563b))


### Bug Fixes

* **admin-ui:** wire refresh buttons to query refetch, fix i18n key and unused imports ([2e2d678](https://github.com/dcc-mcp/dcc-mcp-core/commit/2e2d678810022f6e12cb2c7dc6072b7159867ac9))
* **gateway:** activate skill groups by default ([98959e1](https://github.com/dcc-mcp/dcc-mcp-core/commit/98959e18712912586a2a4ad16688ddfbd3c97fa1))
* **gateway:** update admin HTML test to match @tanstack/query template literal URL ([3c09c30](https://github.com/dcc-mcp/dcc-mcp-core/commit/3c09c301a2528235e775e956a0b1556602a25579))


### Code Refactoring

* **admin-ui:** replace manual fetch/polling with @tanstack/react-query ([95a676c](https://github.com/dcc-mcp/dcc-mcp-core/commit/95a676c11646177a757b0b1886906c6c64a9e28c))


### Documentation

* **gateway:** document runtime supervisor topology + add cross-DCC regression tests ([2b603da](https://github.com/dcc-mcp/dcc-mcp-core/commit/2b603da7164b1859b83c864b56440e657931bc5d))

## [0.17.53](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.17.52...v0.17.53) (2026-06-03)


### Features

* **gateway:** add LLM usage header support and token visibility plumbing ([fdbe4c6](https://github.com/dcc-mcp/dcc-mcp-core/commit/fdbe4c66995ff1cb21a7efa7b30a2291354da732))


### Bug Fixes

* **gateway:** project llm_usage into admin API rows and SQLite persistence ([44f7a3d](https://github.com/dcc-mcp/dcc-mcp-core/commit/44f7a3d13e05e9a8d8da04279e71edcd3711680f))
* **release:** retry flaky admin-ui npm ci on release builds ([#1486](https://github.com/dcc-mcp/dcc-mcp-core/issues/1486)) ([6e699c0](https://github.com/dcc-mcp/dcc-mcp-core/commit/6e699c05805753caf812ead8bee0714990cf184e))

## [0.17.52](https://github.com/dcc-mcp/dcc-mcp-core/compare/v0.17.51...v0.17.52) (2026-06-03)


### Documentation

* **gateway:** define daemon runtime vocabulary ([d89537b](https://github.com/dcc-mcp/dcc-mcp-core/commit/d89537bb47859b687fa5df5bce6f3c6678c51a20))

## [0.17.51](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.50...v0.17.51) (2026-06-02)


### Features

* **admin:** refresh dcc mcp logo ([af801de](https://github.com/loonghao/dcc-mcp-core/commit/af801de093e564f1c140b5d3bae12b2081be6b5d))
* **brand:** refresh DCC MCP logo ([f646252](https://github.com/loonghao/dcc-mcp-core/commit/f646252e9940fe846ce12f3f275b4997599c0393))
* **gateway:** expose dispatch readiness in instances ([7ec2f46](https://github.com/loonghao/dcc-mcp-core/commit/7ec2f46d3318d733f4d0cb3f612735f496a4b65b))
* **gateway:** expose dispatch readiness in readyz ([a4387d6](https://github.com/loonghao/dcc-mcp-core/commit/a4387d66576f680cacaf64059845a85d7b6c6f65))
* **gateway:** expose recovery diagnostics ([6d5ecfa](https://github.com/loonghao/dcc-mcp-core/commit/6d5ecfa5fa64acd23fb40c937baaeedded4a49a1))
* **gateway:** publish live guardian metadata ([167263b](https://github.com/loonghao/dcc-mcp-core/commit/167263b9edefe6fdd6fe2cd5bc22d23cbc6bedde))
* **install:** classify sidecar dispatch capability ([3b09a32](https://github.com/loonghao/dcc-mcp-core/commit/3b09a3233e9967ac51a127c11530660130d452d1))
* **install:** expose sidecar dispatch contract ([c8def96](https://github.com/loonghao/dcc-mcp-core/commit/c8def96b232d01c1d728882bd6fe3e94d32220d4))
* **server:** publish gateway guardian metadata ([4b53056](https://github.com/loonghao/dcc-mcp-core/commit/4b53056d62b3ad4e13a60be87dd7d6d43fbdd3a7))
* **sidecar:** add launch readiness contract ([e227eae](https://github.com/loonghao/dcc-mcp-core/commit/e227eaeb5cf949755587642e4af26be551be620c))
* **sidecar:** reconnect delayed host rpc ([9039c25](https://github.com/loonghao/dcc-mcp-core/commit/9039c2581f0277668a9df322de07db592667956f))
* **sidecar:** surface gateway guardian mode ([33f871a](https://github.com/loonghao/dcc-mcp-core/commit/33f871a46ed1228c874669c99ccf0e49882bea3d))


### Bug Fixes

* **admin:** expose sidecar dispatch state ([1df91e5](https://github.com/loonghao/dcc-mcp-core/commit/1df91e5db566f2ad1156dc96331ae9d4e57d9d23))
* **gateway:** evict host-died instances immediately ([8b577e0](https://github.com/loonghao/dcc-mcp-core/commit/8b577e0fa70e7b48b200f478162d6a4a58daa26d))
* **gateway:** ignore stale rows in fingerprints ([193e5ab](https://github.com/loonghao/dcc-mcp-core/commit/193e5abafde44fae449c5859ea75befbebccfe02))
* **gateway:** reject ambiguous instance resource prefixes ([450e672](https://github.com/loonghao/dcc-mcp-core/commit/450e672f9d4c526c2b158450ecaa4392c81de85c))
* **gateway:** summarize daemon recovery readiness ([6170652](https://github.com/loonghao/dcc-mcp-core/commit/6170652a340228f6181ffce1afe721d98efeb872))
* **host-rpc:** surface Maya sidecar bootstrap failures ([90b195d](https://github.com/loonghao/dcc-mcp-core/commit/90b195d6cfcc686cac98a3a0d0c81604a1c5b8bb))
* **install:** validate sidecar host rpc uri ([dadaecc](https://github.com/loonghao/dcc-mcp-core/commit/dadaecc11b0c624c5749c6a3f35156381cdd561f))
* **python:** add sidecar readiness verdict ([712a02b](https://github.com/loonghao/dcc-mcp-core/commit/712a02b807ae5fd604d5995ed9c01049abe6fb24))
* **python:** expose sidecar dispatch readiness ([c2e2b80](https://github.com/loonghao/dcc-mcp-core/commit/c2e2b805b5a108c47987750c884bd17daa6a14fb))
* **python:** probe sidecar tool readiness ([998be07](https://github.com/loonghao/dcc-mcp-core/commit/998be077120cc045743a09b376ebb20e245ba4c6))
* **server:** expose sidecar diagnostics listener ([dfa6b69](https://github.com/loonghao/dcc-mcp-core/commit/dfa6b6916d6332d61c73c1a35cdf08cf80b77508))
* **server:** expose sidecar dispatch readiness ([b4286f4](https://github.com/loonghao/dcc-mcp-core/commit/b4286f4ff3dc9319e837518ff0566cb3b2b77191))
* **server:** guard auto gateway daemon ([cc8d5a8](https://github.com/loonghao/dcc-mcp-core/commit/cc8d5a895196363569b0cad952346949f5c3a5e1))
* **server:** guard translate gateway daemon ([39a9d74](https://github.com/loonghao/dcc-mcp-core/commit/39a9d74ba93c300985657af62034a512f21c6b95))
* **sidecar:** keep stub host rpc non-routable ([1211c77](https://github.com/loonghao/dcc-mcp-core/commit/1211c77f4f1ea9d98f17da897077a8a1bf8b7bbe))
* **sidecar:** make launch readiness explicit ([8bacd85](https://github.com/loonghao/dcc-mcp-core/commit/8bacd857ccd5bb9f99ddd838b249ddc44ce88371))
* **sidecar:** reject ambiguous readiness selectors ([3b24bc5](https://github.com/loonghao/dcc-mcp-core/commit/3b24bc5ef4746efc9d723967001f7ebdd0f76ffb))
* **sidecar:** respect legacy gateway mode ([fa1d798](https://github.com/loonghao/dcc-mcp-core/commit/fa1d7986ce70a81dbc83dc93f1966c877e838636))


### Code Refactoring

* **sidecar:** split gateway daemon modules ([7397539](https://github.com/loonghao/dcc-mcp-core/commit/73975399febd3fed12317789d11117ed49ece1e2))
* **sidecar:** split MCP listener modules ([d81a0fa](https://github.com/loonghao/dcc-mcp-core/commit/d81a0faf21c01e3d76916913d691693d538bce42))
* **sidecar:** split runtime modules ([0bc7f4b](https://github.com/loonghao/dcc-mcp-core/commit/0bc7f4bc87125a5ca5d7279cf894cb3e02193f70))
* **sidecar:** split sidecar runtime crate ([5ca4311](https://github.com/loonghao/dcc-mcp-core/commit/5ca431181bd2c110501d0e669491b97c0aeb82e8))


### Documentation

* **gateway:** clarify daemon-backed auto mode ([79ae717](https://github.com/loonghao/dcc-mcp-core/commit/79ae717eda17172d01a3e89e3be0cbf7ae5b23f5))

## [0.17.50](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.49...v0.17.50) (2026-06-01)


### Features

* add import-light sidecar launch helper ([ca89a05](https://github.com/loonghao/dcc-mcp-core/commit/ca89a05be4705b764656113fe636010dadf0c16b))
* **gateway:** expose progressive tool-group active state in capability search ([f9a1dc9](https://github.com/loonghao/dcc-mcp-core/commit/f9a1dc981b6b6f468d5b183a07e4078c282cec75))
* **gateway:** support calls[] batch on REST /v1/call ([d38b615](https://github.com/loonghao/dcc-mcp-core/commit/d38b61537b620a2797a3dbc4286f47390837eada))
* **server:** ensure Python adapters start gateway daemon ([55b8704](https://github.com/loonghao/dcc-mcp-core/commit/55b8704e2806252c4d90dba24e9319d7bc4f2490))
* **server:** ensure standalone gateway by default ([e953f60](https://github.com/loonghao/dcc-mcp-core/commit/e953f6088daea346f6a4d49dcd65e5b822e6272c))


### Bug Fixes

* **gateway:** address review feedback on progressive group search ([22b5910](https://github.com/loonghao/dcc-mcp-core/commit/22b5910bb2e1fd3f3fc751847f6c3150311c0b55))
* **mcp:** simplify exposed input schemas ([095c548](https://github.com/loonghao/dcc-mcp-core/commit/095c5482067de4d4d0bb7543c614ba0a905e7ba8))
* **server:** guard sidecar gateway daemon ([6b4b125](https://github.com/loonghao/dcc-mcp-core/commit/6b4b12532af616fc7fb6b1aa9f29baf95a91e77f))
* **server:** recover stale gateway launch locks ([f7c07a2](https://github.com/loonghao/dcc-mcp-core/commit/f7c07a22750dea108c6e7a45d4c9063ddee3541d))
* **server:** report daemon gateway diagnostic modes ([bdc2024](https://github.com/loonghao/dcc-mcp-core/commit/bdc2024d5c671c7b6a16f88b5ab2d211a1efc10b))
* **server:** restart gateway daemon from Python guardian ([#1439](https://github.com/loonghao/dcc-mcp-core/issues/1439)) ([cfc7e8c](https://github.com/loonghao/dcc-mcp-core/commit/cfc7e8c5a9624b1a55cba55bd08e3ea5bacd2445))
* **skill-rest:** generate inputSchema from Python signatures in describe for unloaded skills ([dbbaf0e](https://github.com/loonghao/dcc-mcp-core/commit/dbbaf0e6f87cd25f187fc42040163dd89cbf1595))


### Documentation

* add DCC MCP brand logo ([8e3045f](https://github.com/loonghao/dcc-mcp-core/commit/8e3045f666d9667fd9b93180c738014df3d137bf))
* add skill persistence, semantic search, lifecycle hooks, and agent memory documentation ([12e1524](https://github.com/loonghao/dcc-mcp-core/commit/12e152495ddb18fc3ae0c881f7614a7943447af5))
* **server:** document daemon-backed gateway default mode ([f496d51](https://github.com/loonghao/dcc-mcp-core/commit/f496d5166ce79af0e14c463c673d1f87cafaaae3))

## [0.17.49](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.48...v0.17.49) (2026-05-31)


### Features

* **ci:** add macOS cp37 wheel via Rosetta 2 cross-compilation ([6a14902](https://github.com/loonghao/dcc-mcp-core/commit/6a14902c09e3f3bc23283ac592f6c460b3978256))

## [0.17.48](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.47...v0.17.48) (2026-05-31)


### Documentation

* add lifecycle hooks, agent memory, built-in skills, and readiness documentation ([#1427](https://github.com/loonghao/dcc-mcp-core/issues/1427)) ([eee3f35](https://github.com/loonghao/dcc-mcp-core/commit/eee3f35780c5003295c0d19a81112ab2d368a3f5))
* fix dead link in Chinese skill-maintenance translation ([1019a21](https://github.com/loonghao/dcc-mcp-core/commit/1019a2169b328222d4b413156c9020be6d3906bd))
* update sidebar navigation and add missing Chinese translations ([4632f9e](https://github.com/loonghao/dcc-mcp-core/commit/4632f9ebde6c0c111d9134364b7eb8fdf72873e4))

## [0.17.47](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.46...v0.17.47) (2026-05-30)


### Bug Fixes

* **semantic:** switch fastembed to rustls and ship aarch64-only macOS wheel ([a60cff1](https://github.com/loonghao/dcc-mcp-core/commit/a60cff10962919341794b40909dc2cea8e6f3c15))

## [0.17.46](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.45...v0.17.46) (2026-05-30)


### Bug Fixes

* **ci:** unblock v0.17.45 wheel builds and Rust coverage ([558648d](https://github.com/loonghao/dcc-mcp-core/commit/558648d6377abeda6f30b00c2e889b5c69e21592))
* **semantic:** align embedder with pyo3 0.28 and fastembed 5 APIs ([1813e2d](https://github.com/loonghao/dcc-mcp-core/commit/1813e2ddc06a52fc4777984d696f7d29f38c331c))

## [0.17.45](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.44...v0.17.45) (2026-05-30)


### Bug Fixes

* **ci:** override maturin docker entrypoint and bump vx to 0.9.11 ([a5c203c](https://github.com/loonghao/dcc-mcp-core/commit/a5c203cd91e46efcc785528080afdffce6521ad6))

## [0.17.44](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.43...v0.17.44) (2026-05-30)


### Bug Fixes

* **ci:** install openssl-devel in manylinux semantic wheel build ([7526c01](https://github.com/loonghao/dcc-mcp-core/commit/7526c019a381a3a96dab1ec13de2ae7e4a6f8cb3))
* **ci:** install openssl-devel in manylinux semantic wheel build ([7526c01](https://github.com/loonghao/dcc-mcp-core/commit/7526c019a381a3a96dab1ec13de2ae7e4a6f8cb3))
* **ci:** install openssl-devel in manylinux semantic wheel build ([5ae67e0](https://github.com/loonghao/dcc-mcp-core/commit/5ae67e0b9aa8c8d283f088f91f1045b70ac2893f))

## [0.17.43](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.42...v0.17.43) (2026-05-29)


### Bug Fixes

* **skills:** pass skills to register_recipes_tools in builtin registration ([05c79c2](https://github.com/loonghao/dcc-mcp-core/commit/05c79c2379bf6381a3121c67fad60f4f85784cdd))

## [0.17.42](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.41...v0.17.42) (2026-05-29)


### Bug Fixes

* **ci:** add execute permission to build_manylinux_semantic_wheel.sh ([245be73](https://github.com/loonghao/dcc-mcp-core/commit/245be7390ea7595c70ebb197bb3cbbd8806fadef))

## [0.17.41](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.40...v0.17.41) (2026-05-29)


### Features

* **admin-ui:** default to dark scheme, unify accent colors, add responsive nav ([97a518d](https://github.com/loonghao/dcc-mcp-core/commit/97a518d279e3e8bf579d8f156670da257d4ee72f))
* **admin-ui:** equal-height skill cards + light/dark theme switcher ([a93e6db](https://github.com/loonghao/dcc-mcp-core/commit/a93e6db01adad73e532dcf36c2e4c617acdf1b05))
* **admin-ui:** TokenTracker-style redesign + richer stats/workflow/task metrics ([f8922a0](https://github.com/loonghao/dcc-mcp-core/commit/f8922a08826b9e13be45976f65e543a4b39d00eb))


### Bug Fixes

* **admin:** green CI rust tests + split styles.css under size cap ([696a1ab](https://github.com/loonghao/dcc-mcp-core/commit/696a1ab649d0818cb99e570c2c186266ca92bcbf))

## [0.17.40](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.39...v0.17.40) (2026-05-29)


### Features

* **skill-rest:** surface next-tools at describe-time ([c204901](https://github.com/loonghao/dcc-mcp-core/commit/c2049016d5422ce2a536dee631e366dcdb18e14f)), closes [#1408](https://github.com/loonghao/dcc-mcp-core/issues/1408)

## [0.17.39](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.38...v0.17.39) (2026-05-28)


### Features

* **admin-ui:** marketplace skill cards with branding, links, and modal drill-down ([2da992a](https://github.com/loonghao/dcc-mcp-core/commit/2da992a9a8b8bcf6b017b31bdddb5a2354ea5066))
* **server:** pick up admin-UI-added skill paths without restart ([#1400](https://github.com/loonghao/dcc-mcp-core/issues/1400)) ([e83b9e0](https://github.com/loonghao/dcc-mcp-core/commit/e83b9e0416df997f2408f011c5ed580849b305b7))
* **skills:** de-prioritise infrastructure / example layers in search ([#1398](https://github.com/loonghao/dcc-mcp-core/issues/1398)) ([a8918de](https://github.com/loonghao/dcc-mcp-core/commit/a8918de3b6eb14f9c3cd6bcb1ed722d10c09623d))
* **skills:** persist loaded skills + active groups across restarts ([#1405](https://github.com/loonghao/dcc-mcp-core/issues/1405)) ([51a1ce0](https://github.com/loonghao/dcc-mcp-core/commit/51a1ce0f63ed33c24b0de57184addfe5a5bff362))
* **skills:** rank user-curated skill paths above bundled material ([#1403](https://github.com/loonghao/dcc-mcp-core/issues/1403)) ([a87bad2](https://github.com/loonghao/dcc-mcp-core/commit/a87bad2513989a572de3c67c17fbdb54cae328a8))


### Bug Fixes

* **models:** add branding/links/example_prompts to PyO3 constructor and regenerate stubs ([162e4a2](https://github.com/loonghao/dcc-mcp-core/commit/162e4a2f2e0d62f8d3160a3c8dc446c87c2e549a))
* **tests:** remaining exact-name queries for layer=example bypass ([dc03795](https://github.com/loonghao/dcc-mcp-core/commit/dc037951ac2b95042d078c8d9ee7febfda2c273c))
* **tests:** use exact skill names to bypass layer=example exclusion ([ed27c7a](https://github.com/loonghao/dcc-mcp-core/commit/ed27c7a13c24d90a61527133d0adb6b493ec23fa))


### Code Refactoring

* **skills:** drop example skills, lower thin-harness rank ([#1398](https://github.com/loonghao/dcc-mcp-core/issues/1398)) ([abeb0f7](https://github.com/loonghao/dcc-mcp-core/commit/abeb0f7120b9864dc05c4774ea9759c1fd7ca1af))

## [0.17.38](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.37...v0.17.38) (2026-05-28)


### Features

* add HTTP instance registration ([34c0181](https://github.com/loonghao/dcc-mcp-core/commit/34c0181d5fcdd863a8d1c3180911737fff47c15e)), closes [#1361](https://github.com/loonghao/dcc-mcp-core/issues/1361)
* add optional mDNS instance discovery ([4b26827](https://github.com/loonghao/dcc-mcp-core/commit/4b2682717385195140a8f53656d92dfd2be7f22d))
* add relay-backed gateway discovery ([#1385](https://github.com/loonghao/dcc-mcp-core/issues/1385)) ([8b639ce](https://github.com/loonghao/dcc-mcp-core/commit/8b639cec5f73bf0a9990a3991a5890e4f2aa8c74))
* capability graph skeleton for cross-skill agent reasoning ([#1336](https://github.com/loonghao/dcc-mcp-core/issues/1336)) ([5696533](https://github.com/loonghao/dcc-mcp-core/commit/5696533f747b195e9a561b094b3011a59015601a))
* demote scripting tools to explicit escape hatches ([#1325](https://github.com/loonghao/dcc-mcp-core/issues/1325)) ([269e32c](https://github.com/loonghao/dcc-mcp-core/commit/269e32c14a6a0eee1fe26950d528bb616c0a2c43))
* **diagnostics:** add dcc_diagnostics__gateway_failover tool ([#1355](https://github.com/loonghao/dcc-mcp-core/issues/1355)) ([e1d76ec](https://github.com/loonghao/dcc-mcp-core/commit/e1d76ec3fcdf5606b77f24131c7b4618667b715c))
* dispatch lifecycle hooks from public APIs ([13d2491](https://github.com/loonghao/dcc-mcp-core/commit/13d24912d49329e885ef3a554eff2fae73ba273e)), closes [#1337](https://github.com/loonghao/dcc-mcp-core/issues/1337)
* **gateway:** enforce bearer-token auth and per-token DCC scope on registration plane ([bfc2812](https://github.com/loonghao/dcc-mcp-core/commit/bfc2812e6592cac6bc37076e732e33c39f39b0bd))
* **gateway:** surface host execution readiness in admin and selection ([#1331](https://github.com/loonghao/dcc-mcp-core/issues/1331)) ([0d41d86](https://github.com/loonghao/dcc-mcp-core/commit/0d41d866797957121a12969659a8cb0714047eb9))
* inject bounded agent memory summaries ([bf72891](https://github.com/loonghao/dcc-mcp-core/commit/bf728917e04322e191d472284685741e850ce057)), closes [#1334](https://github.com/loonghao/dcc-mcp-core/issues/1334)
* **models:** standardize skill metadata for intelligent recall ([#1335](https://github.com/loonghao/dcc-mcp-core/issues/1335)) ([ef2cc75](https://github.com/loonghao/dcc-mcp-core/commit/ef2cc75dc907e6eabee9fdd79f51e36b858bba90))
* **qt-ui-inspector:** expose as default capability via register_qt_ui_inspector ([#1332](https://github.com/loonghao/dcc-mcp-core/issues/1332)) ([9757b08](https://github.com/loonghao/dcc-mcp-core/commit/9757b08f739b3b7716486d04ec3dcd7ce0b75f44))
* semantic skill index with BM25 + RRF fusion ([#1333](https://github.com/loonghao/dcc-mcp-core/issues/1333)) ([007b712](https://github.com/loonghao/dcc-mcp-core/commit/007b712f40bcd1282469a45cdf4fd7a140b71f95))
* **semantic:** add VectorSkillIndex with zero-dep default + optional ONNX extra ([#1393](https://github.com/loonghao/dcc-mcp-core/issues/1393)) ([0e67740](https://github.com/loonghao/dcc-mcp-core/commit/0e677407f1d135d2c30010da517f1aae35f27738))
* **semantic:** wire OnnxEmbedder to fastembed with env-var model loading ([#1393](https://github.com/loonghao/dcc-mcp-core/issues/1393)) ([24c03c9](https://github.com/loonghao/dcc-mcp-core/commit/24c03c9216cc39e188162dd1f720a9d5235f2be2))
* **server:** incorporate default built-in skills and unify registration ([#1332](https://github.com/loonghao/dcc-mcp-core/issues/1332))\n\nPromotes infrastructure tools to first-class built-ins registered\nautomatically by DccServerBase.\n\n* New module dcc_mcp_core.skills.builtin: centralizes registration\n  of diagnostics, introspect, feedback, recipes, and qt-ui-inspector.\n* DccServerBase registers these in-process tools during __init__,\n  ensuring they are always available and highly efficient.\n* qt-ui-inspector moved to dcc_mcp_core.skills and added as a\n  bundled SKILL.md package for discovery.\n* dcc_server.py diagnostic tools unified under dcc_diagnostics__ prefix.\n* Redundant registration logic removed from ServerRuntimeController.\n* Deduplication handled via idempotent ToolRegistry and path filtering\n  in collect_skill_search_paths. ([6de7317](https://github.com/loonghao/dcc-mcp-core/commit/6de7317c9de0dd211f6ee5ff4939c65660581128))
* **server:** typed lifecycle-hook framework for DCC adapters ([#1337](https://github.com/loonghao/dcc-mcp-core/issues/1337)) ([3ebbb00](https://github.com/loonghao/dcc-mcp-core/commit/3ebbb0090771d0de56e4c4d70a1bd23d1cb2bdf5))
* **skill:** qt-ui-inspector skill for DCC-agnostic Qt introspection ([#1332](https://github.com/loonghao/dcc-mcp-core/issues/1332)) ([c5e54a6](https://github.com/loonghao/dcc-mcp-core/commit/c5e54a6345856e81ee56951ef2547d5de07c10ae))
* three-tier agent memory layers for DCC adapters ([#1334](https://github.com/loonghao/dcc-mcp-core/issues/1334)) ([81230a4](https://github.com/loonghao/dcc-mcp-core/commit/81230a4d02d53aedaf39ee4c099db87e3e9b59b3))
* unify gateway instance source metadata ([#1386](https://github.com/loonghao/dcc-mcp-core/issues/1386)) ([3a910fe](https://github.com/loonghao/dcc-mcp-core/commit/3a910fe0a700e7a30c3c1da6c715b57a9b40bffb))


### Bug Fixes

* change default standalone registry DCC type from generic to python ([db3a6bd](https://github.com/loonghao/dcc-mcp-core/commit/db3a6bd9d4b780d93702bb4e4a0e655fb466d927))
* **ci:** unblock lint, stub-gen, and wheel-build for the qt-ui-inspector stack ([#1332](https://github.com/loonghao/dcc-mcp-core/issues/1332)) ([b5f4ffd](https://github.com/loonghao/dcc-mcp-core/commit/b5f4ffdd47925f83b6da663cbe18427ec2bae368))
* **compat:** backport typing.Protocol for Python 3.7 wheel parity ([86f951f](https://github.com/loonghao/dcc-mcp-core/commit/86f951fc67ade6936ddf46582e33959edfedc6f5))
* honor no-log-file in idle smoke ([3cbfd1e](https://github.com/loonghao/dcc-mcp-core/commit/3cbfd1ea5f82e02002b932a70d0c99237849616b)), closes [#1354](https://github.com/loonghao/dcc-mcp-core/issues/1354)


### Code Refactoring

* add explicit server run modes ([9041fee](https://github.com/loonghao/dcc-mcp-core/commit/9041feedac26ee968de4ace8ea1e3ded6f18d995)), closes [#1360](https://github.com/loonghao/dcc-mcp-core/issues/1360)
* **gateway-core:** sink namespace pure helpers ([#1368](https://github.com/loonghao/dcc-mcp-core/issues/1368)) ([206860d](https://github.com/loonghao/dcc-mcp-core/commit/206860d35af7735e24f9c07a9ee6c11338dabc82))
* **gateway:** drop `gateway::namespace` facade ([#1368](https://github.com/loonghao/dcc-mcp-core/issues/1368)) ([94f6959](https://github.com/loonghao/dcc-mcp-core/commit/94f69592e3f8f07eb83275c10da610555dffafa8))
* **gateway:** formalise standalone gateway daemon mode ([#1358](https://github.com/loonghao/dcc-mcp-core/issues/1358)) ([84000f2](https://github.com/loonghao/dcc-mcp-core/commit/84000f204a79c544fb3b31a0dc082d44ed0cd12d))
* **http:** gate auto-gateway bootstrap behind a cargo feature ([#1357](https://github.com/loonghao/dcc-mcp-core/issues/1357)) ([0545077](https://github.com/loonghao/dcc-mcp-core/commit/05450775ee96f3d588159c72c706e1f7eacd1851))
* **server:** add Cargo features for composition binary ([#1359](https://github.com/loonghao/dcc-mcp-core/issues/1359)) ([c05b7c7](https://github.com/loonghao/dcc-mcp-core/commit/c05b7c7422345dd239297ae3e88e4bb38ddd0287))
* split admin ui structure ([ac39390](https://github.com/loonghao/dcc-mcp-core/commit/ac393901437d126be5216cb6f34078b26eb65426))


### Documentation

* add migration guide and topology recipes ([#1366](https://github.com/loonghao/dcc-mcp-core/issues/1366)) ([169a44b](https://github.com/loonghao/dcc-mcp-core/commit/169a44bab8f127a1dc97dee77ba62d469fc24ac8))
* **agents:** note OnnxEmbedder env-var overrides in decision table ([f265eb2](https://github.com/loonghao/dcc-mcp-core/commit/f265eb23f1fa8fbe17f632270b888bf9dd4a226e))
* **gateway:** use dash bullets to match file style ([#1358](https://github.com/loonghao/dcc-mcp-core/issues/1358)) ([36a03a3](https://github.com/loonghao/dcc-mcp-core/commit/36a03a3f82738ef1a22f0f3ba71123f30084be09))

## [0.17.37](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.36...v0.17.37) (2026-05-27)


### Features

* add admin skill adoption health ([b1ef40d](https://github.com/loonghao/dcc-mcp-core/commit/b1ef40df7e57e2b22ad4ac14e01b5d6b6599a496))
* add admin workflow graph detail ([1dd4b6c](https://github.com/loonghao/dcc-mcp-core/commit/1dd4b6c6c53193ea616c9eaaea7e0ccafa7a6bb7))
* group admin tasks by outcome ([0fe71dd](https://github.com/loonghao/dcc-mcp-core/commit/0fe71dd80440c991d109332aaa2c79be27f5e5cd))


### Bug Fixes

* clarify admin traffic capture state ([2dca8e7](https://github.com/loonghao/dcc-mcp-core/commit/2dca8e70c09ed4e62220afd108e9d8c1ea8cd080))
* populate admin client attribution ([fe07677](https://github.com/loonghao/dcc-mcp-core/commit/fe07677c2a423334dc753aaf278e0a1dd0722319))
* request JSON for CLI gateway REST calls ([21e19b5](https://github.com/loonghao/dcc-mcp-core/commit/21e19b5ccb2e586f43252bfd5300090d9adbfed5))


### Documentation

* clarify DCC discovery workflow ([9a1f819](https://github.com/loonghao/dcc-mcp-core/commit/9a1f819be7c7e8e4e6b9f049b299709227e79e4b)), closes [#1318](https://github.com/loonghao/dcc-mcp-core/issues/1318) [#1319](https://github.com/loonghao/dcc-mcp-core/issues/1319)

## [0.17.36](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.35...v0.17.36) (2026-05-26)


### Features

* add agent trace packets ([9fb0286](https://github.com/loonghao/dcc-mcp-core/commit/9fb02868d37752e06279980b5f5a002f0f9e2f6a))
* highlight slow gateway calls ([2fdd54e](https://github.com/loonghao/dcc-mcp-core/commit/2fdd54e8e0af6ff06e6b79f9fc9a229a81c37e12))
* improve skill detail rendering ([083972d](https://github.com/loonghao/dcc-mcp-core/commit/083972d44b89eae39882e2dbbb1b81c8e1e6e0f0))
* make debug endpoints compact-aware ([112a450](https://github.com/loonghao/dcc-mcp-core/commit/112a4506bc120cf6be4a0d93658f840654d22ede))


### Bug Fixes

* add public-safe admin issue reports ([c67b3f6](https://github.com/loonghao/dcc-mcp-core/commit/c67b3f6351973c767c6775229005b3f2ad66f545))
* align setup ide card actions ([f92bd0d](https://github.com/loonghao/dcc-mcp-core/commit/f92bd0d0202fb002c5be4856f52c04335f2639a0))
* clarify admin token accounting ([94567c0](https://github.com/loonghao/dcc-mcp-core/commit/94567c0e44fad477bbffa5285830b7f6624f0420))

## [0.17.35](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.34...v0.17.35) (2026-05-26)


### Features

* add admin attribution filters ([39f32e2](https://github.com/loonghao/dcc-mcp-core/commit/39f32e2b6f67485b5487076983fe85148b5804b3))
* add attribution trust guardrails ([6fd6970](https://github.com/loonghao/dcc-mcp-core/commit/6fd6970c957a840de6af2edf3bccb9953fe94cbe))
* add host pump controller ([ac49b3e](https://github.com/loonghao/dcc-mcp-core/commit/ac49b3e99317fbbe452f7f12f3f8d748167094a9))
* add sidecar action dispatcher ([9ee9d9f](https://github.com/loonghao/dcc-mcp-core/commit/9ee9d9fa7e1e38cea3c263876b882de9e8045354))
* add skill file helper APIs ([d17f8e8](https://github.com/loonghao/dcc-mcp-core/commit/d17f8e8312bf77bb227d929def6f9fab8a29e94f))
* add skill http helpers ([b01890c](https://github.com/loonghao/dcc-mcp-core/commit/b01890c9f9c865ea30cbe544c5bc4f610fc1b96f))
* add skills helper namespace ([4263a72](https://github.com/loonghao/dcc-mcp-core/commit/4263a7263c6983d59d4ebb8749cbe723df6fdf1a))
* define caller attribution schema ([f84211c](https://github.com/loonghao/dcc-mcp-core/commit/f84211cf8aca07101a438a841923ed4e7e897289))
* expand skill data codec helpers ([b39161b](https://github.com/loonghao/dcc-mcp-core/commit/b39161bb0bb91813c38b42a21e9c2343cac03f09))
* improve admin observability ui ([570c77f](https://github.com/loonghao/dcc-mcp-core/commit/570c77fc7415c7ec9797c9c52b32e3147dd9cf47))
* promote skill helper adoption ([5511774](https://github.com/loonghao/dcc-mcp-core/commit/5511774e3c22873fa834adf69ad1eeb26c7aab51))
* propagate caller attribution ([1b81092](https://github.com/loonghao/dcc-mcp-core/commit/1b8109279ff7f27eb4d6b34fdc308a7b4268cdfe))
* publish qt dispatcher api ([4493e7f](https://github.com/loonghao/dcc-mcp-core/commit/4493e7f3c1244c62719f0bc2f26c205c57ad19ee))


### Bug Fixes

* encode async main-thread dispatch output ([d13f2da](https://github.com/loonghao/dcc-mcp-core/commit/d13f2da94353c5374996596e0438afb2a88b601d))
* keep host pump aliases python37-compatible ([7b2a12c](https://github.com/loonghao/dcc-mcp-core/commit/7b2a12c09e6b4c51e6806e465f80940048e0afae))
* keep skill result fallback source-only ([4e12911](https://github.com/loonghao/dcc-mcp-core/commit/4e129118a42c81b616406f4f4084ea47290621aa))


### Code Refactoring

* add host ui dispatcher hooks ([102a3c8](https://github.com/loonghao/dcc-mcp-core/commit/102a3c858f510a2f1d0ef84e08c1688d14a2f2b5))
* split gateway admin modules ([9a24360](https://github.com/loonghao/dcc-mcp-core/commit/9a2436009695146a79e12177de1a0d95a5e5cf30))


### Documentation

* add dispatcher migration fixtures ([7fefa0b](https://github.com/loonghao/dcc-mcp-core/commit/7fefa0bc00a40484cb8def1a2211277a0a3034ac))

## [0.17.34](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.33...v0.17.34) (2026-05-25)


### Features

* add admin ui i18n runtime ([350b3e5](https://github.com/loonghao/dcc-mcp-core/commit/350b3e5595c4dd42b3edb8f95b451b4d8ec1dee2))
* add metadata-driven registration helper ([aedaced](https://github.com/loonghao/dcc-mcp-core/commit/aedaced8a47801e2b2ec52f8c05690dec43c2817))
* localize admin ui copy ([5db7fa9](https://github.com/loonghao/dcc-mcp-core/commit/5db7fa980204e4b214b87fb8d7a285f41a128b42))
* namespace admin ui translations ([a1b2937](https://github.com/loonghao/dcc-mcp-core/commit/a1b2937cf875f43775ee68bcc635fcc2ea7996ef))


### Bug Fixes

* derive prompts from skill metadata ([73bc467](https://github.com/loonghao/dcc-mcp-core/commit/73bc46711187d63adc2b7d86a093f4f9e3c11b53))
* support standalone main-affinity execution ([26f55ea](https://github.com/loonghao/dcc-mcp-core/commit/26f55ea30c10e0802b421b0158d5fa3a2b11d964))


### Code Refactoring

* clean up admin ui instance naming ([3c6093e](https://github.com/loonghao/dcc-mcp-core/commit/3c6093e5899bf2375e79fb0be9ca42ae63adf87f))

## [0.17.33](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.32...v0.17.33) (2026-05-25)


### Features

* add admin live traffic capture ([a4fa359](https://github.com/loonghao/dcc-mcp-core/commit/a4fa35912e5486699aca858eec3749c2e8b70da7))
* **admin:** improve debug triage telemetry ([8ffa011](https://github.com/loonghao/dcc-mcp-core/commit/8ffa011ee69125515c7dd4bb2190a3f801664ba4))


### Bug Fixes

* compact materialize script tool listing ([891d357](https://github.com/loonghao/dcc-mcp-core/commit/891d357853532125e08a8462321f81198d5e01fa))

## [0.17.32](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.31...v0.17.32) (2026-05-25)


### Features

* add adapter readiness binder ([a133696](https://github.com/loonghao/dcc-mcp-core/commit/a1336968808db7d99b221e650ffccedfe9c8f859))
* add script materialization store ([21aac24](https://github.com/loonghao/dcc-mcp-core/commit/21aac243da0f80373c6056da3ac1f5f898eb1315))
* add skill load transform hooks ([889d657](https://github.com/loonghao/dcc-mcp-core/commit/889d657e5c37f108852a89586a5b79e5ab2eadd7))
* enforce file-backed script execution ([eba909c](https://github.com/loonghao/dcc-mcp-core/commit/eba909ca9760783c0d466a000fc51173bbec00d5))
* expose materialized script agent APIs ([f287637](https://github.com/loonghao/dcc-mcp-core/commit/f2876372da2f895898c3bfbd63838bc5596af775))

## [0.17.31](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.30...v0.17.31) (2026-05-25)


### Features

* add optional runtime metadata ([63b6911](https://github.com/loonghao/dcc-mcp-core/commit/63b69110313ef97e165e0e33b42d0dbebf2001b2))
* add traffic capture replay tools ([08a938e](https://github.com/loonghao/dcc-mcp-core/commit/08a938ef4a76f0921abeb6db8d602cc109fce131))
* add unified dcc mcp creator skills ([bb3aeeb](https://github.com/loonghao/dcc-mcp-core/commit/bb3aeeb5df0f7731e4a93a1da2288cea21cd1a27))
* add usd project resource conventions ([eb7b0d2](https://github.com/loonghao/dcc-mcp-core/commit/eb7b0d22bdaa9aed09811bc7182bfb3f2265653b))
* capture bounded agent turn context ([4bc3fcb](https://github.com/loonghao/dcc-mcp-core/commit/4bc3fcbb1d10d56206c3352896e004c9d73abd4b)), closes [#1198](https://github.com/loonghao/dcc-mcp-core/issues/1198)
* default gateway REST to compact responses ([fb24c75](https://github.com/loonghao/dcc-mcp-core/commit/fb24c75ddbfeee307587de54c4ccf7cc01c93e80)), closes [#1157](https://github.com/loonghao/dcc-mcp-core/issues/1157)
* emit gateway handoff notifications ([ee1b427](https://github.com/loonghao/dcc-mcp-core/commit/ee1b427abf8c6f41d602b825a784e88ba5390d1a))
* expose server resource helpers ([7b2264c](https://github.com/loonghao/dcc-mcp-core/commit/7b2264cd6e7b2737150720040008512ceadb0364))
* record token savings telemetry ([ea72a41](https://github.com/loonghao/dcc-mcp-core/commit/ea72a41c7cec26a0bdb6b7ed778efcfeec3ea9af)), closes [#1155](https://github.com/loonghao/dcc-mcp-core/issues/1155)
* show admin token savings ([9b030b2](https://github.com/loonghao/dcc-mcp-core/commit/9b030b2e4f4c40d49592ba1fe3fe60a0827deed6)), closes [#1156](https://github.com/loonghao/dcc-mcp-core/issues/1156)


### Documentation

* clarify skill metadata layout ([b5cc3ae](https://github.com/loonghao/dcc-mcp-core/commit/b5cc3ae8015f781610c825ad8f1d4d62ad82a0ba))

## [0.17.30](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.29...v0.17.30) (2026-05-25)


### Features

* add admin traffic governance view ([89f7fb9](https://github.com/loonghao/dcc-mcp-core/commit/89f7fb963eb5c990b26e23b7c9a852c6a458bbd3))
* add admin workflow view ([77e69f2](https://github.com/loonghao/dcc-mcp-core/commit/77e69f25cf4a8e5798f4ebb422372ab8529d2afd))
* add gateway agent workflow spans ([90939b5](https://github.com/loonghao/dcc-mcp-core/commit/90939b53d81b331baeb5d652ed2603e122823e76))
* add gateway agent workflow spans ([90939b5](https://github.com/loonghao/dcc-mcp-core/commit/90939b53d81b331baeb5d652ed2603e122823e76)), closes [#1180](https://github.com/loonghao/dcc-mcp-core/issues/1180)
* add gateway openapi contract ([3188afd](https://github.com/loonghao/dcc-mcp-core/commit/3188afd4dceb1a73ede617136624cdce8e3a2e84))
* add gateway policy gating ([#1188](https://github.com/loonghao/dcc-mcp-core/issues/1188)) ([121b2dc](https://github.com/loonghao/dcc-mcp-core/commit/121b2dc2e93bb669712da996e174c6459f65ca61))
* add gateway search quality telemetry ([4f9d02c](https://github.com/loonghao/dcc-mcp-core/commit/4f9d02ccbaee7a423d31d5caa8ea809c2ec094cb)), closes [#1179](https://github.com/loonghao/dcc-mcp-core/issues/1179)
* add hybrid capability ranking explanations ([73abf8f](https://github.com/loonghao/dcc-mcp-core/commit/73abf8f583a549533988afc345238046c0ef0fb3)), closes [#1177](https://github.com/loonghao/dcc-mcp-core/issues/1177)
* add MCP compact response mode ([8b6c86c](https://github.com/loonghao/dcc-mcp-core/commit/8b6c86cce394ca13dbe107c1d08712b6cba02f2c)), closes [#1154](https://github.com/loonghao/dcc-mcp-core/issues/1154)
* index capability aliases and schema tokens ([0be6ec6](https://github.com/loonghao/dcc-mcp-core/commit/0be6ec6b993f17843c33167c85f9eda5e15d35ef))
* normalize gateway rest metadata ([#1187](https://github.com/loonghao/dcc-mcp-core/issues/1187)) ([28cf92d](https://github.com/loonghao/dcc-mcp-core/commit/28cf92d17b8bcca2a1b6b2118b4fdb328314c8f8))

## [0.17.29](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.28...v0.17.29) (2026-05-24)


### Features

* add compact search response codec ([1c1fd0b](https://github.com/loonghao/dcc-mcp-core/commit/1c1fd0b90b1fc97be1612cc34b69bcd532155343))
* expose canonical gateway MCP tools ([f38d433](https://github.com/loonghao/dcc-mcp-core/commit/f38d43393697e13ae6edef23b63e1c2650220686))
* extend compact REST responses ([be79fb4](https://github.com/loonghao/dcc-mcp-core/commit/be79fb4114d479bc13fbd985e5776eae24b7fdc5)), closes [#1153](https://github.com/loonghao/dcc-mcp-core/issues/1153)
* make skill loading self describing ([e6556c1](https://github.com/loonghao/dcc-mcp-core/commit/e6556c1f3ffd0def7c195378e733819384c50fb1))
* streamline gateway MCP surface and admin UI ([f0fc1d1](https://github.com/loonghao/dcc-mcp-core/commit/f0fc1d11fdd7882c63a5463504108abfad72fd1a))


### Bug Fixes

* update clawhub publish cli ([1b4cba6](https://github.com/loonghao/dcc-mcp-core/commit/1b4cba6ade64e951578ace9b4ce096231431bd0b))

## [0.17.28](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.27...v0.17.28) (2026-05-24)


### Features

* add mutable skill object loading ([ea5e7be](https://github.com/loonghao/dcc-mcp-core/commit/ea5e7bef7acd995afc7caeba076cbf5a1e0e2453))
* add release smoke instance targeting ([67c7385](https://github.com/loonghao/dcc-mcp-core/commit/67c7385fb468c4503d1899cd3dbbd006b4ec1ce8)), closes [#1158](https://github.com/loonghao/dcc-mcp-core/issues/1158)

## [0.17.27](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.26...v0.17.27) (2026-05-24)


### Features

* add windows uia app ui backend ([8b02c64](https://github.com/loonghao/dcc-mcp-core/commit/8b02c64b93d44fddf3c6576040242bf78568cebe))
* expose app ui gateway metadata ([d0d62ff](https://github.com/loonghao/dcc-mcp-core/commit/d0d62ffa7ce260a85a73cedaed4d6a7ac5dd42c6))
* harden app ui policy audit controls ([eda87a3](https://github.com/loonghao/dcc-mcp-core/commit/eda87a33c86f3ff26edb2a08f0c5cfe6f81edd4b))


### Bug Fixes

* align cli smoke mcp accept headers ([c6982eb](https://github.com/loonghao/dcc-mcp-core/commit/c6982eb33837a53db9ba76ea9ce0ce7021805ed8))
* keep healthy gateway residents active ([cd3a6a0](https://github.com/loonghao/dcc-mcp-core/commit/cd3a6a04d8cf55182a980dea094c86b18dcc05bc))
* preserve host failure envelopes through gateway ([3fc266b](https://github.com/loonghao/dcc-mcp-core/commit/3fc266bb6943f27df46cbdfcc29c553e6066365a)), closes [#1160](https://github.com/loonghao/dcc-mcp-core/issues/1160)


### Documentation

* add app ui workflow examples ([26baee9](https://github.com/loonghao/dcc-mcp-core/commit/26baee97a28251e0cc86e0dabc541abb2cba66c6))

## [0.17.26](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.25...v0.17.26) (2026-05-24)


### Features

* add event bus veto hooks ([a34e58c](https://github.com/loonghao/dcc-mcp-core/commit/a34e58cb11c5176313d81023498a505b3850d312))
* add event webhook delivery ([2f5d4c3](https://github.com/loonghao/dcc-mcp-core/commit/2f5d4c314ad8fb8713846f82b616f08e0e1bc673))
* add gateway traffic JSONL capture ([2f0fb56](https://github.com/loonghao/dcc-mcp-core/commit/2f0fb56e1697805183b9ae210846afa5a0ffd61d))
* add traffic capture sqlite filters ([740963c](https://github.com/loonghao/dcc-mcp-core/commit/740963c713bae08705269e356a61de13db395599))

## [0.17.25](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.24...v0.17.25) (2026-05-23)


### Features

* add app ui cdp backend presets ([4760bb7](https://github.com/loonghao/dcc-mcp-core/commit/4760bb7bd12e8ba2bf1ae6e133e7105a0b01e84c))
* add app ui contract and mock skill ([2bd442d](https://github.com/loonghao/dcc-mcp-core/commit/2bd442dbd85cb96cbf0578f93a74f4807a1e29d9))
* add edge and agent browser app ui presets ([f920bbe](https://github.com/loonghao/dcc-mcp-core/commit/f920bbe8144ca9d3d134105f33521f8d7cc4217a))


### Bug Fixes

* repair diagnostics routing and CLI load-skill ([fe878cd](https://github.com/loonghao/dcc-mcp-core/commit/fe878cda82be3b629c225bef2b1da42a5e1f19c5))


### Code Refactoring

* split app ui contracts into crate ([208abb1](https://github.com/loonghao/dcc-mcp-core/commit/208abb18b91c303910a47016877b7955f6b7a1b3))

## [0.17.24](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.23...v0.17.24) (2026-05-23)


### Features

* add event bus lifecycle hooks ([d6efc03](https://github.com/loonghao/dcc-mcp-core/commit/d6efc0361e04eb7ec015d47decf80e816292edb6))
* log server version on startup ([5593210](https://github.com/loonghao/dcc-mcp-core/commit/55932102a173ad127e7ba27d5c2584062fcde5ee))


### Bug Fixes

* correct tool lifecycle event outcomes ([3b9648b](https://github.com/loonghao/dcc-mcp-core/commit/3b9648b8439d71c359e4f3babd0aa9eaca99f6c4))
* keep soft dependency skills discoverable ([1957ad4](https://github.com/loonghao/dcc-mcp-core/commit/1957ad46c05601409845cd01db4ec9c13b8d44da))
* preserve action result context envelopes ([3d6e802](https://github.com/loonghao/dcc-mcp-core/commit/3d6e802cfe08972bd8ebbe27da00fc07a3d37828))

## [0.17.23](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.22...v0.17.23) (2026-05-23)


### Features

* add gateway lifecycle diagnostics and IDE setup ([42f93bf](https://github.com/loonghao/dcc-mcp-core/commit/42f93bff1cf83527f221762a0473f55b86109a55))
* show admin skill markdown details ([960106e](https://github.com/loonghao/dcc-mcp-core/commit/960106e1a14aec1c95648f18d421b6ccb301b969))
* update dcc skills creator templates ([c613d60](https://github.com/loonghao/dcc-mcp-core/commit/c613d60bd1a96ea1f25993068edda323b5385248))


### Bug Fixes

* address client-safe tool regressions ([63cf209](https://github.com/loonghao/dcc-mcp-core/commit/63cf209c86a6636518ca51405d81035aa5247f01))
* address gateway admin review feedback ([641c969](https://github.com/loonghao/dcc-mcp-core/commit/641c9691eda9aef7e5d8fb1d051edb1c8bc42b78))
* align admin debug docs and codex setup ([cbf5770](https://github.com/loonghao/dcc-mcp-core/commit/cbf57703f76ba0c59fd171dda62be2da1da7ef4b))
* enforce client-safe core tool names ([f0f68ec](https://github.com/loonghao/dcc-mcp-core/commit/f0f68ecfcc21b1029f99c28fbcfa3ea281866b79))
* point admin docs nav to github docs ([f519b25](https://github.com/loonghao/dcc-mcp-core/commit/f519b25c6cae4d50773c008ed2dbc7c0cca01092))
* refresh admin skill inventory ([0e5ebce](https://github.com/loonghao/dcc-mcp-core/commit/0e5ebce9d1df7dd9c8270f5e799d54dcad836966))
* show platform-specific ide config paths ([31dc525](https://github.com/loonghao/dcc-mcp-core/commit/31dc5250141a501580555ad0f92e3d76211356ad))
* stabilize gateway debug and admin time display ([2a062c7](https://github.com/loonghao/dcc-mcp-core/commit/2a062c79b9bf4d7f1af9e4a0d2236aef29d7198f))
* use gateway url in admin ide setup ([83fd5c6](https://github.com/loonghao/dcc-mcp-core/commit/83fd5c66658eed157b9180c362b16ba700ec1e0c))


### Documentation

* add admin update screenshots ([07182a3](https://github.com/loonghao/dcc-mcp-core/commit/07182a3252fc127809784e557af75735f5867bca))
* add gateway RFC proposals ([bac4ff7](https://github.com/loonghao/dcc-mcp-core/commit/bac4ff7ccd916024f6d80cfcf641ebbc057e9590))
* remove RFC source references ([b118ec3](https://github.com/loonghao/dcc-mcp-core/commit/b118ec3de3746dc5d38cab17414faeb2660b72a2))

## [0.17.22](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.21...v0.17.22) (2026-05-22)


### Features

* add import-light install lifecycle helpers ([7dbadae](https://github.com/loonghao/dcc-mcp-core/commit/7dbadaee224a57fb83e0126a6d037a15b0f9b8d7))


### Bug Fixes

* fsync FileRegistry snapshots before rename ([fd3b8d6](https://github.com/loonghao/dcc-mcp-core/commit/fd3b8d6c0e65aff7b5944963d956ea84ae35adc9))
* recover zero-padded FileRegistry snapshots ([9c2e67d](https://github.com/loonghao/dcc-mcp-core/commit/9c2e67d902f0f08aff39c358780365cfd3f7ddec))

## [0.17.21](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.20...v0.17.21) (2026-05-22)


### Features

* add gateway trace context ([0b8c771](https://github.com/loonghao/dcc-mcp-core/commit/0b8c77155eac6ee95bd92e32f66cc7559eee669b))
* expose stable gateway debug api ([96b66e1](https://github.com/loonghao/dcc-mcp-core/commit/96b66e11b375fa5e9f420e1e3b4a4c10c73eec27))


### Bug Fixes

* honor debug list limits ([dd9491f](https://github.com/loonghao/dcc-mcp-core/commit/dd9491f3b4ccfd4d178c17408e24c8d76b85866a))
* normalize debug bundle links ([1788975](https://github.com/loonghao/dcc-mcp-core/commit/1788975fc1cb00fa8568789409414abda616901e))


### Documentation

* fix debug api table lint ([3585ffb](https://github.com/loonghao/dcc-mcp-core/commit/3585ffbb6f6aa58af8bec19fe524fa240981a5ed))
* sync zh rest debug api ([b3b87a1](https://github.com/loonghao/dcc-mcp-core/commit/b3b87a1511e37f09a4ba433ebfee474f4b3efd8a))

## [0.17.20](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.19...v0.17.20) (2026-05-21)


### Bug Fixes

* **gateway:** keep health endpoints in unreachable error ([b1b4ce1](https://github.com/loonghao/dcc-mcp-core/commit/b1b4ce17e65327f26975b10953a316442c5267d5))
* harden registry write transactions ([f04305f](https://github.com/loonghao/dcc-mcp-core/commit/f04305f646bd64545eed5c766fb0e8ea078b4a1d))
* recreate registry dir before registry writes ([a56e5aa](https://github.com/loonghao/dcc-mcp-core/commit/a56e5aa53fdca8dd7a5f2981be4a01ace9f1cc53))
* reduce cooperative yield fallback log noise ([9f25084](https://github.com/loonghao/dcc-mcp-core/commit/9f25084f8e3ddd5652787c6f11b67255ed8342cc))


### Code Refactoring

* extract server config resolution ([56554a2](https://github.com/loonghao/dcc-mcp-core/commit/56554a2e91a131a5c340de4b7786fbfe1fd5cd7a))
* slim Python server facade ([9471664](https://github.com/loonghao/dcc-mcp-core/commit/947166440ef76798c93c25d41c9d99886d593301))

## [0.17.19](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.18...v0.17.19) (2026-05-20)


### Features

* add adapter runtime observation contracts ([60c6733](https://github.com/loonghao/dcc-mcp-core/commit/60c67331196f6e1f2ae53c5e1693fda840f6739e))
* add admin agent telemetry context ([0155560](https://github.com/loonghao/dcc-mcp-core/commit/0155560d74d619510f29445f6b02cd1d3668da15))
* add admin postmortem debug context ([3515295](https://github.com/loonghao/dcc-mcp-core/commit/35152956245afc12ad4e2171dcea6a4608cc3e84))
* add admin trace share links ([9a3c365](https://github.com/loonghao/dcc-mcp-core/commit/9a3c365099e3ba2e0e934c05047da45c89514c9c))
* add dcc skill developer guidance ([10f37a9](https://github.com/loonghao/dcc-mcp-core/commit/10f37a97fdc6322130dd7aeff7cb82aa452353c0))
* add openapi inspector dashboard panel ([cbd4361](https://github.com/loonghao/dcc-mcp-core/commit/cbd4361046fbf1325c64164be6e93bc855ab7036))
* add per-instance openapi dashboard links ([88033d8](https://github.com/loonghao/dcc-mcp-core/commit/88033d8a33c31b60dd892c410e34e1529f27b698))
* export admin issue report json ([099eea9](https://github.com/loonghao/dcc-mcp-core/commit/099eea99d18173fa8ec94ebd767560fabe2369ce))


### Bug Fixes

* address dcc skill guidance review ([f58d7aa](https://github.com/loonghao/dcc-mcp-core/commit/f58d7aa36194ff4e6e088822e4cfcd5ae20c4aa3))
* keep skill helpers importable without core ([457b441](https://github.com/loonghao/dcc-mcp-core/commit/457b441c849cf4c0d717b4692e2ae102e2bfdf98))
* prefer core json dumps in skill fallback ([f59e346](https://github.com/loonghao/dcc-mcp-core/commit/f59e346ed35a7dca4945e61649f0d48550aaa6b2))
* respect middleware mutations in admin traces ([e7adc38](https://github.com/loonghao/dcc-mcp-core/commit/e7adc383ecec976d3a13e735c68ea7363efdb01f))


### Documentation

* add skill developer sync guidance ([f085a48](https://github.com/loonghao/dcc-mcp-core/commit/f085a48b16cdaddabdf06558cedb55168aebbeb1))

## [0.17.18](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.17...v0.17.18) (2026-05-20)


### Bug Fixes

* address PR review for affinity and gateway diagnostics ([29fb2bc](https://github.com/loonghao/dcc-mcp-core/commit/29fb2bc6d33c8938025c00042e33b3d915d0a22b))
* improve thread-affinity and gateway backend diagnostics ([#1075](https://github.com/loonghao/dcc-mcp-core/issues/1075), [#1076](https://github.com/loonghao/dcc-mcp-core/issues/1076)) ([bbebae6](https://github.com/loonghao/dcc-mcp-core/commit/bbebae627f2b3bbe25db47302f97b713e1b413ec))

## [0.17.17](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.16...v0.17.17) (2026-05-19)


### Bug Fixes

* clarify gateway mcp auth hints ([5b94b4b](https://github.com/loonghao/dcc-mcp-core/commit/5b94b4b189a59b20ad5341e5737e4734b4a7113e))

## [0.17.16](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.15...v0.17.16) (2026-05-19)


### Features

* add standalone gateway diagnostics ([508e043](https://github.com/loonghao/dcc-mcp-core/commit/508e043823d0f153f5f39306da4c8f15202c170a))
* add websocket host rpc client ([cebd688](https://github.com/loonghao/dcc-mcp-core/commit/cebd68890c32796df2a15188359fbb41da90d6a1))

## [0.17.15](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.14...v0.17.15) (2026-05-19)


### Bug Fixes

* **ci:** prebuild admin ui before release binaries ([4887c75](https://github.com/loonghao/dcc-mcp-core/commit/4887c754d479d6501a7a951c601cbb58ef21be4b))

## [0.17.14](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.13...v0.17.14) (2026-05-19)


### Bug Fixes

* **ci:** disable vx cache in wheel action ([276ca42](https://github.com/loonghao/dcc-mcp-core/commit/276ca420ab437d83048ce6c7f733da195653ee03))
* **ci:** restore manylinux wheel ownership ([0a1a001](https://github.com/loonghao/dcc-mcp-core/commit/0a1a00139634fb58ae1139e32b814e75c025948a))

## [0.17.13](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.12...v0.17.13) (2026-05-19)


### Bug Fixes

* **ci:** extract release wheel scripts ([893df3a](https://github.com/loonghao/dcc-mcp-core/commit/893df3a74c6e23600c104c6a67b75d3387b1b023))

## [0.17.12](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.11...v0.17.12) (2026-05-19)


### Bug Fixes

* **ci:** prebuild admin ui for manylinux release ([cc0df46](https://github.com/loonghao/dcc-mcp-core/commit/cc0df46aa9fa0bf4f911cf1f02cb30ef20a1a33b))

## [0.17.11](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.10...v0.17.11) (2026-05-19)


### Bug Fixes

* fall back to release with cli asset ([d877a7b](https://github.com/loonghao/dcc-mcp-core/commit/d877a7b51f1ef7f22a251df21db5c1436b022030))
* isolate manylinux release target dir ([47583b6](https://github.com/loonghao/dcc-mcp-core/commit/47583b65b987d17aca2d8ada849f53c1824d71b5))

## [0.17.10](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.9...v0.17.10) (2026-05-19)


### Features

* add admin activity ledger ([d327e15](https://github.com/loonghao/dcc-mcp-core/commit/d327e1546b56fadfc19c5681a3b7cad0578634bc))


### Bug Fixes

* align async host busy handling ([5780fa0](https://github.com/loonghao/dcc-mcp-core/commit/5780fa074b116f137750e4c10a8fa50b477d7ea9))
* build server linux wheel for manylinux2014 ([35560fc](https://github.com/loonghao/dcc-mcp-core/commit/35560fc49294429dfa1cdd7053f7378c31c897b1))
* **http-server:** block REST dispatch on host-bridge runtime from OS thread ([c1f34cd](https://github.com/loonghao/dcc-mcp-core/commit/c1f34cd1c0dd885a1adfd6eb406a22199a4a4370))
* **http-server:** preserve REST dispatch errors and validation_skipped ([004d781](https://github.com/loonghao/dcc-mcp-core/commit/004d78175d2f1ef0269538d356249cf5121d8781))
* **http-server:** safe REST routing on current_thread Tokio runtime ([c10c046](https://github.com/loonghao/dcc-mcp-core/commit/c10c0469fc23159d29fe014b471f0d052a929ca0))
* **http:** keep with_executor() startable without host_bridge_runtime ([cd9631a](https://github.com/loonghao/dcc-mcp-core/commit/cd9631abf6b264357153b1097eacd54b429aaf63))
* **http:** route REST /v1/call through main-thread executor ([99085b1](https://github.com/loonghao/dcc-mcp-core/commit/99085b1bd60beb2e36550b1eb39827844722ed5a))
* reject unavailable main affinity dispatch ([b156044](https://github.com/loonghao/dcc-mcp-core/commit/b15604427e60835b943f6bfe16d750464ec612ec))


### Code Refactoring

* **http-server:** split rmcp_tool_call_dispatch for file-size limit ([5e49e9d](https://github.com/loonghao/dcc-mcp-core/commit/5e49e9d160fd87eddc060fad8ca2649e3efef396))


### Documentation

* add admin ui screenshots ([af5dd7b](https://github.com/loonghao/dcc-mcp-core/commit/af5dd7b0b90ac3c9174041388ea17a070024d8e8))
* add architecture diagram ([16bcdb3](https://github.com/loonghao/dcc-mcp-core/commit/16bcdb38b6092939ed0994ee7cefab7574b50d2e))
* improve readme onboarding flow ([691b242](https://github.com/loonghao/dcc-mcp-core/commit/691b242591d8f5f5828523cd7148a57d6a2620db))
* simplify architecture readme section ([2dbd4d1](https://github.com/loonghao/dcc-mcp-core/commit/2dbd4d1657286df152af6933988cef339c2795ad))

## [0.17.9](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.8...v0.17.9) (2026-05-18)


### Features

* add gateway smoke command ([27bbecd](https://github.com/loonghao/dcc-mcp-core/commit/27bbecd1527e0bc503056286d4e8ef28b89345a3))
* add remote gateway listener ([14e989d](https://github.com/loonghao/dcc-mcp-core/commit/14e989d6d625c3e226f251b18751b6b15c258503))
* complete rest progressive loading ([bfb8e11](https://github.com/loonghao/dcc-mcp-core/commit/bfb8e11bc3327e639f4efdf1de18ccbce8a04987))


### Bug Fixes

* harden gateway initialization ([67d11f3](https://github.com/loonghao/dcc-mcp-core/commit/67d11f3c1a3943549e89b2c2dc153e97e05be56d))
* keep unloaded capabilities instance scoped ([3126347](https://github.com/loonghao/dcc-mcp-core/commit/3126347807196fead7cbbfe79baca41014e8c358))
* publish challenger sentinel during gateway takeover ([0df3adf](https://github.com/loonghao/dcc-mcp-core/commit/0df3adf6059b6f144ac4fe0f7f51636188c7a4fa))
* retry gateway cooperative yield ([ef6fccb](https://github.com/loonghao/dcc-mcp-core/commit/ef6fccb0566af44da827c9fe224a2396f06f5434))


### Code Refactoring

* split skill rest service tests ([62f0b48](https://github.com/loonghao/dcc-mcp-core/commit/62f0b4895200bb1be04ea636204efe6259f24bcc))

## [0.17.8](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.7...v0.17.8) (2026-05-18)


### Features

* add skill lint command ([aa71cab](https://github.com/loonghao/dcc-mcp-core/commit/aa71cab6e6518ad78fee53588c1f831759ab1e93))
* **admin-ui:** improve gateway dashboard interactions ([67ce886](https://github.com/loonghao/dcc-mcp-core/commit/67ce886d8202d809ef7e61f4ea27dc7110ec41e0))


### Bug Fixes

* default affinity enforcement from declarations ([0f1d2e9](https://github.com/loonghao/dcc-mcp-core/commit/0f1d2e93cf0dad52534b921bcfcf7f6fd6f6eab4))

## [0.17.7](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.6...v0.17.7) (2026-05-17)


### Features

* add cli gateway clawhub skill ([d6446be](https://github.com/loonghao/dcc-mcp-core/commit/d6446be710fec202354585836fbadcfee5ac64ef))
* structure gateway yield fallback ([e026dca](https://github.com/loonghao/dcc-mcp-core/commit/e026dcad95487a7999d14ed4135c02d4b9ba1387))

## [0.17.6](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.5...v0.17.6) (2026-05-17)


### Bug Fixes

* retag server binary wheels as py3-none ([5d98fcc](https://github.com/loonghao/dcc-mcp-core/commit/5d98fcca4daad36d53ea593567e2fd1580cca8e3))
* surface gateway host death metadata ([6beb173](https://github.com/loonghao/dcc-mcp-core/commit/6beb17345ceafef920ae287f8bc96cb78b58271f))

## [0.17.5](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.4...v0.17.5) (2026-05-17)


### Features

* **cli:** add dcc-mcp control plane ([605ae03](https://github.com/loonghao/dcc-mcp-core/commit/605ae0371fbb88e8483c0f4a33c79f76d5ef5763))
* **cli:** add one-click install and release assets ([edc5a90](https://github.com/loonghao/dcc-mcp-core/commit/edc5a9079c48f366dd68561afb8b05d7ced0394e))
* **skills:** add dcc-rest-gateway ClawHub skill for REST-only DCC control ([8c9c39f](https://github.com/loonghao/dcc-mcp-core/commit/8c9c39ff6dc47ad84eb2ffaae81341e3d11bfd27))


### Bug Fixes

* **release:** support Python 3.7 server wheels ([6540efb](https://github.com/loonghao/dcc-mcp-core/commit/6540efb1827f3805bcb45421b30ede40d7becaf3))
* **transport:** force registry reload before ghost pruning ([6af8160](https://github.com/loonghao/dcc-mcp-core/commit/6af8160d113c0233617c381959f87585e0585001))

## [0.17.4](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.3...v0.17.4) (2026-05-17)


### Bug Fixes

* **dispatcher:** use typing aliases for Python 3.7 wheel import ([222aee3](https://github.com/loonghao/dcc-mcp-core/commit/222aee36254326a2fbd24925bb11c86b867f6287))

## [0.17.3](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.2...v0.17.3) (2026-05-17)


### Features

* **dispatcher:** add HostUiDispatcherBase and tools/list stub policy ([11a1251](https://github.com/loonghao/dcc-mcp-core/commit/11a1251b662973e966dcc156b67939b664f34c7a))
* **sandbox:** wire SandboxPolicy into in-process executor ([70bc970](https://github.com/loonghao/dcc-mcp-core/commit/70bc970731acdd5b87f86cc3ebc0bd4968e4dc26))
* **sidecar:** gateway health probe and election failover ([1b996a1](https://github.com/loonghao/dcc-mcp-core/commit/1b996a1d492919a7669ca1bc5b74488efdc14b62))


### Bug Fixes

* **test:** patch _is_port_free in election e2e to avoid TIME_WAIT flake ([4ca228a](https://github.com/loonghao/dcc-mcp-core/commit/4ca228a3685b34fb85d5d720ec7b8f9482ff2bd4))

## [0.17.2](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.1...v0.17.2) (2026-05-16)


### Features

* **commandport:** dynamic Python-module bootstrap on connect (RFC [#998](https://github.com/loonghao/dcc-mcp-core/issues/998)) ([#1008](https://github.com/loonghao/dcc-mcp-core/issues/1008)) ([8bfcca3](https://github.com/loonghao/dcc-mcp-core/commit/8bfcca3018fb6cfca2369a69a7562bca109783f2))
* **gateway:** progressive list_skills and instance-offline provenance ([88b6b5f](https://github.com/loonghao/dcc-mcp-core/commit/88b6b5fd6d7f88f6abaf1a85f35342f4b3defb8f))
* **host-rpc:** CommandPortClient + URI scheme registry, sidecar wires HostRpcClient on startup (RFC [#998](https://github.com/loonghao/dcc-mcp-core/issues/998) Phase 2) ([30411c6](https://github.com/loonghao/dcc-mcp-core/commit/30411c6b181b4c596b6b80cb0b34e80308f9f186))
* **host-rpc:** qtserver:// scheme + universal dcc_qt_dispatcher (RFC [#998](https://github.com/loonghao/dcc-mcp-core/issues/998)) ([a930e23](https://github.com/loonghao/dcc-mcp-core/commit/a930e236c574e4df51842f9c7316085176f13848))
* **sidecar:** MCP HTTP listener inside the sidecar process (RFC [#998](https://github.com/loonghao/dcc-mcp-core/issues/998) Phase 2, closes the dispatch loop) ([#1010](https://github.com/loonghao/dcc-mcp-core/issues/1010)) ([ff3dd84](https://github.com/loonghao/dcc-mcp-core/commit/ff3dd848de89d69654e4f82bb0ef5e4d8d91e489))
* **transport:** ServiceEntry::display_id() + expose on gateway://instances (RFC [#998](https://github.com/loonghao/dcc-mcp-core/issues/998)) ([2616546](https://github.com/loonghao/dcc-mcp-core/commit/2616546d868d5f49076ad59ca2d568f54a08b1d6))


### Bug Fixes

* **gateway:** cap initialize latency and log slow MCP dispatch ([#1009](https://github.com/loonghao/dcc-mcp-core/issues/1009)) ([40d8d88](https://github.com/loonghao/dcc-mcp-core/commit/40d8d889ec8b736a56d3745a8bc1ce386ce94ab5))
* **gateway:** consolidate MCP meta-tools to 6 verbs and preserve describe schemas ([e7e3c7e](https://github.com/loonghao/dcc-mcp-core/commit/e7e3c7ec5f36a174a6eeb8d24ff13fd32197d649))
* **gateway:** make failover work after a gateway crashes (TIME_WAIT recovery) ([1226ce3](https://github.com/loonghao/dcc-mcp-core/commit/1226ce391c962ec4f8c1319cfb126d4d83d540b7))
* **gateway:** sweep stale __gateway__ sentinels on every win (rotation pollution) ([e52ba8f](https://github.com/loonghao/dcc-mcp-core/commit/e52ba8f3624abd50db0223c99ec4f1ae8ce75b22))
* **inprocess:** plumb timeout_hint_secs into dispatcher timeout_ms ([ed9f52e](https://github.com/loonghao/dcc-mcp-core/commit/ed9f52e7f4a3de9c23d787119808a8c8f63c7672))
* **sidecar:** align `default_registry_dir` with `GatewayRunner::new` ([ed49194](https://github.com/loonghao/dcc-mcp-core/commit/ed49194648be51c2d98f74994060e5dae407b60c))

## [Unreleased]

## [0.17.1](https://github.com/loonghao/dcc-mcp-core/compare/v0.17.0...v0.17.1) (2026-05-16)


### Features

* **admin:** add SQLite persistence, skill-path CRUD, and AdminPersistConfig refactor ([3735162](https://github.com/loonghao/dcc-mcp-core/commit/37351624137f01492b3b5181521a612a2313f3f6))
* **host-rpc:** new crate `dcc-mcp-host-rpc` — HostRpcClient trait + HostDied envelope (RFC [#998](https://github.com/loonghao/dcc-mcp-core/issues/998)) ([8e66787](https://github.com/loonghao/dcc-mcp-core/commit/8e667874690ee2766522ae31f816c26c027d188f))
* **models:** add `risk_class` to ToolDeclaration (RFC [#998](https://github.com/loonghao/dcc-mcp-core/issues/998) Phase 1, schema only) ([c71cab7](https://github.com/loonghao/dcc-mcp-core/commit/c71cab78858a19111f9e558d3f558e4f030eaed6))
* **server:** add `sidecar` subcommand + PyPI binary distribution ([#998](https://github.com/loonghao/dcc-mcp-core/issues/998), [#1002](https://github.com/loonghao/dcc-mcp-core/issues/1002)) ([7ed63a3](https://github.com/loonghao/dcc-mcp-core/commit/7ed63a3e770d99a28f31c84075b804c913b8f4a1))


### Bug Fixes

* **gateway:** preserve full inputSchema, index unloaded skills, demote meta-tools ([#992](https://github.com/loonghao/dcc-mcp-core/issues/992), [#993](https://github.com/loonghao/dcc-mcp-core/issues/993), [#994](https://github.com/loonghao/dcc-mcp-core/issues/994)) ([7eb72ef](https://github.com/loonghao/dcc-mcp-core/commit/7eb72ef94c6aac040ff8a3dce3d92120f1a13fc4))
* **search:** strengthen meta-tool exclusion — zero score when query doesn't target tool name ([c14657b](https://github.com/loonghao/dcc-mcp-core/commit/c14657b948fc175d0a06fdc7b1bdb03347d5a445))
* **test:** mark regression tests as xfail for [#992](https://github.com/loonghao/dcc-mcp-core/issues/992)/[#993](https://github.com/loonghao/dcc-mcp-core/issues/993)/[#995](https://github.com/loonghao/dcc-mcp-core/issues/995)/[#996](https://github.com/loonghao/dcc-mcp-core/issues/996) ([2a8b403](https://github.com/loonghao/dcc-mcp-core/commit/2a8b4035822a49f656308c717bbc01b14d9aee33))


### Code Refactoring

* **release:** fold dcc-mcp-server wheel build into release.yml (no second cargo build) ([695fd03](https://github.com/loonghao/dcc-mcp-core/commit/695fd0329647944e2b3e2ecfb589e8348133a702))

## [0.17.0](https://github.com/loonghao/dcc-mcp-core/compare/v0.16.0...v0.17.0) (2026-05-15)


### Features

* **mcp:** migrate MCP transport to rmcp SDK ([#985](https://github.com/loonghao/dcc-mcp-core/issues/985)) ([27a4b44](https://github.com/loonghao/dcc-mcp-core/commit/27a4b4400be8d5cded2f276d499c1ce54d9a228a))
* **mcp:** add rmcp SDK integration spike behind feature flag ([#985](https://github.com/loonghao/dcc-mcp-core/issues/985)) ([88a28ce](https://github.com/loonghao/dcc-mcp-core/commit/88a28ce6b4eb0db395fb24e36d069358ee6faf98))
* **skills:** auto-generate inputSchema from Python script signatures (closes [#978](https://github.com/loonghao/dcc-mcp-core/issues/978)) ([d6fa942](https://github.com/loonghao/dcc-mcp-core/commit/d6fa94231df67e5b3bee6370d8294afa3ad7e822))


### Bug Fixes

* **admin:** inline DCC icons, enable admin feature, migrate to cargo-llvm-cov ([#974](https://github.com/loonghao/dcc-mcp-core/issues/974)) ([05bfc0f](https://github.com/loonghao/dcc-mcp-core/commit/05bfc0f1f0947fdfed0bf4263dcbe245f8b3115b))
* **ci:** add Python setup to rust-check job and fix cargo-clippy conflict ([1bde6c8](https://github.com/loonghao/dcc-mcp-core/commit/1bde6c8cd74181f2103f36d71636964fd0441b64))
* **ci:** add cargo PATH verification before maturin build (macOS) ([73cfa7f](https://github.com/loonghao/dcc-mcp-core/commit/73cfa7f8092e4789f44bd3ae8b436bdf907a7a41))
* **ci:** make schema_gen test resilient + bypass cargo subcommand on macOS ([c7dc8d9](https://github.com/loonghao/dcc-mcp-core/commit/c7dc8d9c0d5d9beb33cafb0029b9ae8e6a098efd))
* **ci:** unblock Windows wheel build (admin-ui npm + stubgen order) ([2f75655](https://github.com/loonghao/dcc-mcp-core/commit/2f756555a1a59f916dcc9efc84a4081fbdf7bc8e))
* **ci:** use vx cargo in stubgen recipe to ensure correct cargo in PATH ([a64d04e](https://github.com/loonghao/dcc-mcp-core/commit/a64d04e7d44b2ee249055a984b511347750ba427))
* **ci:** verify cargo is in PATH after Rust toolchain setup (macOS fix) ([d473662](https://github.com/loonghao/dcc-mcp-core/commit/d4736626c9587a5fce4c6202eed19e239531c830))
* **docs:** repair skill ownership links ([0aef2fe](https://github.com/loonghao/dcc-mcp-core/commit/0aef2fe0207f0f196c1fb20408e7f861fde6ffdd))
* **mcp:** adapt tests for rmcp stateless mode and fix CHANGELOG ordering ([a8bcfbd](https://github.com/loonghao/dcc-mcp-core/commit/a8bcfbdf48373540700552c776fad3946af7337f))
* **mcp:** restore full rmcp tools/list and tools/call parity ([eac90bc](https://github.com/loonghao/dcc-mcp-core/commit/eac90bc4a593da84b9fbf923a37112e4821eed0f))
* **mcp:** restore async job dispatch and satisfy clippy ([77a5276](https://github.com/loonghao/dcc-mcp-core/commit/77a52766381ef2b17b2840284fa832dc19d2b032))
* **mcp:** split async dispatch, emit isError=false, run rustfmt ([05f3fce](https://github.com/loonghao/dcc-mcp-core/commit/05f3fce43dbbefb69b6b254faee23cb96196e1ed))
* **mcp:** restore async meta opt-in and gateway SSE Accept ([1290e75](https://github.com/loonghao/dcc-mcp-core/commit/1290e754bb3a2c3fde322686b40d6cd418f7f60b))
* **mcp:** restore initialize negotiation and protocol JSON-RPC errors ([bc669f0](https://github.com/loonghao/dcc-mcp-core/commit/bc669f02402ae706ca53eee15bfbc6274dd89c35))
* **release:** align gateway dependency version ([39e3fc7](https://github.com/loonghao/dcc-mcp-core/commit/39e3fc7b3f423b188478ddeba8c2f7c1378194ce))
* **skills:** skip *args/**kwargs by kind in input-schema helper ([727736b](https://github.com/loonghao/dcc-mcp-core/commit/727736b0c1fa1f953f0557eac549362addbcf05c))
* **tests:** align rmcp async/job assertions with sync fallback ([ed8643c](https://github.com/loonghao/dcc-mcp-core/commit/ed8643cd51c62a1c68ac184b0611a84a1f0bd957))
* **tests:** handle missing properties in schema_gen test ([c1e1bab](https://github.com/loonghao/dcc-mcp-core/commit/c1e1baba14ed749b12737259a8d65dd0022ee064))
* update Cargo.lock with corrected dependencies ([#977](https://github.com/loonghao/dcc-mcp-core/issues/977)) ([9dff49a](https://github.com/loonghao/dcc-mcp-core/commit/9dff49a0b92f06b708872888dc4e1aea82a5d2b2))


### Code Refactoring

* **mcp:** drop bare-name fallback in rmcp tools/call ([de4c2a1](https://github.com/loonghao/dcc-mcp-core/commit/de4c2a11df3e52db0bf17612b4d50d4696bfb971))
* **mcp:** replace tool_list_legacy with mcp_tool_catalog ([54b9a61](https://github.com/loonghao/dcc-mcp-core/commit/54b9a61bcf3736825c8bdf80c9188903f8254d24))


### Documentation

* refresh AI agent gateway references ([69b57e8](https://github.com/loonghao/dcc-mcp-core/commit/69b57e83e3c73d2e61a567aabc86a6c65aa860e0))

## [0.16.0](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.9...v0.16.0) (2026-05-13)


### ⚠ BREAKING CHANGES

* Clients must use i_<id8>__<escaped> for gateway-prefixed MCP tool/prompt names. SKILL tools must use canonical YAML keys.

### Features

* **admin-ui:** expand call trace details ([#961](https://github.com/loonghao/dcc-mcp-core/issues/961)) ([0a2a888](https://github.com/loonghao/dcc-mcp-core/commit/0a2a8882e26f82af37a3af7c8b05cfb038df1315))
* **admin-ui:** refresh embedded gateway console theme ([ed83dac](https://github.com/loonghao/dcc-mcp-core/commit/ed83daca724d049d92a859b7d5596f55e00a8a9f))
* **admin:** add DCC-type icons + on-disk log rendering ([3c5146f](https://github.com/loonghao/dcc-mcp-core/commit/3c5146f720a4fb2b6c6afb17286b8f94ea19c96b)), closes [#963](https://github.com/loonghao/dcc-mcp-core/issues/963)
* **admin:** add maya icon (autodesk.svg) to DCC_ICON_MAP ([e79bd55](https://github.com/loonghao/dcc-mcp-core/commit/e79bd558d197bd5fa625573d13e82976c667f179))
* **admin:** persist audit traces as jsonl ([#964](https://github.com/loonghao/dcc-mcp-core/issues/964)) ([fac89f6](https://github.com/loonghao/dcc-mcp-core/commit/fac89f600830bbfb947079d8edde80ade31415b5))
* **admin:** ship Vite/React admin SPA and wire release-please ([bc3d980](https://github.com/loonghao/dcc-mcp-core/commit/bc3d980da59e2835924f46230de6042b9dbeecdc))
* core iteration - gateway search refactor + write_temp_file skill ([0d94c6d](https://github.com/loonghao/dcc-mcp-core/commit/0d94c6dbe4ce134586ba97ef30b9badb0110d629))
* **gateway:** add call_tools MCP and POST /v1/call_batch ([3ade6b2](https://github.com/loonghao/dcc-mcp-core/commit/3ade6b2a08d44dabb2edae8b02cb0c4790fee3bc))


### Bug Fixes

* add canonical MCP wire crate ([689e5e3](https://github.com/loonghao/dcc-mcp-core/commit/689e5e3d69b6368ad3d0c28f698c6020c4c9c7cb))
* **admin-ui:** add data-panel attrs so admin HTML passes gateway tests ([a18158d](https://github.com/loonghao/dcc-mcp-core/commit/a18158d156acb91472482cb465a7d289394bfcdf))
* **admin:** generate embedded UI during cargo build ([dcf7496](https://github.com/loonghao/dcc-mcp-core/commit/dcf7496e00a387b4d1ec51873c745154f7379832))
* **admin:** maya icon matching + default log dir in tests ([fd134cf](https://github.com/loonghao/dcc-mcp-core/commit/fd134cfac2e1fd69f9c788e4ab40b47e3fb09d60))
* **ci:** manylinux wheel build without vx in Docker ([e0f7cf7](https://github.com/loonghao/dcc-mcp-core/commit/e0f7cf70d70d857cf61a6fa2c2e4510d204412cb))
* **gateway:** include resilience modules and normalize tool arguments ([2a2ab1b](https://github.com/loonghao/dcc-mcp-core/commit/2a2ab1be28b45c876d88ef25bf53cf0cbfe81510))
* **gateway:** refresh FileRegistry before pool lease mutations ([59eced9](https://github.com/loonghao/dcc-mcp-core/commit/59eced923d63f78aecaf204c4c55c74f54ba5252))
* **gateway:** remove cursor-safe toggles; pin just dev to .venv ([a025899](https://github.com/loonghao/dcc-mcp-core/commit/a025899a4f08ba9f8268ea5956af7010c2e0b245))
* **gateway:** repair GatewayState construction after Default removal ([e06063b](https://github.com/loonghao/dcc-mcp-core/commit/e06063b355f4d4b4d01618db6723cba063ccbbf6))
* **models:** add SkillMetadata.stage for dcc-mcp.skills loader ([36b6ef0](https://github.com/loonghao/dcc-mcp-core/commit/36b6ef0ad231a08fbcb5fa0c1ec1e7d70eb5fef6))


### Code Refactoring

* **python:** remove legacy server compatibility paths ([ef385ed](https://github.com/loonghao/dcc-mcp-core/commit/ef385ed2f9b8329e13aceb12df8acdfa27d5fde2))
* tighten gateway routing and skill YAML surface ([017fdeb](https://github.com/loonghao/dcc-mcp-core/commit/017fdeb96ba9ac2eb349ba158883a1e05ead9c6e))


### Documentation

* add skill ownership policy for bundled adapter skills ([200f810](https://github.com/loonghao/dcc-mcp-core/commit/200f810e382f29f1d2280412ff00005b66b39440)), closes [#967](https://github.com/loonghao/dcc-mcp-core/issues/967)
* document gateway call_tool wrapper payloads and object-shaped arguments ([3a3bdb6](https://github.com/loonghao/dcc-mcp-core/commit/3a3bdb6423db0a896f8d3a0af1c1fa91b4739a7b))
* document gateway call_tool wrapper payloads and object-shaped arguments ([f7b60e5](https://github.com/loonghao/dcc-mcp-core/commit/f7b60e5cd9fc789b1606a6cd60cf6775b8af2407)), closes [#968](https://github.com/loonghao/dcc-mcp-core/issues/968)
* enforce skill ownership policy for bundled adapter skills ([f115d03](https://github.com/loonghao/dcc-mcp-core/commit/f115d03ad1a6416eceeb2a28d1cac0f52c9fdccb))
* refresh gateway admin observability docs ([2c714b0](https://github.com/loonghao/dcc-mcp-core/commit/2c714b06fde6527b4828fa91b6506d662a4a4f3f))
* remove legacy server construction references ([2988314](https://github.com/loonghao/dcc-mcp-core/commit/29883142b48a1e97c6c9aa523ed13d3a31dfb286))
* **zh:** sync admin ui guide ([#962](https://github.com/loonghao/dcc-mcp-core/issues/962)) ([77a1165](https://github.com/loonghao/dcc-mcp-core/commit/77a1165128201e9e321bf9b1a72624d2f1653d93))

## [0.15.9](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.8...v0.15.9) (2026-05-12)


### Features

* add VRS HTTP trace replayer and CI validation ([50dfa58](https://github.com/loonghao/dcc-mcp-core/commit/50dfa58fbef3f46f9d378fa8112ad4f64d45e437))
* **admin-ui:** add traces and stats panels ([#958](https://github.com/loonghao/dcc-mcp-core/issues/958)) ([263aa38](https://github.com/loonghao/dcc-mcp-core/commit/263aa38b8bce0753d80d1985b1d47d0175cd9372))
* **admin-ui:** infer DCC for gateway calls ([#960](https://github.com/loonghao/dcc-mcp-core/issues/960)) ([56bb832](https://github.com/loonghao/dcc-mcp-core/commit/56bb8329fb1074878f420e333e99fdc180bbb782))
* **admin-ui:** show call request ids and errors ([#959](https://github.com/loonghao/dcc-mcp-core/issues/959)) ([b5766ab](https://github.com/loonghao/dcc-mcp-core/commit/b5766abb6a49861c08944f4e2b8efe97e3dbd021))
* **skills:** enforce thread affinity opt-in ([#957](https://github.com/loonghao/dcc-mcp-core/issues/957)) ([3dd7fbb](https://github.com/loonghao/dcc-mcp-core/commit/3dd7fbbeec48a387c630d8d1e25a1455553fa160))


### Bug Fixes

* **gateway:** exclude status-stale instances ([#951](https://github.com/loonghao/dcc-mcp-core/issues/951)) ([f049a00](https://github.com/loonghao/dcc-mcp-core/commit/f049a00e104f16ba13a24d94625980583583bb05))
* **gateway:** find short capability queries ([#955](https://github.com/loonghao/dcc-mcp-core/issues/955)) ([17c6df7](https://github.com/loonghao/dcc-mcp-core/commit/17c6df7b3b9fedfcfbd40d83a62cb5997874c1c4))
* **gateway:** share instance id resolution ([#956](https://github.com/loonghao/dcc-mcp-core/issues/956)) ([1948602](https://github.com/loonghao/dcc-mcp-core/commit/1948602882e3a6dc7e6499867e2b1b533f4d58c3))
* **skills:** activate groups when loading skills ([#954](https://github.com/loonghao/dcc-mcp-core/issues/954)) ([1b913f5](https://github.com/loonghao/dcc-mcp-core/commit/1b913f5d0894589c0053d9630d8c3023cfd2efa9))
* **transport:** expose stale service status to Python ([#953](https://github.com/loonghao/dcc-mcp-core/issues/953)) ([5697752](https://github.com/loonghao/dcc-mcp-core/commit/5697752ffd530769522863a7b92f56586b0a158a))


### Documentation

* expand VRS guidance and gateway REST trace catalog ([2b6b9f8](https://github.com/loonghao/dcc-mcp-core/commit/2b6b9f8ae1e99a3aab22cf952da3b6ad71be81bb))
* refresh agent architecture indexes ([d1cf57f](https://github.com/loonghao/dcc-mcp-core/commit/d1cf57f44cc63f0bb5f2198c7dec3764e89e7e42))

## [0.15.8](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.7...v0.15.8) (2026-05-11)


### Features

* **admin:** Phase 2 per-call dispatch traces with payload capture ([#863](https://github.com/loonghao/dcc-mcp-core/issues/863)) ([b87c94a](https://github.com/loonghao/dcc-mcp-core/commit/b87c94a1ac67621b292cee5e02901e950f7dbea9))
* **admin:** Phase 3 statistics dashboard — GET /admin/api/stats ([#863](https://github.com/loonghao/dcc-mcp-core/issues/863)) ([1178135](https://github.com/loonghao/dcc-mcp-core/commit/11781353a3e6d54f267234360e4365b7e69287cd))
* **admin:** Phase 4 — per-instance Worker cards ([#863](https://github.com/loonghao/dcc-mcp-core/issues/863)) ([#883](https://github.com/loonghao/dcc-mcp-core/issues/883)) ([418686a](https://github.com/loonghao/dcc-mcp-core/commit/418686a219f39965d56dc4f8d967c74b161caf17))


### Bug Fixes

* **election:** faster failover + bind-based port probe for Windows TIME_WAIT ([#855](https://github.com/loonghao/dcc-mcp-core/issues/855)) ([002abbf](https://github.com/loonghao/dcc-mcp-core/commit/002abbf2eaf788e7f6699eb2536c449055fdab1d))
* **errors:** add #[must_use] to all error types and Result aliases ([#844](https://github.com/loonghao/dcc-mcp-core/issues/844)) ([10cdf86](https://github.com/loonghao/dcc-mcp-core/commit/10cdf862ff8c1918b58852b21031a11369c3dfbe))
* **errors:** remove #[must_use] from type aliases — not valid in Rust 1.95 ([f62acaf](https://github.com/loonghao/dcc-mcp-core/commit/f62acaf8c72b5094f9f880f0bfb3377d5385d589))
* **gateway:** add circuit-breaker to SSE reconnect loop to stop reconnect storm ([#861](https://github.com/loonghao/dcc-mcp-core/issues/861)) ([34f9edc](https://github.com/loonghao/dcc-mcp-core/commit/34f9edc01cea5fc7b1b2c7393f0ba72defc0d33a))
* **gateway:** fix clippy::unused_mut and useless_conversion in tasks.rs ([ab591b7](https://github.com/loonghao/dcc-mcp-core/commit/ab591b7bd55a57c8f2a32bd6f74ef78b4c61e71b))
* **gateway:** reduce eviction window from ~60s to ~7s and probe concurrently ([#854](https://github.com/loonghao/dcc-mcp-core/issues/854)) ([fc95884](https://github.com/loonghao/dcc-mcp-core/commit/fc958848dba7d90c315fece09a81ee08215e04fe))
* **gateway:** replace production unwrap() calls with safe alternatives ([#840](https://github.com/loonghao/dcc-mcp-core/issues/840)) ([f1f7a81](https://github.com/loonghao/dcc-mcp-core/commit/f1f7a8178e7655d691819a9e484811434a7e89d1))
* **gateway:** surface unloaded-skill tools in search_tools ([#858](https://github.com/loonghao/dcc-mcp-core/issues/858)) ([4ab1647](https://github.com/loonghao/dcc-mcp-core/commit/4ab16474c4f7fccf58e4d148c2f49d3f607700fd))
* **gateway:** wire AuditMiddleware into default chain — Admin UI Calls tab now populated ([#864](https://github.com/loonghao/dcc-mcp-core/issues/864)) ([#869](https://github.com/loonghao/dcc-mcp-core/issues/869)) ([24b3874](https://github.com/loonghao/dcc-mcp-core/commit/24b3874f1caf8081e67a990659bee94cc54ac0d1))
* **http-py:** register WorkspaceRoots in _core ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#922](https://github.com/loonghao/dcc-mcp-core/issues/922)) ([3f0fc7d](https://github.com/loonghao/dcc-mcp-core/commit/3f0fc7df9a3696869ec8b02b90351c74a3ff81b9))
* **http:** add missing health_check_interval_secs/failures fields to GatewayConfig ([#854](https://github.com/loonghao/dcc-mcp-core/issues/854)) ([49f4d6f](https://github.com/loonghao/dcc-mcp-core/commit/49f4d6f1bf876c9cde9e26101383938bc205383a))
* **lint:** ruff fixes for DccServerOptions PR ([#850](https://github.com/loonghao/dcc-mcp-core/issues/850)) ([f9b9adc](https://github.com/loonghao/dcc-mcp-core/commit/f9b9adc1bacf84c5a28a89454eab1046a45e6676))
* **models:** accept inputSchema/outputSchema camelCase in tools.yaml ([#857](https://github.com/loonghao/dcc-mcp-core/issues/857)) ([0b46f66](https://github.com/loonghao/dcc-mcp-core/commit/0b46f66f7dad8151e6b48bedc3ecca1e1142acfd))
* **output:** pause OutputCapture during execute_python to stop stdout double-capture ([#856](https://github.com/loonghao/dcc-mcp-core/issues/856)) ([bb9786f](https://github.com/loonghao/dcc-mcp-core/commit/bb9786fe6094daff17a2dbdf96959794eac18749))
* **server:** add missing health_check_interval_secs/failures to GatewayConfig ([#854](https://github.com/loonghao/dcc-mcp-core/issues/854)) ([1f9f0e1](https://github.com/loonghao/dcc-mcp-core/commit/1f9f0e13da3e3c4f2bf80e2ed53bfd0b58104223))
* **skills:** migrate dcc-diagnostics SKILL.md to nested dcc-mcp: metadata format ([#859](https://github.com/loonghao/dcc-mcp-core/issues/859)) ([2120ed5](https://github.com/loonghao/dcc-mcp-core/commit/2120ed5064fdae718d184063fe909d0d1d4b7aa3))
* **transport:** stable write_atomic temp filename eliminates AV/EDR churn on Windows ([#853](https://github.com/loonghao/dcc-mcp-core/issues/853)) ([b590e40](https://github.com/loonghao/dcc-mcp-core/commit/b590e40c9d2f3e45fe7bb1dfc43d52e2d2317aef))


### Code Refactoring

* **gateway:** introduce dcc-mcp-gateway-core domain crate ([#845](https://github.com/loonghao/dcc-mcp-core/issues/845)) ([#894](https://github.com/loonghao/dcc-mcp-core/issues/894)) ([d5c7981](https://github.com/loonghao/dcc-mcp-core/commit/d5c7981f0a34d07faefa9ba0ce024dcb261c3937))
* **gateway:** introduce SRP sub-state views on GatewayState ([#839](https://github.com/loonghao/dcc-mcp-core/issues/839)) ([#893](https://github.com/loonghao/dcc-mcp-core/issues/893)) ([c7761d7](https://github.com/loonghao/dcc-mcp-core/commit/c7761d7b869956c9339ff7d37e8bacd91a0e9975))
* **gateway:** migrate CapabilityRecord + slug helpers to gateway-core ([#845](https://github.com/loonghao/dcc-mcp-core/issues/845)) ([#896](https://github.com/loonghao/dcc-mcp-core/issues/896)) ([99eca5a](https://github.com/loonghao/dcc-mcp-core/commit/99eca5a580977d11aaa4c8898c9fc42bfaa70c3e))
* **gateway:** migrate IndexSnapshot + InstanceFingerprint to gateway-core ([#845](https://github.com/loonghao/dcc-mcp-core/issues/845)) ([#900](https://github.com/loonghao/dcc-mcp-core/issues/900)) ([25d857b](https://github.com/loonghao/dcc-mcp-core/commit/25d857b0bc4c8dfc14401ced3558edc81705e20b))
* **gateway:** migrate RefreshReason + BuildOutcome to gateway-core ([#845](https://github.com/loonghao/dcc-mcp-core/issues/845)) ([#903](https://github.com/loonghao/dcc-mcp-core/issues/903)) ([a8c92d4](https://github.com/loonghao/dcc-mcp-core/commit/a8c92d4c3e99ba4d5ec670bf27d72789d91b8cbd))
* **gateway:** migrate search_ranking scorers to gateway-core ([#845](https://github.com/loonghao/dcc-mcp-core/issues/845)) ([#905](https://github.com/loonghao/dcc-mcp-core/issues/905)) ([a2c09e0](https://github.com/loonghao/dcc-mcp-core/commit/a2c09e0231d8ca40a97588a3f66e9b9239373b46))
* **gateway:** migrate SearchQuery/Hit/Page/Mode wire types to gateway-core ([#845](https://github.com/loonghao/dcc-mcp-core/issues/845)) ([#898](https://github.com/loonghao/dcc-mcp-core/issues/898)) ([5d6d9a9](https://github.com/loonghao/dcc-mcp-core/commit/5d6d9a99696139fe00e7dbe1bd4dbfaf5844c53c))
* **gateway:** move event wire types to core ([c575701](https://github.com/loonghao/dcc-mcp-core/commit/c575701860174fdde982c03be6c399930a145ccc))
* **gateway:** move OpenAPI auth types to core ([0b4115a](https://github.com/loonghao/dcc-mcp-core/commit/0b4115a9c9c93affc6150d489907431974bdb6f1))
* **gateway:** move resource URI domain helpers to core ([00ccf78](https://github.com/loonghao/dcc-mcp-core/commit/00ccf789aba42e1ec813b73790b6a82ebb5c7b2f))
* **gateway:** move search ranking loop to gateway-core ([#845](https://github.com/loonghao/dcc-mcp-core/issues/845)) ([9bcc67f](https://github.com/loonghao/dcc-mcp-core/commit/9bcc67f91dbb89b78b470bdee56467f70e398a66))
* **gateway:** split backend client tests ([bd0b4e7](https://github.com/loonghao/dcc-mcp-core/commit/bd0b4e78f69e4a78d5448c03d5a295db0431b246))
* **gateway:** split backend_client.rs (1491 lines) into focused submodules ([#841](https://github.com/loonghao/dcc-mcp-core/issues/841)) ([#870](https://github.com/loonghao/dcc-mcp-core/issues/870)) ([72fa312](https://github.com/loonghao/dcc-mcp-core/commit/72fa3123d87d5291dbc0ecc28207f0f35c78fe13))
* **gateway:** split state view modules ([b2fc838](https://github.com/loonghao/dcc-mcp-core/commit/b2fc838f1622cecb98f22b8c7942464b88f9cf7e))
* **gateway:** split task supervisors ([546a2a6](https://github.com/loonghao/dcc-mcp-core/commit/546a2a617baa25083925adf7c6142a5397ad357a))
* **http-py:** move remaining Python bindings from http crate ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#921](https://github.com/loonghao/dcc-mcp-core/issues/921)) ([613570e](https://github.com/loonghao/dcc-mcp-core/commit/613570e5580875586c331c987d6a2f46001de25e))
* **http-py:** move WorkspaceRoots binding out of http crate ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#920](https://github.com/loonghao/dcc-mcp-core/issues/920)) ([8d54fe8](https://github.com/loonghao/dcc-mcp-core/commit/8d54fe8decabca6eee5f10d1636e22eed0ed6c23))
* **http-py:** split config support modules ([f9ce1af](https://github.com/loonghao/dcc-mcp-core/commit/f9ce1af425afa03e2d3a1ecd8ee6f657e9ecc5b1))
* **http-py:** use http-types for config bindings ([fadb318](https://github.com/loonghao/dcc-mcp-core/commit/fadb318f08f6035e286c07e317e874e8e31753d9))
* **http-server:** split executor tests ([587911b](https://github.com/loonghao/dcc-mcp-core/commit/587911b4414d7918e15334b6d9089d67395e3263))
* **http-types:** split config aggregate module ([13cf9b7](https://github.com/loonghao/dcc-mcp-core/commit/13cf9b75eabe20225d8c847fc8896566b49a6a4b))
* **http-types:** split config value modules ([0054c79](https://github.com/loonghao/dcc-mcp-core/commit/0054c793cc54110ac152c0abcf04ef853382dd8d))
* **http:** centralize ServerState construction ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#916](https://github.com/loonghao/dcc-mcp-core/issues/916)) ([3d36571](https://github.com/loonghao/dcc-mcp-core/commit/3d365713040e55827d65348468b4005ca95e7027))
* **http:** extract server support crate ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([6199573](https://github.com/loonghao/dcc-mcp-core/commit/6199573c17899066a156f3a801105c619bf8af18))
* **http:** introduce dcc-mcp-http-types crate ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#895](https://github.com/loonghao/dcc-mcp-core/issues/895)) ([4ee7c2c](https://github.com/loonghao/dcc-mcp-core/commit/4ee7c2c71e4fef5c878ad77ae19cdcccebef33cd))
* **http:** introduce http-py facade crate ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#919](https://github.com/loonghao/dcc-mcp-core/issues/919)) ([6cc6d7e](https://github.com/loonghao/dcc-mcp-core/commit/6cc6d7ecbab1c0502cdb5a9ca9af183cdee2a063))
* **http:** migrate HttpError + HttpResult to dcc-mcp-http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#897](https://github.com/loonghao/dcc-mcp-core/issues/897)) ([d31f74c](https://github.com/loonghao/dcc-mcp-core/commit/d31f74ce70cd864f00a81a9e47c0e777e5bea411))
* **http:** migrate InstanceConfig to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([05d5e8f](https://github.com/loonghao/dcc-mcp-core/commit/05d5e8f8e7edeecc0bc2c34347ed8b6a28300329))
* **http:** migrate JobConfig + WorkflowConfig to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([c408a04](https://github.com/loonghao/dcc-mcp-core/commit/c408a047ca8a4073bd0906cc6db91d8c3a43b2a5))
* **http:** migrate output wire types to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([05e4f77](https://github.com/loonghao/dcc-mcp-core/commit/05e4f773359e57af38fd6d14c5d5d253cd715d32))
* **http:** migrate prompt spec types to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([7a50be3](https://github.com/loonghao/dcc-mcp-core/commit/7a50be3a1b64a68224601c5d75e14f37fd3944f9))
* **http:** migrate QueueConfig to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#917](https://github.com/loonghao/dcc-mcp-core/issues/917)) ([cd41557](https://github.com/loonghao/dcc-mcp-core/commit/cd41557e9aaee110b3349b04c0f71ef6ae2de1cf))
* **http:** migrate resource value types to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([d78d0d8](https://github.com/loonghao/dcc-mcp-core/commit/d78d0d84cbb535eb247de83375f10f8dfcae015c))
* **http:** migrate server/session/gateway config to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#918](https://github.com/loonghao/dcc-mcp-core/issues/918)) ([82627a1](https://github.com/loonghao/dcc-mcp-core/commit/82627a1b09a481340ee9993e68154bf8d0ada1a9))
* **http:** migrate ServerSpawnMode + JobRecoveryPolicy to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([5737aed](https://github.com/loonghao/dcc-mcp-core/commit/5737aedbf6ace8833c39a510056facf80e899d90))
* **http:** migrate session log types to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([690f7ab](https://github.com/loonghao/dcc-mcp-core/commit/690f7ab95824c2a6f0451ab05af11f29c6a3b2ef))
* **http:** migrate TelemetryConfig + FeatureFlags to http-types ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#902](https://github.com/loonghao/dcc-mcp-core/issues/902)) ([f4252c9](https://github.com/loonghao/dcc-mcp-core/commit/f4252c95c2c1c6564964a0d32903c6dc919d3124))
* **http:** move AppState runtime fields into http-server ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([#914](https://github.com/loonghao/dcc-mcp-core/issues/914)) ([6644465](https://github.com/loonghao/dcc-mcp-core/commit/6644465489c0e288601efa03a3caae7f632245ae))
* **http:** move core tool builder to http-server ([#852](https://github.com/loonghao/dcc-mcp-core/issues/852)) ([75281d3](https://github.com/loonghao/dcc-mcp-core/commit/75281d33eef5e7ec548ee9b087a70c2cd131abef))
* **http:** move dynamic tool spec to types ([7500bb4](https://github.com/loonghao/dcc-mcp-core/commit/7500bb4189677f316f6c967adb5e6071c9718497))
* **http:** move McpHttpConfig aggregate to types ([a5d52d0](https://github.com/loonghao/dcc-mcp-core/commit/a5d52d0b88f874bd80af05ac31e8203457d65410))
* **init:** replace eager imports with __getattr__ lazy loading ([#849](https://github.com/loonghao/dcc-mcp-core/issues/849)) ([c020e65](https://github.com/loonghao/dcc-mcp-core/commit/c020e6539189b8b885712d25092b113c78e11750))
* **protocols:** split DccSceneManager into DccSceneQuery+DccFileIO+DccSelection ([#843](https://github.com/loonghao/dcc-mcp-core/issues/843)) ([5075d8c](https://github.com/loonghao/dcc-mcp-core/commit/5075d8cf5525c7965f784a9e5791f688fc4237ab))
* **python:** apply SOLID + Clean Architecture improvements ([f99d168](https://github.com/loonghao/dcc-mcp-core/commit/f99d168bc7d8e5ea4406ac367ae26a1665214ac5))
* **server_base:** drop test-double __getattr__ fallback, add _testing helper ([#851](https://github.com/loonghao/dcc-mcp-core/issues/851)) ([#882](https://github.com/loonghao/dcc-mcp-core/issues/882)) ([d514e7c](https://github.com/loonghao/dcc-mcp-core/commit/d514e7cc5297520f9b6d61f6d9847233c84cd21d))
* **server:** collapse GatewayConfig literals to ..default() ([#854](https://github.com/loonghao/dcc-mcp-core/issues/854)) ([1735d20](https://github.com/loonghao/dcc-mcp-core/commit/1735d20ef0be4d9d1f9f2273100cbb53e01eaec5))


### Documentation

* **gateway:** document admin public API + fix pre-existing CI gates ([#847](https://github.com/loonghao/dcc-mcp-core/issues/847)) ([#887](https://github.com/loonghao/dcc-mcp-core/issues/887)) ([64b074b](https://github.com/loonghao/dcc-mcp-core/commit/64b074b23133313610e6e36f3c8a4162b36606ed))
* **gateway:** document middleware/namespace/event_log public API ([#847](https://github.com/loonghao/dcc-mcp-core/issues/847)) ([047fd3e](https://github.com/loonghao/dcc-mcp-core/commit/047fd3eae22696dd057236a41edb3219a2dca158))
* refresh agent-facing project guidance ([1105708](https://github.com/loonghao/dcc-mcp-core/commit/1105708398a5b5573eeec350682cc962e546e99c))
* refresh interface architecture indexes ([53bd32c](https://github.com/loonghao/dcc-mcp-core/commit/53bd32c14c5117b82f791faeb12b3df30d9a1b70))
* refresh REST gateway agent guidance ([073f2bc](https://github.com/loonghao/dcc-mcp-core/commit/073f2bc278555b0ec1abeaf443393d6fed5c3086))
* refresh workspace architecture map ([7db5503](https://github.com/loonghao/dcc-mcp-core/commit/7db55034be4be7aa4b90672060d169e8f38bab25))

## [0.15.7](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.6...v0.15.7) (2026-05-08)


### Bug Fixes

* **ci:** sync generated cargo metadata for bot PRs ([37101db](https://github.com/loonghao/dcc-mcp-core/commit/37101dbdd140c4af95fb0619ac5b605e32db7590))
* **ci:** sync workspace-hack after tokio update ([14f5377](https://github.com/loonghao/dcc-mcp-core/commit/14f53770235418d9698e2c8974decabf67911350))
* **gateway:** annotate gateway meta tools ([37cb668](https://github.com/loonghao/dcc-mcp-core/commit/37cb668896108c06dea558aea9a04ce46e62f266))


### Code Refactoring

* **types:** canonicalize tool terminology ([fcfd427](https://github.com/loonghao/dcc-mcp-core/commit/fcfd427797ad177ea991b54709a2dde70141d984))

## [0.15.6](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.5...v0.15.6) (2026-05-08)


### Features

* **gateway:** promote DCC instance registry to gateway://instances MCP resource ([#813](https://github.com/loonghao/dcc-mcp-core/issues/813) phase 1 / [#818](https://github.com/loonghao/dcc-mcp-core/issues/818) phase 0) ([f7ab97d](https://github.com/loonghao/dcc-mcp-core/commit/f7ab97d3826d0a91c8a165477c2bc27e05be2283))
* **gateway:** promote diagnostics + catalog to MCP resources, refactor native_resources into SOLID submodules ([#813](https://github.com/loonghao/dcc-mcp-core/issues/813) phase 2 / [#818](https://github.com/loonghao/dcc-mcp-core/issues/818) phase 0) ([8c880f0](https://github.com/loonghao/dcc-mcp-core/commit/8c880f0a47498b150639da01d5617ae4dccdf39c))
* **http:** wire ResourceRegistry + PromptRegistry into SkillRestService ([#818](https://github.com/loonghao/dcc-mcp-core/issues/818) bridge) ([56a09f4](https://github.com/loonghao/dcc-mcp-core/commit/56a09f403afb33aafc5cb07a57603bc737511807))
* **skill-rest:** add SSE job/resource event streams + job cancel ([#818](https://github.com/loonghao/dcc-mcp-core/issues/818) phase 1b) ([de069ec](https://github.com/loonghao/dcc-mcp-core/commit/de069eceb753019d60cead2e58101d26962d562c))
* **skill-rest:** expose MCP resources & prompts over REST ([#818](https://github.com/loonghao/dcc-mcp-core/issues/818) phase 1a) ([8700d76](https://github.com/loonghao/dcc-mcp-core/commit/8700d761b1f96f961136d0c10edebfd93166a2bd))


### Bug Fixes

* **ci:** quiet pytest failure logs ([1cd7dc5](https://github.com/loonghao/dcc-mcp-core/commit/1cd7dc5464397e442056a18246a930ad8ed6fd86))
* **gateway:** forward prompt arguments over REST ([4c5d260](https://github.com/loonghao/dcc-mcp-core/commit/4c5d26058ecaa92d1d9b8d09b7aa1a2f6cc57cca))
* **gateway:** route skill management via MCP ([c8e926e](https://github.com/loonghao/dcc-mcp-core/commit/c8e926e980f539d26b4a67d7eaa30e793191a561))
* **tests:** fix integration test mock backends and try_fetch_tools for REST migration ([#818](https://github.com/loonghao/dcc-mcp-core/issues/818) phase 2) ([640d222](https://github.com/loonghao/dcc-mcp-core/commit/640d2226c806044be92531e5c22e22c2c99c8877))
* **tests:** skip gateway-native URIs in resource-prefixing test ([#813](https://github.com/loonghao/dcc-mcp-core/issues/813) phase 2) ([9145d8a](https://github.com/loonghao/dcc-mcp-core/commit/9145d8a1c92e990894f8d59ce101385acb07b668))


### Code Refactoring

* **gateway:** switch backend communication from MCP JSON-RPC to REST ([#818](https://github.com/loonghao/dcc-mcp-core/issues/818) phase 2) ([cfff2af](https://github.com/loonghao/dcc-mcp-core/commit/cfff2af20a889f01a39ee946eb15f88fd4228efe))
* **types:** collapse jsonrpc::McpToolAnnotations into protocols::ToolAnnotations ([#812](https://github.com/loonghao/dcc-mcp-core/issues/812) part 1) ([5652b9b](https://github.com/loonghao/dcc-mcp-core/commit/5652b9b618db8aafdeca7a960a3266f4c2d1ae04))


### Documentation

* fix markdown lint after gateway resource update ([80d0c48](https://github.com/loonghao/dcc-mcp-core/commit/80d0c48956f1eb002da493e0361ed0e264798e57))
* refresh gateway instance resource guidance ([7c41d97](https://github.com/loonghao/dcc-mcp-core/commit/7c41d9755e212377121a146bdde96834815ff755))

## [0.15.5](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.4...v0.15.5) (2026-05-07)


### Bug Fixes

* **http:** make McpHttpConfig::set_host return Result instead of panicking ([fc15902](https://github.com/loonghao/dcc-mcp-core/commit/fc15902426d8d459846c8be7c1f47ab9e01465de)), closes [#811](https://github.com/loonghao/dcc-mcp-core/issues/811)

## [0.15.4](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.3...v0.15.4) (2026-05-07)


### Bug Fixes

* cover py37 import compatibility ([62286d3](https://github.com/loonghao/dcc-mcp-core/commit/62286d3b6f337764d4aafe1fe149ae5abfd74d59))

## [0.15.3](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.2...v0.15.3) (2026-05-07)


### Bug Fixes

* **release:** sync Cargo.lock package versions ([635b179](https://github.com/loonghao/dcc-mcp-core/commit/635b179c97dbdb585d9932c04cc46b8d15979ed8))

## [0.15.2](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.1...v0.15.2) (2026-05-07)


### Documentation

* refresh agent and operations guidance ([42030c5](https://github.com/loonghao/dcc-mcp-core/commit/42030c5ff04c16b44164be26addc2e4702a40fde))

## [0.15.1](https://github.com/loonghao/dcc-mcp-core/compare/v0.15.0...v0.15.1) (2026-05-06)


### Features

* **gateway:** enable admin by default ([4253e73](https://github.com/loonghao/dcc-mcp-core/commit/4253e7345136822733a35de6e543d7fed688ce69))
* **http:** expose prompts registration on Python McpHttpServer ([#792](https://github.com/loonghao/dcc-mcp-core/issues/792)) ([cdf31ae](https://github.com/loonghao/dcc-mcp-core/commit/cdf31ae96943fa99450a7cba36c46aa59fe1bbd7))
* **server:** rename CLI dcc flags to app ([0f04f87](https://github.com/loonghao/dcc-mcp-core/commit/0f04f87455bcb124ac570c974b4d9f60b30d4c59))


### Bug Fixes

* **#793:** isolate test registry dirs and add FileRegistry drop cleanup ([cb88af3](https://github.com/loonghao/dcc-mcp-core/commit/cb88af3e9bb4215e62cad49d8843581ccc1ebcd9)), closes [#793](https://github.com/loonghao/dcc-mcp-core/issues/793)
* **http:** tighten Python prompt registration API ([5ce717b](https://github.com/loonghao/dcc-mcp-core/commit/5ce717b3794acdb89993aa21bd02095dfb3ca9ba))
* remove FileRegistry::Drop to fix test_read_alive_instances_prunes_dead_pid ([d47de8a](https://github.com/loonghao/dcc-mcp-core/commit/d47de8adb4aa1f056fa93ba4bd7ca263dceeb9a0))


### Documentation

* refresh gateway agent guidance ([fb9f7ab](https://github.com/loonghao/dcc-mcp-core/commit/fb9f7abc4943e1b90cc316e1e76304423cffc583))

## [0.15.0](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.28...v0.15.0) (2026-05-05)


### ⚠ BREAKING CHANGES

* **skills:** align fixtures with nested metadata.dcc-mcp.* contract
* **skills:** SKILL.md authors using the flat-form shorthand must migrate to the nested form. The flat-form keys will still parse as YAML (the loader does not reject them) but they stop populating the typed SkillMetadata fields, so meta.dcc, meta.layer, meta.tags, etc. will all fall back to their serde defaults.
* **gateway:**
* **adapter:** `DccApiDocEntry`, `DccApiDocIndex`, and `register_dcc_api_docs` are removed from the public Python API (`dcc_mcp_core.adapter_context` and `dcc_mcp_core.__all__`). No known downstream consumer imports these symbols.
* **skills:** SKILL.md files that declare dcc-mcp-core extensions (dcc, version, tags, tools, groups, depends, search-hint, next-tools, policy, products, external_deps, allow_implicit_invocation) at the YAML frontmatter root are no longer accepted. Move these keys under `metadata.dcc-mcp.*` (nested or flat form) per agentskills.io 1.0. The PyO3 surface `SkillMetadata.is_spec_compliant()` and `SkillMetadata.legacy_extension_fields` are removed; a successful parse now implies spec compliance, and `validate_skill()` reports any non-spec top-level key as a frontmatter error.

### Features

* **catalog:** public DCC-MCP catalog + dcc_catalog__search/describe MCP tools + CLI ([64ac075](https://github.com/loonghao/dcc-mcp-core/commit/64ac075b3fb857c16ab5c3160aaef40f8948311c))
* **gateway:** bundled zero-build /admin web UI (read-only instances/tools/calls/logs/sessions/health) ([b805db6](https://github.com/loonghao/dcc-mcp-core/commit/b805db6bd64be5268b023f487f707dc8765ccaef))
* **gateway:** pluggable BeforeCall/AfterCall middleware chain for cross-cutting policies ([710af57](https://github.com/loonghao/dcc-mcp-core/commit/710af57b692fdde52752b5201946bffc27561bac))
* **http:** framework-enforced payload size limits + SSE chunking + truncation envelope ([#780](https://github.com/loonghao/dcc-mcp-core/issues/780)) ([81cbeb2](https://github.com/loonghao/dcc-mcp-core/commit/81cbeb2c1c07381da1b6fff96e0f35737acfd91c))
* **observability:** export gateway contention events as resources + metrics ([b616bd6](https://github.com/loonghao/dcc-mcp-core/commit/b616bd60f687db99c35eb11974a227ef1fc68905))
* **rest:** OpenAPI-to-MCP mount helper — auto-expose REST endpoints as MCP tools ([6e9306b](https://github.com/loonghao/dcc-mcp-core/commit/6e9306bb7daf2c4e28b60245f0d6d2ea18a3d119))
* **server:** add 'translate' subcommand to expose any stdio MCP server over HTTP/SSE/Streamable-HTTP ([87d0fb1](https://github.com/loonghao/dcc-mcp-core/commit/87d0fb12705906672e60dbbe0745cecd7e7c0bf4))
* **telemetry:** wire OTLP gRPC exporter behind existing otlp-exporter feature ([e935b4f](https://github.com/loonghao/dcc-mcp-core/commit/e935b4fd3f194240df5028ccd19a718dfd6cc877))
* **tunnel:** add dcc-mcp-tunnel-relay and dcc-mcp-tunnel-agent CLI binaries ([daa0231](https://github.com/loonghao/dcc-mcp-core/commit/daa023148b5f668862a6726bc1953f829ffc9ff3))


### Bug Fixes

* **admin:** add middleware_chain to all GatewayState test constructors ([2869bbb](https://github.com/loonghao/dcc-mcp-core/commit/2869bbb0eadff83108748111eb3d348edf077522))
* **admin:** add missing middleware_chain+admin fields in translate.rs GatewayConfig ([76c05e9](https://github.com/loonghao/dcc-mcp-core/commit/76c05e9b90a2d13f5acf2f2fad3f5831e775edfa))
* **http:** correct field path after McpHttpConfig sub-config split ([2385a97](https://github.com/loonghao/dcc-mcp-core/commit/2385a97486ac184da98aba9488dff41f32f71774))
* **http:** correct field path after McpHttpConfig sub-config split ([#788](https://github.com/loonghao/dcc-mcp-core/issues/788)) ([d3e8a0a](https://github.com/loonghao/dcc-mcp-core/commit/d3e8a0a503054a390b16c4111724215775ae5f24))
* **http:** fix field-to-method access in background_impl.rs for prometheus feature ([825361c](https://github.com/loonghao/dcc-mcp-core/commit/825361c41da233adccf9eb080d455d715d287334))
* **observability:** use OnceLock singletons for Prometheus counters to prevent AlreadyReg panic on multi-gateway process ([e995509](https://github.com/loonghao/dcc-mcp-core/commit/e995509245cebe2bc00775a5c96ac1bacc72a307))
* **test:** allow resources://gateway/ URIs without 8-hex prefix in resources forwarding test ([bdd1d7d](https://github.com/loonghao/dcc-mcp-core/commit/bdd1d7dc639f15109e9c8397df8c8dbdc6e1e220))


### Code Refactoring

* **adapter:** drop unused register_dcc_api_docs helper and DccApiDoc types ([f86214b](https://github.com/loonghao/dcc-mcp-core/commit/f86214b930fdc8f7e4d58b34f1156c8f34c231ea))
* **gateway:** converge MCP surface to discover+dispatch primitives ([396ec68](https://github.com/loonghao/dcc-mcp-core/commit/396ec6834da2434f705b280769eb1829b41c3a85))
* **http:** split McpHttpConfig into cohesive sub-configs (closes [#764](https://github.com/loonghao/dcc-mcp-core/issues/764)) ([6f1b55c](https://github.com/loonghao/dcc-mcp-core/commit/6f1b55c82c3d4a08f5eb03259086849b02d589f1))
* **search:** strategy-pattern for SearchMode + Scorer ([5e146bc](https://github.com/loonghao/dcc-mcp-core/commit/5e146bca7a71dfeccdcc4c66a28cc63360f6e202))
* **skills:** drop flat-form SKILL.md metadata parser ([69ff3a9](https://github.com/loonghao/dcc-mcp-core/commit/69ff3a9d7b5ed45196df1dc37feb6bfb3a925046))
* **skills:** reject legacy top-level SKILL.md keys; drop compat API ([591b936](https://github.com/loonghao/dcc-mcp-core/commit/591b936fd3d58e28a5e04c85c1bf5ab1b147f4fe))


### Documentation

* add REST, CLI, and gateway-diagnostics guides (zh+en) ([9a213c5](https://github.com/loonghao/dcc-mcp-core/commit/9a213c5cf1eac6fbd176cf8db03dffc7fb598d9e))
* **agents:** steer custom-resource callers to helpers before server.resources() ([2542744](https://github.com/loonghao/dcc-mcp-core/commit/2542744fb831cd038d96db855ff9290c09623aea))
* **guide:** rewrite what-is-dcc-mcp-core around gateway MCP + DCC REST ([b235412](https://github.com/loonghao/dcc-mcp-core/commit/b235412e736913a7dd6508a3d83dc0a74ff66251))
* **llms:** refresh llms.txt and llms-full.txt for minimal MCP surface ([60e812e](https://github.com/loonghao/dcc-mcp-core/commit/60e812e4d1f61b1c6355d77790958ccac987956e))
* refresh agent skill metadata guidance ([7f297ac](https://github.com/loonghao/dcc-mcp-core/commit/7f297acb88b88aa888d1e63001c16cc2ea567366))


### Tests

* **skills:** align fixtures with nested metadata.dcc-mcp.* contract ([daa8bf1](https://github.com/loonghao/dcc-mcp-core/commit/daa8bf1f9936bd4e3a1c96dbef3272c2cce1dd20))

## [0.14.28](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.27...v0.14.28) (2026-05-04)


### Features

* **gateway:** aggregate prompts/list + prompts/get across backends ([#731](https://github.com/loonghao/dcc-mcp-core/issues/731)) ([fd850d3](https://github.com/loonghao/dcc-mcp-core/commit/fd850d38cfb1d624892ab0fecef39b130decf07f))
* **gateway:** forward backend resources with namespaced URIs ([#732](https://github.com/loonghao/dcc-mcp-core/issues/732)) ([64872b2](https://github.com/loonghao/dcc-mcp-core/commit/64872b2c261c8f297d8c5b85a1cb42a60ebff4fa))
* **http:** expose ResourceRegistry mutating API to Python ([#730](https://github.com/loonghao/dcc-mcp-core/issues/730)) ([2d1bc6c](https://github.com/loonghao/dcc-mcp-core/commit/2d1bc6c73e049c37eba8d2f3c4319799a0254f49))
* **http:** expose ResourceRegistry mutating API to Python ([#730](https://github.com/loonghao/dcc-mcp-core/issues/730)) ([#751](https://github.com/loonghao/dcc-mcp-core/issues/751)) ([669b01b](https://github.com/loonghao/dcc-mcp-core/commit/669b01bd6097225e93fd980a4a2855b5333435e2))
* **python:** add defensive handle shutdown ([#754](https://github.com/loonghao/dcc-mcp-core/issues/754)) ([0502638](https://github.com/loonghao/dcc-mcp-core/commit/05026384c0c31891a7a9a0a58b49bbbffc06a415))
* **server:** add DCC quit hooks ([#753](https://github.com/loonghao/dcc-mcp-core/issues/753)) ([f23ad7b](https://github.com/loonghao/dcc-mcp-core/commit/f23ad7bd239620a7d1bcfab076cf13ccf87d277c))
* **server:** handle shutdown signals ([#756](https://github.com/loonghao/dcc-mcp-core/issues/756)) ([12eec51](https://github.com/loonghao/dcc-mcp-core/commit/12eec5142e374609e7b05a86f81f028160fe95e6))
* **skills:** declare static MCP resources from YAML ([#752](https://github.com/loonghao/dcc-mcp-core/issues/752)) ([b18d271](https://github.com/loonghao/dcc-mcp-core/commit/b18d2719a41b4d14cdfba48527f29b509cc2e1bb))
* **transport:** add registry sentinel locks ([#755](https://github.com/loonghao/dcc-mcp-core/issues/755)) ([4b6f563](https://github.com/loonghao/dcc-mcp-core/commit/4b6f5633619679e271dd2c668d817d982595fb43))


### Bug Fixes

* **gateway:** bind forwarded resources/subscribe to backend SSE session ([#732](https://github.com/loonghao/dcc-mcp-core/issues/732)) ([b5efef4](https://github.com/loonghao/dcc-mcp-core/commit/b5efef4b488f6180efa1ad1023b79a7c825ad773))
* **gateway:** reload registry before pruning ghost rows, test cross-process eviction ([66772b0](https://github.com/loonghao/dcc-mcp-core/commit/66772b08dd415def9d402ee287589bfea86d20e7))
* **tests:** gateway resources subscribe test must target backend B explicitly ([7d3efb3](https://github.com/loonghao/dcc-mcp-core/commit/7d3efb3fdbcf0348ab84c0113c1606352911e632))
* **tests:** keep writer registry handles alive to simulate real ownership ([9828aa1](https://github.com/loonghao/dcc-mcp-core/commit/9828aa1ede0d3adcd5f2762d37a3aedf01fa0d37))


### Code Refactoring

* **gateway:** split SOLID responsibilities, adopt Rust 1.95 cfg_select ([57e63df](https://github.com/loonghao/dcc-mcp-core/commit/57e63dfae7415e892b131bd9f46becbda9961248))


### Documentation

* refresh AI agent onboarding ([3200aa8](https://github.com/loonghao/dcc-mcp-core/commit/3200aa8c2943360b57b1ddecbfa08c995eedc281))

## [0.14.27](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.26...v0.14.27) (2026-05-03)


### Features

* **gateway:** probe /v1/readyz three-state readiness instead of /health ([#713](https://github.com/loonghao/dcc-mcp-core/issues/713)) ([#727](https://github.com/loonghao/dcc-mcp-core/issues/727)) ([203d412](https://github.com/loonghao/dcc-mcp-core/commit/203d412e0a1d03e8b13b7009f9a78d0999f2bf50))
* **http+rest:** gate tools/call on shared ReadinessProbe ([#714](https://github.com/loonghao/dcc-mcp-core/issues/714)) ([#724](https://github.com/loonghao/dcc-mcp-core/issues/724)) ([af8816d](https://github.com/loonghao/dcc-mcp-core/commit/af8816d63824761e7dc9b175fb2d5b6c63525fec))
* **queue:** observability + configurable backpressure for DeferredExecutor / host_bridge / QueueDispatcher ([#715](https://github.com/loonghao/dcc-mcp-core/issues/715)) ([#726](https://github.com/loonghao/dcc-mcp-core/issues/726)) ([0ca9da2](https://github.com/loonghao/dcc-mcp-core/commit/0ca9da2996aabb76d12d9a0e06e26c7ce3607d52))


### Bug Fixes

* **gateway+transport:** graceful shutdown deregisters from FileRegistry ([#718](https://github.com/loonghao/dcc-mcp-core/issues/718)) ([#725](https://github.com/loonghao/dcc-mcp-core/issues/725)) ([eeba019](https://github.com/loonghao/dcc-mcp-core/commit/eeba019dcfa31354f96d6afa876f60bd6037f591))
* **gateway:** prune dead PIDs on list_dcc_instances read path ([#719](https://github.com/loonghao/dcc-mcp-core/issues/719)) ([3a34b86](https://github.com/loonghao/dcc-mcp-core/commit/3a34b86a58923f521a4669c4d721e964c85f04eb))
* **http:** expose thread_affinity kwarg on Python register_handler ([#716](https://github.com/loonghao/dcc-mcp-core/issues/716)) ([#728](https://github.com/loonghao/dcc-mcp-core/issues/728)) ([7b36766](https://github.com/loonghao/dcc-mcp-core/commit/7b36766738f4c9e8839e335df24534ff658f748f))
* **http:** honour ThreadAffinity on sync tools/call path ([#716](https://github.com/loonghao/dcc-mcp-core/issues/716)) ([1b672a5](https://github.com/loonghao/dcc-mcp-core/commit/1b672a5145f04437c2e17593178b256e95a4b4a8))
* **tests:** replace str.removesuffix with slice for py3.7/3.8 compatibility ([bf1a145](https://github.com/loonghao/dcc-mcp-core/commit/bf1a145c6aba3bba90df76cfd2a0e4e97bd77f6e))
* unloaded skill search, GatewayToolExposure cleanup, log retention ([#677](https://github.com/loonghao/dcc-mcp-core/issues/677), [#674](https://github.com/loonghao/dcc-mcp-core/issues/674)) ([#721](https://github.com/loonghao/dcc-mcp-core/issues/721)) ([3e33a90](https://github.com/loonghao/dcc-mcp-core/commit/3e33a9041e486eda89817dd92d711703bde976ff))

## [0.14.26](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.25...v0.14.26) (2026-05-03)


### Bug Fixes

* **release-please:** add x-release-please-version marker to llms.txt ([e0dc096](https://github.com/loonghao/dcc-mcp-core/commit/e0dc0961d31fbd27a4bc5c8ee13fa846431835f9))


### Documentation

* collapse per-agent MD files into thin shims pointing at AGENTS.md ([05f2f74](https://github.com/loonghao/dcc-mcp-core/commit/05f2f742d09f8359a766f70b8aed89b0da00cddf))

## [0.14.25](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.24...v0.14.25) (2026-05-03)


### Bug Fixes

* **skills:** reject invalid strict scan directories ([87cf1db](https://github.com/loonghao/dcc-mcp-core/commit/87cf1dbe110946ca6952ae1fcd1bcc6515e3e1fd))

## [0.14.24](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.23...v0.14.24) (2026-05-03)


### Features

* **verifier:** ship SceneStats contract + verifier skill template ([#688](https://github.com/loonghao/dcc-mcp-core/issues/688)) ([a8f8787](https://github.com/loonghao/dcc-mcp-core/commit/a8f87873ee2b5dba7b1f3f583a639cc20a09275b))


### Bug Fixes

* address CI regressions ([c3dda55](https://github.com/loonghao/dcc-mcp-core/commit/c3dda55bdd3dbb34ecb874db5f9f13acdf8b9330))
* resolve gateway REST and docs issues ([a49a58e](https://github.com/loonghao/dcc-mcp-core/commit/a49a58ef83e410f0cc9baba195627398d52f242d))


### Documentation

* add HostAdapter/StandaloneHost to llms-full.txt ([5db08fd](https://github.com/loonghao/dcc-mcp-core/commit/5db08fdacc67a326ffee0746faf35f2a83acb51d))
* update version in llms.txt to 0.14.23 ([d86c3e0](https://github.com/loonghao/dcc-mcp-core/commit/d86c3e0b2b60953ff6868fab6eb7f46b709210ac))

## [0.14.23](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.22...v0.14.23) (2026-05-02)


### Features

* **host:** bridge DccDispatcher into McpHttpServer.tools/call via attach_dispatcher (P2b) ([ec44e4d](https://github.com/loonghao/dcc-mcp-core/commit/ec44e4d7e7c4a42661a4135155978142c25388eb))
* **host:** expose DccDispatcher as Python primitives + StandaloneHost driver (P2a) ([109f7ab](https://github.com/loonghao/dcc-mcp-core/commit/109f7ab51cd4dc942d390ef16c16ea356b5ba4ee))
* **host:** introduce dcc-mcp-host crate for cross-DCC main-thread dispatch ([94f7e1a](https://github.com/loonghao/dcc-mcp-core/commit/94f7e1a083180a67f3091d7c1b07ca1e45aaf5f9))
* **host:** ship HostAdapter base class + authoring guide (P3 rescoped, closes [#687](https://github.com/loonghao/dcc-mcp-core/issues/687)) ([410cc3d](https://github.com/loonghao/dcc-mcp-core/commit/410cc3d07e0b3c425a8afdfe0291a9245b5430bc))


### Bug Fixes

* harden gateway dynamic capability routing ([f643610](https://github.com/loonghao/dcc-mcp-core/commit/f6436107d5acac48755dc6dbde58c9d3230d2330))
* **host:** use typing.Callable + typing.Optional for Py3.7/3.8 compat ([be66bd7](https://github.com/loonghao/dcc-mcp-core/commit/be66bd7ab1dab49ccd943925993c1964eb2ff835))
* **http:** filter stubs from search_tools and surface unloaded skills ([#677](https://github.com/loonghao/dcc-mcp-core/issues/677)) ([da322d8](https://github.com/loonghao/dcc-mcp-core/commit/da322d88bae878841f8d030915e3104429617c23))
* **http:** synthesise progressive-loading stubs in search_tools ([83c4463](https://github.com/loonghao/dcc-mcp-core/commit/83c446376fc5e0b6ff3a872cf1dabd648b48f584))
* **http:** trim search_tools description + include_stubs param under caps ([b33eb65](https://github.com/loonghao/dcc-mcp-core/commit/b33eb655d90d6e63241c5c05f9cb7ffc01036a3c))
* remove Maya-only DCC assumptions ([a057b6d](https://github.com/loonghao/dcc-mcp-core/commit/a057b6d433bf51403904e0191602141f7ca5080e))


### Documentation

* update version in llms.txt to 0.14.22 ([6b46af9](https://github.com/loonghao/dcc-mcp-core/commit/6b46af928fa37bb0186276ab153903c56e8edbde))

## [0.14.22](https://github.com/loonghao/dcc-mcp-core/compare/v0.14.21...v0.14.22) (2026-05-02)


### Features

* **_tool_registration:** add output_schema to ToolSpec ([#242](https://github.com/loonghao/dcc-mcp-core/issues/242)) ([5fb5904](https://github.com/loonghao/dcc-mcp-core/commit/5fb5904555953bed97655deab300e03b83170939))
* **examples:** typed-schema-demo skill using derived schema ([#242](https://github.com/loonghao/dcc-mcp-core/issues/242)) ([47b0ee9](https://github.com/loonghao/dcc-mcp-core/commit/47b0ee94798b6b033f6967249078b25bbeb989bd))
* **gateway:** add capability index + REST/MCP dynamic-capability wrappers ([#653](https://github.com/loonghao/dcc-mcp-core/issues/653), [#654](https://github.com/loonghao/dcc-mcp-core/issues/654), [#655](https://github.com/loonghao/dcc-mcp-core/issues/655)) ([#664](https://github.com/loonghao/dcc-mcp-core/issues/664)) ([64c3ebc](https://github.com/loonghao/dcc-mcp-core/commit/64c3ebc2dd48234d2296e40a4457b234c66bb252))
* **gateway:** add configurable slim/rest tool-exposure mode ([#652](https://github.com/loonghao/dcc-mcp-core/issues/652)) ([#661](https://github.com/loonghao/dcc-mcp-core/issues/661)) ([e111232](https://github.com/loonghao/dcc-mcp-core/commit/e111232246bd1a62542eb4f014087deb14bf6f10))
* **gateway:** emit Cursor-safe tool names and keep legacy dotted decode ([#656](https://github.com/loonghao/dcc-mcp-core/issues/656)) ([968199a](https://github.com/loonghao/dcc-mcp-core/commit/968199af7a388b88f28c19ed63d3a33246b222e6))
* **gateway:** high-performance fuzzy search over capability metadata ([#659](https://github.com/loonghao/dcc-mcp-core/issues/659)) ([992ac8f](https://github.com/loonghao/dcc-mcp-core/commit/992ac8f7e032c167f0ca5e70e99c436b55c8e931))
* **schema:** tool_spec_from_callable helper ([#242](https://github.com/loonghao/dcc-mcp-core/issues/242)) ([2d5ee9a](https://github.com/loonghao/dcc-mcp-core/commit/2d5ee9a1053070b0eb6af624e4e5ee46568f58e7))
* **schema:** zero-dep type to JSON Schema helper ([#242](https://github.com/loonghao/dcc-mcp-core/issues/242)) ([6639bbb](https://github.com/loonghao/dcc-mcp-core/commit/6639bbb14473382dda49413b62ce35f02a4a954a))
* **skill-rest:** per-DCC RESTful skill API surface ([#658](https://github.com/loonghao/dcc-mcp-core/issues/658), [#660](https://github.com/loonghao/dcc-mcp-core/issues/660)) ([136036c](https://github.com/loonghao/dcc-mcp-core/commit/136036cbe68af7fa48ab37699a65cf1394d52cc2))


### Bug Fixes

* **gateway:** update doctest imports after crate extraction ([9a1905a](https://github.com/loonghao/dcc-mcp-core/commit/9a1905a7e1494454d94bdf33ae05ce2f6822a76b))
* keep schema helpers py37 compatible ([50d2a6d](https://github.com/loonghao/dcc-mcp-core/commit/50d2a6dbe147bb5fe69734e45bb2327b652f6466))


### Code Refactoring

* **http:** split dcc-mcp-http into 4 SOLID-aligned crates ([94dca59](https://github.com/loonghao/dcc-mcp-core/commit/94dca59f20fddd5263ab20839fbc92eaefe7e30f))


### Documentation

* document structured schema derivation ([#242](https://github.com/loonghao/dcc-mcp-core/issues/242)) ([eb611d3](https://github.com/loonghao/dcc-mcp-core/commit/eb611d3a98cb1e330f0fa9880201dbda93a0eebe))
* optimize AI agent documentation and Skills-First emphasis ([36d182d](https://github.com/loonghao/dcc-mcp-core/commit/36d182d20accbe872ecf0e93869ccff643ee120c))
* optimize documentation for AI agent discoverability and Skills-First emphasis ([3a0b65d](https://github.com/loonghao/dcc-mcp-core/commit/3a0b65d441e8ee7d12ec7ff4ac8f59b17cf456ab))
* update version in llms.txt to 0.14.21 ([fa65bec](https://github.com/loonghao/dcc-mcp-core/commit/fa65bece482fccc2545848d6e93cc6ed7b40907b))

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

## v0.10.0 (2026-03-28)

### Feat

- replace pre-commit with vx prek and add justfile
- add Skills system for zero-code script registration as MCP tools

### Fix

- resolve lint errors in test files (isort, ruff format, D106/F841)
- add cross-platform shell support to justfile
- resolve isort issues and migrate CI to vx

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
