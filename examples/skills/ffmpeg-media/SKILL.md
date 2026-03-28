---
name: ffmpeg-media
description: "Media conversion and processing tools powered by FFmpeg"
tools: ["Bash", "Read", "Write"]
tags: ["media", "video", "audio", "ffmpeg", "conversion"]
dcc: python
version: "1.0.0"
metadata:
  openclaw:
    requires:
      bins:
        - ffmpeg
        - ffprobe
    install:
      - kind: brew
        formula: ffmpeg
        bins: [ffmpeg, ffprobe]
---

# FFmpeg Media Skill

Integrates [FFmpeg](https://ffmpeg.org/) — the industry-standard open-source
multimedia framework — as MCP-discoverable tools.

This skill demonstrates how `dcc-mcp-core` can wrap **any CLI tool** as a skill,
making it available to AI agents via the MCP protocol.

## Scripts

- **probe.py** — Extract media file metadata (resolution, codec, duration, etc.)
- **convert.py** — Convert media files between formats (e.g. MP4 → WebM)
- **thumbnail.py** — Generate thumbnail images from video files

## Example

```bash
# Probe a video file
python scripts/probe.py --input video.mp4

# Convert to WebM
python scripts/convert.py --input video.mp4 --output video.webm --codec libvpx-vp9

# Generate thumbnail at 5 seconds
python scripts/thumbnail.py --input video.mp4 --time 5 --output thumb.jpg
```

## ClawHub Compatibility

This skill follows the [ClawHub](https://clawhub.ai) format and can be published
directly:

```bash
clawhub publish ./ffmpeg-media --slug ffmpeg-media --version 1.0.0
```
