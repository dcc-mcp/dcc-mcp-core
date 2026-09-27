---
name: unity-project
description: >-
  Domain skill — Inspect and compile an open Unity project, read or safely
  upsert bounded source assets, configure generated PNG sprites, run typed Unity
  tests, change Play Mode, and build Windows or Android players. Not for GameObject edits
  — use unity-scene.
license: MIT
compatibility: "Unity 2018.4.25f1+ (.NET 4.x); dcc-mcp-core 0.19.90+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: unity
    layer: domain
    version: "0.13.0"  # x-release-please-version
    search-hint: "Unity project source script sprite PNG TextureImporter CAS compile Play Mode Windows Android APK AAB player build"
    tags: "unity,project,assets,game-development"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# Unity Project

Inspect before assuming the active project, scene, Unity version, or Play/compile state.
`configure_sprite_importer` imports one existing `Assets/.../*.png` as a single Sprite through
Unity's native `TextureImporter`; choose point or bilinear filtering and an explicit pixels-per-unit
value. Source writes require the operator-owned environment gate, an `Assets/...` allowlisted text
path, bounded UTF-8 content, and either `expected_sha256: absent` for creation or the digest returned
by `read_text_asset` for replacement.

Every long or domain-reloading mutation keeps its Core async job open until Unity reports a
terminal persistent state. Reuse the same UUID and wait on that job; use
`unity_diagnostics__inspect_job` for reconnect or audit recovery, and never replace an ambiguous
request with a new ID. Build uses only enabled scenes and writes a new request directory below
`Builds/DccMcp`.

`build_android_player` accepts only `apk` or `aab`. It uses the project's saved Player Settings,
never accepts signing secrets, requires custom project signing for AAB delivery, restores the
temporary app-bundle toggle, and returns the BuildReport outcome plus artifact size and SHA-256.

`run_tests` invokes the installed Unity Test Framework through an exact typed contract. Use
`edit_mode` or `play_mode` and optional exact fully-qualified test or fixture names; an empty list
runs the selected mode. Wait for the returned Core job and treat `result.outcome`, counts, report
SHA-256, and request-scoped NUnit XML as the test evidence. The tool never launches another Unity
process.

## Mobile input and device acceptance

When a user supplies a game file and requests Android/iOS testing, first call
`inspect_mobile_input`. Do not assume source is available. Classification only
reads container structure and does not authorize installation, extraction, opening
an unknown project, signing, or launching a game. Explain the returned route and
ask only for missing information needed by that route. With no input yet, explain
the supported formats; do not require a device for a capability discussion.

For a reviewed Unity Android project, build an APK, wait for `inspect_job` success,
then call `enumerate_android_devices` and select an exact authorized `device_id`.
USB debugging and the phone's authorization prompt are handled by the user. Never
bypass authorization or the lock screen. `install_android_apk` requires the completed
`build_request_id` and verifies the installed base APK against its SHA-256. Tools
never accept arbitrary shell commands or silently choose a device.

Use a fresh UUID `request_id` for each intended operation. Repeating the same UUID
and arguments returns its durable receipt without repeating a mutation. A request
with `state=unknown` and `retry_safe=false` may have changed the device; do not issue
a replacement install/launch until the state is inspected. Failed read-only calls
can be retried with a new UUID after transport recovery. Restart is an explicit
stop followed by a launch, each with its own request identity. Core async jobs wrap
these bounded calls; normal Core job polling remains available.

The deployment receipt appears under `device_request`. On success, use
`launch_android_package`, `stop_android_package`, `grant_android_permission`,
`read_android_logcat`, and `capture_android_screenshot`. Each verifies the installed
APK first. Permission grants must be declared by the package. Logs cover one live
main-process PID, at most 1000 entries/1 MiB, optionally waiting for a literal
marker; they cannot prove absence of crashes/ANRs outside that process or after it
exits. Screenshot artifacts are request-scoped PNGs with verified pixel payload,
dimensions, size and hash. No screenshots or game launches occur during intake.

External APK import, split APK/AAB conversion, and iOS build/deploy are follow-up
capabilities. An APK does not require source in principle, but this iteration only
trusts completed Unity build provenance. AAB needs bundletool conversion/signing;
a Windows EXE cannot be converted to a mobile game. iOS source builds require a
supported Mac/Xcode host, signing/provisioning and device pairing; IPA eligibility
depends on its distribution method and signing. Do not promise equivalent native
iOS build/deploy on Windows. See `docs/mobile-device-workflow.md` for the matrix.
