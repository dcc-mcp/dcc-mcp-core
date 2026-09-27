---
name: unreal-build-package
description: >-
  Build installable DCC MCP Unreal plugin archives and package Unreal projects
  as Windows archives, Setup executables, SteamPipe builds, or WeGame submission
  folders. Use for long-running compilation, cooking, staging, and packaging.
  Do not use for platform account registration or store publication.
license: MIT
compatibility: Windows, Unreal Engine 4.18+, Python 3.9+
allowed-tools: Read Bash Write
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.2.0"
    layer: domain
    stage: runtime
    search-hint: "unreal build compile package setup installer steam SteamPipe WeGame exe cook stage plugin zip prerequisites RunUAT BuildCookRun"
    tags: [unreal, pipeline, build, packaging, executable, steam, wegame]
    tools: tools.yaml
---

# Unreal Build and Package

Run bounded, asynchronous Unreal build workflows and return their artifacts and
log paths. Both tools execute off the Unreal game thread and support cooperative
cancellation while waiting for the external build process.

## Workflow

1. Use `build_plugin_package` for this repository's installable
   `DccMcpUnreal` ZIP. Pass a source checkout and an Unreal Engine root.
2. Use `package_project_executable` for a saved `.uproject`. Choose `archive`,
   `installer`, `steam`, or `wegame` with `release_profile`.
3. Poll the returned DCC MCP job until it completes, then inspect `artifacts`
   and `log_path`. A queued job is not proof that an executable was produced.

The project packaging tool targets Windows `Win64` and uses Unreal's
`BuildCookRun` with build, cook, stage, pak, and archive enabled. `installer`
requires Inno Setup 6 and produces a Setup executable that installs Unreal's
Win64 prerequisites. Commercial distribution must use an appropriately licensed
Inno Setup compiler. `steam` produces preview-only SteamPipe VDF files; enable
the required Common Redistributables in Steamworks before release. `wegame`
produces a content folder and preflight record; project approval, Rail SDK
integration, the developer client, authenticated upload, signing, and store
publication remain operator-owned steps.

## Scripts

- `build_plugin_package`
- `package_project_executable`

