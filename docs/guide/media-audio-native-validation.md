# Typed audio native qualification

Status: proposed CI lane; no native result is implied by adding this workflow.
The lane consumes `wheel-ubuntu-latest` from this same CI run. It must pass before
claiming actual WAV/Vorbis production readiness. Existing mocked tests are not
substitutes for this qualification.

## Provenance and execution

- Official vx release: [v0.9.37](https://github.com/vx-org/vx/releases/tag/v0.9.37),
  `vx-0.9.37-x86_64-unknown-linux-gnu.tar.gz`, 11,284,396 bytes,
  SHA256 `18b751f7735502723cff7f9a78dd90bcf40572448435a25dcafa1c6a44f8c7f6`.
- Official provider-selected mirror: [FFmpeg 8.1](https://github.com/vx-org/mirrors/releases/tag/ffmpeg-8.1),
  `ffmpeg-8.1-linux64-lgpl.tar.xz`, 127,584,512 bytes,
  SHA256 `dd03558b41409d111784bfe5da9bce4ddf8dae92871ede3b34acb3ad0f0889b0`.
  Release metadata attributes builds to BtbN/FFmpeg-Builds. This is a vx mirror,
  not a claim that FFmpeg upstream publishes this compiled archive itself.
- Verify both archives before extracting the vx launcher or reading reference
  executable members. Invoke real `vx install ffmpeg@8.1`, then resolve the
  managed bundle and compare both executable hashes with the verified reference
  archive before invoking FFmpeg or FFprobe. No fabricated store, fake launcher,
  direct system FFmpeg, alternate-source retry, or cache seeding.
- The media test exercises the installed candidate skill through Core HTTP MCP,
  requires an actual async job ID, polls `jobs_get_status`, and tests REST job
  cancellation. It matches the real encoding argv, staging output, PID and Linux
  process creation identity, requires `terminated_and_reaped`, then checks that
  the same identity is absent even if reparented. It waits for handler completion,
  rather than treating a cancelled status alone as sufficient.
- The generated short cue, periodic 32-second stereo input, and 120-second
  cancellation input are deterministic test fixtures. They do not replace any
  production showcase masters. WAV must preserve PCM exactly; Vorbis must
  preserve full sample count and satisfy declared head/tail/loop tolerances.
  These tolerances are test-signal checks, not a subjective music-quality claim.

## vx side effects and source audit

The most recent official release found during review was also v0.9.37.
`--cache-mode offline` is NOT a promise of offline/read-only CLI behavior:

- [CLI entry](https://github.com/vx-org/vx/blob/v0.9.37/crates/vx-cli/src/lib.rs)
  initializes metrics and invokes the update checker after ordinary commands.
- [Update checker](https://github.com/vx-org/vx/blob/v0.9.37/crates/vx-cli/src/update_checker.rs)
  can fetch release information and creates/writes `~/.vx/cache/update_check.json`.
  This path uses HOME, not VX_HOME; isolate both. No offline-mode guard was found
  in `notify_if_update_available`.
- [Registry](https://github.com/vx-org/vx/blob/v0.9.37/crates/vx-cli/src/registry.rs)
  initializes embedded provider metadata and reads user/project overrides.
- [Pinned FFmpeg provider](https://github.com/vx-org/vx/blob/v0.9.37/crates/vx-providers/ffmpeg/provider.star)
  supplies the mirror URL and archive layout, declares no dependencies, and has
  a no-op post_install. It declares version test commands; that alone does not
  establish execution during installation.
- [Manifest install](https://github.com/vx-org/vx/blob/v0.9.37/crates/vx-runtime/src/manifest_runtime/install.rs)
  directly returns the archive-install result on Linux. Download failures
  propagate rather than falling through to system package managers. The reviewed
  branch resolves paths without executing FFmpeg. System strategies apply when
  no direct download exists, which is not this pinned Linux provider route.
- [CLI install handler](https://github.com/vx-org/vx/blob/v0.9.37/crates/vx-cli/src/commands/install/handler.rs)
  calls pre/post install hooks, updates existing lock files, and invalidates
  executable caches. The isolated working directory has no project lockfile.
- [Default verification](https://github.com/vx-org/vx/blob/v0.9.37/crates/vx-runtime/src/runtime/verify.rs)
  checks executable existence/permissions rather than launching version tests.

The lane explicitly permits known updater HTTP/cache and metrics effects inside
its disposable HOME. The subprocess PATH contains only the verified vx directory
during provisioning and native tests. Provisioning children receive only explicitly
constructed HOME, VX_HOME, XDG_CACHE_HOME, TMPDIR, PATH, LANG and media-control
variables; no runner environment is inherited. The native pytest launcher similarly uses execve with a closed environment before
conftest or Core imports, adding only verified media paths and explicit gateway,
skill-path, registry/checkpoint and test controls. It never restores runner variables,
including on fixture failure. Owned jobs are cancelled and handler completion is
checked during teardown; unconfirmed cleanup fails rather than passing. This is environment hygiene, not a claim that a
hosted runner has no credentials elsewhere. No privilege or security settings are
changed, and no system package manager is intentionally used.

## Budgets and evidence

- One Linux hosted job, 15-minute job ceiling; qualification 7 minutes, tests 6.
- Require at least 5 GiB free initially; retain a 2 GiB observed disk reserve.
- Archive member expansion acceptance cap: 1 GiB. vx executable member: 128 MiB.
- Expected binary archive traffic: 266,453,420 bytes, including the intentionally
  duplicated FFmpeg download. 384 MiB is a PLANNING allowance, not a hard network
  cap on vx's own retries, version queries, or update checks. Candidate-wheel,
  checkout, setup-python, and pip dependency transfers are additional CI costs.
- Direct downloads enforce exact pinned byte size/hash and a 180-second checked
  deadline plus 20-second socket timeout. Each supervised subprocess gets a
  deadline, 4 MiB observed log cap, isolated process group, forced cleanup on
  failure, and a wait for the launcher. The polling limits are not OS quotas.
- Native Core audio enforces its own input, output, decoded PCM, stderr, and
  deadline limits. Independent verification uses a separate invocation of the same verified
  FFmpeg build, not an independently implemented decoder. Its outputs are bounded by
  expected sample length plus one stereo frame. Tests run serially.
- Artifacts include command argv/status/logs, qualification and errors, actual
  MCP requests/responses, handler receipts, cancellation evidence, sample
  metrics, and JUnit. Runtime archives, PCM arrays, and the disposable HOME are
  not uploaded.

Before publication: AST/YAML parsing, formatter/linter checks, existing 98-test
non-media regression subset (exclude exactly `test_sequence_to_mp4_smoke_with_vx`
and `test_image_stats_command_runs_against_real_ffmpeg`; retain the bounded Python
process-tree unit fixtures), four mocked supervisor regressions (terminal log
overflow, bounded failure tail, cleanup uncertainty), opt-in module skip behavior,
independent static review, then this hosted native lane against the candidate wheel.
A skipped, failed, absent, or inconclusive native lane is not successful proof.
