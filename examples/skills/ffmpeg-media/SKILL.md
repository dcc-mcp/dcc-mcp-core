---
name: ffmpeg-media
description: "Media conversion and processing powered by FFmpeg — convert video/audio formats, extract frames, resize, and transcode. Use when working with video, audio, or image sequences."
license: MIT
compatibility: Requires ffmpeg and ffprobe binaries on PATH
allowed-tools: Bash Read Write
metadata:
  category: media
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
tags: [media, video, audio, ffmpeg, conversion]
dcc: python
version: "1.0.0"
tools:
  - name: convert
    description: Convert a media file to a different format
    input_schema:
      type: object
      required: [input, output]
      properties:
        input: {type: string, description: Input file path}
        output: {type: string, description: Output file path}
        codec: {type: string, description: Output codec (e.g. h264, vp9)}
    source_file: scripts/convert.py

  - name: extract_frames
    description: Extract video frames as image files
    input_schema:
      type: object
      required: [input, output_dir]
      properties:
        input: {type: string, description: Input video path}
        output_dir: {type: string, description: Directory to save frames}
        fps: {type: number, description: Frames per second to extract, default 1}
    source_file: scripts/extract_frames.py
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
