---
name: unreal-fab-assets
description: >-
  Acquire free Fab marketplace content through Unreal Engine's official Fab
  integration, then verify the imported Content Browser assets. Use when the
  user asks to find, download, add, or import Fab or Unreal Marketplace assets.
  Not for arbitrary file imports - use unreal-assets instead.
license: MIT
compatibility: Unreal Engine 5.3+, Python 3.9+
allowed-tools: Read Bash
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.2.0"
    layer: domain
    stage: interchange
    depends: ["ui-control", "unreal-assets"]
    search-hint: "unreal fab marketplace free asset download add to project license content browser"
    tags: [unreal, fab, marketplace, assets, pipeline]
    tools: tools.yaml
---

# Unreal Fab Assets

Use the official Fab window in Unreal Engine for acquisition. UE-format Fab
content must be added to a project through the Fab integration or Epic Games
Launcher; do not scrape listing downloads or reproduce Fab's authenticated
client.

## Workflow

1. Inventory the live Unreal instance and load `ui-control` plus `unreal-assets`.
2. Call `inspect_fab_session`. It reports only a boolean authentication state;
   access and refresh tokens never leave the Unreal process. Opening an
   approved listing triggers Fab's persistent Epic login. If it still requires
   login, call `request_fab_login`, let the user complete Epic's account portal,
   then inspect again. Never request credentials from the user.
3. Turn the user's art direction into an explicit visual gate before search:
   required and rejected silhouette, material, palette, era, and animation
   traits. Filter to **Free**, Unreal Engine format, and a version compatible
   with the running engine. Reject a technically compatible listing when it
   fails the visual gate; do not use low-poly or stylized stand-ins for a
   realistic brief.
4. Before acquisition, report the listing title, publisher, source URL,
   license, supported engine version, and expected destination. Do not accept
   a new EULA, acquire a listing, or add it to the project without task-scoped
   user approval.
5. Call `open_fab_listing` with the approved listing UUID, then use the Fab
   integration's **Add to My Library** and **Add to Project** flow through
   scoped `ui-control`.
   Never enter credentials, solve a CAPTCHA, bypass region/account policy, or
   automate paid content. Stop on authentication or confirmation boundaries.
6. Take a fresh snapshot after every UI action. When the download finishes,
   verify the new package with `unreal_assets__list_assets` and
   `unreal_assets__get_asset_info`. For characters and creatures, also verify
   skeletal mesh, skeleton, idle/move/attack/hit/death animation coverage, and
   material instances before replacing a prototype asset.
7. Record a compact manifest containing the listing metadata, license, source
   URL, acquisition time, Content Browser paths, and verification result.
8. Always finish with `ui_control__stop_computer_use`, including failure and user
   interruption paths.

Fab content may be incorporated into a packaged project under its listing
license. Do not redistribute downloaded source assets as a standalone bundle.

## Scripts

- `prepare_free_asset_acquisition`
- `inspect_fab_session`
- `request_fab_login`
- `open_fab_listing`
