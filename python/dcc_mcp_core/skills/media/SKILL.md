---
name: media
description: >-
  Infrastructure skill - DCC-agnostic media probing, transcoding, thumbnails,
  frame extraction, and image-sequence-to-MP4 conversion through vx-managed
  FFmpeg. Use when agents need to inspect or share render/playblast outputs.
  Not for arbitrary shell or vx execution - use typed media tools only.
license: MIT
compatibility: Uses vx on PATH or bootstraps vx with the official install script; ffmpeg/ffprobe are provisioned via vx.
metadata:
  dcc-mcp:
    dcc: python
    version: "0.1.0"
    layer: infrastructure
    search-hint: "media, ffmpeg, ffprobe, vx ffmpeg, image sequence, render output, playblast, mp4, transcode, extract frames, thumbnail, probe video"
    tags: "media, video, image-sequence, ffmpeg, ffprobe, vx, infrastructure, dcc-agnostic"
    tools: tools.yaml
---

# Media

DCC-agnostic media utilities for render, playblast, flipbook, and review
artifacts. The tools invoke FFmpeg and FFprobe through `vx`, so a fresh machine
does not need users to install FFmpeg manually. If `vx` is not available on
`PATH`, the skill runs the official `loonghao/vx` installer for the current
platform and then retries the media command with the installed binary.

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
- `DCC_MCP_MEDIA_AUTO_INSTALL_VX=0`: disable automatic vx installation.
- `VX_INSTALL_DIR`: override the official vx installer target directory.

Installer commands:

```bash
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```

Use absolute paths where possible. Relative paths are resolved by the process
running the DCC-MCP server, which may not be the same working directory as the
agent.
