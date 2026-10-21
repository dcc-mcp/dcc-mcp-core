---
name: media
description: >-
  Infrastructure skill - DCC-agnostic media probing, transcoding, thumbnails,
  frame extraction, and image-sequence-to-MP4 conversion through installed
  FFmpeg. Use when agents need to preflight, inspect, or share media outputs.
  Not for arbitrary shell or vx execution - use typed media tools only.
license: MIT
compatibility: Requires external FFmpeg and matching FFprobe; explicit executable overrides, then installed vx runtime, then system PATH. Never installs or downloads dependencies.
metadata:
  dcc-mcp:
    dcc: python
    recall-context:
      app_type: python
      domain: media
      workflow_stage: interchange
      task_category: export
    version: "0.1.0"
    layer: infrastructure
    search-hint: "media, ffmpeg, ffprobe, vx ffmpeg, check runtime, preflight, encoder, image sequence, render output, playblast, mp4, transcode, extract frames, thumbnail, probe video, audio-only, wav, ogg, vorbis"
    tags: "media, video, image-sequence, ffmpeg, ffprobe, vx, infrastructure, dcc-agnostic"
    tools: tools.yaml
---

# Media

DCC-agnostic media utilities for render, playblast, flipbook, and review
artifacts. FFmpeg and matching FFprobe are external, operator-approved
prerequisites, not bundled with Core. Every media tool uses only installed
binaries; none installs vx, downloads FFmpeg, or invokes a launcher that might
bootstrap dependencies. A missing runtime is an actionable preflight failure.

Use `media__check_runtime` before expensive render/export work. Prefer native
DCC skills for creating the render output, then use `media__probe`,
`media__sequence_to_mp4`, `media__thumbnail`, `media__transcode`,
`media__transcode_audio`, or `media__extract_frames` to inspect or repackage
files already on disk. Discover the capability, follow the returned load or
describe step, and call the returned tool slug with typed arguments. Do not
guess gateway slugs or replace the tool with arbitrary shell commands.

## Safety Contract

This skill intentionally exposes typed media operations, not a generic
`vx <command>` runner. Every tool builds a fixed argument vector for the
resolved FFmpeg or FFprobe executable without `shell=True`, validates paths
and enum fields, rejects option-like paths, and requires explicit overwrite
intent before replacing outputs. Verified audio-only output is stricter: its
destination must be fresh and cannot be overwritten.

## Runtime selection and preparation

FFmpeg selection follows this priority:

1. `DCC_MCP_MEDIA_FFMPEG_BIN`, if set to an absolute executable path.
2. An already-installed FFmpeg in the local vx store (`VX_HOME`, or `~/.vx`
   by default), discovered from local files without executing vx.
3. `ffmpeg` on the server process's system `PATH` if there is no installed vx
   runtime.

The vx lookup checks the vx 0.9.37 provider layouts under
`<vx-root>/store/ffmpeg/<version>`, with at most 128 entries and the highest
numeric stable version first. It does not require a vx launcher, consult
`vx.toml` or `vx.lock`, or select preview/custom version names. Pin a version
or use an unsupported layout through explicit executable overrides instead.

`DCC_MCP_MEDIA_FFPROBE_BIN` may specify the matching probe's absolute
executable path. Otherwise, resolve FFprobe only as a sibling of the resolved
FFmpeg binary, never from an unrelated `PATH` entry. FFmpeg and FFprobe must
have matching release/build identities. An invalid explicit override fails;
it is not permission to switch installations or silently choose another codec.
Override values are executable paths, not command names, shell fragments, or
arguments. Only native ELF, PE, or Mach-O executables are accepted; scripts and
known vx shim directories are rejected. Native wrappers cannot be identified
reliably from file headers. Spaces in paths are supported. Configure
these variables in the media server's environment, not only the agent's shell.

`DCC_MCP_MEDIA_VX_BIN` and `DCC_MCP_MEDIA_AUTO_INSTALL_VX` no longer control
media execution. Migrate an old pinned vx launcher to the FFmpeg/FFprobe
executable overrides. `VX_INSTALL_DIR` is not a media runtime selector, and no
installer configuration enables media-tool downloads.

Before an approved online setup, ask the user for permission to install or
download dependencies. Use an existing vx installation with
`vx install ffmpeg@8.1`, or the operating system's package manager, to obtain
FFmpeg and FFprobe together. If vx needs installation, download its official
installer to a local file, inspect it, and execute it only after approval;
never pipe an installer directly into a shell. Prepare approved binaries and
required shared libraries before an offline workload. Core's MIT license does
not replace FFmpeg's build-dependent LGPL/GPL license; review the selected
distribution's terms before redistribution.

Use absolute paths where possible. Relative paths are resolved by the process
running the DCC-MCP server, which may not be the same working directory as the
agent. Runtime overrides always require absolute paths.

## Read-only runtime preflight

Discover and call `media__check_runtime` before scheduling media work. Its
optional `required_encoders` array defaults to `pcm_s16le`, `libvorbis`, and
`pcm_f32le`; request the encoders needed for the intended workflow.
`timeout_secs` is bounded to 60 seconds. This check creates no media output
and performs no installation. The resolver itself starts no package manager
or network request; preflight executes the selected local binaries for
version and capability checks, so they remain operator-trusted code rather
than a sandboxed guarantee against arbitrary binary behavior.

Review the selected source, resolved absolute executable paths, versions,
SHA-256 hashes, and required encoder availability. Keep this evidence with the
job, and stop on missing binaries, build mismatches, or missing capabilities.
Selection priority is not a trust attestation: a local vx installation or
system path is an operator-trusted prerequisite, and a hash only identifies
the executable bytes. Preflight does not replace native codec, output, or
playback qualification. A later operation resolves and validates its own
runtime; an earlier successful check is not a guarantee that files or the
environment have remained unchanged.

## Verified audio-only encoding

Use `media__transcode_audio` for a fresh `.wav`/`pcm_s16le` or
`.ogg`/`libvorbis` output. Container and codec are explicit; incompatible pairs
are rejected before launch. Optional `audio_quality` is Vorbis-only (-1 to 10,
default 5). No resampling, channel conversion, overwrite, padding, trimming,
or codec fallback is exposed. The first audio stream is selected explicitly;
video, subtitle, data and inherited metadata are excluded. Source sample rate
and channel count are preserved, and a known source channel layout must match.

OGG output uses fixed `-page_duration 1` muxer pagination to keep the priming
packet off the final EOS page. This avoids the documented FFmpeg single-page
Vorbis duration-accounting issue without padding, trimming, resampling, or
changing compressed audio packets. See the
[upstream diagnosis](https://ffmpeg.org/pipermail/ffmpeg-devel/2022-May/295939.html).
More pages add container overhead; the full-decode sample-count gate is still
mandatory, including for very short clips. This is not a codec fallback.


This operation uses the same **already installed** FFmpeg/FFprobe selection
policy as preflight; it does not install anything. The selected executables
are **operator-trusted prerequisites**, not a supply-chain attestation. The
tool pins their resolved absolute paths and records each executable's version,
SHA-256 and resolution basis. Hashes identify the bytes used; neither a hash
nor the discovery source independently authenticates those bytes. Requested
encoder and `pcm_f32le` verification-encoder capabilities are checked before
encoding; a missing encoder fails without silently choosing another build.

Input is limited to 128 MiB, 8–192 kHz and 1–8 channels. Full f32le PCM decode
is streamed, never saved, with a 256 MiB cap for each source/output decode.
Text stdout has a 1 MiB cap; stderr has a 4 MiB total cap and retains only a
4 KiB diagnostic tail. Crossing a stream cap stops the owned process. Output
uses FFmpeg's `-fs` limit, owner-thread size monitoring, and a strict 64 MiB
pre-publication acceptance gate. FFmpeg can slightly overshoot `-fs`; this is
**not an OS disk quota**. Limits and the shared operation timeout appear in
receipts. Hashing also checks cancellation, time and byte budgets.

Encoding writes only to a private staging directory beside the destination.
The final pathname is published through an atomic no-replace hard link only
after complete validation. Filesystems without that operation fail closed;
there is no copy/replace fallback. Failed operations retain diagnostic staging
artifacts, never publish them or retry automatically. Successful publication
is the commit point; a late cancellation does not retract a published file.

Receipts bind source/output hashes, actual codec/container/rate/channels,
full-decode sample-frame counts and PCM hashes, resolved executable evidence,
exact argv, Core-owned job identity when present, and cleanup state. Counts
come from decoder PCM (including its codec-delay/skip handling), not container
duration or packet count. A count change is a failure, never repaired. Lossy
waveform quality, independent decoder/loop-boundary checks and browser playback
remain separate qualification; matching counts alone do not prove quality. Lossy
Vorbis can change very short signals substantially: preserving one sample is
not a promise to reproduce that sample's amplitude. Do not waive fidelity
requirements, pad/trim samples, or treat count acceptance as a quality score.

The existing Core async job lifecycle supplies job identity and cancellation.
The owner thread checks `check_dcc_cancelled()` at most 50 ms between process
polls, and during hashing and before publication. Cancellation and timeout stop
the owned group and retain the original reason/receipt. Poll the Core job to
terminal state; admission is not artifact acceptance. POSIX processes run in
owned groups. Windows uses `taskkill /T /F` and launcher reaping; failure or an
already-exited launcher leaves cleanup `unknown`. Windows live cancellation,
real Core transport cancellation and native codec qualification remain separate
from host-free/POSIX subprocess tests. Cleanup has a separate bounded grace
period; an uncertain cleanup never grants permission to retry.
