---
name: ffmpeg-media
description: >-
  Infrastructure skill — media conversion and processing via FFmpeg: convert
  video/audio formats, extract frames, resize, and transcode. Use when
  manipulating raw media files (mp4, mov, wav, image sequences) regardless of
  DCC context. Not for DCC-specific render output handling — use a domain
  pipeline skill for post-render processing tied to a specific DCC.
license: MIT
compatibility: Requires ffmpeg and ffprobe binaries on PATH
allowed-tools: Bash Read Write
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: infrastructure
  dcc-mcp.search-hint: "ffmpeg, video transcode, audio convert, extract frames, resize video, image sequence, media processing"
  dcc-mcp.tags: "media, video, audio, ffmpeg, conversion, infrastructure"
  dcc-mcp.tools: tools.yaml
  openclaw:
    requires:
      bins:
        - ffmpeg
        - ffprobe
    install:
      - kind: brew
        formula: ffmpeg
        bins: [ffmpeg, ffprobe]
    emoji: "🎬"
    homepage: https://ffmpeg.org
---

# FFmpeg Media Tools

Cross-platform media conversion and processing tools powered by FFmpeg.

## Tools

### `ffmpeg_media__convert`
Convert between video and audio formats.

### `ffmpeg_media__extract_frames`
Extract individual frames from a video file.

## Prerequisites

Install FFmpeg:
- **macOS**: `brew install ffmpeg`
- **Linux**: `apt install ffmpeg` or `yum install ffmpeg`
- **Windows**: Download from https://ffmpeg.org/download.html

Verify installation: `ffmpeg -version`
