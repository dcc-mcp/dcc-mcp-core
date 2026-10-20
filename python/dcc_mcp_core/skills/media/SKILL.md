---
name: media
description: >-
  Infrastructure skill - DCC-agnostic media probing, transcoding, thumbnails,
  frame extraction, and image-sequence-to-MP4 conversion through vx-managed
  FFmpeg. Use when agents need to inspect or share render/playblast outputs.
  Not for arbitrary shell or vx execution - use typed media tools only.
license: MIT
compatibility: Uses vx on PATH; non-read-only media tools can bootstrap vx with the official install script.
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
    search-hint: "media, ffmpeg, ffprobe, vx ffmpeg, image sequence, render output, playblast, mp4, transcode, extract frames, thumbnail, probe video, audio-only, wav, ogg, vorbis"
    tags: "media, video, image-sequence, ffmpeg, ffprobe, vx, infrastructure, dcc-agnostic"
    tools: tools.yaml
---

# Media

DCC-agnostic media utilities for render, playblast, flipbook, and review
artifacts. The tools invoke FFmpeg and FFprobe through `vx`, so a fresh machine
does not need users to install FFmpeg manually. If `vx` is not available on
`PATH`, non-read-only media tools run the official `loonghao/vx` installer for
the current platform and then retry the media command with the installed
binary. `media__probe` remains side-effect-free: when `vx` is missing it
returns `vx_not_found` instead of installing anything.

Use this skill after a DCC-native render/playblast/export tool has produced
files on disk. Prefer native DCC skills for creating the render output, then use
`media__probe`, `media__sequence_to_mp4`, `media__thumbnail`,
`media__transcode`, or `media__extract_frames` to inspect or repackage the
artifact.

## Safety Contract

This skill intentionally exposes typed media operations, not a generic
`vx <command>` runner. Every tool builds a fixed `vx ffmpeg` or `vx ffprobe`
argument vector without `shell=True`, validates paths and enum fields, rejects
option-like paths, and requires explicit overwrite intent before replacing
outputs.

Configuration:

- `DCC_MCP_MEDIA_VX_BIN`: explicit vx executable path or command name.
- `DCC_MCP_MEDIA_AUTO_INSTALL_VX=0`: disable automatic vx installation for
  non-read-only media tools.
- `VX_INSTALL_DIR`: override the official vx installer target directory.

Installer commands:

```bash
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```

Use absolute paths where possible. Relative paths are resolved by the process
running the DCC-MCP server, which may not be the same working directory as the
agent.

## Verified audio-only encoding

Use `media__transcode_audio` for a fresh `.wav`/`pcm_s16le` or
`.ogg`/`libvorbis` output. Container and codec are explicit; incompatible pairs
are rejected before launch. Optional `audio_quality` is Vorbis-only (-1 to 10,
default 5). No resampling, channel conversion, overwrite, padding, trimming,
or codec fallback is exposed. The first audio stream is selected explicitly;
video, subtitle, data and inherited metadata are excluded. Source sample rate
and channel count are preserved, and a known source channel layout must match.

This operation requires **already installed** vx-managed FFmpeg and FFprobe;
it does not install anything. The launcher from `DCC_MCP_MEDIA_VX_BIN` or PATH
is an **operator-trusted prerequisite**, not a supply-chain attestation. Its
absolute path, version and hash are recorded. The tool resolves FFmpeg using
`vx --output-format json where ffmpeg`, rejects reported system/global fallback,
and resolves FFprobe only in the same real bundle directory (the official vx
provider layout). It pins these absolute paths and records each executable's
version, SHA-256 and resolution basis. Hashes identify the bytes used; neither
a hash nor vx's self-reported source independently authenticates those bytes.
Requested encoder and PCM verification-encoder capabilities are checked before
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
remain separate qualification; matching counts alone do not prove quality.

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
